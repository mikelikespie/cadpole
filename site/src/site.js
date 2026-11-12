// Cadpole site. No dependencies; the 3D module (site3d.js) is fetched only
// when a visitor asks for, or on a wide screen approaches, something live.
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const html = document.documentElement;
const mqReduce = matchMedia("(prefers-reduced-motion: reduce)");
const q = new URLSearchParams(location.search);
const reduced = () => mqReduce.matches || q.get("rm") === "1";
const phone = () => Math.min(innerWidth, innerHeight) < 700;
// Safari decodes AVIF several times slower than WebP: too slow to scrub a frame set on an iPad.
const safari = /^((?!chrome|chromium|android|crios|fxios|edg).)*safari/i.test(navigator.userAgent) || q.get("set") === "w";
const debug = q.get("debug") === "1";
const engineMb = html.dataset.engineMb || "9";
const saveData = () => !!(navigator.connection && (navigator.connection.saveData || /2g/.test(navigator.connection.effectiveType || "")));

// ---- header, share, copy ---------------------------------------------------
const top = $(".top");
const onScrollHead = () => top.classList.toggle("scrolled", scrollY > 8);
addEventListener("scroll", onScrollHead, { passive: true });
onScrollHead();

const shareBtn = $("[data-share]");
if (shareBtn) {
  if (navigator.share) {
    shareBtn.hidden = false;
    shareBtn.addEventListener("click", () => navigator.share({ title: document.title, url: location.href.split("?")[0] }).catch(() => {}));
  } else shareBtn.remove();
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const t = document.createElement("textarea");
    t.value = text;
    t.style.cssText = "position:fixed;opacity:0";
    document.body.append(t);
    t.select();
    let ok = false;
    try { ok = document.execCommand("copy"); } catch {}
    t.remove();
    return ok;
  }
}
// What a screen reader should hear after a copy: one shared status line.
const liveStatus = $("#live-status");
const announce = (text) => { if (!liveStatus) return; liveStatus.textContent = ""; setTimeout(() => { liveStatus.textContent = text; }, 30); };
for (const b of $$("[data-copy]")) {
  b.addEventListener("click", async () => {
    const src = b.dataset.copy ? $(b.dataset.copy) : b.parentElement.querySelector("code, pre, textarea");
    const ok = await copyText(src.value ?? src.textContent);
    announce(ok ? "Copied" : "Copy failed: select the text and copy it");
    const was = b.textContent;
    b.textContent = ok ? "Copied" : "Select and copy";
    b.classList.toggle("done", ok);
    setTimeout(() => { b.textContent = was; b.classList.remove("done"); }, 1400);
  });
}

// ---- rails: dots follow the swipe -----------------------------------------
for (const rail of $$(".rail")) {
  const dots = rail.nextElementSibling?.classList.contains("dots") ? rail.nextElementSibling : null;
  if (!dots) continue;
  const cards = [...rail.children];
  dots.innerHTML = cards.map(() => "<i></i>").join("");
  const io = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) [...dots.children].forEach((d, i) => d.classList.toggle("on", cards[i] === e.target));
  }, { root: rail, threshold: 0.6 });
  cards.forEach((c) => io.observe(c));
}

// ---- 3D: one module, one engine, one canvas --------------------------------
let mod3d = null;
let m3 = null; // the module, once it has loaded
const load3d = () => (mod3d ??= import("./site3d.js").then((m) => (m3 = m)));
const can3d = (() => {
  try { const c = document.createElement("canvas"); return !!(c.getContext("webgl2") || c.getContext("webgl")); } catch { return false; }
})();
if (!can3d) html.classList.add("no3d");
const viewOf = (s) => { const [az, el, fov, zoom] = (s || "-30,20,24,1").split(",").map(Number); return { az, el, fov: fov || 24, zoom: zoom || 1 }; };
let liveOwner = null; // { el, release }
function takeStage(el, release) {
  if (liveOwner && liveOwner.el !== el) liveOwner.release();
  liveOwner = { el, release };
}
function dropStage(el) {
  if (liveOwner?.el === el) liveOwner = null;
  // The stage is free again: a section that lent it may want it back.
  mod3d?.then((m) => { m.close(el); document.dispatchEvent(new CustomEvent("cp3d-free")); });
}

