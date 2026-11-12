// The project site's interactive 3D (the `site` package at the repository
// root): one engine worker and one WebGL context for the whole page.
//
// Reused from the web app: `WorkerEngine` + `engine-worker.js` (the wasm
// engine off the main thread, meshes moved as transferables), the studio
// lighting (studio.ts) and the camera. The drawing
// side is a slim renderer rather than the app's `Viewer`: that class owns a
// canvas and a WebGPU renderer per instance and carries the app's picking,
// grid, labels and sections, and a page that shows 3D in several sections
// needs exactly one context that moves between them. The camera is the app's
// own `CameraController` (controls.ts), so orbit, pan, pinch, zoom to the
// cursor and inertia are the same code as the full viewer's.

import {
  Box3,
  BufferAttribute,
  Color,
  BufferGeometry,
  DataTexture,
  DataUtils,
  DirectionalLight,
  DoubleSide,
  EdgesGeometry,
  EquirectangularReflectionMapping,
  Group,
  HalfFloatType,
  LinearFilter,
  LinearSRGBColorSpace,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshStandardMaterial,
  NeutralToneMapping,
  PerspectiveCamera,
  Raycaster,
  RGBAFormat,
  Scene,
  Sphere,
  Vector2,
  Vector3,
  WebGLRenderer,
} from "three";

import { inflate } from "../baked.js";
import { CameraController } from "../controls.js";
import type { Evaluated, MeshData } from "../engine.js";
import { STUDIO, STUDIO_AZIMUTH, STUDIO_SURROUND_SIZE, studioSurround } from "../studio.js";
import type { Model, ParamValue, ScriptFile } from "../types.js";
import { WorkerEngine } from "../worker-engine.js";

export interface View {
  /** Degrees from the front toward the right, as `cadpole render --view az=`. */
  az: number;
  /** Degrees above the horizon. */
  el: number;
  fov?: number;
  /** Fraction of the fitted distance; 1 fits the model. */
  zoom?: number;
}

const RAD = Math.PI / 180;

/** The full viewer's surround (`studioSurround`), for this renderer. */
function studioEnvironment(): DataTexture {
  const { width, height } = STUDIO_SURROUND_SIZE;
  const tex = new DataTexture(studioSurround((v) => DataUtils.toHalfFloat(v)), width, height, RGBAFormat, HalfFloatType);
  tex.mapping = EquirectangularReflectionMapping;
  tex.colorSpace = LinearSRGBColorSpace;
  tex.minFilter = LinearFilter;
  tex.magFilter = LinearFilter;
  tex.needsUpdate = true;
  return tex;
}

class Stage {
  readonly canvas = document.createElement("canvas");
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  private readonly camera = new PerspectiveCamera(24, 1, 0.1, 10000);
  private readonly root = new Group();
  private readonly rig = new Group();
  private readonly tmpV = new Vector3();
  private readonly ro = new ResizeObserver(() => this.resize());
  private readonly bodyMat = new MeshStandardMaterial({ color: 0xb9c4c1, roughness: 0.5, metalness: 0.1, side: DoubleSide, polygonOffset: true, polygonOffsetFactor: 0, polygonOffsetUnits: 2 });
  private readonly edgeMat = new LineBasicMaterial({ color: 0x0b1211, transparent: true, opacity: 0.55 });
  private readonly ray = new Raycaster();
  /** The web app's own camera controller (controls.ts): same springs, inertia and gestures. */
  private readonly controls: CameraController;
  private owned: { dispose(): void }[] = [];
  private pending = 0;
  private fit = 100;
  private radius = 50;
  private readonly centre = new Vector3();
  private view: View = { az: -30, el: 20 };
  host: HTMLElement | null = null;
  /** True once the visitor asked for full control: wheel zooms and pans, touch moves freely. */
  grabbed = false;
  /** A mouse click on the model hands it the wheel until the pointer leaves it. */
  private armed = false;
  private hintEl: HTMLElement | null = null;
  private hintTimer = 0;
  /** Whose model is drawn: the key its caller gave `setModel` (the playground's tab). */
  modelKey = "";
  triangles = 0;
  lost = false;
  /** Frames drawn, and the camera each time: what the page's checks read. */
  frames = 0;

