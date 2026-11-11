// How much of the viewport's edges something lying over it covers: what the
// viewer is told (`Viewer.setViewportInsets`) so that the model is drawn in
// the middle of the part still in view.

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export const NO_INSETS: Insets = Object.freeze({ top: 0, right: 0, bottom: 0, left: 0 });

/** A cover counts along an edge once it spans this much of it. */
const SPAN = 0.6;

/**
 * The widest stretch of `lo..hi` that none of `spans` covers, as how far in
 * from each end it starts.
 */
function widestClear(lo: number, hi: number, spans: readonly [number, number][]): [number, number] {
  const sorted = [...spans].sort((a, b) => a[0] - b[0]);
  let best: [number, number] = [lo, lo];
  let at = lo;
  for (const [from, to] of [...sorted, [hi, hi] as [number, number]]) {
    if (from - at > best[1] - best[0]) best = [at, from];
    at = Math.max(at, to);
  }
  return [best[0] - lo, hi - best[1]];
}

/**
 * The insets that `covers` make on `view`, all in one coordinate space.
 *
 * A cover that lies across most of the viewport's width is a band across
 * it, and one that runs most of the height a band down it. The insets are
 * what bounds the largest stretch no band covers: a sheet along the bottom
 * takes that edge in as far up as it reaches, and a sheet lifted by a
 * keyboard takes it in from the sheet's top, whatever shows between the two.
 * A card over one corner is not a band: the model shows around it, and no
 * single shift would clear it. A cover that does not overlap the viewport --
 * a panel beside it -- makes no inset at all.
 */
export function coveredInsets(view: Box, covers: readonly Box[]): Insets {
  const vw = view.right - view.left;
  const vh = view.bottom - view.top;
  if (!(vw > 0 && vh > 0)) return { ...NO_INSETS };
  const across: [number, number][] = [];
  const down: [number, number][] = [];
  for (const c of covers) {
    const left = Math.max(view.left, c.left);
    const right = Math.min(view.right, c.right);
    const top = Math.max(view.top, c.top);
    const bottom = Math.min(view.bottom, c.bottom);
    if (!(right > left && bottom > top)) continue;
    if (right - left >= vw * SPAN) across.push([top, bottom]);
    else if (bottom - top >= vh * SPAN) down.push([left, right]);
  }
  const [top, bottom] = widestClear(view.top, view.bottom, across);
  const [left, right] = widestClear(view.left, view.right, down);
  return { top, right, bottom, left };
}

export function sameInsets(a: Insets, b: Insets): boolean {
  return a.top === b.top && a.right === b.right && a.bottom === b.bottom && a.left === b.left;
}
