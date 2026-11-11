// Camera controller with macOS Preview / Xcode-style navigation, the same
// on every surface that shows a model.
//
//   Trackpad  two-finger scroll PANS; pinch zooms toward the cursor (ctrl +
//             wheel in Chromium and Firefox, gesture events in Safari); a
//             two-finger twist in Safari turns the model; shift + two-finger
//             scroll orbits.
//   Mouse     left drag orbits; middle/right drag pans; shift + left drag
//             selects a box; a notched wheel zooms toward the cursor.
//   Touch     one finger orbits; two fingers pinch-zoom, pan and twist at
//             once; double-tap focuses; a long press asks for context info.
//
// Two-finger pans rather than orbits because that is what a Mac trackpad means
// everywhere else: two fingers scroll the thing you are looking at. Orbiting on
// it made the model lurch whenever someone meant to scroll, which is the single
// most disorienting thing a viewport can do. Orbit is the click-drag, which is
// the deliberate gesture, and shift keeps it reachable from the trackpad alone.
//
// Which input is which -- a touch screen on a Mac in Safari or in Chrome, a
// notched wheel or a trackpad -- is decided in one place, input/router.ts;
// this file is what each intent does to the camera.
//
// Every channel (azimuth, polar, log-distance, target) is a critically-damped
// spring toward a goal, and drags carry inertia after release. With
// prefers-reduced-motion everything snaps.

import { Vector3, type PerspectiveCamera } from "three/webgpu";
import { InputRouter, type GestureTarget, type WheelMode } from "./input/router.js";

export type { WheelMode } from "./input/router.js";

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
  /** A context request was the start of a drag (a press held, then moved): take it back. */
  onContextDismiss?(): void;
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
/** The orbit target stays within this many radii of the framed model's centre. */
const PAN_LIMIT = 1.5;
/** The camera stays between these multiples of the framed model's radius. */
const DIST_MIN = 0.01;
const DIST_MAX = 60;

