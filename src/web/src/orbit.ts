// What every orbiting 3D view shares, in the browser: the elevation limits
// and how a flick eases onto them, the bed's fade as the camera goes under it,
// and which wheel input does what. The Swift views share the same rules
// (src/apple/ViewerCore); both are tested against one reference,
// src/apple/ViewerCore/viewer_conformance.json
// (test/viewer-conformance.test.mjs here), so they cannot drift apart.
//
// Elevations are degrees above the horizon through the target (the camera
// controller's polar angle is 90 degrees minus this).

/** Under the bed, nearly straight up at it. */
export const MIN_ELEVATION = -85;
/** Over the top, nearly straight down. */
export const MAX_ELEVATION = 89;
/** A coasting camera heading for a limit slows over about this long (s). */
export const LIMIT_EASE_SECONDS = 0.12;

export function clampElevation(e: number): number {
  return Math.min(Math.max(e, MIN_ELEVATION), MAX_ELEVATION);
}

/**
 * One step of coasting in elevation: where a camera at `e` moving at `v`
 * (degrees/s) is `dt` seconds later, and its speed then. Heading for a limit,
 * the speed is held to what would cover the room left in
 * `LIMIT_EASE_SECONDS`, so the camera eases onto the limit: it never passes
 * it, and its last steps are small.
 */
export function coastElevation(e: number, v: number, dt: number): [number, number] {
  if (dt <= 0 || v === 0) return [clampElevation(e), v];
  const limit = v < 0 ? MIN_ELEVATION : MAX_ELEVATION;
  const room = Math.abs(limit - e);
  const cap = room / Math.max(LIMIT_EASE_SECONDS, dt);
  const v2 = v < 0 ? Math.max(v, -cap) : Math.min(v, cap);
  const e2 = clampElevation(e + v2 * dt);
  return [e2, Math.abs(limit - e2) < 0.01 ? 0 : v2];
}

/** The camera's angle above the bed (degrees) where the fade starts, and where it is complete. */
export const FADE_START = 2;
export const FADE_END = -10;
/** What is left of the bed's fill, and of its lines and labels, under it. */
export const FILL_UNDER = 0.12;
export const LINES_UNDER = 0.2;

export interface BedFade {
  /** 1 with the camera above the bed, 0 once it is well under it. */
  above: number;
  /** The bed's fill: an opacity multiplier, 1 to 0.12. */
  fill: number;
  /** Its grid, outline and labels: an opacity multiplier, 1 to 0.2. */
  lines: number;
  /** The shadow cast on it: 1 to 0. */
  shadow: number;
  /** A light from below, for the part's underside: 0 to 1. */
  underLight: number;
}

/**
 * How much of a print bed (or the ground grid) shows with the camera
 * `degrees` above its plane: all of it from above, a faint sheet from below,
 * smoothly across the horizon so the part's first layer is never hidden.
 */
export function bedFade(degrees: number): BedFade {
  const t = (degrees - FADE_END) / (FADE_START - FADE_END);
  const u = Math.min(Math.max(t, 0), 1);
  const above = u * u * (3 - 2 * u);
  return {
    above,
    fill: FILL_UNDER + (1 - FILL_UNDER) * above,
    lines: LINES_UNDER + (1 - LINES_UNDER) * above,
    shadow: above,
    underLight: 1 - above,
  };
}

/**
 * The fade for a camera `height` above the bed's plane looking at a target
 * `distance` away: the angle at which its line of sight meets the plane.
 */
export function bedFadeFromHeight(height: number, distance: number): BedFade {
  const s = distance > 0 ? Math.min(Math.max(height / distance, -1), 1) : height < 0 ? -1 : 1;
  return bedFade((Math.asin(s) * 180) / Math.PI);
}

export type WheelInput = "wheel" | "trackpadScroll" | "pinch";
export type WheelAction = "zoom" | "pan" | "orbit" | "page";

/**
 * What a scroll or a pinch does (src/web/README.md, "Viewport controls"):
 * Option + wheel always zooms toward the pointer; a pinch zooms; the plain
 * wheel scrolls the page the view sits in, and zooms where nothing scrolls; a
 * trackpad's two fingers pan (shift: orbit) where that steals no page's
 * scrolling.
 */
export function wheelAction(input: WheelInput, option: boolean, shift: boolean, inPage: boolean): WheelAction {
  if (input === "pinch" || option) return "zoom";
  if (input === "wheel") return inPage ? "page" : "zoom";
  return inPage ? "page" : shift ? "orbit" : "pan";
}
