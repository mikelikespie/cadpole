// Camera controller with macOS Preview / Xcode-style navigation.
//
//   Trackpad  two-finger scroll PANS; pinch zooms toward the cursor;
//             shift + two-finger scroll orbits.
//   Mouse     left drag orbits; middle/right drag pans; shift + left drag
//             selects a box; wheel zooms toward the cursor.
//
// Two-finger pans rather than orbits because that is what a Mac trackpad means
// everywhere else: two fingers scroll the thing you are looking at. Orbiting on
// it made the model lurch whenever someone meant to scroll, which is the single
// most disorienting thing a viewport can do. Orbit is the click-drag, which is
// the deliberate gesture, and shift keeps it reachable from the trackpad alone.
//   Touch     one finger orbits; two fingers pinch-zoom and pan at once;
//             double-tap focuses; long-press asks for context info.
//
// Every channel (azimuth, polar, log-distance, target) is a critically-damped
// spring toward a goal, and drags carry inertia after release. With
// prefers-reduced-motion everything snaps.

import { Vector3, type PerspectiveCamera } from "three/webgpu";

export type WheelMode = "auto" | "orbit" | "zoom";

export interface Pose {
  theta: number;
  phi: number;
  distance: number;
  target: Vector3;
}

/** The controller's own channel goals, as `exactPose()` reports them. */
export interface ExactPose {
  theta: number;
  phi: number;
  logDist: number;
  tx: number;
  ty: number;
  tz: number;
}

export interface ControllerCallbacks {
  /** Something changed; a frame should be rendered. */
  onChange(): void;
  onClick(x: number, y: number, button: number, additive: boolean): void;
  onDoubleClick(x: number, y: number): void;
  onContext(x: number, y: number): void;
}

/** Unity-style critically damped smoothing. */
function smoothDamp(current: number, goal: number, vel: { v: number }, smoothTime: number, dt: number): number {
  if (smoothTime <= 0 || dt <= 0) {
    vel.v = 0;
    return goal;
  }
  const omega = 2 / smoothTime;
  const x = omega * dt;
  const exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
  const change = current - goal;
  const temp = (vel.v + omega * change) * dt;
  vel.v = (vel.v - omega * temp) * exp;
  let out = goal + (change + temp) * exp;
  if (goal - current > 0 === out > goal) {
    out = goal;
    vel.v = 0;
  }
  return out;
}

class Channel {
  value: number;
  goal: number;
  readonly vel = { v: 0 };
  /** Inertia carried after a release, in units/s. */
  drift = 0;

  constructor(v: number) {
    this.value = v;
    this.goal = v;
  }

  step(smoothTime: number, dt: number, driftTau: number): boolean {
    if (this.drift !== 0) {
      this.goal += this.drift * dt;
      this.drift *= Math.exp(-dt / driftTau);
      if (Math.abs(this.drift) < 1e-4) this.drift = 0;
    }
    this.value = smoothDamp(this.value, this.goal, this.vel, smoothTime, dt);
    return Math.abs(this.value - this.goal) > 1e-6 || this.drift !== 0;
  }

  snap(v: number): void {
    this.value = v;
    this.goal = v;
    this.vel.v = 0;
    this.drift = 0;
  }
}

const EPS_PHI = 0.0005;
const LONG_PRESS_MS = 500;
const DOUBLE_TAP_MS = 320;
/**
 * How far a pointer may travel and still be a click. A fingertip rolls as it
 * lifts, so a touch gets twice the mouse's allowance: an orbit that selects
 * is a nuisance, and so is a tap that orbits instead of selecting.
 */
const CLICK_SLOP_MOUSE = 5;
const CLICK_SLOP_TOUCH = 10;