  constructor() {
    const c = this.canvas;
    c.className = "cp3d-canvas";
    c.tabIndex = 0;
    c.setAttribute("role", "img");
    c.setAttribute("aria-label", "Interactive 3D model. Drag to orbit, arrow keys to turn, plus and minus to zoom.");
    this.renderer = new WebGLRenderer({ canvas: c, antialias: true, alpha: true, powerPreference: "high-performance" });
    // As the web app's viewer: device pixels up to 2x, at rest and in motion.
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.toneMapping = NeutralToneMapping;
    this.renderer.toneMappingExposure = STUDIO.exposure;
    this.renderer.setClearColor(0x000000, 0);
    this.scene.environment = studioEnvironment();
    this.scene.environmentRotation.set(-Math.PI / 2, 0, 0);
    for (const l of STUDIO.lights) {
      const light = new DirectionalLight(new Color().setRGB(l.color[0], l.color[1], l.color[2]), l.intensity);
      light.position.set(l.dir[0], l.dir[1], l.dir[2]);
      this.rig.add(light);
    }
    this.camera.up.set(0, 0, 1);
    this.scene.add(this.root, this.rig);

    // The browser took the context: this stage draws nothing more (the next
    // one asked for is a new stage), so its canvas leaves the page and the
    // section that was showing it is told, to show its picture or ask again.
    c.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      this.lost = true;
      const host = this.host;
      this.unmount();
      host?.dispatchEvent(new CustomEvent("cp3d-lost"));
    });
    // Who gets the wheel. These run before the controller's own listeners
    // (same element, added first).
    //  - A view the visitor opened (the lightbox, a scene's "Open in 3D", the
    //    playground after "Zoom and pan") is `grabbed` and takes the wheel:
    //    a two-finger scroll (or a finger on a touch screen in Chrome, which
    //    arrives the same way) moves the model, a notched mouse wheel zooms
    //    about the pointer (input/router.ts tells them apart).
    //  - A view that is part of the scrolling page (the playground before it is
    //    taken) leaves the wheel to the page until a mouse click on the model
    //    arms it; it stays armed until the pointer leaves the model. Meanwhile
    //    a wheel over it scrolls the page and shows a one-line hint.
    //  - A pinch (ctrl + wheel in Chromium and Firefox, gesture events in
    //    Safari) is always the model's: the page itself never zooms.
    c.addEventListener(
      "wheel",
      (e) => {
        if (!this.grabbed && !this.armed && !e.ctrlKey) {
          e.stopImmediatePropagation();
          this.hint();
        }
      },
      { passive: false },
    );
    c.addEventListener("pointerdown", (e) => {
      if (e.pointerType === "mouse") this.armed = true;
    });
    c.addEventListener("pointerleave", (e) => {
      if (e.pointerType === "mouse") this.armed = false;
    });
    this.controls = new CameraController(this.camera as never, c, {
      onChange: () => this.requestRender(),
      onClick: () => {},
      onDoubleClick: (x, y) => this.focusAt(x, y),
      onContext: () => {},
    });
    c.addEventListener("keydown", (e) => {
      const k = e.key;
      const p = this.controls.pose();
      const rect = c.getBoundingClientRect();
      if (k === "ArrowLeft") this.controls.setPose({ theta: p.theta + 8 * RAD });
      else if (k === "ArrowRight") this.controls.setPose({ theta: p.theta - 8 * RAD });
      else if (k === "ArrowUp") this.controls.setPose({ phi: p.phi - 6 * RAD });
      else if (k === "ArrowDown") this.controls.setPose({ phi: p.phi + 6 * RAD });
      else if (k === "+" || k === "=") this.controls.zoomAt(rect.width / 2, rect.height / 2, 0.88);
      else if (k === "-") this.controls.zoomAt(rect.width / 2, rect.height / 2, 1.14);
      else if (k === "0" || k === "f") this.frame(true);
      else if (k === "Escape") {
        this.host?.dispatchEvent(new CustomEvent("cp3d-exit"));
        return;
      } else return;
      e.preventDefault();
    });
  }

  /** A wheel the page kept: say once, briefly, how to give it to the model. */
  private hint(): void {
    const h = this.host;
    if (!h) return;
    if (!this.hintEl) {
      this.hintEl = document.createElement("div");
      this.hintEl.className = "cp3d-hint";
      this.hintEl.setAttribute("aria-hidden", "true");
      this.hintEl.textContent = "Click the model to give it the wheel";
    }
    if (this.hintEl.parentElement !== h) h.appendChild(this.hintEl);
    this.hintEl.classList.add("on");
    clearTimeout(this.hintTimer);
    this.hintTimer = window.setTimeout(() => this.hintEl?.classList.remove("on"), 1600);
  }

  /** Double-click or double-tap: orbit about the point under it, or fit the model again. */
  private focusAt(x: number, y: number): void {
    const rect = this.canvas.getBoundingClientRect();
    this.ray.setFromCamera(new Vector2((x / Math.max(1, rect.width)) * 2 - 1, -(y / Math.max(1, rect.height)) * 2 + 1), this.camera);
    const hit = this.ray.intersectObject(this.root, true).find((h) => (h.object as Mesh).isMesh);
    if (hit) this.controls.focus(hit.point.clone());
    else this.frame(true);
  }

  /** Fit the model from the section's own direction. */
  private frame(animate: boolean): void {
    const v = this.view;
    this.controls.setBounds(this.centre, this.radius);
    this.controls.setPose(
      { theta: (v.az - 90) * RAD, phi: (90 - v.el) * RAD, distance: this.fit * (v.zoom ?? 1), target: this.centre.clone() as never },
      animate,
    );
  }

  /**
   * The camera, in the terms the web app's viewer stores its own
   * (`CameraController.pose()`), for opening the full viewer on the same view.
   * The viewer's lens is 40 degrees; the distance is scaled so the model fills
   * the same share of the window.
   */
  viewerPose(): { theta: number; phi: number; distance: number; target: { x: number; y: number; z: number } } {
    const p = this.controls.pose();
    const k = Math.tan((this.camera.fov * RAD) / 2) / Math.tan((40 * RAD) / 2);
    return { ...p, distance: p.distance * k };
  }

  /**
   * The camera as the full viewer's address takes it (`?eye=x,y,z&target=x,y,z`, mm):
   * the eye moved along the line of sight so the viewer's 40 degree lens frames what
   * this one's does.
   */
  eyeTarget(): { eye: [number, number, number]; target: [number, number, number] } {
    const t = this.controls.pose().target;
    const k = Math.tan((this.camera.fov * RAD) / 2) / Math.tan((40 * RAD) / 2);
    const p = this.camera.position;
    return { eye: [t.x + (p.x - t.x) * k, t.y + (p.y - t.y) * k, t.z + (p.z - t.z) * k], target: [t.x, t.y, t.z] };
  }

  /** The displayed camera, for checks: eye position and distance. */
  cameraNow(): { x: number; y: number; z: number; d: number; frames: number } {
    const p = this.camera.position;
    return { x: p.x, y: p.y, z: p.z, d: this.controls.currentDistance, frames: this.frames };
  }

  mount(host: HTMLElement, grabbed: boolean): void {
    if (this.host !== host) {
      this.host?.dispatchEvent(new CustomEvent("cp3d-unmount"));
      this.ro.disconnect();
      this.host = host;
      this.armed = false;
      host.appendChild(this.canvas);
      this.ro.observe(host);
    }
    this.grabbed = grabbed;
    // Not taken: an up-and-down swipe still scrolls the page, sideways orbits.
    this.canvas.style.touchAction = grabbed ? "none" : "pan-y";
    this.resize();
  }

  /**
   * Take the canvas out of the page. The section it leaves hears
   * `cp3d-unmount` on its host, here and when `mount` moves the canvas to
   * another host: there is one stage for the page, and a section that still
   * says it is live after losing it would show an empty box.
   */
  unmount(): void {
    const host = this.host;
    this.ro.disconnect();
    this.canvas.remove();
    this.host = null;
    this.clear();
    host?.dispatchEvent(new CustomEvent("cp3d-unmount"));
  }

  private resize(): void {
    const h = this.host;
    if (!h) return;
    const w = Math.max(1, h.clientWidth);
    const hh = Math.max(1, h.clientHeight);
    this.renderer.setSize(w, hh, false);
    this.camera.aspect = w / hh;
    this.camera.updateProjectionMatrix();
    this.draw();
  }

  private clear(): void {
    this.root.clear();
    for (const o of this.owned) o.dispose();
    this.owned = [];
    this.triangles = 0;
    this.modelKey = "";
  }

  /** Take the model away and draw the empty view, keeping the canvas where it is. */
  blank(): void {
    this.clear();
    this.draw();
  }

  /**
   * Show a model. With a `view` it is a new model, framed from that
   * direction. Without one it replaces the model on show (a slider or an edit
   * of the same script): the camera stays where it is.
   */
  setModel(ev: { model: Model; meshes: MeshData[] }, view: View | null, key = ""): void {
    this.clear();
    this.modelKey = key;
    const geoms = new Map<number, BufferGeometry>();
    const geom = (i: number): BufferGeometry | null => {
      const have = geoms.get(i);
      if (have) return have;
      const m = ev.meshes[i];
      if (!m || m.indices.length === 0) return null;
      const g = new BufferGeometry();
      g.setAttribute("position", new BufferAttribute(m.positions, 3));
      g.setAttribute("normal", new BufferAttribute(m.normals, 3));
      g.setIndex(new BufferAttribute(m.indices, 1));
      geoms.set(i, g);
      this.owned.push(g);
      return g;
    };
    let tris = 0;
    for (const b of ev.model.bodies) tris += b.visible ? (ev.meshes[b.mesh_index]?.indices.length ?? 0) / 3 : 0;
    const edges = tris < 120_000;
    const edgeGeoms = new Map<number, EdgesGeometry>();
    for (const b of ev.model.bodies) {
      if (!b.visible) continue;
      const g = geom(b.mesh_index);
      if (!g) continue;
      let mat = this.bodyMat;
      const bm = b.material;
      if (bm) {
        mat = this.bodyMat.clone();
        this.owned.push(mat);
        if (bm.color) mat.color.setRGB(bm.color[0], bm.color[1], bm.color[2]);
        if (bm.roughness !== null) mat.roughness = bm.roughness;
        if (bm.metallic !== null) mat.metalness = bm.metallic;
        const op = bm.opacity === null ? 1 : bm.opacity;
        if (op < 0.999) {
          mat.transparent = true;
          mat.opacity = Math.max(0.05, op);
          mat.depthWrite = false;
        }
      }
      const mesh = new Mesh(g, mat);
      const t = b.transform;
      if (t) {
        mesh.position.set(t.translation.x, t.translation.y, t.translation.z);
        mesh.quaternion.set(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w);
      }
      if (edges) {
        let eg = edgeGeoms.get(b.mesh_index);
        if (!eg) {
          eg = new EdgesGeometry(g, 30);
          edgeGeoms.set(b.mesh_index, eg);
          this.owned.push(eg);
        }
        const lines = new LineSegments(eg, this.edgeMat);
        lines.raycast = () => {};
        mesh.add(lines);
      }
      this.root.add(mesh);
    }
    this.triangles = tris;
    this.root.updateMatrixWorld(true);
    const box = new Box3().setFromObject(this.root);
    if (!box.isEmpty()) {
      const s = box.getBoundingSphere(new Sphere());
      if (view) {
        this.view = view;
        this.camera.fov = view.fov ?? 24;
      }
      this.centre.copy(s.center);
      this.radius = s.radius;
      const vfov = this.camera.fov * RAD;
      const hfov = 2 * Math.atan(Math.tan(vfov / 2) * Math.min(1, this.camera.aspect));
      this.fit = (s.radius / Math.sin(Math.min(vfov, hfov) / 2)) * 1.02;
      this.camera.updateProjectionMatrix();
      // A new model is framed. A changed model keeps the camera, so a slider
      // or an edit does not make the view jump; "0" or a double-click on
      // empty space frames it again.
      if (view) this.frame(false);
    }
    this.draw();
  }

  requestRender(): void {
    if (this.pending) return;
    this.pending = requestAnimationFrame((now) => {
      this.pending = 0;
      const moving = this.controls.update(now);
      this.draw(true);
      if (moving) this.requestRender();
    });
  }

  private draw(stepped = false): void {
    if (!this.host || this.lost) return;
    if (!stepped) this.controls.update(performance.now());
    const d = this.controls.currentDistance;
    const cam = this.camera;
    const toCentre = cam.position.distanceTo(this.centre);
    const far = toCentre + this.radius * 2;
    // Outside the model the near plane sits half way to its bounding sphere: the depth buffer's
    // precision goes to the model instead of the empty air in front of it, which is what keeps
    // faces a few hundredths of a millimetre apart from fighting on a 24-bit phone or laptop GPU.
    cam.near = Math.max(0.01, Math.min(this.radius / 100, d / 50), (toCentre - this.radius) * 0.5);
    cam.far = Math.max(far, d * 2) * 1.5;
    cam.updateProjectionMatrix();
    // The lights turn with the camera, as the viewer's do (`aimRig`).
    cam.getWorldDirection(this.tmpV);
    if (Math.hypot(this.tmpV.x, this.tmpV.y) < 1e-3) this.tmpV.set(0, 0, 1).applyQuaternion(cam.quaternion).negate();
    this.rig.rotation.z = Math.atan2(-this.tmpV.y, -this.tmpV.x) - STUDIO_AZIMUTH;
    this.renderer.render(this.scene, cam);
    this.frames++;
  }
}

