// The size of the viewport's drawing buffer, and the one rule about changing
// it: a canvas whose buffer is resized is cleared, so the new buffer is drawn
// into before anything else can run. Whatever asks for a new size -- the
// viewport's box, the screen's pixel ratio, the lighter picture drawn while
// the view moves -- comes through `DrawingBuffer.sync`, which resizes and
// draws as one step. Free of the renderer, so the rule is testable.

/** A drawing buffer: CSS pixels, and device pixels per CSS pixel. */
export interface BufferSize {
  width: number;
  height: number;
  ratio: number;
}

/** The most device pixels drawn per CSS pixel: past this the cost grows and the picture does not. */
export const MAX_RATIO = 2;

/**
 * The buffer for a viewport of `cssWidth` by `cssHeight` on a screen of
 * `devicePixelRatio`; `lean` is the lighter picture, one device pixel a CSS
 * pixel at most. Null when the viewport has no area (hidden, collapsed, not
 * laid out yet): there is nothing to draw into, and the buffer it had is kept
 * for when it has one again.
 */
export function bufferSizeFor(cssWidth: number, cssHeight: number, devicePixelRatio: number, lean: boolean): BufferSize | null {
  const width = Math.floor(cssWidth);
  const height = Math.floor(cssHeight);
  if (!(width >= 1) || !(height >= 1)) return null;
  const rest = Math.min(devicePixelRatio > 0 ? devicePixelRatio : 1, MAX_RATIO);
  return { width, height, ratio: lean ? Math.min(1, rest) : rest };
}

export function sameBuffer(a: BufferSize | null, b: BufferSize | null): boolean {
  return a !== null && b !== null && a.width === b.width && a.height === b.height && a.ratio === b.ratio;
}

/** What a `DrawingBuffer` drives. */
export interface BufferTarget {
  /** Give the canvas this buffer. Its contents are gone when this returns. */
  resize(size: BufferSize): void;
  /** Draw the picture into the buffer as it now is, before returning. */
  draw(): void;
}

export class DrawingBuffer {
  private current: BufferSize | null = null;

  constructor(private readonly target: BufferTarget) {}

  /** The buffer as last set, or null before the first. */
  get size(): BufferSize | null {
    return this.current;
  }

  /**
   * Bring the buffer to what the viewport now asks for. A change resizes it
   * and draws into it before returning, so no cleared buffer is ever left to
   * be shown; no change does nothing. True when it changed.
   */
  sync(cssWidth: number, cssHeight: number, devicePixelRatio: number, lean: boolean): boolean {
    const next = bufferSizeFor(cssWidth, cssHeight, devicePixelRatio, lean);
    if (next === null || sameBuffer(next, this.current)) return false;
    this.current = next;
    this.target.resize(next);
    this.target.draw();
    return true;
  }

  /** The canvas has a new renderer: whatever size it had is not this one's. */
  reset(): void {
    this.current = null;
  }
}