export class CameraController {
  wheelMode: WheelMode = "auto";
  rotateSpeed = 1.0;
  private readonly theta: Channel;
  private readonly phi: Channel;
  private readonly logDist: Channel;
  private readonly tx: Channel;
  private readonly ty: Channel;
  private readonly tz: Channel;
  private smoothTime = 0.09;
  private readonly driftTau = 0.32;
  private disposed = false;
  private lastStep = 0;
  // Pointer state.
  private readonly pointers = new Map<number, { x: number; y: number; type: string }>();
  private dragMode: "none" | "orbit" | "pan" = "none";
  private lastX = 0;
  private lastY = 0;
  private downX = 0;
  private downY = 0;
  private downAt = 0;
  private downButton = -1;
  private moved = false;
  /** The press landed on a view still coasting: it stops the view, and is not a tap. */
  private caught = false;
  /** Pointers down, as the viewer reads it to draw lighter while a gesture runs. */
  onGesture: ((active: boolean) => void) | null = null;
  private velTheta = 0;
  private velPhi = 0;
  private velTx = 0;
  private velTy = 0;
  private velTz = 0;
  private lastMoveAt = 0;
  private pinchDist = 0;
  private pinchMidX = 0;
  private pinchMidY = 0;
  private longPressTimer: number | null = null;
  private lastTapAt = 0;
  private lastTapX = 0;
  private lastTapY = 0;
  private wheelSticky: "mouse" | "trackpad" | null = null;
  private wheelStickyAt = 0;
  /** Safari's trackpad pinch: the gesture's scale when it was last applied; 0 when none is running. */
  private gestureScale = 0;
  private readonly reduced: MediaQueryList;
  /**
   * Where the centre of projection is drawn, in CSS pixels from the middle
   * of the element: off the middle while the camera carries a view offset.
   */
  readonly centre = { x: 0, y: 0 };
  // Scratch vectors, reused to avoid allocating per event.
  private readonly right = new Vector3();
  private readonly up = new Vector3();
  private readonly tmp = new Vector3();

  constructor(
    readonly camera: PerspectiveCamera,
    readonly element: HTMLElement,
    private readonly callbacks: ControllerCallbacks,
  ) {
    this.theta = new Channel(Math.atan2(-0.95, 1));
    this.phi = new Channel(Math.acos(0.8 / Math.hypot(1, 0.95, 0.8)));
    this.logDist = new Channel(Math.log(250));
    this.tx = new Channel(0);
    this.ty = new Channel(0);
    this.tz = new Channel(10);
    this.reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    element.style.touchAction = "none";
    element.addEventListener("pointerdown", this.onPointerDown);
    element.addEventListener("pointermove", this.onPointerMove);
    element.addEventListener("pointerup", this.onPointerUp);
    element.addEventListener("pointercancel", this.onPointerUp);
    element.addEventListener("wheel", this.onWheel, { passive: false });
    for (const t of ["gesturestart", "gesturechange", "gestureend"]) element.addEventListener(t, this.onSafariGesture, { passive: false });
    element.addEventListener("dblclick", this.onDblClick);
    element.addEventListener("contextmenu", (e) => e.preventDefault());
    this.applyPose();
  }

  // ------------------------------------------------------------- pose

  get target(): Vector3 {
    return this.tmp.set(this.tx.goal, this.ty.goal, this.tz.goal);
  }

  get distance(): number {
    return Math.exp(this.logDist.goal);
  }

  /**
   * The orbit pose, in the terms `setPose` takes, so it can be stored and
   * restored exactly.
   *
   * `cameraState()` reports an eye and a target for a render request; this is
   * the controller's own state, and round-tripping through it loses nothing.
   */
  pose(): { theta: number; phi: number; distance: number; target: { x: number; y: number; z: number } } {
    return {
      theta: this.theta.goal,
      phi: this.phi.goal,
      distance: Math.exp(this.logDist.goal),
      target: { x: this.tx.goal, y: this.ty.goal, z: this.tz.goal },
    };
  }

  /**
   * The controller's own numbers, for putting the camera back bit for bit.
   *
   * `pose()` reports the distance as `exp(logDist)` and `setPose` takes its
   * `log` again, and that round trip moves the last bits: a view restored
   * through it is a hair off, which is a different picture. The plates view
   * promises the model view back exactly, so it keeps these.
   */
  exactPose(): ExactPose {
    return {
      theta: this.theta.goal,
      phi: this.phi.goal,
      logDist: this.logDist.goal,
      tx: this.tx.goal,
      ty: this.ty.goal,
      tz: this.tz.goal,
    };
  }