let stage: Stage | null = null;

/** Whether this browser can draw at all; the page keeps its posters if not. */
export function supported(): boolean {
  try {
    const c = document.createElement("canvas");
    return !!(c.getContext("webgl2") || c.getContext("webgl"));
  } catch {
    return false;
  }
}

function theStage(): Stage {
  if (!stage || stage.lost) stage = new Stage();
  return stage;
}

let enginePromise: Promise<WorkerEngine> | null = null;

/** What starting the engine reported, in order; the page prints it with ?debug=1. */
export const engineLog: string[] = [];
export interface EngineProgress {
  loaded: number;
  total: number;
}
const progressListeners = new Set<(p: EngineProgress) => void>();
const logListeners = new Set<(line: string) => void>();
const t0 = performance.now();
function note(line: string): void {
  const l = `${((performance.now() - t0) / 1000).toFixed(2)}s ${line}`;
  engineLog.push(l);
  for (const f of logListeners) f(l);
}
export function onEngineLog(f: (line: string) => void): void {
  logListeners.add(f);
}
const mb = (n: number): string => `${(n / 1048576).toFixed(1)} MB`;

/** Content-Security-Policy refusals seen on this page, named: directive and what was blocked. */
const cspSeen: string[] = [];
document.addEventListener("securitypolicyviolation", (e) => {
  const what = `${e.effectiveDirective || e.violatedDirective} blocked ${e.blockedURI || "inline code"}`;
  cspSeen.push(what);
  note(`Content-Security-Policy: ${what} (policy: ${e.originalPolicy})`);
});

