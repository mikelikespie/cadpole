// One input model for every view that shows a model or a drawing.
//
// The router turns raw DOM input -- pointer, wheel, Safari's gesture events,
// double-click and the context menu -- into a handful of intents, in the
// element's own CSS pixels, and hands them to a `GestureTarget`: the 3D
// camera (controls.ts) or a 2D pan-and-zoom (panzoom.ts). The target decides
// what an intent means; the router alone decides which input is which.
//
//   primary drag      a left-button drag, or one finger:       orbit (3D) / pan (2D)
//   secondary drag    shift + drag, right or middle drag:       pan
//   pan               two-finger trackpad scroll, two fingers:  pan
//   zoom              pinch (ctrl + wheel, gesture scale, two touch points),
//                     a notched mouse wheel:                    zoom about the point
//   twist             two fingers turning (gesture rotation, two touch points)
//   tap, double-tap, context (right click or a long press)
//
// A touch screen on a Mac reaches a page as neither touch nor pointer events
// of type "touch". Safari reports a finger as a mouse (a drag is a left-button
// drag) and two fingers as gesture events (scale and rotation) alongside a
// one-button drag at their centroid, which pans. Chrome reports a finger drag
// as a pixel wheel stream, the same as a trackpad scroll, so it pans; a press
// held still and then dragged arrives there as a real drag. Both report a
// press held still as a right click (a context menu), which a drag that
// follows at once takes back.
//
// Telling a notched mouse wheel from a trackpad or a finger: each wheel
// stream (events less than 300 ms apart) is classified by its first event. A
// line-mode wheel, or a pixel event with no sideways part whose size is a
// whole multiple of 40 px (a line, in Chromium and Safari on macOS) or at
// least 100 px (Chromium on other systems), starts a mouse stream; anything
// else, and any event flagged as momentum, is a trackpad. A mouse stream
// turns into a trackpad stream at its first event of another shape; a
// trackpad stream never turns into a mouse one, so a trackpad delta that
// happens to land on 40 px does not zoom.

export type WheelMode = "auto" | "orbit" | "zoom";

/** What a view does with each intent. Coordinates are CSS pixels in the element. */
export interface GestureTarget {
  /** A drag began; `mode` is what its moves will be. */
  dragStart(mode: "primary" | "pan"): void;
  /** A primary drag moved: orbit in 3D, pan in 2D. */
  primary(dx: number, dy: number): void;
  pan(dx: number, dy: number): void;
  /** Zoom by `scale` about (x, y): below 1 comes closer. */
  zoomAt(x: number, y: number, scale: number): void;
  /** Turn by `rad`, clockwise on screen, about (x, y). Optional: a drawing does not turn. */
  twistAt?(x: number, y: number, rad: number): void;
  /** The drag ended; `moving` when the pointer was still moving as it lifted (inertia). */
  dragEnd(mode: "primary" | "pan", moving: boolean): void;
  /** A gesture is running (anything down, a pinch, a twist): views draw lighter meanwhile. */
  active(on: boolean): void;
  /** True while the view is still coasting: a press stops it and is not a tap. */
  coasting(): boolean;
  /** Stop any coasting. */
  stop(): void;
  tap(x: number, y: number, additive: boolean): void;
  doubleTap(x: number, y: number): void;
  context(x: number, y: number): void;
  /** A context request turned out to be the start of a drag: take it back. */
  contextDismiss?(): void;
  /** Shift + two-finger scroll: orbit in 3D. Without it, shift + scroll pans. */
  shiftScroll?(dx: number, dy: number): void;
}

const LONG_PRESS_MS = 500;
const DOUBLE_TAP_MS = 320;
/**
 * How far a pointer may travel and still be a click. A fingertip rolls as it
 * lifts, so a touch gets twice the mouse's allowance: an orbit that selects
 * is a nuisance, and so is a tap that orbits instead of selecting.
 */
const CLICK_SLOP_MOUSE = 5;
const CLICK_SLOP_TOUCH = 10;
/** Wheel events closer together than this belong to one stream. */
const WHEEL_STREAM_MS = 300;
/** A twist turns the model only past this many degrees, and only while it outweighs the pinch. */
const TWIST_ENGAGE_DEG = 10;
/** Degrees of twist that weigh the same as a pinch of ln(scale) = 1. */
const TWIST_PER_LN_SCALE = 45;
/** A two-finger touch in Safari: its first finger's press is cancelled this soon. */
const PAIR_CANCEL_MS = 150;
/** ...and the centroid drag that follows starts within this. */
const PAIR_FOLLOW_MS = 300;
/** A press held into a context menu and then dragged: the drag's press comes within this, this near. */
const HOLD_DRAG_MS = 1500;
const HOLD_DRAG_PX = 24;

