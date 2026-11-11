// The studio a model is lit by, as numbers and nothing else: no renderer and
// no DOM, so any bundle that draws a model can import the same light instead
// of keeping a copy of it.

/**
 * The viewport's light, after `cadpole render`'s default studio
 * (`Lights::studio` in cadpole-core): the surround's three stops and the
 * three lights, in linear RGB. `dir` points toward the light with the
 * camera at the iso view; the rig turns about z with the camera.
 *
 * A surround matters most to a metal, which has no diffuse colour: with
 * nothing to reflect it draws black between its highlights.
 *
 * The directions and colours are that studio's. The strengths are not quite:
 * the tool's picture goes through a filmic curve, which lifts the middle and
 * rolls the top off, and the viewport keeps the neutral one so that a
 * material's colour stays the colour it was given. A softer key under more
 * exposure lands the same middle without burning a polished metal's
 * highlight; `e2e/shading.spec.ts` holds the result to a band.
 */
export const STUDIO = {
  sky: [0.3, 0.36, 0.48],
  horizon: [0.19, 0.19, 0.21],
  ground: [0.22, 0.21, 0.2],
  /** What the surround is multiplied by. */
  surround: 1.15,
  lights: [
    { dir: [-0.4, -0.6, 0.7], color: [1, 0.96, 0.88], intensity: 3.4 },
    { dir: [0.6, 0.3, 0.2], color: [0.71, 0.8, 1], intensity: 0.78 },
    { dir: [0.3, 0.8, 0.4], color: [1, 1, 1], intensity: 0.5 },
  ],
  exposure: 1.3,
} as const;

/**
 * The camera azimuth `STUDIO.lights` are written for: the iso view's, which
 * looks along (1, -0.95) on the ground plane.
 */
export const STUDIO_AZIMUTH = Math.atan2(-0.95, 1);

/** The surround map's size, texels: `studioSurround` fills this many. */
export const STUDIO_SURROUND_SIZE = { width: 256, height: 128 } as const;

/**
 * The surround as the texels of an equirectangular environment map, RGBA
 * half floats: ground below, the horizon at the equator, sky at the zenith,
 * blended as `cadpole render` blends them. It depends on elevation alone, so
 * it is small and computed rather than downloaded. Rows run from straight
 * down to straight up. `toHalfFloat` is the caller's (three's
 * `DataUtils.toHalfFloat`), so this module stays free of the renderer.
 */
export function studioSurround(toHalfFloat: (value: number) => number): Uint16Array {
  // A prefilter sizes its cube from the map's width, a quarter of it a
  // face: 256 gives the 64-texel faces a gradient this smooth needs.
  const { width: W, height: H } = STUDIO_SURROUND_SIZE;
  const data = new Uint16Array(W * H * 4);
  const one = toHalfFloat(1);
  for (let j = 0; j < H; j++) {
    const up = Math.sin(((j + 0.5) / H - 0.5) * Math.PI);
    const t = Math.abs(up);
    const k = t * t * (3 - 2 * t);
    const to = up >= 0 ? STUDIO.sky : STUDIO.ground;
    for (let i = 0; i < W; i++) {
      const o = (j * W + i) * 4;
      for (let c = 0; c < 3; c++) {
        const h0 = STUDIO.horizon[c] as number;
        data[o + c] = toHalfFloat((h0 + ((to[c] as number) - h0) * k) * STUDIO.surround);
      }
      data[o + 3] = one;
    }
  }
  return data;
}