let channel: BroadcastChannel | null = null;
let memoryWaiters: ((bytes: number) => void)[] = [];
/** The engine's wasm memory right now, bytes. It only grows, so this is also its peak. */
export function engineMemory(): Promise<number> {
  return new Promise((resolve) => {
    if (!channel) return resolve(0);
    memoryWaiters.push(resolve);
    channel.postMessage({ ask: "memory" });
    setTimeout(() => resolve(0), 2000);
  });
}

/** A browser that cannot run the engine, in plain words; null when it can try. */
function cannotRun(): string | null {
  if (typeof WebAssembly !== "object" || typeof WebAssembly.instantiate !== "function") {
    return "WebAssembly is switched off in this browser. On an iPhone or iPad, Lockdown Mode does that.";
  }
  if (typeof Worker === "undefined") return "This browser has no web workers.";
  try {
    // The smallest module there is. A page served under a Content-Security-Policy
    // without 'wasm-unsafe-eval' refuses it here, before 9 MB is downloaded.
    new WebAssembly.Module(Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0));
  } catch (e) {
    const text = e instanceof Error ? e.message : String(e);
    note(`WebAssembly refused a trivial module: ${text}`);
    if (cspSeen.length > 0 || /Content Security Policy|unsafe-eval|CSP/i.test(text)) {
      return "This address is served with a Content-Security-Policy that blocks WebAssembly (script-src lacks 'wasm-unsafe-eval'), so the engine cannot run from here. It is the server's setting, not your device.";
    }
    return `This browser refused to compile WebAssembly: ${text}`;
  }
  // The engine is built with fixed-width SIMD (Safari 16.4, Chrome 91, Firefox 89).
  const simd = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11);
  if (!WebAssembly.validate(simd)) return "This browser is too old for the engine: it needs WebAssembly SIMD (iOS 16.4 or later, Chrome 91, Firefox 89).";
  return null;
}