  /** Put an `exactPose()` back, with no animation. */
  restoreExact(p: ExactPose): void {
    this.theta.snap(p.theta);
    this.phi.snap(p.phi);
    this.logDist.snap(p.logDist);
    this.tx.snap(p.tx);
    this.ty.snap(p.ty);
    this.tz.snap(p.tz);
    this.callbacks.onChange();
  }

  /** The damped (currently displayed) orbit distance. */
  get currentDistance(): number {
    return Math.exp(this.logDist.value);
  }

  /** The damped (currently displayed) orbit target. */
  currentTarget(out: Vector3): Vector3 {
    return out.set(this.tx.value, this.ty.value, this.tz.value);
  }

  getPose(): Pose {
    return {
      theta: this.theta.goal,
      phi: this.phi.goal,
      distance: Math.exp(this.logDist.goal),
      target: new Vector3(this.tx.goal, this.ty.goal, this.tz.goal),
    };
  }

  /** Move to a pose; animated with a slightly longer, still critically damped, ease. */
  setPose(pose: Partial<Pose>, animate = true): void {
    const snap = !animate || this.reduced.matches;
    const set = (c: Channel, v: number | undefined): void => {
      if (v === undefined) return;
      if (snap) c.snap(v);
      else {
        c.goal = v;
        c.drift = 0;
      }
    };
    if (pose.theta !== undefined) {
      // Take the short way around.
      let t = pose.theta;
      const cur = this.theta.value;
      while (t - cur > Math.PI) t -= 2 * Math.PI;
      while (t - cur < -Math.PI) t += 2 * Math.PI;
      set(this.theta, t);
    }
    set(this.phi, pose.phi === undefined ? undefined : Math.min(Math.PI - EPS_PHI, Math.max(EPS_PHI, pose.phi)));
    set(this.logDist, pose.distance === undefined ? undefined : Math.log(Math.max(1e-3, pose.distance)));
    if (pose.target) {
      set(this.tx, pose.target.x);
      set(this.ty, pose.target.y);
      set(this.tz, pose.target.z);
    }
    if (!snap) this.smoothTime = 0.22;
    this.callbacks.onChange();
  }

  /** Re-centre the orbit on a world point, keeping direction and distance. */
  focus(point: Vector3): void {
    this.setPose({ target: point }, true);
  }

  /**
   * Dolly by `scale` (< 1 zooms in) keeping the point under (px, py) on the
   * target plane fixed. Works for perspective and orthographic projections
   * (both see the same lateral offset on the target plane).
   */
  zoomAt(px: number, py: number, scale: number): void {
    const rect = this.element.getBoundingClientRect();
    const w = Math.max(1, rect.width);
    const hgt = Math.max(1, rect.height);
    const nx = ((px - this.centre.x) / w) * 2 - 1;
    const ny = -((py - this.centre.y) / hgt) * 2 + 1;
    const d = Math.exp(this.logDist.goal);
    const half = d * Math.tan((this.camera.fov * Math.PI) / 360);
    const ox = nx * half * (w / hgt);
    const oy = ny * half;
    this.right.setFromMatrixColumn(this.camera.matrixWorld, 0);
    this.up.setFromMatrixColumn(this.camera.matrixWorld, 1);
    const k = 1 - scale;
    this.tx.goal += (this.right.x * ox + this.up.x * oy) * k;
    this.ty.goal += (this.right.y * ox + this.up.y * oy) * k;
    this.tz.goal += (this.right.z * ox + this.up.z * oy) * k;
    this.logDist.goal = Math.log(Math.max(1e-3, d * scale));
    this.smoothTime = 0.09;
    this.callbacks.onChange();
  }

  private orbitBy(dx: number, dy: number): void {
    const k = ((2 * Math.PI) / Math.max(1, this.element.clientHeight)) * this.rotateSpeed;
    this.theta.goal -= dx * k;
    this.phi.goal = Math.min(Math.PI - EPS_PHI, Math.max(EPS_PHI, this.phi.goal - dy * k));
    this.smoothTime = 0.09;
    this.callbacks.onChange();
  }