// ---- "Full viewer": the same model in the Cadpole web app -------------------
// Each model has a viewer page under view/<name>/. The viewer takes its opening
// camera from the address (?eye=x,y,z&target=x,y,z, millimetres), so the link
// carries the view the visitor had here. A link may also name a cut or a display
// mode (OPENS below), which the viewer reads the same way.
const OPENS = {};
const fullDocs = JSON.parse(document.getElementById("full-viewers")?.textContent || "{}");
const nameOfModel = (url) => (url || "").replace(/^.*\//, "").replace(/\.cpmesh$/, "");
function wireFull(a, host, nameNow) {
  if (!a) return;
  const point = () => {
    const n = nameNow ? nameNow() : a.dataset.full;
    if (!n || !fullDocs[n]) return;
    a.dataset.full = n;
    const q = [];
    if (OPENS[n]) q.push(OPENS[n]);
    try {
      const c = host ? m3?.eyeTarget(host) : null;
      const f = (v) => v.map((x) => +x.toFixed(2)).join(",");
      if (c) q.push(`eye=${f(c.eye)}`, `target=${f(c.target)}`);
    } catch {}
    a.href = `view/${n}/${q.length ? "?" + q.join("&") : ""}`;
  };
  point();
  for (const ev of ["pointerenter", "pointerdown", "focus", "click", "auxclick", "contextmenu"]) a.addEventListener(ev, point);
}
for (const a of document.querySelectorAll(".card a[data-full]")) wireFull(a, null);

// ---- pinned scroll scenes --------------------------------------------------
const scenes = [];
class Scene {
  constructor(el) {
    this.el = el;
    this.vis = $(".vis", el);
    this.canvas = $("canvas.seq", el);
    this.ctx = null;
    this.steps = $$(".step", el).map((s) => ({ el: s, from: +s.dataset.from, to: +s.dataset.to }));
    this.bar = $(".track i", el);
    this.read = $(".read b", el);
    this.readSpec = this.read ? JSON.parse(this.read.dataset.read) : null;
    const small = phone();
    // a wide picture on a sharp screen gets the 1920 set (data-xl), so frames are not drawn above their own size
    const sharp = !small && !safari && el.dataset.xl && el.clientWidth * devicePixelRatio > 1700;
    this.dir = `${el.dataset.seq}/${small ? "m" : safari ? "w" : sharp ? "x" : "d"}`;
    this.n = +(small ? el.dataset.nm : el.dataset.n);
    this.blobs = new Array(this.n).fill(undefined);
    this.bitmaps = new Map();
    this.decoding = new Set();
    this.keep = small ? 20 : 14;
    this.loading = false;
    this.p = -1;
    this.want = 0;
    this.drawn = -1;
    this.top = 0;
    this.span = 1;
    this.active = false;
    this.measure();
  }
  measure() {
    const r = this.el.getBoundingClientRect();
    this.top = r.top + scrollY;
    this.span = Math.max(1, r.height - innerHeight);
  }
  progress() { return Math.min(1, Math.max(0, (scrollY - this.top) / this.span)); }
  url(i) { return `seq/${this.dir}/${String(i).padStart(3, "0")}.${/\/[dx]$/.test(this.dir) ? "avif" : "webp"}`; }
  // Near the scene: only the first few frames, so the landing view stays light.
  async prime() {
    if (this.primed || this.loading) return;
    this.primed = true;
    const c = Math.round(this.progress() * (this.n - 1));
    await Promise.all(Array.from({ length: Math.min(scrollY > 8 ? 8 : 3, this.n - c) }, async (_, k) => {
      try { const r = await fetch(this.url(c + k)); if (r.ok && !this.blobs[c + k]) this.blobs[c + k] = await r.blob(); } catch {}
    }));
    this.ensure(c);
  }
  async loadAll() {
    if (this.loading) return;
    this.loading = true;
    const order = [];
    const c = Math.round(this.progress() * (this.n - 1));
    for (let d = 0; d < this.n; d++) { if (c + d < this.n) order.push(c + d); if (d && c - d >= 0) order.push(c - d); }
    let at = 0;
    const worker = async () => {
      while (at < order.length) {
        const i = order[at++];
        if (this.blobs[i]) continue;
        try {
          const r = await fetch(this.url(i));
          if (r.ok) { this.blobs[i] = await r.blob(); if (Math.abs(i - this.want) <= 2) this.ensure(this.want); }
        } catch {}
      }
    };
    await Promise.all(Array.from({ length: 6 }, worker));
    // a dropped connection must not leave a hole in the sequence
    if (this.blobs.includes(undefined) && (this.tries = (this.tries || 0) + 1) < 4) {
      setTimeout(() => { this.loading = false; this.loadAll(); }, 1200 * this.tries);
    }
  }
  ensure(i) {
    if (i < 0 || i >= this.n || this.bitmaps.has(i) || this.decoding.has(i) || !this.blobs[i]) return;
    this.decoding.add(i);
    createImageBitmap(this.blobs[i]).then((bm) => {
      this.decoding.delete(i);
      this.bitmaps.set(i, bm);
      if (this.bitmaps.size > this.keep) {
        let far = -1, fd = -1;
        for (const k of this.bitmaps.keys()) { const d = Math.abs(k - this.want); if (d > fd) { fd = d; far = k; } }
        if (far >= 0 && far !== i) { this.bitmaps.get(far).close(); this.bitmaps.delete(far); }
      }
      if (this.active) this.paint();
    }, () => this.decoding.delete(i));
  }
  paint() {
    // the wanted frame, or the nearest one that is decoded
    let best = -1, bd = 1e9;
    for (const k of this.bitmaps.keys()) { const d = Math.abs(k - this.want); if (d < bd) { bd = d; best = k; } }
    if (best < 0 || best === this.drawn) return;
    const bm = this.bitmaps.get(best);
    if (!this.ctx) {
      this.canvas.width = bm.width;
      this.canvas.height = bm.height;
      this.ctx = this.canvas.getContext("2d", { alpha: false });
    }
    this.ctx.drawImage(bm, 0, 0);
    this.drawn = best;
    this.vis.classList.add("ready");
  }
  update(force) {
    const p = this.progress();
    if (p === this.p && !force) return;
    const dirn = p >= this.p ? 1 : -1;
    this.p = p;
    this.el.dataset.p = p.toFixed(3);
    for (const s of this.steps) s.el.classList.toggle("on", p >= s.from && (p < s.to || s.to >= 1));
    if (this.bar) this.bar.style.transform = `scaleX(${p.toFixed(4)})`;
    if (this.readSpec) this.read.textContent = readout(this.readSpec, p);
    this.want = Math.round(p * (this.n - 1));
    for (let k = 0; k <= 4; k++) this.ensure(this.want + k * dirn);
    this.ensure(this.want - dirn);
    this.paint();
  }
  step() { return (this.steps.find((x) => x.el.classList.contains("on")) ?? this.steps[0]).el; }
  view() { return viewOf(this.step().dataset.view); }
}
// A readout is piecewise linear in scroll progress: [[p, value], ...].
function readout(spec, p) {
  const k = spec.keys;
  let v = k[0][1];
  for (let i = 1; i < k.length; i++) {
    if (p <= k[i][0]) { const t = (p - k[i - 1][0]) / Math.max(1e-6, k[i][0] - k[i - 1][0]); v = k[i - 1][1] + (k[i][1] - k[i - 1][1]) * Math.max(0, t); break; }
    v = k[i][1];
  }
  return v.toFixed(spec.digits ?? 2) + (spec.unit ?? "");
}

function setupScenes() {
  if (reduced()) {
    html.classList.add("rm");
    for (const el of $$(".scene")) {
      for (const s of $$(".step", el)) {
        if (s.querySelector("img") || s.previousElementSibling?.classList.contains("hero")) continue;
        const im = new Image();
        im.loading = "lazy"; im.decoding = "async"; im.width = 720; im.height = 720;
        im.alt = s.dataset.alt || "";
        im.src = `seq/${el.dataset.seq}/m/${String(Math.round(+s.dataset.still * (+el.dataset.nm - 1))).padStart(3, "0")}.webp`;
        s.prepend(im);
      }
    }
    return;
  }
  for (const el of $$(".scene")) scenes.push(new Scene(el));
  let raf = 0;
  const tick = () => {
    raf = 0;
    let any = false;
    for (const s of scenes) if (s.active) { s.update(false); any = true; }
    if (any) raf = requestAnimationFrame(tick);
  };
  const near = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) { e.target._scene.prime(); }
  }, { rootMargin: "120% 0px" });
  const soon = new IntersectionObserver((es) => {
    // not before the visitor has moved: the landing view pays for the poster and a few frames only
    for (const e of es) if (e.isIntersecting) { const sc = e.target._scene; if (scrollY > 8) sc.loadAll(); else addEventListener("scroll", () => sc.loadAll(), { once: true, passive: true }); }
  }, { rootMargin: "25% 0px" });
  const on = new IntersectionObserver((es) => {
    for (const e of es) {
      const s = e.target._scene;
      s.active = e.isIntersecting;
      if (s.active) { s.measure(); s.update(true); if (!raf) raf = requestAnimationFrame(tick); }
      else if (s.vis.classList.contains("is-live")) s.exit3d?.();
    }
  });
  for (const s of scenes) { s.el._scene = s; near.observe(s.el); soon.observe(s.el); on.observe(s.el); wireScene3d(s); }
  let rz = 0;
  addEventListener("resize", () => { clearTimeout(rz); rz = setTimeout(() => scenes.forEach((s) => { s.measure(); s.update(true); }), 120); });
}

function wireScene3d(s) {
  const open = $("[data-open3d]", s.el), done = $("[data-close3d]", s.el), host = $(".live", s.vis);
  wireFull($("a[data-full]", s.el), host, () => nameOfModel(s.vis.classList.contains("is-live") ? s.liveModel : (s.step().dataset.model || open?.dataset.open3d)));
  if (!open) return;
  if (!can3d) { open.remove(); return; }
  s.exit3d = () => { s.vis.classList.remove("is-live"); dropStage(host); open.focus({ preventScroll: true }); };
  done.addEventListener("click", s.exit3d);
  host.addEventListener("cp3d-exit", s.exit3d);
  open.addEventListener("click", async () => {
    const label = open.textContent;
    open.textContent = "Loading…"; open.disabled = true;
    try {
      const m = await load3d();
      takeStage(host, () => s.vis.classList.remove("is-live"));
      host.addEventListener("cp3d-lost", () => { s.vis.classList.remove("is-live"); if (liveOwner?.el === host) liveOwner = null; }, { once: true });
      s.vis.classList.add("is-live");
      s.liveModel = s.step().dataset.model || open.dataset.open3d;
      const r = await m.openModel(host, s.liveModel, s.view(), true);
      if (r.shown) m.focusCanvas(); else s.vis.classList.remove("is-live");
    } catch (e) {
      s.vis.classList.remove("is-live");
      console.warn("cadpole: 3D unavailable:", e);
    }
    open.textContent = label; open.disabled = false;
  });
}