export class CameraController {
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
  /** Pointers down, as the viewer reads it to draw lighter while a gesture runs. */
  onGesture: ((active: boolean) => void) | null = null;
  // Release inertia: the drag's velocity, blended over its last moves.
  private velTheta = 0;
  private velPhi = 0;
  private velTx = 0;
  private velTy = 0;
  private velTz = 0;
  private lastMoveAt = 0;
  /** The framed model, for keeping the camera on it: centre and radius. */
  private bounds: { x: number; y: number; z: number; r: number } | null = null;
  private readonly reduced: MediaQueryList;
  private readonly input: InputRouter;
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
    this.input = new InputRouter(element, this.gestures(), { twist: true });
    this.applyPose();
  }

  /** What the wheel does: classified per stream ("auto"), or always one thing. */
  get wheelMode(): WheelMode {
    return this.input.wheelMode;
  }

  set wheelMode(m: WheelMode) {
    this.input.wheelMode = m;
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

  /**
   * The framed model's bounding sphere. Panning and zooming keep the camera
   * on it: the target stays within `PAN_LIMIT` radii of the centre and the
   * distance within `DIST_MIN`..`DIST_MAX` radii, so no run of gestures can
   * lose the model. `null` lifts the limits.
   */
  setBounds(centre: { x: number; y: number; z: number } | null, radius = 0): void {
    this.bounds = centre && radius > 0 ? { x: centre.x, y: centre.y, z: centre.z, r: radius } : null;
  }

  private clampGoal(): void {
    const b = this.bounds;
    if (!b) return;
    const dx = this.tx.goal - b.x;
    const dy = this.ty.goal - b.y;
    const dz = this.tz.goal - b.z;
    const len = Math.hypot(dx, dy, dz);
    const max = b.r * PAN_LIMIT;
    if (len > max) {
      const k = max / len;
      this.tx.goal = b.x + dx * k;
      this.ty.goal = b.y + dy * k;
      this.tz.goal = b.z + dz * k;
      for (const c of [this.tx, this.ty, this.tz]) c.drift = 0;
    }
    const lo = Math.log(b.r * DIST_MIN);
    const hi = Math.log(b.r * DIST_MAX);
    if (this.logDist.goal < lo) this.logDist.goal = lo;
    if (this.logDist.goal > hi) this.logDist.goal = hi;
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
    this.clampGoal();
    this.smoothTime = 0.09;
    this.callbacks.onChange();
  }

  /**
   * A twist of `rad` (clockwise on screen) about the point under (px, py):
   * the model turns about the vertical through that point, so the camera's
   * up stays up and the point stays under the fingers. Seen from above this
   * is exactly turning a map.
   */
  twistAt(px: number, py: number, rad: number): void {
    const rect = this.element.getBoundingClientRect();
    const w = Math.max(1, rect.width);
    const hgt = Math.max(1, rect.height);
    // Where the ray under (px, py) meets the horizontal plane through the target.
    this.tmp.set(((px - this.centre.x) / w) * 2 - 1, -((py - this.centre.y) / hgt) * 2 + 1, 0.5).unproject(this.camera);
    const o = this.camera.position;
    const dir = this.right.copy(this.tmp).sub(o);
    let pxw = this.tx.goal;
    let pyw = this.ty.goal;
    if (Math.abs(dir.z) > 1e-9) {
      const t = (this.tz.goal - o.z) / dir.z;
      if (t > 0 && Number.isFinite(t)) {
        pxw = o.x + dir.x * t;
        pyw = o.y + dir.y * t;
      }
    }
    const c = Math.cos(rad);
    const s = Math.sin(rad);
    const ox = this.tx.goal - pxw;
    const oy = this.ty.goal - pyw;
    this.tx.goal = pxw + ox * c - oy * s;
    this.ty.goal = pyw + ox * s + oy * c;
    this.theta.goal += rad;
    this.clampGoal();
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
    this.clampGoal();
    this.smoothTime = 0.09;
    this.callbacks.onChange();
  }

  /** Advance the springs; returns true while still moving. */
  update(now: number): boolean {
    if (this.disposed) return false;
    const dt = this.lastStep ? Math.min(0.05, (now - this.lastStep) / 1000) : 0.016;
    this.lastStep = now;
    const st = this.reduced.matches ? 0 : this.smoothTime;
    if (this.tx.drift !== 0 || this.ty.drift !== 0 || this.tz.drift !== 0 || this.logDist.drift !== 0) this.clampGoal();
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

  // ----------------------------------------------------------- intents

  /** The camera's answer to each input intent (input/router.ts). */
  private gestures(): GestureTarget {
    // Velocity estimate (blend) for release inertia, from the goals' change per move.
    const track = (move: () => void): void => {
      const now = performance.now();
      const dt = Math.max(1, now - this.lastMoveAt) / 1000;
      this.lastMoveAt = now;
      const t0 = this.theta.goal;
      const p0 = this.phi.goal;
      const x0 = this.tx.goal;
      const y0 = this.ty.goal;
      const z0 = this.tz.goal;
      move();
      const blend = 0.5;
      this.velTheta = this.velTheta * (1 - blend) + ((this.theta.goal - t0) / dt) * blend;
      this.velPhi = this.velPhi * (1 - blend) + ((this.phi.goal - p0) / dt) * blend;
      this.velTx = this.velTx * (1 - blend) + ((this.tx.goal - x0) / dt) * blend;
      this.velTy = this.velTy * (1 - blend) + ((this.ty.goal - y0) / dt) * blend;
      this.velTz = this.velTz * (1 - blend) + ((this.tz.goal - z0) / dt) * blend;
    };
    return {
      dragStart: () => {
        this.velTheta = this.velPhi = this.velTx = this.velTy = this.velTz = 0;
        this.lastMoveAt = performance.now();
      },
      primary: (dx, dy) => track(() => this.orbitBy(dx, dy)),
      pan: (dx, dy) => track(() => this.panBy(dx, dy)),
      zoomAt: (x, y, scale) => this.zoomAt(x, y, scale),
      twistAt: (x, y, rad) => this.twistAt(x, y, rad),
      shiftScroll: (dx, dy) => this.orbitBy(dx, dy),
      dragEnd: (mode, moving) => {
        // Inertia: only if the pointer was still moving when released.
        if (!moving || this.reduced.matches) return;
        if (mode === "primary") {
          this.theta.drift = this.velTheta * 0.6;
          this.phi.drift = this.velPhi * 0.6;
        } else {
          this.tx.drift = this.velTx * 0.6;
          this.ty.drift = this.velTy * 0.6;
          this.tz.drift = this.velTz * 0.6;
        }
        this.callbacks.onChange();
      },
      active: (on) => this.onGesture?.(on),
      coasting: () => [this.theta, this.phi, this.tx, this.ty, this.tz].some((c) => Math.abs(c.drift) > 1e-3),
      stop: () => {
        for (const c of [this.theta, this.phi, this.logDist, this.tx, this.ty, this.tz]) c.drift = 0;
      },
      tap: (x, y, additive) => this.callbacks.onClick(x, y, 0, additive),
      doubleTap: (x, y) => this.callbacks.onDoubleClick(x, y),
      context: (x, y) => this.callbacks.onContext(x, y),
      contextDismiss: () => this.callbacks.onContextDismiss?.(),
    };
  }

  dispose(): void {
    this.disposed = true;
    this.input.dispose();
  }
}