  private panBy(dx: number, dy: number): void {
    const d = Math.exp(this.logDist.goal);
    const k = (2 * d * Math.tan((this.camera.fov * Math.PI) / 360)) / Math.max(1, this.element.clientHeight);
    this.right.setFromMatrixColumn(this.camera.matrixWorld, 0);
    this.up.setFromMatrixColumn(this.camera.matrixWorld, 1);
    this.tx.goal += -this.right.x * dx * k + this.up.x * dy * k;
    this.ty.goal += -this.right.y * dx * k + this.up.y * dy * k;
    this.tz.goal += -this.right.z * dx * k + this.up.z * dy * k;
    this.smoothTime = 0.09;
    this.callbacks.onChange();
  }

  /** Advance the springs; returns true while still moving. */
  update(now: number): boolean {
    if (this.disposed) return false;
    const dt = this.lastStep ? Math.min(0.05, (now - this.lastStep) / 1000) : 0.016;
    this.lastStep = now;
    const st = this.reduced.matches ? 0 : this.smoothTime;
    let moving = false;
    moving = this.theta.step(st, dt, this.driftTau) || moving;
    moving = this.phi.step(st, dt, this.driftTau) || moving;
    this.phi.value = Math.min(Math.PI - EPS_PHI, Math.max(EPS_PHI, this.phi.value));
    moving = this.logDist.step(st, dt, this.driftTau) || moving;
    moving = this.tx.step(st, dt, this.driftTau) || moving;
    moving = this.ty.step(st, dt, this.driftTau) || moving;
    moving = this.tz.step(st, dt, this.driftTau) || moving;
    this.applyPose();
    if (!moving) this.lastStep = 0;
    return moving;
  }

  private applyPose(): void {
    const d = Math.exp(this.logDist.value);
    const sp = Math.sin(this.phi.value);
    this.camera.position.set(
      this.tx.value + d * sp * Math.cos(this.theta.value),
      this.ty.value + d * sp * Math.sin(this.theta.value),
      this.tz.value + d * Math.cos(this.phi.value),
    );
    this.tmp.set(this.tx.value, this.ty.value, this.tz.value);
    this.camera.up.set(0, 0, 1);
    this.camera.lookAt(this.tmp);
    this.camera.updateMatrixWorld();
  }

  // ---------------------------------------------------------- pointers

  /** The first two active pointers (only called when there are two). */
  private pair(): [{ x: number; y: number }, { x: number; y: number }] {
    const it = this.pointers.values();
    const a = it.next().value ?? { x: 0, y: 0 };
    const b = it.next().value ?? a;
    return [a, b];
  }

