// Performance HUD: CPU frame time (EMA), triangle/body counts, backend,
// last evaluation time. DOM writes are throttled to ~4 Hz.

import type { AppStore } from "../state.js";
import type { FrameStats } from "../viewer.js";
import { h } from "./dom.js";

export interface Hud {
  el: HTMLElement;
  frame(stats: FrameStats): void;
}

export function createHud(store: AppStore): Hud {
  const frameLine = h("div", null, "frame –");
  const geomLine = h("div", null, "tris –");
  const evalLine = h("div", null, "eval –");
  const backendLine = h("div", null, "backend –");
  const el = h("div", { class: "hud", role: "status", aria: { live: "off", label: "Performance" } }, frameLine, geomLine, evalLine, backendLine);
  el.hidden = true;

  let ema = 0;
  let frames = 0;
  let lastWrite = 0;
  let windowStart = performance.now();
  let windowFrames = 0;
  let fps = 0;

  const frame = (s: FrameStats): void => {
    frames++;
    windowFrames++;
    ema = ema === 0 ? s.frameMs : ema * 0.9 + s.frameMs * 0.1;
    const now = performance.now();
    if (now - windowStart >= 1000) {
      fps = (windowFrames * 1000) / (now - windowStart);
      windowStart = now;
      windowFrames = 0;
    }
    if (el.hidden || now - lastWrite < 250) return;
    lastWrite = now;
    frameLine.textContent = `frame ${ema.toFixed(2)} ms cpu · ${fps > 0 ? fps.toFixed(0) : "–"} fps (on demand) · #${frames}`;
    geomLine.textContent = `tris ${s.triangles.toLocaleString()} · bodies ${s.bodies}`;
    backendLine.textContent = `backend ${s.backend} · dpr ${Math.min(window.devicePixelRatio || 1, 2).toFixed(2)}`;
  };

  store.subscribe((s, prev) => {
    if (s.showHud !== prev.showHud) el.hidden = !s.showHud;
    if (s.evalInfo !== prev.evalInfo || s.engineKind !== prev.engineKind) {
      evalLine.textContent = s.evalInfo
        ? `eval ${s.evalInfo.millis.toFixed(1)} ms · ${s.engineKind} · ${s.evalInfo.triangles.toLocaleString()} tris`
        : `eval – · ${s.engineKind}`;
    }
  });
  return { el, frame };
}