// ---- vector drawings: the SVG files themselves, inlined as they near the screen ----
const svgCache = new Map();
const svgText = (url) => { if (!svgCache.has(url)) svgCache.set(url, fetch(url).then((r) => { if (!r.ok) throw new Error(`${url}: ${r.status}`); return r.text(); })); return svgCache.get(url); };
{
  const io = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) {
      io.unobserve(e.target);
      svgText(e.target.dataset.svg).then((t) => {
        if (!e.target.firstElementChild) {
          e.target.innerHTML = t;
          for (const n of e.target.querySelectorAll("[id]")) n.removeAttribute("id");
          // Inside a button the drawing is a picture of what the button names: its text is not the button's.
          if (e.target.closest("button")) for (const n of e.target.querySelectorAll("svg, text")) n.setAttribute("aria-hidden", "true");
          // A tile shows the part of the sheet worth seeing at its size: the views, not the margins and title block.
          const c = e.target.dataset.crop, svg = e.target.firstElementChild;
          if (c && svg?.viewBox?.baseVal) {
            const [x0, y0, x1, y1] = c.split(",").map(Number), v = svg.viewBox.baseVal;
            svg.setAttribute("viewBox", `${v.x + v.width * x0} ${v.y + v.height * y0} ${v.width * (x1 - x0)} ${v.height * (y1 - y0)}`);
          }
        }
        e.target.dispatchEvent(new Event("dwg-ready"));
      }).catch((err) => console.warn("cadpole: drawing not loaded:", err));
    }
  }, { rootMargin: "60% 0px" });
  for (const d of $$(".dwg[data-svg]")) io.observe(d);
  for (const z of $$("[data-zoom]")) zoomPair(z);
  // floor plan: one parameter, five real evaluations of the script
  const pl = $(".plan-live"), ps = $("#plan-w");
  if (pl && ps) {
    const urls = JSON.parse(pl.dataset.states), vals = JSON.parse(ps.dataset.vals), out = $("output", ps.parentElement);
    let want = 0;
    pl.addEventListener("dwg-ready", () => urls.forEach((u) => svgText(u).catch(() => {})), { once: true });
    ps.setAttribute("aria-valuetext", vals[+ps.value]);
    ps.addEventListener("input", async () => {
      const i = want = +ps.value;
      out.textContent = vals[i];
      ps.setAttribute("aria-valuetext", vals[i]);
      pl.closest("[data-draw]")?.setAttribute("data-draw", urls[i]);  // the enlarged plan is the width shown
      const t = await svgText(urls[i]).catch(() => null);
      if (t && i === want) pl.innerHTML = t;
    });
  }
}