  private local(e: PointerEvent | MouseEvent): { x: number; y: number } {
    const r = this.element.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private readonly onPointerDown = (e: PointerEvent): void => {
    const p = this.local(e);
    this.pointers.set(e.pointerId, { x: p.x, y: p.y, type: e.pointerType });
    this.element.setPointerCapture(e.pointerId);
    if (this.pointers.size === 1) {
      this.caught = this.drifting();
      this.onGesture?.(true);
    }
    this.stopDrift();
    this.velTheta = this.velPhi = this.velTx = this.velTy = this.velTz = 0;
    if (this.pointers.size === 1) {
      this.downX = this.lastX = p.x;
      this.downY = this.lastY = p.y;
      this.downAt = performance.now();
      this.downButton = e.button;
      this.moved = false;
      if (e.pointerType === "touch") {
        this.dragMode = "orbit";
        this.longPressTimer = window.setTimeout(() => {
          this.longPressTimer = null;
          if (!this.moved && this.pointers.size === 1) {
            this.dragMode = "none";
            this.callbacks.onContext(p.x, p.y);
          }
        }, LONG_PRESS_MS);
      } else if (e.button === 0 && !e.shiftKey) this.dragMode = "orbit";
      else if (e.button === 0 || e.button === 1 || e.button === 2) this.dragMode = "pan";
    } else if (this.pointers.size === 2) {
      this.cancelLongPress();
      this.dragMode = "none";
      const [a, b] = this.pair();
      this.pinchDist = Math.hypot(a.x - b.x, a.y - b.y);
      this.pinchMidX = (a.x + b.x) / 2;
      this.pinchMidY = (a.y + b.y) / 2;
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
      if (this.pinchDist > 0 && dist > 0) this.zoomAt(midX, midY, this.pinchDist / dist);
      this.panBy(midX - this.pinchMidX, midY - this.pinchMidY);
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
    }
    if (!this.moved || this.dragMode === "none") return;
    const now = performance.now();
    const dt = Math.max(1, now - this.lastMoveAt) / 1000;
    this.lastMoveAt = now;
    const beforeTheta = this.theta.goal;
    const beforePhi = this.phi.goal;
    const bx = this.tx.goal;
    const by = this.ty.goal;
    const bz = this.tz.goal;
    if (this.dragMode === "orbit") this.orbitBy(dx, dy);
    else this.panBy(dx, dy);
    // Velocity estimate (blend) for release inertia.
    const blend = 0.5;
    this.velTheta = this.velTheta * (1 - blend) + ((this.theta.goal - beforeTheta) / dt) * blend;
    this.velPhi = this.velPhi * (1 - blend) + ((this.phi.goal - beforePhi) / dt) * blend;
    this.velTx = this.velTx * (1 - blend) + ((this.tx.goal - bx) / dt) * blend;
    this.velTy = this.velTy * (1 - blend) + ((this.ty.goal - by) / dt) * blend;
    this.velTz = this.velTz * (1 - blend) + ((this.tz.goal - bz) / dt) * blend;
    this.lastX = p.x;
    this.lastY = p.y;
  };

  private readonly onPointerUp = (e: PointerEvent): void => {
    if (!this.pointers.has(e.pointerId)) return;
    const p = this.local(e);
    this.pointers.delete(e.pointerId);
    this.cancelLongPress();
    if (this.element.hasPointerCapture(e.pointerId)) this.element.releasePointerCapture(e.pointerId);
    if (this.pointers.size > 0) {
      // Went from two fingers to one: restart a single-finger orbit cleanly.
      const rest = [...this.pointers.values()][0] as { x: number; y: number };
      this.lastX = rest.x;
      this.lastY = rest.y;
      this.dragMode = "orbit";
      this.moved = true;
      return;
    }
    this.onGesture?.(false);
    const now = performance.now();
    const wasDrag = this.moved;
    const mode = this.dragMode;
    this.dragMode = "none";
    if (wasDrag) {
      // Inertia: only if the pointer was still moving when released.
      if (!this.reduced.matches && now - this.lastMoveAt < 80) {
        if (mode === "orbit") {
          this.theta.drift = this.velTheta * 0.6;
          this.phi.drift = this.velPhi * 0.6;
        } else {
          this.tx.drift = this.velTx * 0.6;
          this.ty.drift = this.velTy * 0.6;
          this.tz.drift = this.velTz * 0.6;
        }
        this.callbacks.onChange();
      }
      return;
    }
    if (e.type === "pointercancel") return;
    if (now - this.downAt > 700) return;
    if (e.pointerType === "touch") {
      if (this.caught) return;
      if (now - this.lastTapAt < DOUBLE_TAP_MS && Math.hypot(p.x - this.lastTapX, p.y - this.lastTapY) < 30) {
        this.lastTapAt = 0;
        this.callbacks.onDoubleClick(p.x, p.y);
        return;
      }
      this.lastTapAt = now;
      this.lastTapX = p.x;
      this.lastTapY = p.y;
      this.callbacks.onClick(p.x, p.y, 0, false);
      return;
    }
    if (this.downButton === 2) this.callbacks.onContext(p.x, p.y);
    else if (this.downButton === 0) this.callbacks.onClick(p.x, p.y, 0, e.shiftKey || e.ctrlKey || e.metaKey);
  };

  private readonly onDblClick = (e: MouseEvent): void => {
    const p = this.local(e);
    this.callbacks.onDoubleClick(p.x, p.y);
  };

  private cancelLongPress(): void {
    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }
  }

  private drifting(): boolean {
    return [this.theta, this.phi, this.tx, this.ty, this.tz].some((c) => Math.abs(c.drift) > 1e-3);
  }