export interface RouterOptions {
  /** Turn on two-finger twist (3D). Off for drawings. */
  twist?: boolean;
}

export class InputRouter {
  wheelMode: WheelMode = "auto";
  private readonly pointers = new Map<number, { x: number; y: number; type: string }>();
  private dragMode: "none" | "primary" | "pan" = "none";
  private lastX = 0;
  private lastY = 0;
  private downX = 0;
  private downY = 0;
  private downAt = 0;
  private downButton = -1;
  private moved = false;
  private caught = false;
  private lastMoveAt = 0;
  private pinchDist = 0;
  private pinchMidX = 0;
  private pinchMidY = 0;
  private pairAngle = 0;
  private pairRot = 0;
  private pairScale = 1;
  private pairEngaged = false;
  private pairCancelAt = 0;
  private longPressTimer: number | null = null;
  private lastTapAt = 0;
  private lastTapX = 0;
  private lastTapY = 0;
  private wheelStream: { kind: "mouse" | "trackpad"; at: number } | null = null;
  private readonly pinchSigns: { s: number; at: number }[] = [];
  private gesture: { scale: number; rot: number; rot0: number; scale0: number; engaged: boolean; touch: boolean } | null = null;
  private contextAt: { x: number; y: number; t: number } | null = null;
  /** A right press's menu, held until the press shows it is not a right drag (a pan). */
  private contextPending: { x: number; y: number; timer: number } | null = null;
  private readonly listeners: [string, EventListener, AddEventListenerOptions?][];

  constructor(
    readonly element: HTMLElement,
    private readonly target: GestureTarget,
    private readonly opts: RouterOptions = {},
  ) {
    // Set here, before the first frame: a view's first gesture must behave
    // like every later one.
    element.style.touchAction = "none";
    // A double-tap, or a press held and then dragged, never selects the page's text.
    element.style.userSelect = "none";
    element.style.setProperty("-webkit-user-select", "none");
    element.style.setProperty("-webkit-touch-callout", "none");
    const prevent = (e: Event): void => e.preventDefault();
    this.listeners = [
      ["pointerdown", this.onPointerDown as EventListener],
      ["pointermove", this.onPointerMove as EventListener],
      ["pointerup", this.onPointerUp as EventListener],
      ["pointercancel", this.onPointerUp as EventListener],
      ["wheel", this.onWheel as EventListener, { passive: false }],
      ["gesturestart", this.onGestureEvent, { passive: false }],
      ["gesturechange", this.onGestureEvent, { passive: false }],
      ["gestureend", this.onGestureEvent, { passive: false }],
      ["dblclick", this.onDblClick as EventListener],
      ["selectstart", prevent],
      ["contextmenu", this.onContextMenu as EventListener],
    ];
    for (const [t, f, o] of this.listeners) element.addEventListener(t, f, o);
  }

  dispose(): void {
    this.cancelLongPress();
    this.dropPendingContext();
    for (const [t, f] of this.listeners) this.element.removeEventListener(t, f);
  }