/** Why the engine did not start, for a person. */
export function explainEngineFailure(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e);
  const blocked = cspSeen.find((c) => /worker-src|script-src/.test(c));
  if (/Content Security Policy|Content-Security-Policy|unsafe-eval/i.test(text) || blocked) {
    return `This address is served with a Content-Security-Policy that blocks the engine (${blocked ?? "WebAssembly compilation"}). It is the server's setting, not your device.`;
  }
  if (/out of memory|allocation failed|could not allocate/i.test(text)) return "This device ran out of memory while loading the engine. Closing other tabs usually frees enough.";
  if (/did not start within/.test(text)) return "The engine took too long to download on this connection. Try again on Wi-Fi.";
  if (/HTTP \d+|Failed to fetch|Load failed|NetworkError|Importing a module script failed/i.test(text)) return `The engine could not be downloaded (${text}). Check the connection and try again.`;
  return text;
}

/**
 * The one engine. Starting it fetches and compiles the wasm, off this thread;
 * `onProgress` hears how much of the download has arrived.
 */
export function engine(onProgress?: (p: EngineProgress) => void): Promise<WorkerEngine> {
  if (onProgress) progressListeners.add(onProgress);
  if (!enginePromise) {
    const size = Number(document.documentElement.dataset["engineBytes"]) || 0;
    const url = new URL(`./engine/site-engine.js?n=${size}`, import.meta.url).href;
    note(`starting the engine: ${navigator.userAgent}`);
    note(`device memory ${(navigator as { deviceMemory?: number }).deviceMemory ?? "unknown"} GB, ${navigator.hardwareConcurrency ?? "?"} cores, secure context ${window.isSecureContext}`);
    const no = cannotRun();
    if (no) {
      note(`cannot run: ${no}`);
      enginePromise = Promise.reject(new Error(no));
    } else {
      if (!channel && typeof BroadcastChannel === "function") {
        channel = new BroadcastChannel("cadpole-site-engine");
        let shown = -1;
        channel.onmessage = (ev) => {
          const m = ev.data as { stage?: string; loaded?: number; total?: number; memory?: number; ask?: string };
          if (m.ask) return;
          if (m.stage) note(`worker: ${m.stage}${m.memory ? ` (wasm memory ${mb(m.memory)})` : ""}`);
          else if (typeof m.memory === "number") {
            for (const w of memoryWaiters) w(m.memory);
            memoryWaiters = [];
          }
          if (typeof m.loaded === "number" && typeof m.total === "number") {
            for (const f of progressListeners) f({ loaded: m.loaded, total: m.total });
            const tenth = Math.floor((10 * m.loaded) / Math.max(1, m.total));
            if (tenth !== shown) {
              shown = tenth;
              note(`worker: downloaded ${mb(m.loaded)} of ${mb(m.total)}`);
            }
          }
        };
      }
      note(`worker: ${url}`);
      enginePromise = WorkerEngine.start(url, 180_000).then((e) => {
        note(`ready: ${e.buildId}`);
        return e;
      });
    }
    enginePromise.catch((e: unknown) => {
      note(`failed: ${e instanceof Error ? `${e.name}: ${e.message}` : String(e)}`);
      enginePromise = null;
    });
  }
  return enginePromise;
}