  private stopDrift(): void {
    for (const c of [this.theta, this.phi, this.logDist, this.tx, this.ty, this.tz]) c.drift = 0;
  }

  // ------------------------------------------------------------- wheel

  private classify(e: WheelEvent): "pinch" | "mouse" | "trackpad" {
    if (e.ctrlKey) return "pinch";
    if (this.wheelMode === "orbit") return "trackpad";
    if (this.wheelMode === "zoom") return "mouse";
    const now = performance.now();
    if (this.wheelSticky && now - this.wheelStickyAt < 400) {
      this.wheelStickyAt = now;
      return this.wheelSticky;
    }
    let kind: "mouse" | "trackpad" = "trackpad";
    const wdy = (e as WheelEvent & { wheelDeltaY?: number }).wheelDeltaY;
    if (e.deltaMode !== WheelEvent.DOM_DELTA_PIXEL) kind = "mouse";
    else if (typeof wdy === "number" && wdy !== 0 && wdy % 120 === 0 && e.deltaX === 0) kind = "mouse";
    else if (Math.abs(e.deltaY) >= 100 && e.deltaX === 0 && Number.isInteger(e.deltaY)) kind = "mouse";
    this.wheelSticky = kind;
    this.wheelStickyAt = now;
    return kind;
  }

  /**
   * A trackpad pinch in Safari arrives as gesture events (scale since the
   * gesture began) rather than as ctrl + wheel. On a touch screen the same
   * events accompany a two-finger touch, which the pointer handlers already
   * turn into a pinch, so they are ignored while a pointer is down.
   */
  private readonly onSafariGesture = (e: Event): void => {
    const g = e as Event & { scale?: number; clientX?: number; clientY?: number };
    if (this.pointers.size > 0) return;
    e.preventDefault();
    if (e.type === "gestureend") {
      if (this.gestureScale !== 0) this.onGesture?.(false);
      this.gestureScale = 0;
      return;
    }
    const scale = g.scale && g.scale > 0 ? g.scale : 1;
    if (e.type === "gesturestart" || this.gestureScale === 0) {
      this.gestureScale = scale;
      this.stopDrift();
      this.onGesture?.(true);
      return;
    }
    const p = this.local(g as unknown as MouseEvent);
    this.zoomAt(p.x, p.y, this.gestureScale / scale);
    this.gestureScale = scale;
  };

  private readonly onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    this.stopDrift();
    const p = this.local(e);
    const kind = this.classify(e);
    if (kind === "pinch") {
      // The same pinch, if this browser also reports it as gesture events.
      if (this.gestureScale !== 0) return;
      this.zoomAt(p.x, p.y, Math.exp(Math.max(-40, Math.min(40, e.deltaY)) * 0.01));
      return;
    }
    if (e.shiftKey) {
      // Shift + two-finger scroll orbits: the deliberate gesture, kept on the
      // trackpad for anyone without a mouse button to hold.
      this.orbitBy(-e.deltaX * 0.9, -e.deltaY * 0.9);
      return;
    }
    if (kind === "mouse") {
      const dy = e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY : e.deltaY * 40;
      this.zoomAt(p.x, p.y, Math.exp(Math.max(-60, Math.min(60, dy)) * 0.0022));
      return;
    }
    // Plain two-finger scroll pans. Chrome reports a shift-held horizontal
    // scroll as deltaX, so both axes are passed through and the sign is the
    // same either way: the content follows the fingers.
    this.panBy(-e.deltaX, -e.deltaY);
  };

  dispose(): void {
    this.disposed = true;
    this.cancelLongPress();
    this.element.removeEventListener("pointerdown", this.onPointerDown);
    this.element.removeEventListener("pointermove", this.onPointerMove);
    this.element.removeEventListener("pointerup", this.onPointerUp);
    this.element.removeEventListener("pointercancel", this.onPointerUp);
    this.element.removeEventListener("wheel", this.onWheel);
    for (const t of ["gesturestart", "gesturechange", "gestureend"]) this.element.removeEventListener(t, this.onSafariGesture);
    this.element.removeEventListener("dblclick", this.onDblClick);
  }
}