  private local(e: { clientX: number; clientY: number }): { x: number; y: number } {
    const r = this.element.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private pair(): [{ x: number; y: number }, { x: number; y: number }] {
    const it = this.pointers.values();
    const a = it.next().value ?? { x: 0, y: 0 };
    const b = it.next().value ?? a;
    return [a, b];
  }

  // ---------------------------------------------------------- pointers

  private readonly onPointerDown = (e: PointerEvent): void => {
    const p = this.local(e);
    this.pointers.set(e.pointerId, { x: p.x, y: p.y, type: e.pointerType });
    try {
      this.element.setPointerCapture(e.pointerId);
    } catch {
      // A pointer the browser no longer tracks (a press it already cancelled).
    }
    const now = performance.now();
    this.dropPendingContext();
    if (this.pointers.size === 1) {
      this.caught = this.target.coasting();
      this.target.active(true);
    }
    this.target.stop();
    if (this.pointers.size === 1) {
      this.downX = this.lastX = p.x;
      this.downY = this.lastY = p.y;
      this.downAt = now;
      this.downButton = e.button;
      this.moved = false;
      // A press held into a context menu, then dragged: the menu was not wanted.
      const c = this.contextAt;
      if (c && e.button === 0 && now - c.t < HOLD_DRAG_MS && Math.hypot(p.x - c.x, p.y - c.y) < HOLD_DRAG_PX) {
        this.contextAt = null;
        this.target.contextDismiss?.();
      }
      if (this.gesture?.touch || (e.pointerType === "mouse" && e.button === 0 && now - this.pairCancelAt < PAIR_FOLLOW_MS)) {
        // Safari, two fingers on a touch screen: this press is the drag it
        // makes at their centroid, and two fingers pan.
        this.dragMode = "pan";
      } else if (e.pointerType === "touch") {
        this.dragMode = "primary";
        this.longPressTimer = window.setTimeout(() => {
          this.longPressTimer = null;
          if (!this.moved && this.pointers.size === 1) {
            this.dragMode = "none";
            this.openContext(p.x, p.y);
          }
        }, LONG_PRESS_MS);
      } else if (e.button === 0 && !e.shiftKey) this.dragMode = "primary";
      else if (e.button === 0 || e.button === 1 || e.button === 2) this.dragMode = "pan";
      if (this.dragMode !== "none") this.target.dragStart(this.dragMode);
    } else if (this.pointers.size === 2) {
      this.cancelLongPress();
      this.dragMode = "none";
      const [a, b] = this.pair();
      this.pinchDist = Math.hypot(a.x - b.x, a.y - b.y);
      this.pinchMidX = (a.x + b.x) / 2;
      this.pinchMidY = (a.y + b.y) / 2;
      this.pairAngle = Math.atan2(b.y - a.y, b.x - a.x);
      this.pairRot = 0;
      this.pairScale = 1;
      this.pairEngaged = false;
    }
  };

  private readonly onPointerMove = (e: PointerEvent): void => {
    if (!this.pointers.has(e.pointerId)) return;
    const p = this.local(e);
    this.pointers.set(e.pointerId, { x: p.x, y: p.y, type: e.pointerType });
    if (this.pointers.size === 2) {
      const [a, b] = this.pair();
      const dist = Math.hypot(a.x - b.x, a.y - b.y);
      const midX = (a.x + b.x) / 2;
      const midY = (a.y + b.y) / 2;
      if (this.pinchDist > 0 && dist > 0) {
        this.target.zoomAt(midX, midY, this.pinchDist / dist);
        this.pairScale *= dist / this.pinchDist;
      }
      this.target.pan(midX - this.pinchMidX, midY - this.pinchMidY);
      // The twist: the turn of the line between the fingers since the last move.
      const angle = Math.atan2(b.y - a.y, b.x - a.x);
      let da = angle - this.pairAngle;
      while (da > Math.PI) da -= 2 * Math.PI;
      while (da < -Math.PI) da += 2 * Math.PI;
      this.pairAngle = angle;
      this.pairRot += da;
      if (this.opts.twist && this.target.twistAt) {
        if (!this.pairEngaged && twistEngages((this.pairRot * 180) / Math.PI, this.pairScale)) {
          this.pairEngaged = true;
          da = 0;
        }
        if (this.pairEngaged && da !== 0) this.target.twistAt(midX, midY, da);
      }
      this.pinchDist = dist;
      this.pinchMidX = midX;
      this.pinchMidY = midY;
      this.moved = true;
      return;
    }
    let dx = p.x - this.lastX;
    let dy = p.y - this.lastY;
    const touch = e.pointerType === "touch";
    if (!this.moved && Math.hypot(p.x - this.downX, p.y - this.downY) > (touch ? CLICK_SLOP_TOUCH : CLICK_SLOP_MOUSE)) {
      // A finger's drag starts from here, not from where it came down: its
      // wider slop is not paid back as a jump.
      if (touch) dx = dy = 0;
      this.moved = true;
      this.cancelLongPress();
      this.dropPendingContext();
    }
    if (!this.moved || this.dragMode === "none") return;
    this.lastMoveAt = performance.now();
    if (this.dragMode === "primary") this.target.primary(dx, dy);
    else this.target.pan(dx, dy);
    this.lastX = p.x;
    this.lastY = p.y;
  };

  private readonly onPointerUp = (e: PointerEvent): void => {
    if (!this.pointers.has(e.pointerId)) return;
    const now = performance.now();
    // Safari cancels the first finger's press as soon as a second finger
    // makes it a two-finger gesture; the drag that follows is theirs.
    if (e.type === "pointercancel" && e.pointerType === "mouse" && !this.moved && now - this.downAt < PAIR_CANCEL_MS) this.pairCancelAt = now;
    const p = this.local(e);
    this.pointers.delete(e.pointerId);
    this.cancelLongPress();
    if (this.element.hasPointerCapture(e.pointerId)) this.element.releasePointerCapture(e.pointerId);
    if (this.pointers.size > 0) {
      // Went from two fingers to one: restart a single-finger drag cleanly.
      const rest = [...this.pointers.values()][0] as { x: number; y: number };
      this.lastX = rest.x;
      this.lastY = rest.y;
      this.dragMode = "primary";
      this.target.dragStart("primary");
      this.moved = true;
      return;
    }
    if (!this.gesture) this.target.active(false);
    const wasDrag = this.moved;
    const mode = this.dragMode;
    this.dragMode = "none";
    if (wasDrag) {
      if (mode !== "none") this.target.dragEnd(mode, now - this.lastMoveAt < 80);
      return;
    }
    if (e.type === "pointercancel") return;
    if (now - this.downAt > 700) return;
    if (e.pointerType === "touch") {
      if (this.caught) return;
      if (now - this.lastTapAt < DOUBLE_TAP_MS && Math.hypot(p.x - this.lastTapX, p.y - this.lastTapY) < 30) {
        this.lastTapAt = 0;
        this.target.doubleTap(p.x, p.y);
        return;
      }
      this.lastTapAt = now;
      this.lastTapX = p.x;
      this.lastTapY = p.y;
      this.target.tap(p.x, p.y, false);
      return;
    }
    const pending = this.contextPending;
    if (pending) {
      this.dropPendingContext();
      this.openContext(pending.x, pending.y);
      return;
    }
    if (this.downButton === 0) this.target.tap(p.x, p.y, e.shiftKey || e.ctrlKey || e.metaKey);
  };

  private readonly onDblClick = (e: MouseEvent): void => {
    e.preventDefault();
    const p = this.local(e);
    this.target.doubleTap(p.x, p.y);
  };

  /**
   * A right click, or a press held still (a long press: AppKit turns one into
   * a right click on a Mac touch screen, in Safari and in Chrome alike). The
   * system menu never opens over a view; the view's own actions do.
   */
  private readonly onContextMenu = (e: MouseEvent): void => {
    e.preventDefault();
    const p = this.local(e);
    if (this.pointers.size === 1 && this.downButton === 2 && !this.moved) {
      // A right press: it may yet be a right drag, which pans. The menu waits
      // for the release, or for the press to stay put a moment (a long press
      // on a touch screen never sends a release).
      this.dropPendingContext();
      const timer = window.setTimeout(() => {
        const c = this.contextPending;
        this.contextPending = null;
        if (c && !this.moved) this.openContext(c.x, c.y);
      }, 400);
      this.contextPending = { x: p.x, y: p.y, timer };
      return;
    }
    this.openContext(p.x, p.y);
  };

  private dropPendingContext(): void {
    if (this.contextPending) window.clearTimeout(this.contextPending.timer);
    this.contextPending = null;
  }

  private openContext(x: number, y: number): void {
    const now = performance.now();
    // One press, one menu: a touch long press can also bring a contextmenu event.
    const c = this.contextAt;
    if (c && now - c.t < 600 && Math.hypot(x - c.x, y - c.y) < HOLD_DRAG_PX) return;
    this.contextAt = { x, y, t: now };
    this.target.context(x, y);
  }

  private cancelLongPress(): void {
    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }
  }