// ---- zooming a vector drawing -------------------------------------------------------
// One component for every drawing the visitor can zoom: the drawings section's picture-and-lines
// pair and the lightbox. The SVG is zoomed by its viewBox, so it is drawn again from the lines at
// every magnification and nothing in the page is laid out again. Gestures, as in map and design
// apps: a trackpad pinch (a wheel event with ctrl, or Safari's gesture events) or Cmd/Ctrl and the
// wheel zooms about the pointer; a two-finger scroll or a plain wheel moves the drawing; on touch,
// one finger moves it and two pinch. Inside the page a plain wheel scrolls the page until the
// visitor has clicked or tapped the drawing, and again once the pointer has left it, so the frame
// never traps the page's scrolling.
const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const zoomKey = isMac ? "⌘" : "Ctrl";
function zoomView(box, svg, o = {}) {
  const MAX = o.max || 40, ac = new AbortController(), on = { signal: ac.signal };
  const out = o.readout, note = o.note, canvas = o.canvas, pen = canvas?.getContext("2d");
  const [X0, Y0, VW, VH] = svg.getAttribute("viewBox").split(/[\s,]+/).map(Number);
  let W = 0, H = 0, s0 = 1, k = 1, cx = X0 + VW / 2, cy = Y0 + VH / 2;
  let anim = 0, bitmap = null, active = !!o.alwaysActive, kind = "mouse", noteT = 0, told = false;
  const sc = () => s0 * k;
  const clamp = () => {
    const hw = W / (2 * sc()), hh = H / (2 * sc());
    cx = VW <= 2 * hw ? X0 + VW / 2 : Math.min(X0 + VW - hw, Math.max(X0 + hw, cx));
    cy = VH <= 2 * hh ? Y0 + VH / 2 : Math.min(Y0 + VH - hh, Math.max(Y0 + hh, cy));
  };
  // The picture's pixels under the frame: smoothed while it has pixels to spare, drawn as the square
  // blocks they are once it has none. Only the visible part is touched, however far in.
  const paint = (bx, by, s) => {
    if (!bitmap) return;
    const d = devicePixelRatio || 1, cw = Math.round(W * d), ch = Math.round(H * d);
    if (canvas.width !== cw || canvas.height !== ch) { canvas.width = cw; canvas.height = ch; }
    const ppu = bitmap.width / VW;
    let sx = (bx - X0) * ppu, sy = (by - Y0) * ppu, sw = (W / s) * ppu, sh = (H / s) * ppu;
    const f = cw / sw;
    pen.imageSmoothingEnabled = f < 1;
    pen.imageSmoothingQuality = "high";
    pen.clearRect(0, 0, cw, ch);
    let dx = 0, dy = 0;
    if (sx < 0) { dx = -sx * f; sw += sx; sx = 0; }
    if (sy < 0) { dy = -sy * f; sh += sy; sy = 0; }
    sw = Math.min(sw, bitmap.width - sx); sh = Math.min(sh, bitmap.height - sy);
    if (sw > 0 && sh > 0) pen.drawImage(bitmap, sx, sy, sw, sh, dx, dy, sw * f, sh * f);
  };
  const draw = () => {
    if (!W) return;
    clamp();
    const s = sc(), bx = cx - W / (2 * s), by = cy - H / (2 * s);
    svg.setAttribute("viewBox", `${bx.toFixed(3)} ${by.toFixed(3)} ${(W / s).toFixed(3)} ${(H / s).toFixed(3)}`);
    paint(bx, by, s);
    box.style.setProperty("--zs", Math.pow(k, 0.3).toFixed(3));
    if (out) { const t = `${k < 9.95 ? Math.round(k * 10) / 10 : Math.round(k)}×`; if (out.textContent !== t) out.textContent = t; }
    box.dataset.k = k.toFixed(3);
    box.classList.toggle("zoomed", k > 1.001);
  };
  const under = (px, py) => { const s = sc(); return [cx - W / (2 * s) + px / s, cy - H / (2 * s) + py / s]; };
  const centreFor = ([x, y], px, py, k1) => { const s = s0 * k1; return [x - px / s + W / (2 * s), y - py / s + H / (2 * s)]; };
  const zoomAt = (px, py, f) => {
    const p = under(px, py), k1 = Math.min(MAX, Math.max(1, k * f));
    [cx, cy] = centreFor(p, px, py, k1);
    k = k1;
    draw();
  };
  const panBy = (dx, dy) => { cx -= dx / sc(); cy -= dy / sc(); draw(); };
  const halt = () => { cancelAnimationFrame(anim); anim = 0; };
  const fly = (k1, x1, y1) => {
    halt();
    k1 = Math.min(MAX, Math.max(1, k1));
    if (reduced()) { k = k1; cx = x1; cy = y1; draw(); return; }
    clamp();
    const k0 = k, x0 = cx, y0 = cy, t0 = performance.now(), T = 480;
    const step = (t) => {
      const u = Math.min(1, Math.max(0, (t - t0) / T)), e = 1 - (1 - u) ** 3;
      k = k0 * (k1 / k0) ** e; cx = x0 + (x1 - x0) * e; cy = y0 + (y1 - y0) * e;
      draw();
      anim = u < 1 ? requestAnimationFrame(step) : 0;
    };
    anim = requestAnimationFrame(step);
  };
  const home = () => fly(1, X0 + VW / 2, Y0 + VH / 2);
  const zoomInAt = (px, py) => {
    if (k >= MAX * 0.6) return home();
    const k1 = Math.min(MAX, k * 2.5);
    fly(k1, ...centreFor(under(px, py), px, py, k1));
  };
  const say = (text) => {
    if (!note) return;
    note.textContent = text;
    note.classList.add("on");
    clearTimeout(noteT);
    noteT = setTimeout(() => note.classList.remove("on"), 1800);
  };
  const layout = () => {
    const w = box.clientWidth, h = box.clientHeight;
    if (!w || !h || (w === W && h === H)) return;
    const first = !W;
    W = w; H = h; s0 = Math.min(W / VW, H / VH) * (o.fit || 1);
    if (first && o.start) { [cx, cy, k] = o.start; }
    draw();
  };
  const ro = new ResizeObserver(layout);
  ro.observe(box);
  layout();
  if (o.picture) {
    fetch(o.picture).then((r) => r.blob()).then((b) => createImageBitmap(b)).then((b) => {
      if (ac.signal.aborted) return b.close?.();
      bitmap = b; canvas.hidden = false; draw();
    }).catch((err) => console.warn("cadpole: picture not loaded:", err));
  }

  // pointers: one moves the drawing and glides on when let go, two pinch about their midpoint
  const pts = new Map();
  let vel = [0, 0], lastT = 0, moved = 0, tap = null;
  const local = (e) => { const r = box.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
  box.addEventListener("pointerdown", (e) => {
    if (e.button > 0 || e.target.closest(".zf-split, button, a")) return;
    kind = e.pointerType; active = true;
    halt();
    if (e.isPrimary) pts.clear(); // a new gesture: forget a finger whose end was never reported
    box.setPointerCapture(e.pointerId);
    pts.set(e.pointerId, local(e));
    if (pts.size === 1) moved = 0;
    vel = [0, 0]; lastT = e.timeStamp;
  }, on);
  box.addEventListener("pointermove", (e) => {
    if (!pts.has(e.pointerId)) return;
    const was = pts.get(e.pointerId), now = local(e);
    if (pts.size === 1) {
      const dx = now[0] - was[0], dy = now[1] - was[1], dt = Math.max(1, e.timeStamp - lastT);
      moved += Math.abs(dx) + Math.abs(dy);
      panBy(dx, dy);
      vel = [0.7 * (dx / dt) + 0.3 * vel[0], 0.7 * (dy / dt) + 0.3 * vel[1]];
      lastT = e.timeStamp;
    } else if (pts.size === 2) {
      const p = [...pts].find(([id]) => id !== e.pointerId)[1];
      const d0 = Math.hypot(was[0] - p[0], was[1] - p[1]), d1 = Math.hypot(now[0] - p[0], now[1] - p[1]);
      const m0 = [(was[0] + p[0]) / 2, (was[1] + p[1]) / 2], m1 = [(now[0] + p[0]) / 2, (now[1] + p[1]) / 2];
      moved += 99;
      if (d0 > 0) { zoomAt(m0[0], m0[1], d1 / d0); panBy(m1[0] - m0[0], m1[1] - m0[1]); }
    }
    pts.set(e.pointerId, now);
  }, on);
  const glide = () => {
    let [vx, vy] = vel, t0 = performance.now();
    const step = (t) => {
      const dt = Math.min(32, Math.max(0, t - t0));
      t0 = t;
      panBy(vx * dt, vy * dt);
      const f = 0.995 ** dt;
      vx *= f; vy *= f;
      anim = Math.hypot(vx, vy) > 0.02 ? requestAnimationFrame(step) : 0;
    };
    anim = requestAnimationFrame(step);
  };
  const up = (e) => {
    if (!pts.has(e.pointerId)) return;
    const p = pts.get(e.pointerId);
    pts.delete(e.pointerId);
    if (pts.size) { vel = [0, 0]; lastT = e.timeStamp; return; }
    if (e.type !== "pointerup") return;
    if (moved < 10 && e.pointerType !== "mouse") {
      if (tap && e.timeStamp - tap.t < 350 && Math.hypot(p[0] - tap.p[0], p[1] - tap.p[1]) < 40) { tap = null; zoomInAt(...p); } else tap = { t: e.timeStamp, p };
    } else if (k > 1 && !reduced() && e.timeStamp - lastT < 60 && Math.hypot(...vel) > 0.15) glide();
  };
  box.addEventListener("pointerup", up, on);
  box.addEventListener("pointercancel", up, on);
  box.addEventListener("pointerleave", (e) => { if (!o.alwaysActive && e.pointerType === "mouse") active = false; }, on);
  box.addEventListener("focus", () => { active = true; }, on);
  box.addEventListener("blur", () => { if (!o.alwaysActive) active = false; }, on);
  box.addEventListener("dblclick", (e) => { if (kind === "mouse" && !e.target.closest(".zf-split, button, a")) zoomInAt(...local(e)); }, on);
  // Safari's trackpad pinch
  let g0 = 1;
  box.addEventListener("gesturestart", (e) => { e.preventDefault(); halt(); g0 = k; }, on);
  box.addEventListener("gesturechange", (e) => { e.preventDefault(); zoomAt(...local(e), (g0 * e.scale) / k); }, on);
  box.addEventListener("gestureend", (e) => e.preventDefault(), on);
  box.addEventListener("wheel", (e) => {
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? H : 1, dx = e.deltaX * unit, dy = e.deltaY * unit;
    if (e.ctrlKey || e.metaKey) {
      // a pinch sends small steps, a wheel turned with the key held large ones
      e.preventDefault(); halt();
      zoomAt(...local(e), Math.exp(-dy * (Math.abs(dy) < 40 ? 0.01 : 0.002)));
      return;
    }
    if (!active) { if (!told) { told = true; say(`Click to move the drawing · ${zoomKey} + scroll to zoom`); } return; }
    e.preventDefault(); halt();
    panBy(e.shiftKey && !dx ? -dy : -dx, e.shiftKey && !dx ? 0 : -dy);
    if (!told) { told = true; say(`${zoomKey} + scroll or pinch to zoom`); }
  }, { passive: false, signal: ac.signal });
  box.addEventListener("keydown", (e) => {
    if (e.target !== box) return;
    if (e.key === "+" || e.key === "=") fly(k * 2, cx, cy);
    else if (e.key === "-" || e.key === "_") fly(k / 2, cx, cy);
    else if (e.key === "0") home();
    else if (e.key.startsWith("Arrow")) { halt(); panBy(e.key === "ArrowLeft" ? 80 : e.key === "ArrowRight" ? -80 : 0, e.key === "ArrowUp" ? 80 : e.key === "ArrowDown" ? -80 : 0); }
    else return;
    e.preventDefault();
  }, on);
  return {
    home, zoomIn: () => fly(k * 2, cx, cy), zoomOut: () => fly(k / 2, cx, cy),
    destroy: () => { ac.abort(); ro.disconnect(); halt(); clearTimeout(noteT); bitmap?.close?.(); },
  };
}

// The drawings section's pair: the picture left of a line the visitor moves by its grip, the lines
// right of it, both under one zoom.
function zoomPair(box) {
  const fig = box.closest("figure"), split = $(".zf-split", box);
  const start = box.dataset.start.split(",").map(Number);
  $(".zf-svg", box).addEventListener("dwg-ready", (e) => {
    const v = zoomView(box, e.target.firstElementChild, { start, picture: box.dataset.picture, canvas: $(".zf-png", box), readout: $(".zk", fig), note: $(".zf-note", box) });
    for (const b of $$("[data-z]", fig)) b.addEventListener("click", () => (b.dataset.z === "reset" ? v.home() : b.dataset.z === "in" ? v.zoomIn() : v.zoomOut()));
  }, { once: true });
  const setSplit = (v) => {
    v = Math.min(100, Math.max(0, v));
    box.style.setProperty("--split", `${v}%`);
    split.setAttribute("aria-valuenow", String(Math.round(v)));
    split.setAttribute("aria-valuetext", `${Math.round(v)}% SVG`);
  };
  split.addEventListener("pointerdown", (e) => {
    e.stopPropagation();
    split.setPointerCapture(e.pointerId);
    const move = (ev) => setSplit(((ev.clientX - box.getBoundingClientRect().left) / box.clientWidth) * 100);
    const end = () => { split.removeEventListener("pointermove", move); split.removeEventListener("pointerup", end); split.removeEventListener("pointercancel", end); };
    split.addEventListener("pointermove", move);
    split.addEventListener("pointerup", end);
    split.addEventListener("pointercancel", end);
  });
  split.addEventListener("keydown", (e) => {
    const v = +split.getAttribute("aria-valuenow");
    if (e.key === "ArrowLeft" || e.key === "ArrowDown") setSplit(v - 5);
    else if (e.key === "ArrowRight" || e.key === "ArrowUp") setSplit(v + 5);
    else return;
    e.preventDefault();
  });
}

// ---- links within the page: a smooth scroll to the section ----
document.addEventListener("click", (e) => {
  const a = e.target.closest?.('a[href^="#"]');
  if (!a || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const id = a.getAttribute("href").slice(1), t = id && !id.startsWith("f-") && document.getElementById(id);
  if (!t) return;
  e.preventDefault();
  history.pushState(null, "", `#${id}`);
  // Until the glide ends, the address keeps the section it names (see "where a reload lands").
  html.dataset.gliding = "1";
  const done = () => { delete html.dataset.gliding; removeEventListener("scrollend", done); clearTimeout(cap); };
  const cap = setTimeout(done, 2000);
  addEventListener("scrollend", done);
  t.scrollIntoView({ behavior: reduced() ? "auto" : "smooth", block: "start" });
});

// ---- agent recording: made only when wanted, plays only while it is on screen ----
{
  const box = $(".agent-vid");
  if (box) {
    const btn = $("[data-vplay]", box.parentElement), cap = $(".agent-cap", box.parentElement);
    const dots = $$(".agent-prog [data-seek]", box.parentElement), count = $(".agent-n", box.parentElement);
    let video = null, list = [], wanted = false;
    const sync = () => {
      let cur = null;
      for (const li of list) if (+li.dataset.t <= video.currentTime + 0.05) cur = li;
      for (const li of list) li.classList.toggle("on", li === cur);
      cap.textContent = cur ? cur.textContent : "";
      // the segments: those before the current one done, the current one lit
      const n = list.indexOf(cur);
      dots.forEach((d, i) => { d.classList.toggle("done", i < n); if (i === n) d.setAttribute("aria-current", "step"); else d.removeAttribute("aria-current"); });
      if (count) count.textContent = n < 0 ? "" : `${n + 1} / ${dots.length}`;
    };
    const make = () => {
      const base = box.dataset.video;
      list = $$(".agent-steps li");
      video = document.createElement("video");
      video.muted = true; video.playsInline = true; video.loop = true; video.preload = "auto";
      video.setAttribute("muted", ""); video.setAttribute("playsinline", ""); video.setAttribute("aria-label", "Screen recording of the Cadpole web app while an agent edits the script");
      // Each address carries the recording's digest, so a browser never plays a copy it kept from before.
      const v = box.dataset.v;
      video.poster = $("picture img", box)?.currentSrc || `${base}-poster.webp`;
      video.innerHTML = `<source src="${base}.webm?v=${v}" type="video/webm"><source src="${base}.mp4?v=${v}" type="video/mp4">`;
      video.addEventListener("timeupdate", sync);
      video.addEventListener("seeked", sync);
      video.addEventListener("play", () => { box.classList.add("playing"); btn.textContent = "Pause"; btn.setAttribute("aria-pressed", "true"); });
      video.addEventListener("pause", () => { box.classList.remove("playing"); btn.textContent = "Play"; btn.setAttribute("aria-pressed", "false"); });
      video.addEventListener("click", () => { if (video.paused) { wanted = true; video.play().catch(() => {}); } else { wanted = false; video.pause(); } });
      box.prepend(video);
      $("picture", box).hidden = true;
    };
    const play = () => { if (!video) make(); wanted = true; video.play().catch(() => {}); };
    btn.addEventListener("click", () => { if (video && !video.paused) { wanted = false; video.pause(); } else play(); });
    // A segment goes to its step and plays from there.
    for (const d of dots) d.addEventListener("click", () => {
      if (!video) make();
      const go = () => { video.currentTime = +d.dataset.seek + 0.05; sync(); };
      if (video.readyState >= 1) go(); else video.addEventListener("loadedmetadata", go, { once: true });
      play();
    });
    new IntersectionObserver((es) => {
      const vis = es[es.length - 1].isIntersecting;
      if (!vis) { video?.pause(); return; }
      // a wide screen starts it by itself; a phone, or a visitor who asked for less motion, taps
      if (wanted || (!phone() && !reduced() && !saveData())) play();
    }, { threshold: 0.6 }).observe(box);
  }
}

// ---- lightbox: a picture at once, the live model over it when it is ready ----
const sheet = $("dialog.sheet");
if (sheet) {
  const view = $(".sheet-view", sheet), host = $(".live", sheet), pic = $(".sheet-pic", sheet), title = $("[data-sheet-title]", sheet), foot = $(".sheet-foot", sheet);
  // The dialog's own "close" event comes a task later; tidy up at once so a picture
  // opened straight after closing is not undone by the late event.
  const draw = $(".sheet-draw", sheet);
  const info = $(".sheet-info", sheet), sheetIn = $(".sheet-in", sheet);
  // A gallery card opened here: its description, script lines and downloads beside the view,
  // and the address names it (#f-<card>) so the link opens it again.
  const setInfo = (b) => {
    if (b.closest(".sheet-info")) return;
    const t = b.closest("li")?.querySelector("template[data-card-info]");
    info.textContent = "";
    if (t) info.append(t.content.cloneNode(true));
    info.hidden = !t;
    sheetIn.classList.toggle("has-info", !!t);
    if (t) history.replaceState(null, "", `#f-${t.dataset.cardInfo}`);
    else if (/^#f-/.test(location.hash)) history.replaceState(null, "", location.pathname + location.search);
  };
  const tidy = () => {
    turn++; view.classList.remove("is-live"); dropStage(host); dz?.destroy(); dz = null; draw.hidden = true; draw.textContent = ""; pic.hidden = false;
    info.hidden = true; info.textContent = ""; sheetIn.classList.remove("has-info");
    if (/^#f-/.test(location.hash)) history.replaceState(null, "", location.pathname + location.search);
  };
  const closeSheet = () => { if (sheet.open) { tidy(); sheet.close(); } };
  const full = $("a[data-full]", sheet);
  let turn = 0;
  wireFull(full, host);
  $("[data-sheet-close]", sheet).addEventListener("click", closeSheet);
  host.addEventListener("cp3d-exit", closeSheet);
  // a click on the dimmed surround, outside the panel
  sheet.addEventListener("click", (e) => { if (e.target === sheet) closeSheet(); });
  sheet.addEventListener("close", () => { if (!sheet.open) tidy(); });
  // A vector drawing: the SVG itself, zoomed as the drawings section's is.
  let dz = null;
  const openDraw = async (b) => {
    const mine = ++turn;
    dropStage(host);
    title.textContent = b.dataset.title;
    view.classList.remove("is-live", "paper");
    pic.hidden = true; pic.removeAttribute("src");
    if (full) full.hidden = true;
    foot.textContent = `${b.dataset.note} · drag or scroll to move · pinch, ${zoomKey} + scroll or double-tap to zoom · arrow keys move, + and − zoom, 0 resets`;
    dz?.destroy(); dz = null;
    draw.textContent = ""; draw.hidden = false; draw.tabIndex = 0;
    setInfo(b);
    if (!sheet.open) sheet.showModal();
    const text = await svgText(b.dataset.draw);
    if (mine !== turn) return;
    const el = document.createElement("div");
    el.className = "dwg";
    el.innerHTML = text;
    for (const n of el.querySelectorAll("[id]")) n.removeAttribute("id");
    draw.append(el);
    dz = zoomView(draw, el.firstElementChild, { alwaysActive: true, fit: 0.95 });
    draw.focus({ preventScroll: true });
  };
  const openPic = async (b) => {
    const mine = ++turn, model = can3d ? b.dataset.sheet3d : "";
    const n = nameOfModel(b.dataset.sheet3d);
    title.textContent = b.dataset.title;
    view.classList.remove("is-live");
    dz?.destroy(); dz = null; draw.hidden = true; draw.textContent = ""; pic.hidden = false;
    view.classList.toggle("paper", "paper" in b.dataset);
    pic.alt = b.querySelector("img")?.alt || b.dataset.title;
    if (b.dataset.pic) pic.src = b.dataset.pic; else pic.removeAttribute("src");
    pic.hidden = !b.dataset.pic;
    if (full) { full.hidden = !fullDocs[n]; full.dataset.full = n; if (fullDocs[n]) full.href = `view/${n}/`; else full.removeAttribute("href"); }
    foot.textContent = model ? "Loading the 3D model…" : b.dataset.sheet3d ? "" : "A rendered picture. There is no 3D model behind this one.";
    setInfo(b);
    if (!sheet.open) sheet.showModal();
    if (!model) return;
    try {
      const m = await load3d();
      if (mine !== turn) return;
      takeStage(host, closeSheet);
      host.addEventListener("cp3d-lost", () => { if (mine !== turn) return; view.classList.remove("is-live"); if (liveOwner?.el === host) liveOwner = null; foot.textContent = "Live 3D stopped here. This is the rendered picture of the same model."; }, { once: true });
      const r = await m.openModel(host, model, viewOf(b.dataset.view), true);
      if (mine !== turn || !r.shown) { if (mine === turn) m.close(host); return; }
      view.classList.add("is-live");
      foot.textContent = `${r.triangles.toLocaleString("en-US")} triangles · drag to orbit · two fingers or scroll to move · pinch to zoom · double-tap a spot to centre it · arrow keys turn, + and − zoom`;
      m.focusCanvas();
    } catch (e) {
      if (mine === turn) foot.textContent = "Live 3D could not start here. This is the rendered picture of the same model.";
      console.warn("cadpole: 3D unavailable:", e);
    }
  };
  for (const b of $$("[data-draw]")) b.addEventListener("click", () => openDraw(b));
  for (const b of $$("[data-pic]")) b.addEventListener("click", () => openPic(b));
  // Inside a card's panel: its own buttons, and the script's copy button.
  info.addEventListener("click", async (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    if (b.matches("[data-sheet3d]")) openPic(b);
    else if (b.matches("[data-draw]")) openDraw(b);
    else if (b.matches("[data-copy]")) {
      const ok = await copyText(b.closest(".code")?.querySelector("pre")?.textContent ?? "");
      announce(ok ? "Copied" : "Copy failed: select the text and copy it");
      const was = b.textContent;
      b.textContent = ok ? "Copied" : "Select and copy";
      setTimeout(() => { b.textContent = was; }, 1400);
    }
  });
  // An address naming a card (#f-<card>) opens it.
  const fromHash = () => {
    const m = /^#f-([\w-]+)$/.exec(location.hash), t = m && document.getElementById(`f-${m[1]}`);
    if (t?.matches(".tile") && !sheet.open) t.click();
  };
  addEventListener("hashchange", fromHash);
  addEventListener("load", fromHash);
  // For the page's checks: what the open lightbox shows.
  window.__sheet = {
    kind: () => (!sheet.open ? "" : !draw.hidden ? (draw.querySelector("svg") ? "svg" : "svg-loading") : view.classList.contains("is-live") ? "3d" : "picture"),
    card: () => (info.hidden ? "" : location.hash),
  };
}

// ---- playground: the real engine, in the page ------------------------------
function setupPlay() {
  const root = $(".play");
  if (!root) return;
  const view = $(".play-view", root), host = $(".live", view), poster = $("img", view), startBtn = $("[data-start]", root);
  const status = $(".status", root), statusText = $("span", status), sliders = $(".sliders", root), editor = $(".editor", root), fileName = $("[data-file]", root);
  const tabs = $$("#play .tab");
  let live = null, current = null, started = false, overrides = {}, editTimer = 0, sliderNames = "";
  const say = (cls, text) => { status.className = `status ${cls}`; statusText.textContent = text; };
  const meter = $(".load", root), take = $("[data-take]", root), fullLink = $("a[data-full]", root);
  wireFull(fullLink, host);
  // ?debug=1: everything the engine start reports, on the page, so a failure on a phone can be read off it.
  let dbg = null;
  const log = (line) => { if (!dbg) return; dbg.textContent += line + "\n"; dbg.scrollTop = dbg.scrollHeight; };
  if (debug) {
    dbg = document.createElement("pre");
    dbg.className = "debug";
    dbg.setAttribute("aria-label", "Engine start log");
    $(".play-right", root).append(dbg);
    log(`debug on · ${navigator.userAgent}`);
    log(`viewport ${innerWidth}x${innerHeight} @${devicePixelRatio}x · WebAssembly ${typeof WebAssembly} · Worker ${typeof Worker} · WebGL ${can3d}`);
    document.addEventListener("securitypolicyviolation", (e) => log(`Content-Security-Policy: ${e.effectiveDirective} blocked ${e.blockedURI || "inline code"}`));
    addEventListener("error", (e) => log(`page error: ${e.message}`));
    addEventListener("unhandledrejection", (e) => log(`unhandled: ${e.reason?.message ?? e.reason}`));
    log("tap Run it live; what happens is written here");
  }
  const memory = async () => {
    const b = await m3?.engineMemory();
    if (!b) return;
    root.dataset.wasmMb = (b / 1048576).toFixed(1);
    log(`engine memory ${root.dataset.wasmMb} MB (wasm memory never shrinks, so this is the peak so far)`);
  };
  const IN = 25.4;
  const fmt = (p, v) => p.unit === "mm" ? `${(v / IN).toFixed(v / IN < 1 ? 3 : 2)} in` : p.unit === "deg" ? `${v.toFixed(0)}°` : Number.isInteger(v) ? String(v) : v.toFixed(2);
  function buildSliders(model) {
    const ps = model.params.filter((p) => typeof p.value === "number" && p.min !== null && p.max !== null).slice(0, 4);
    const names = ps.map((p) => p.name).join(",");
    if (names === sliderNames) return;
    sliderNames = names;
    sliders.textContent = "";
    for (const p of ps) {
      const id = `p-${p.name}`;
      const whole = Number.isInteger(p.default_value) && Number.isInteger(p.min) && Number.isInteger(p.max) && p.unit === "";
      const row = document.createElement("div");
      row.className = "slider";
      row.innerHTML = `<label for="${id}">${p.name}</label><output for="${id}"></output><input id="${id}" type="range">`;
      const input = $("input", row), out = $("output", row);
      input.min = p.min; input.max = p.max; input.step = p.step ?? (whole ? 1 : (p.max - p.min) / 200); input.value = p.value;
      if (p.description) input.title = p.description;
      out.textContent = fmt(p, +p.value);
      input.addEventListener("input", () => { overrides[p.name] = +input.value; out.textContent = fmt(p, +input.value); run(true); });
      input.addEventListener("change", () => run(false));
      sliders.append(row);
    }
  }
  // Every result is for the tab on show: Live drops whatever was evaluated for
  // a tab before the last switch. The check stays as a guard.
  function onResult(r) {
    if (r.key !== current?.dataset.src) return;
    root.dataset.shown = r.drawn ? r.key : "";
    if (r.ok) {
      // A slider moved or a line edited while the stage was lent: it is here again.
      if (away) { away = false; takeStage(host, lent); }
      if (r.drawn) view.classList.add("is-live");
      view.classList.remove("is-switching");
      buildSliders(r.model);
      say("ok", `evaluated in ${r.ms < 10 ? r.ms.toFixed(1) : Math.round(r.ms)} ms · ${r.triangles.toLocaleString("en-US")} triangles · in your browser`);
      if (debug) log(`evaluated ${fileName.textContent} in ${Math.round(r.ms)} ms, ${r.triangles} triangles`);
      memory();
    } else { view.classList.remove("is-switching"); say("err", r.error.split("\n")[0].slice(0, 220)); log(`evaluation failed: ${r.error}`); }
  }
  function run(preview) { live?.run(editor.value, overrides, preview); }
  // The page has one 3D stage. When a lightbox or a scene takes it, or the browser takes its
  // context, the picture comes back in place of the model; when the stage is free and this
  // section is on screen, the script is run again and the model is back, sliders as they were.
  let away = false, seen = false;
  function lent() {
    if (!started || away) return;
    away = true;
    view.classList.remove("is-live", "is-taken");
    if (take) { take.setAttribute("aria-pressed", "false"); take.textContent = "Zoom and pan"; take.classList.remove("primary"); }
  }
  function back() {
    if (!away || !seen || !live) return;
    if (liveOwner && liveOwner.el !== host) {
      // Someone else is showing a model: leave it be while it is on screen.
      const r = liveOwner.el.getBoundingClientRect();
      if (r.width > 0 && r.bottom > 0 && r.top < innerHeight) return;
    }
    away = false;
    takeStage(host, lent);
    say("busy", "evaluating…");
    run(false);
  }
  host.addEventListener("cp3d-unmount", lent);
  host.addEventListener("cp3d-lost", () => { if (liveOwner?.el === host) liveOwner = null; lent(); back(); });
  document.addEventListener("cp3d-free", back);
  new IntersectionObserver((es) => { seen = es[es.length - 1].isIntersecting; back(); }).observe(view);
  async function start() {
    if (started) return;
    started = true;
    if (startBtn) startBtn.disabled = true;
    say("busy", `loading the engine (about ${engineMb} MB, once)…`);
    let stage = "loading the 3D module (site3d.js)";
    log(stage);
    try {
      const m = await load3d();
      if (debug) { m.engineLog.forEach(log); m.onEngineLog(log); }
      stage = "starting the engine";
      if (meter) meter.hidden = false;
      await m.engine(({ loaded, total }) => {
        const f = Math.min(1, loaded / Math.max(1, total));
        if (meter) meter.firstElementChild.style.transform = `scaleX(${f.toFixed(3)})`;
        say("busy", f < 1 ? `loading the engine: ${Math.round(f * 100)}% of about ${engineMb} MB…` : "starting the engine…");
      });
      if (meter) meter.hidden = true;
      stage = "drawing the first model";
      takeStage(host, lent);
      live = new m.Live(host, "main.star", viewOf(current.dataset.view), onResult, 0.1);
      select(current, true);
    } catch (e) {
      started = false;
      if (meter) meter.hidden = true;
      if (startBtn) startBtn.disabled = false;
      const why = m3 ? m3.explainEngineFailure(e) : `The 3D code could not be loaded (${e?.message ?? e}).`;
      say("err", `The live engine did not start. ${why} The pictures on this page are the same models, rendered ahead of time.`);
      root.dataset.engineError = `${stage}: ${e?.name ?? "Error"}: ${e?.message ?? e}`;
      log(`FAILED while ${stage}: ${e?.name ?? "Error"}: ${e?.message ?? e}`);
      if (e?.stack) log(String(e.stack));
      console.warn("cadpole: engine did not start while", stage, e);
    }
  }
  // A tab switch is synchronous from here to the request: the script text, the
  // sliders and the evaluation are the new tab's before anything else can run,
  // and the old tab's model is taken off the view at once.
  function select(tab, force) {
    if (tab === current && !force) return;
    current = tab;
    tabs.forEach((t) => t.setAttribute("aria-pressed", String(t === tab)));
    const src = $(`#${tab.dataset.src}`).textContent;
    clearTimeout(editTimer);
    editor.value = src;
    fileName.textContent = tab.dataset.file;
    overrides = {}; sliderNames = ""; sliders.textContent = "";
    root.dataset.shown = "";
    if (root.classList.contains("code-open")) more?.click();
    root.classList.toggle("code-short", src.trimEnd().split("\n").length <= 12);
    if (!live) { setPoster(tab); return; }
    if (away) { away = false; takeStage(host, lent); }
    view.classList.add("is-switching");
    live.show(tab.dataset.src, viewOf(tab.dataset.view));
    say("busy", "evaluating…");
    run(false);
  }
  const setPoster = (t) => { poster.srcset = `${t.dataset.poster} 720w, ${t.dataset.poster.replace("-720.", "-1200.")} 1200w`; poster.src = t.dataset.poster; };
  tabs.forEach((t) => t.addEventListener("click", () => {
    if (t === current) return;
    if (take?.getAttribute("aria-pressed") === "true") take.click();
    select(t, false);
    if (started) view.classList.remove("is-live");
    setPoster(t);
    if (fullLink) { fullLink.dataset.full = t.dataset.full; fullLink.href = `view/${t.dataset.full}/`; }
  }));
  // In the playground a sideways drag orbits and the page still scrolls;
  // this hands the model the wheel and every touch until Done.
  take?.addEventListener("click", () => {
    const on = take.getAttribute("aria-pressed") !== "true";
    take.setAttribute("aria-pressed", String(on));
    take.textContent = on ? "Done" : "Zoom and pan";
    take.classList.toggle("primary", on);
    view.classList.toggle("is-taken", on);
    m3?.setGrabbed(on);
    if (on) m3?.focusCanvas();
  });
  host.addEventListener("cp3d-exit", () => { if (take?.getAttribute("aria-pressed") === "true") take.click(); });
  // The script, short until asked for.
  const more = $("[data-more]", root);
  more?.addEventListener("click", () => { const open = root.classList.toggle("code-open"); more.textContent = open ? "Show less" : "Show the full script"; more.setAttribute("aria-expanded", String(open)); editor.style.height = open ? `${editor.scrollHeight + 4}px` : ""; });
  editor.addEventListener("input", () => { clearTimeout(editTimer); editTimer = setTimeout(() => { if (started) { say("busy", "evaluating…"); run(false); } else start(); }, 350); });
  editor.addEventListener("keydown", (e) => {
    if (e.key === "Tab" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && editor.selectionStart !== editor.selectionEnd) return;
    if (e.key === "Escape") editor.blur();
  });
  startBtn?.addEventListener("click", start);
  current = tabs[0];
  root.classList.toggle("code-short", editor.value.trimEnd().split("\n").length <= 12);
  if (!can3d) { view.classList.add("no3d"); startBtn?.remove(); say("", "Live 3D needs WebGL, which this browser does not offer."); return; }
  // On a wide screen with no data saver, start as the section approaches.
  if (!phone() && !saveData() && !reduced() && q.get("noauto") !== "1") {
    // Wait for the scroll to rest first: creating the GL context costs a few frames.
    const rested = () => { let t = 0; const arm = () => { clearTimeout(t); t = setTimeout(() => { removeEventListener("scroll", arm); start(); }, 180); }; addEventListener("scroll", arm, { passive: true }); arm(); };
    const io = new IntersectionObserver((es) => { if (es.some((e) => e.isIntersecting)) { io.disconnect(); rested(); } }, { rootMargin: "60% 0px" });
    io.observe(root);
  }
  window.__play = {
    start, select: (i) => tabs[i].click(), state: () => status.textContent, cam: () => m3?.cameraNow() ?? null, memory: () => m3?.engineMemory(),
    // The tab selected, and what the view draws: the tab its model was evaluated for, and its triangles.
    selected: () => current?.dataset.src ?? "", shown: () => m3?.shownIn(host) ?? null,
  };
}

setupScenes();
setupPlay();

// ---- where a reload lands: exactly where the visitor was (the top stays the top) ----
// The browser's own restoring guesses before late layout settles; this page does it itself.
{
  const KEY = "cadpole:scroll";
  try { history.scrollRestoration = "manual"; } catch {}
  // The place is noted while the page is in use, not as it goes away: a phone browser can already
  // have changed the layout by the time pagehide runs.
  let last = null;
  const note = () => { if (document.visibilityState === "visible") last = { y: Math.round(scrollY), w: innerWidth, h: html.scrollHeight }; };
  // Back at the top, the address names no section: a reload then stays at the top. (replaceState only,
  // so scrolling adds nothing to the history; a card's #f- address belongs to its open lightbox.)
  addEventListener("scroll", () => {
    if (location.hash && !html.dataset.gliding && !/^#f-/.test(location.hash) && scrollY < innerHeight * 0.5) history.replaceState(null, "", location.pathname + location.search);
  }, { passive: true });
  for (const ev of ["scroll", "load", "resize"]) addEventListener(ev, note, { passive: true });
  const save = () => { try { if (last) sessionStorage.setItem(KEY, JSON.stringify(last)); } catch {} };
  addEventListener("pagehide", save);
  document.addEventListener("visibilitychange", () => { if (document.visibilityState === "hidden") save(); });
  const nav = performance.getEntriesByType("navigation")[0]?.type;
  if ((nav === "reload" || nav === "back_forward") && !q.has("y") && !q.has("s")) {
    let saved = null;
    try { saved = JSON.parse(sessionStorage.getItem(KEY) || "null"); } catch {}
    const go = () => { if (!saved) return; const y = saved.w === innerWidth ? saved.y : Math.round(saved.y / Math.max(1, saved.h) * html.scrollHeight); if (Math.abs(scrollY - y) > 1) scrollTo(0, y); };
    let moved = false;
    for (const ev of ["wheel", "touchstart", "keydown", "pointerdown"]) addEventListener(ev, () => { moved = true; }, { once: true, passive: true, capture: true });
    go();
    // Late layout (a picture, a drawing set inline) can still change the height: hold the place until it settles.
    const ro = new ResizeObserver(() => { if (moved) ro.disconnect(); else go(); });
    ro.observe(document.body);
    addEventListener("load", () => { if (!moved) go(); setTimeout(() => ro.disconnect(), 2500); });
  }
}

// Test hooks: ?y=<px> scrolls there; ?s=<scene>&p=<0..1> puts a scene at a progress.
addEventListener("load", () => {
  if (q.has("y")) scrollTo(0, +q.get("y"));
  if (q.has("s")) { const s = scenes.find((x) => x.el.id === q.get("s")); if (s) { s.measure(); scrollTo(0, s.top + s.span * +(q.get("p") || 0)); } }
});
window.__scenes = scenes;

// ?debug=1: the last pointer and touch events and what lies under the finger, on the
// page, so one screenshot from a phone shows whether a 3D view is getting its touches.
if (debug) {
  const out = document.createElement("pre"), seen = [];
  out.style.cssText = "position:fixed;left:0;right:0;bottom:0;z-index:2147483647;margin:0;padding:6px 8px calc(6px + env(safe-area-inset-bottom,0px));font:10px/1.3 ui-monospace,monospace;background:#000c;color:#fd8;white-space:pre-wrap;pointer-events:none";
  const where = (el) => (el ? `${el.tagName.toLowerCase()}${el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\s+/).join(".") : ""}` : "nothing");
  const put = (e) => {
    const t = e.touches?.[0] ?? e.changedTouches?.[0] ?? e, x = t.clientX ?? 0, y = t.clientY ?? 0;
    const n = e.touches ? ` fingers=${e.touches.length}` : e.pointerType ? ` ${e.pointerType}` : "";
    const line = `${e.type}${n} at ${Math.round(x)},${Math.round(y)} target=${where(e.target)} under=${where(document.elementFromPoint(x, y))}${e.defaultPrevented ? " (prevented)" : ""}`;
    if (seen[0]?.startsWith(e.type + " ") || seen[0]?.startsWith(e.type + n)) seen[0] = line; else seen.unshift(line);
    seen.length = Math.min(seen.length, 6);
    const c = document.querySelector("canvas.cp3d-canvas"), cam = m3?.cameraNow?.();
    const open = document.querySelector("dialog[open]");
    (open || document.body).append(out);
    out.textContent = `canvas: ${c ? `${where(c.parentElement?.parentElement)} touch-action=${getComputedStyle(c).touchAction}` : "none"} · camera ${cam ? [cam.x, cam.y, cam.z].map((v) => v.toFixed(0)).join(",") : "-"} · page zoom ${visualViewport?.scale.toFixed(2)}\n${seen.join("\n")}`;
  };
  for (const ev of ["pointerdown", "pointermove", "pointerup", "pointercancel", "touchstart", "touchmove", "touchend", "touchcancel", "gesturestart", "wheel", "click"]) addEventListener(ev, put, { capture: true, passive: true });
}

// A phone page does not zoom: pinch and double-tap belong to the 3D views (camera), never to the page.
// iOS ignores user-scalable=no, so the pinch gesture itself is declined. Desktop zoom (ctrl + wheel, keys) is untouched.
if (matchMedia("(pointer: coarse)").matches) {
  for (const ev of ["gesturestart", "gesturechange", "gestureend"]) document.addEventListener(ev, (e) => { if (e.target?.closest?.(".live, .sheet-draw, .zf")) e.preventDefault(); }, { passive: false });
}

// For browsers a test cannot drive (a phone):
// ?debug=1&go=play|scene|sheet presses the button itself and reports on the page.
if (debug && q.has("go")) addEventListener("load", () => {
  const go = q.get("go"), errs = [];
  addEventListener("error", (e) => errs.push(String(e.message || e)));
  addEventListener("unhandledrejection", (e) => errs.push(String(e.reason)));
  document.addEventListener("securitypolicyviolation", (e) => errs.push(`blocked by the site's content policy: ${e.violatedDirective}`));
  const out = document.createElement("pre");
  out.style.cssText = "position:fixed;left:0;right:0;top:0;z-index:99999;margin:0;padding:6px 8px;font:11px/1.35 ui-monospace,monospace;background:#000c;color:#9fe;white-space:pre-wrap;pointer-events:none";
  document.body.append(out);
  const t0 = performance.now();
  setTimeout(() => {
    if (go === "play") { $("#play").scrollIntoView(); window.__play?.start(); }
    if (go === "scene") { const b = $("[data-open3d]"); b?.scrollIntoView({ block: "center" }); b?.click(); }
    if (go === "sheet") { const b = $("[data-sheet3d]"); b?.scrollIntoView({ block: "center" }); b?.click(); }
  }, 600);
  setInterval(() => {
    const live = [...document.querySelectorAll(".live canvas")].filter((c) => c.getBoundingClientRect().width > 0).length;
    out.textContent = `go=${go} t=${((performance.now() - t0) / 1000).toFixed(0)}s live-canvases=${live} zoom=${visualViewport?.scale ?? "?"}\nplay: ${window.__play?.state() ?? ""}\nengine error: ${document.documentElement.dataset.engineError || $("#play")?.dataset.engineError || "none"}\nerrors: ${errs.join(" | ") || "none"}`;
  }, 500);
});