/** Decode a `.cpmesh` (written by the site build's slim.mjs): bodies plus quantized meshes, no topology. */
async function decodeMeshes(z: Uint8Array): Promise<{ model: Model; meshes: MeshData[] }> {
  const raw = await inflate(z);
  const dv = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  if (new TextDecoder().decode(raw.subarray(0, 4)) !== "CPM1") throw new Error("not a cpmesh file");
  const hl = dv.getUint32(4, true);
  const head = JSON.parse(new TextDecoder().decode(raw.subarray(8, 8 + hl))) as {
    bodies: Model["bodies"];
    meshes: { nv: number; ni: number; wide: boolean; lo: number[]; hi: number[] }[];
  };
  let at = 8 + hl;
  const take = (bytes: number): ArrayBuffer => {
    const b = raw.buffer.slice(raw.byteOffset + at, raw.byteOffset + at + bytes) as ArrayBuffer;
    at += bytes + ((4 - (bytes % 4)) % 4);
    return b;
  };
  const meshes = head.meshes.map((m) => {
    const q = new Uint16Array(take(m.nv * 6));
    const n8 = new Int8Array(take(m.nv * 3));
    const indices = m.wide ? new Uint32Array(take(m.ni * 4)) : new Uint32Array(new Uint16Array(take(m.ni * 2)));
    const positions = new Float32Array(m.nv * 3);
    const normals = new Float32Array(m.nv * 3);
    for (let k = 0; k < positions.length; k++) {
      const a = k % 3;
      const lo = m.lo[a] as number;
      positions[k] = lo + (((m.hi[a] as number) - lo) * (q[k] as number)) / 65535;
    }
    for (let k = 0; k < normals.length; k += 3) {
      const x = n8[k] as number, y = n8[k + 1] as number, zz = n8[k + 2] as number;
      const l = Math.hypot(x, y, zz) || 1;
      normals[k] = x / l; normals[k + 1] = y / l; normals[k + 2] = zz / l;
    }
    return { positions, normals, indices } as unknown as MeshData;
  });
  return { model: { bodies: head.bodies } as unknown as Model, meshes };
}