  // ------------------------------------------------------------- wheel

  private classify(e: WheelEvent): "pinch" | "mouse" | "trackpad" {
    if (e.ctrlKey) return "pinch";
    if (this.wheelMode === "orbit") return "trackpad";
    if (this.wheelMode === "zoom") return "mouse";
    const now = performance.now();
    const s = this.wheelStream;
    if (s && now - s.at < WHEEL_STREAM_MS) {
      s.at = now;
      if (s.kind === "mouse" && !notched(e)) s.kind = "trackpad";
      return s.kind;
    }
    const kind = notched(e) ? "mouse" : "trackpad";
    this.wheelStream = { kind, at: now };
    return kind;
  }

  /**
   * A two-finger touch in Chrome on a Mac arrives as ctrl + wheel events
   * whose sign flips back and forth; a pinch keeps its sign. True when the
   * recent ones flip more than they hold.
   */
  private pinchJitter(dy: number): boolean {
    const now = performance.now();
    const signs = this.pinchSigns;
    while (signs.length && now - (signs[0] as { at: number }).at > 300) signs.shift();
    if (dy !== 0) signs.push({ s: Math.sign(dy), at: now });
    if (signs.length > 8) signs.shift();
    if (signs.length < 4) return false;
    let flips = 0;
    for (let i = 1; i < signs.length; i++) if ((signs[i] as { s: number }).s !== (signs[i - 1] as { s: number }).s) flips++;
    return flips * 2 > signs.length - 1;
  }

