// In every pinned scene, at every step, on every window: no button or link
// lies on the drawn model (the picture's pixels that are not the page's
// colour), the model is not cut by the picture's side, and it is centred.
// With the first frame of each scene only (the tree's build), every step is
// checked against that frame.
import { VIEWPORTS, context, expect, test } from "./site.js";

const TOL = 0.08;

for (const name of Object.keys(VIEWPORTS)) {
  test(`buttons clear of the model, ${name}`, async ({ browser, site }) => {
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    await p.goto(site + "?noauto=1", { waitUntil: "load" });
    await p.waitForTimeout(500);
    const scenes = await p.evaluate(() => [...document.querySelectorAll<HTMLElement>(".scene")].map((s) => ({ id: s.id, steps: [...s.querySelectorAll<HTMLElement>(".step")].map((e) => [+e.dataset["from"]!, Math.min(1, +e.dataset["to"]!)] as [number, number]) })));
    const bad: string[] = [];
    let checked = 0;
    for (const sc of scenes) {
      for (const [si, [a, z]] of sc.steps.entries()) {
        for (const f of [a + (z - a) * 0.08, (a + z) / 2, a + (z - a) * 0.92]) {
          await p.evaluate(([id, f]) => { const s = document.getElementById(id as string)!, r = s.getBoundingClientRect(); scrollTo(0, r.top + scrollY + (f as number) * (r.height - innerHeight)); }, [sc.id, f]);
          await p.waitForTimeout(450);
          const geo = await p.evaluate((id) => {
            const s = document.getElementById(id)!, v = s.querySelector(".vis")!.getBoundingClientRect();
            const shown = (e: Element) => {
              const c = getComputedStyle(e);
              if (c.display === "none" || c.visibility === "hidden") return false;
              for (let x: Element | null = e; x && x !== s; x = x.parentElement) if (+getComputedStyle(x).opacity < 0.5) return false;
              const r = e.getBoundingClientRect();
              return r.width > 0 && r.height > 0;
            };
            const btns = [...s.querySelectorAll("a[href], button")].filter(shown).map((e) => { const r = e.getBoundingClientRect(); return { t: e.textContent!.trim().slice(0, 24), x0: r.left, y0: r.top, x1: r.right, y1: r.bottom }; });
            return { vis: { x: Math.max(0, v.left), y: Math.max(0, v.top), w: Math.min(innerWidth, v.right) - Math.max(0, v.left), h: Math.min(innerHeight, v.bottom) - Math.max(0, v.top), fullW: v.width, fullH: v.height, left: v.left, top: v.top }, btns };
          }, sc.id);
          if (geo.vis.w < 20 || geo.vis.h < 20) continue;
          // The picture alone: hide everything that is not it, then find the model's pixels.
          const hide = await p.addStyleTag({ content: ".scene .caps, .hero-brand, header, .bar, .hint3d { visibility: hidden !important }" });
          const png = await p.screenshot({ clip: { x: geo.vis.x, y: geo.vis.y, width: geo.vis.w, height: geo.vis.h }, type: "png" });
          await hide.evaluate((e) => (e as Element).remove());
          const box = await p.evaluate(async (src) => {
            const im = new Image();
            im.src = src;
            await im.decode();
            const W = 320, H = Math.max(1, Math.round((im.height * 320) / im.width));
            const c = document.createElement("canvas");
            c.width = W; c.height = H;
            const x = c.getContext("2d")!;
            x.drawImage(im, 0, 0, W, H);
            const d = x.getImageData(0, 0, W, H).data;
            let x0 = W, x1 = -1, y0 = H, y1 = -1;
            for (let j = 0; j < H; j++) for (let i = 0; i < W; i++) {
              const k = (j * W + i) * 4;
              if (Math.abs(d[k]! - 6) + Math.abs(d[k + 1]! - 10) + Math.abs(d[k + 2]! - 9) > 40) { if (i < x0) x0 = i; if (i > x1) x1 = i; if (j < y0) y0 = j; if (j > y1) y1 = j; }
            }
            return x1 < 0 ? null : [x0 / W, y0 / H, (x1 + 1) / W, (y1 + 1) / H];
          }, "data:image/png;base64," + png.toString("base64"));
          checked++;
          const tag = `${sc.id} step ${si} at ${f.toFixed(2)}`;
          if (!box) { bad.push(`${tag}: no picture drawn`); continue; }
          const m = { x0: geo.vis.x + box[0]! * geo.vis.w, y0: geo.vis.y + box[1]! * geo.vis.h, x1: geo.vis.x + box[2]! * geo.vis.w, y1: geo.vis.y + box[3]! * geo.vis.h };
          for (const q of geo.btns) if (q.x0 < m.x1 - 1 && q.x1 > m.x0 + 1 && q.y0 < m.y1 - 1 && q.y1 > m.y0 + 1) bad.push(`${tag}: "${q.t}" lies on the model`);
          const cx = ((m.x0 + m.x1) / 2 - geo.vis.left) / geo.vis.fullW, cy = ((m.y0 + m.y1) / 2 - geo.vis.top) / geo.vis.fullH;
          if (Math.abs(cx - 0.5) > TOL || Math.abs(cy - 0.5) > TOL + 0.04) bad.push(`${tag}: model centre at ${cx.toFixed(2)}, ${cy.toFixed(2)} of the picture`);
          if (box[0]! <= 0.003 || box[2]! >= 0.997) bad.push(`${tag}: model touches the picture's side`);
        }
      }
    }
    expect(checked, "step views looked at").toBeGreaterThan(20);
    expect(bad).toEqual([]);
    await ctx.close();
  });
}