const packaged = new Map<string, Promise<{ model: Model; meshes: MeshData[] }>>();

/**
 * The place that last asked for the stage. Fetching, decoding and evaluating
 * take time, and the visitor can open something else meanwhile: a model that
 * arrives for a place that is no longer the newest asker is dropped rather
 * than drawn over what was opened after it.
 */
let claimant: HTMLElement | null = null;
let openTurn = 0;

/** Show a packaged model (`.cpmesh`) in `host`. No engine needed. `shown` is false when something opened after it took the stage first. */
export async function openModel(host: HTMLElement, url: string, view: View, grabbed = true): Promise<{ triangles: number; shown: boolean }> {
  claimant = host;
  const turn = ++openTurn;
  let p = packaged.get(url);
  if (!p) {
    p = fetch(url)
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.arrayBuffer();
      })
      .then((b) => decodeMeshes(new Uint8Array(b)));
    packaged.set(url, p);
    p.catch(() => packaged.delete(url));
    // Two decoded models are kept; an older one is fetched again if asked for.
    if (packaged.size > 2) packaged.delete(packaged.keys().next().value as string);
  }
  const m = await p;
  if (claimant !== host || turn !== openTurn) return { triangles: 0, shown: false };
  const s = theStage();
  s.mount(host, grabbed);
  s.setModel(m, view, url);
  return { triangles: s.triangles, shown: true };
}

export interface LiveResult {
  ok: boolean;
  /** Wall clock for the evaluation, ms. */
  ms: number;
  model: Model | null;
  error: string;
  triangles: number;
  /** The key of the script this result belongs to (`Live.show`). */
  key: string;
  /** Whether the model was drawn: not while another place has the stage. */
  drawn: boolean;
}

interface LiveRequest {
  source: string;
  overrides: Record<string, ParamValue>;
  preview: boolean;
  /** Which `show` it belongs to; a result from an older one is dropped. */
  epoch: number;
}

/**
 * A script evaluated in the page: sliders and edits go through `run`.
 *
 * One `Live` serves every script a place shows (the playground's tabs). Each
 * request carries the epoch of the script it was made for; `show` starts a
 * new epoch, so an evaluation still in the worker for the previous script, or
 * for an earlier slider value of it, can finish but is never drawn, and its
 * result never reaches the page. Within one epoch requests run in order and
 * only the newest waiting one is kept, so results arrive oldest first.
 *
 * In-flight work is superseded, not cancelled: the worker can only be
 * stopped by terminating it, and the page's worker would then fetch and
 * compile the engine again, which costs more than the few hundred
 * milliseconds an evaluation here takes.
 */
export class Live {
  private busy = false;
  private next: LiveRequest | null = null;
  private epoch = 0;
  private key = "";
  private view: View;
  /** The current epoch has drawn nothing yet: its first model is framed. */
  private fresh = true;

  constructor(
    private readonly host: HTMLElement,
    private readonly path: string,
    view: View,
    private readonly onResult: (r: LiveResult) => void,
    private readonly tolerance = 0.1,
  ) {
    this.view = view;
  }