  private readonly onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    this.target.stop();
    const p = this.local(e);
    const kind = this.classify(e);
    if (kind === "pinch") {
      // The same pinch as gesture events, if this browser sends both.
      if (this.gesture) return;
      if (this.pinchJitter(e.deltaY)) return;
      this.target.zoomAt(p.x, p.y, Math.exp(Math.max(-40, Math.min(40, e.deltaY)) * 0.01));
      return;
    }
    if (e.shiftKey && this.target.shiftScroll) {
      // Shift + two-finger scroll orbits: the deliberate gesture, kept on the
      // trackpad for anyone without a mouse button to hold.
      this.target.shiftScroll(-e.deltaX * 0.9, -e.deltaY * 0.9);
      return;
    }
    if (kind === "mouse") {
      const dy = e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY : e.deltaY * 40;
      this.target.zoomAt(p.x, p.y, Math.exp(Math.max(-60, Math.min(60, dy)) * 0.0022));
      return;
    }
    // A two-finger scroll pans. Chrome reports a shift-held vertical scroll
    // as deltaX, so both axes are passed through and the sign is the same
    // either way: the content follows the fingers.
    this.target.pan(-e.deltaX, -e.deltaY);
  };

  // --------------------------------------------------- Safari's gestures

  /**
   * Safari's gesture events: a trackpad pinch or twist (no button down), or
   * two fingers on a touch screen (with a one-button drag at their centroid,
   * which pans). Scale zooms about the gesture's point and rotation twists.
   * They are always taken, so the page never zooms under the view.
   */
  private readonly onGestureEvent = (ev: Event): void => {
    const e = ev as Event & { scale?: number; rotation?: number; clientX?: number; clientY?: number };
    e.preventDefault();
    const scale = typeof e.scale === "number" && e.scale > 0 ? e.scale : 1;
    const rot = typeof e.rotation === "number" ? e.rotation : 0;
    if (e.type === "gesturestart") {
      this.target.stop();
      const touch = this.pointers.size > 0 || performance.now() - this.pairCancelAt < PAIR_FOLLOW_MS;
      this.gesture = { scale, rot, rot0: rot, scale0: scale, engaged: false, touch };
      if (touch && this.pointers.size > 0) {
        // The centroid drag that is already running pans from here on.
        if (this.dragMode !== "pan") this.target.dragStart("pan");
        this.dragMode = "pan";
        this.moved = true;
        this.cancelLongPress();
      }
      if (this.pointers.size === 0) this.target.active(true);
      return;
    }
    const g = this.gesture;
    if (!g) return;
    if (e.type === "gestureend") {
      if (this.pointers.size === 0) this.target.active(false);
      this.gesture = null;
      return;
    }
    const p = this.local({ clientX: e.clientX ?? 0, clientY: e.clientY ?? 0 });
    if (scale !== g.scale) this.target.zoomAt(p.x, p.y, g.scale / scale);
    if (this.opts.twist && this.target.twistAt) {
      if (!g.engaged && twistEngages(rot - g.rot0, scale / g.scale0)) {
        g.engaged = true;
        g.rot = rot;
      }
      if (g.engaged && rot !== g.rot) this.target.twistAt(p.x, p.y, ((rot - g.rot) * Math.PI) / 180);
    }
    g.scale = scale;
    g.rot = rot;
  };
}

/**
 * A notched mouse wheel's event: line or page mode, or a whole number of
 * pixels with no sideways part that is a multiple of 40 (a line on macOS, in
 * Chromium and Safari) or at least 100 (Chromium on other systems). Never a
 * momentum event.
 */
export function notched(e: Pick<WheelEvent, "deltaMode" | "deltaX" | "deltaY">): boolean {
  if ((e as WheelEvent & { momentum?: boolean }).momentum) return false;
  // 0 is DOM_DELTA_PIXEL; 1 and 2 are lines and pages.
  if (e.deltaMode !== 0) return true;
  const dy = Math.abs(e.deltaY);
  if (e.deltaX !== 0 || dy === 0 || !Number.isInteger(dy)) return false;
  return dy >= 100 || dy % 40 === 0;
}

/**
 * The twist's dead zone: past `TWIST_ENGAGE_DEG`, and turning more than it
 * pinches (45 degrees weigh as much as a pinch of ln(scale) = 1), so a pinch
 * that wobbles never turns the view.
 */
export function twistEngages(deg: number, scale: number): boolean {
  const r = Math.abs(deg);
  return r >= TWIST_ENGAGE_DEG && r / TWIST_PER_LN_SCALE > Math.abs(Math.log(Math.max(1e-6, scale)));
}