  /**
   * Switch to another script (`key` names it, `view` frames it). Whatever
   * the previous one had queued or in flight is dropped, and the model on
   * show is taken away at once, so nothing of the old script is drawn while
   * the new one evaluates.
   */
  show(key: string, view: View): void {
    this.epoch++;
    this.key = key;
    this.view = view;
    this.fresh = true;
    this.next = null;
    if (stage?.host === this.host) stage.blank();
  }

  /** Nothing is queued or in the worker: every result asked for has been delivered. */
  get idle(): boolean {
    return !this.busy && !this.next;
  }

  /** Latest wins: while one evaluation runs, only the newest request is kept. */
  run(source: string, overrides: Record<string, ParamValue>, preview = false): void {
    claimant = this.host;
    this.next = { source, overrides: { ...overrides }, preview, epoch: this.epoch };
    if (!this.busy) void this.pump();
  }

  private async pump(): Promise<void> {
    this.busy = true;
    while (this.next) {
      const req = this.next;
      this.next = null;
      const t0 = performance.now();
      let ev: Evaluated | null = null;
      let failure = "";
      try {
        const eng = await engine();
        const scripts: ScriptFile[] = [{ path: this.path, source: req.source }];
        ev = await eng.evaluate({ scripts, overrides: req.overrides, tolerance: this.tolerance, deadlineMs: 20_000, preview: req.preview });
      } catch (e) {
        failure = e instanceof Error ? e.message : String(e);
      }
      // Superseded while it ran: another script was chosen. Nothing of it is shown.
      if (req.epoch !== this.epoch) continue;
      if (!ev) {
        this.onResult({ ok: false, ms: performance.now() - t0, model: null, error: failure, triangles: 0, key: this.key, drawn: false });
        continue;
      }
      const err = ev.model.diagnostics.find((d) => d.severity === "error");
      const hasBodies = ev.model.bodies.some((b) => b.visible);
      let triangles = 0;
      let drawn = false;
      // The stage is drawn only while this place is the one that last asked for it.
      if (hasBodies && (claimant === this.host || claimant === null)) {
        claimant = this.host;
        const s = theStage();
        if (s.host !== this.host) {
          s.mount(this.host, false);
          this.fresh = true;
        }
        s.setModel(ev, this.fresh ? this.view : null, this.key);
        this.fresh = false;
        triangles = s.triangles;
        drawn = true;
      }
      this.onResult({ ok: !err && hasBodies, ms: ev.model.eval_millis || performance.now() - t0, model: ev.model, error: err ? err.message : hasBodies ? "" : "the script made no bodies", triangles, key: this.key, drawn });
    }
    this.busy = false;
  }
}

/** Take the canvas out of the page and free the meshes. */
export function close(host?: HTMLElement): void {
  if (!host || claimant === host) claimant = null;
  if (!stage) return;
  if (host && stage.host !== host) return;
  stage.unmount();
}

export function setGrabbed(on: boolean): void {
  if (stage?.host) stage.mount(stage.host, on);
}

export function focusCanvas(): void {
  stage?.canvas.focus({ preventScroll: true });
}

/** The camera of whatever is on the stage, as the full viewer stores one; null with nothing shown. */
export function viewerPose(host?: HTMLElement): ReturnType<Stage["viewerPose"]> | null {
  if (!stage || !stage.host || (host && stage.host !== host)) return null;
  return stage.viewerPose();
}

/** The camera of whatever is on the stage, as the full viewer's address takes it; null with nothing shown. */
export function eyeTarget(host?: HTMLElement): ReturnType<Stage["eyeTarget"]> | null {
  if (!stage || !stage.host || (host && stage.host !== host)) return null;
  return stage.eyeTarget();
}

/** The displayed camera and the frames drawn so far, for the page's own checks. */
export function cameraNow(): ReturnType<Stage["cameraNow"]> | null {
  return stage?.host ? stage.cameraNow() : null;
}

/** What is drawn in `host`: the key the model was shown under and its triangles; null with nothing there. For the page's own checks. */
export function shownIn(host: HTMLElement): { key: string; triangles: number } | null {
  return stage?.host === host && stage.modelKey ? { key: stage.modelKey, triangles: stage.triangles } : null;
}
