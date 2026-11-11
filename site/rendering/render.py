#!/usr/bin/env python3
"""Re-render the site's pictures from the scripts.

    bazel run --config=opt //site:render -- <set> [options]

    stills [NAME ...]    the path-traced stills (static/media/*.webp); all, or the named ones
    frames SET           one scroll scene's frame set: iris, mug or radial
    jet [FIRST [LAST]]   the turbojet scene's frames, one render per frame (restartable)
    drawings [NAME ...]  the vector drawings and plan sheets (static/draw/); all, or the named ones ("cabin": the plan sheets)
    buildings            the building sections: raster pictures (static/media/), the drawing-set
                         sheet (static/draw/) and the solver's numbers (content/layout.json)
    encode               masters -> the encoded frame sets, into the media directory
    lock [DIR]           rewrite media.lock.json from a media directory
    help                 this text

Path tracing is slow: a still takes minutes, a frame set an hour or more, the
jet several hours. Masters (transparent PNGs, with the camera of every frame
beside them) are kept under --work so framing and encoding can be redone
without rendering again. One render runs at a time, at low priority.

  --work DIR    masters and intermediate files (default: ~/.cache/cadpole/site-render)
  --media DIR   where `encode` writes the frame sets (default: <work>/media);
                this is the directory `bazel run //site:assemble -- --media` takes
  --threads N   render threads (default 8)
  --no-ground-grid  `jet` and `frames` framing: leave out the ground grid that is
                otherwise drawn under the model in each frame's own perspective
                (ground_grid.py)

Needs python3 and ffmpeg (with libaom, libwebp and librsvg) on PATH.
"""
import base64
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ground_grid  # noqa: E402
import sets  # noqa: E402

PAGE = "0x060a09"  # the page's colour: masters are flattened onto it before any scaling
LOOK = ["--backend", "path", "--denoise", "--edge-break", "0.25", "--lighting", "product", "--backdrop", "studio", "--floor", "none",
        "--background", "transparent", "--supersample", "2", "--tonemap", "neutral", "--no-grid"]
# The same look for `cadpole op Render`, which the picture-and-drawing pairs use for its parallel camera.
# Flags, not JSON: a flag the CLI does not know is refused, a JSON field it does not know is ignored.
PAIR_LOOK = ["--backend", "RENDER_BACKEND_PATH_TRACE", "--samples", "96", "--denoise", "--edge-break", "0.25", "--lighting", "product", "--backdrop", "studio",
             "--floor", "none", "--supersample", "2", "--tonemap", "neutral", "--no-grid"]


class Run:
    def __init__(self, argv):
        opt = {"work": os.path.join(os.environ.get("XDG_CACHE_HOME", os.path.expanduser("~/.cache")), "cadpole", "site-render"), "threads": "8"}
        self.flags, self.args = set(), []
        i = 0
        while i < len(argv):
            a = argv[i]
            if a == "--no-ground-grid":
                self.flags.add(a)
            elif a.startswith("--"):
                opt[a[2:]] = argv[i + 1]
                i += 1
            else:
                self.args.append(a)
            i += 1
        self.cadpole, self.ws = opt["cadpole"], opt["workspace"]
        self.work = os.path.abspath(opt["work"])
        self.media = os.path.abspath(opt.get("media", os.path.join(self.work, "media")))
        self.threads = opt["threads"]
        self.grid = "--no-ground-grid" not in self.flags
        self.site = os.path.join(self.ws, "site")
        self.static = os.path.join(self.site, "static")

    def script(self, rel):
        """A script named from the workspace root: examples/x.star, site/features/y.star."""
        return os.path.join(self.ws, rel)

    def cli(self, *args, capture=False):
        cmd = ["nice", "-n", "19", self.cadpole, *[str(a) for a in args]]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode:
            sys.exit(f"cadpole {args[0]} failed:\n{r.stderr[-2000:]}")
        return r.stdout if capture else None


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", *[str(a) for a in args]], check=True)


def newer(dst, src):
    return os.path.exists(dst) and os.path.getmtime(dst) >= os.path.getmtime(src)


def size_of(png):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", png],
                         capture_output=True, text=True, check=True).stdout.strip()
    w, h = out.split("x")
    return int(w), int(h)


def unwrap(text, out):
    """The file inside what `cadpole op Drawing` or `op Render` printed (base64 in `data` or `png`)."""
    d = json.loads(text)
    errs = [x["message"] for x in d.get("diagnostics", []) if x.get("severity") == "SEVERITY_ERROR"]
    key = "png" if "png" in d else "data"
    if errs or key not in d:
        sys.exit("error: " + "; ".join(errs or ["no file in the response"]))
    with open(out, "wb") as f:
        f.write(base64.b64decode(d[key]))


# ---- stills -------------------------------------------------------------------
def stills(run):
    """Each still: a transparent 1500 px master, flattened onto the page colour, then the sizes the page asks for."""
    os.makedirs(f"{run.work}/stills", exist_ok=True)
    for name, (script, extra) in sets.STILLS.items():
        if run.args and name not in run.args:
            continue
        master = f"{run.work}/stills/{name}.png"
        if not os.path.exists(master) or run.args:
            run.cli("render", run.script(script), "--size", "1500x1500", "--samples", "96", *LOOK, "--threads", run.threads, *extra, "-o", master + ".tmp.png")
            os.replace(master + ".tmp.png", master)
        flat = f"{run.work}/stills/{name}.flat.png"
        w, h = size_of(master)
        ffmpeg("-i", master, "-filter_complex", f"color=c={PAGE}:s={w}x{h},format=rgba[bg];[0:v]format=rgba[fg];[bg][fg]overlay=format=auto,format=rgb24", "-frames:v", "1", flat)
        widths = [(1200, 84), (720, 80)] + ([(360, 80)] if name.startswith("feat-") else [])
        for w, q in widths:
            ffmpeg("-i", flat, "-vf", f"scale={w}:-2:flags=lanczos", "-frames:v", "1", "-c:v", "libwebp", "-quality", q, "-compression_level", "6", "-preset", "picture",
                   f"{run.static}/media/{name}-{w}.webp")
        print("still", name, flush=True)


# ---- framing ---------------------------------------------------------------------
def alpha_box(png):
    """The box of what a transparent master shows, in its pixels: (x0, y0, x1, y1)."""
    err = subprocess.run(["ffmpeg", "-hide_banner", "-i", png, "-vf", "alphaextract,bbox=min_val=8", "-frames:v", "1", "-f", "null", "-"],
                         capture_output=True, text=True, check=True).stderr
    box = dict(kv.split(":") for kv in err[err.rindex("x1:"):].split()[:4])
    return int(box["x1"]), int(box["y1"]), int(box["x2"]) + 1, int(box["y2"]) + 1


def flatten(src, dst, window, out, grid=None):
    """The `window` (x, y, width, height; it may reach past the picture) of a transparent master, on the
    page colour, over the ground grid when there is one (an SVG the size of the window), scaled to `out`."""
    x, y, w, h = window
    inputs, under = ["-i", src], "[bg]"
    fc = f"color=c={PAGE}:s={w}x{h},format=rgba[bg];"
    if grid:
        with open(dst + ".grid.svg", "w") as f:
            f.write(grid)
        inputs += ["-i", dst + ".grid.svg"]
        fc += "[1:v]format=rgba[gr];[bg][gr]overlay=0:0:format=auto[bg2];"
        under = "[bg2]"
    fc += f"[0:v]format=rgba[fg];{under}[fg]overlay={-x}:{-y}:format=auto,scale={out[0]}:{out[1]}:flags=lanczos,format=rgb24"
    ffmpeg(*inputs, "-filter_complex", fc, "-frames:v", "1", dst)
    if grid:
        os.remove(dst + ".grid.svg")


# ---- the square scenes: one animation each -------------------------------------
def smooth(a, r=4):
    return [sum(a[max(0, i - r):i + r + 1]) / len(a[max(0, i - r):i + r + 1]) for i in range(len(a))]


def frames(run):
    """`cadpole animate` renders the script's scroll_story at 1500 px, transparent, and writes every frame's
    camera. An animation's camera is fitted once to everything it ever shows, so a model that comes apart
    starts small: each frame is then cropped to a square about the model's own centre, sized so the model
    fills `fill` of it, the window smoothed over time so the picture never jumps. Downscale only."""
    if not run.args or run.args[0] not in sets.FRAMES:
        sys.exit(f"frames: one of {', '.join(sets.FRAMES)}")
    name = run.args[0]
    script, n, fill = sets.FRAMES[name]
    raw, out = f"{run.work}/frames/{name}", f"{run.work}/framed/{name}"
    os.makedirs(raw, exist_ok=True)
    os.makedirs(out, exist_ok=True)
    camfile = f"{run.work}/frames/{name}.camera.json"
    if len(glob.glob(f"{raw}/frame_*.png")) != n or not os.path.exists(camfile):
        run.cli("animate", run.script(script), "--name", "scroll_story", "--frames", n, "--size", "1500x1500", "--format", "frames", "--samples", "64", *LOOK,
                "--camera-json", camfile, "--threads", run.threads, "-o", raw + "/")
    cam = json.load(open(camfile))
    W, fr = cam["width"], cam["frames"]
    win, cx, cy = [], [], []
    for f in fr:
        x0, y0, x1, y1 = f["bounds"]["px"]
        win.append(min(W, max(x1 - x0, y1 - y0) / fill))
        cx.append((x0 + x1) / 2)
        cy.append((y0 + y1) / 2)
    win, cx, cy = smooth(win), smooth(cx), smooth(cy)
    ground = ground_grid.plane(fr)
    minor = ground_grid.spacing(fr[0], ground)

    def one(i):
        s = round(win[i] / 2) * 2
        x = min(max(0, round(cx[i] - s / 2)), W - s)
        y = min(max(0, round(cy[i] - s / 2)), W - s)
        window = (x, y, s, s)
        flatten(f"{raw}/frame_{i:04d}.png", f"{out}/frame_{i:04d}.png", window, (1500, 1500),
                ground_grid.svg(fr[i], (W, cam["height"]), window, ground, minor) if run.grid else None)
    with ThreadPoolExecutor(6) as ex:
        list(ex.map(one, range(len(fr))))
    print(f"{name}: {len(fr)} frames, window {min(win):.0f}..{max(win):.0f} px of {W}; now: encode")


# ---- the jet: one render per frame ----------------------------------------------
def ss(x):
    return 0 if x <= 0 else 1 if x >= 1 else x * x * (3 - 2 * x)


def jet(run):
    """The turbojet, cut away throughout, assembled to apart in one direction under a slow orbit. Each frame
    is its own render, fitted to the model, so the model is centred in every one; framing then sets its share
    of the picture's width, eased from the first frame to the last, for a 16:9 set and a 4:3 one."""
    N = sets.JET["frames"]
    first = int(run.args[0]) if run.args else 0
    last = int(run.args[1]) if len(run.args) > 1 else N - 1
    raw = f"{run.work}/frames/jet"
    os.makedirs(raw, exist_ok=True)
    for i in range(first, last + 1):
        f = f"{raw}/{i:03d}.png"
        if os.path.exists(f) and os.path.exists(f + ".json"):
            continue
        t = i / (N - 1)
        pose = [run.script(sets.JET["script"]), "--param", f"explode={18 * ss((t - .06) / .86):.4f}", "--param", "cutaway=true",
                "--view", f"az={-32 + 14 * ss(t):.3f},el={16 + 3 * ss(t):.3f}", "--fov", "24", "--size", "2160x1440", "--threads", run.threads]
        if os.path.exists(f):
            # A master without its camera: the camera comes from the model and the view, not from the
            # backend, so a one-sample raster pass writes the same one.
            run.cli("render", *pose, "--backend", "raster", "--samples", "1", "--lighting", "product", "--floor", "none", "--background", "transparent",
                    "--no-grid", "--camera-json", f + ".json", "-o", f + ".camera.tmp.png")
            os.remove(f + ".camera.tmp.png")
            print("camera", i, flush=True)
            continue
        run.cli("render", *pose, "--samples", "64", *LOOK, "--camera-json", f + ".json", "-o", f + ".tmp.png")
        os.replace(f + ".tmp.png", f)
        print("frame", i, flush=True)
    have = [i for i in range(N) if os.path.exists(f"{raw}/{i:03d}.png") and os.path.exists(f"{raw}/{i:03d}.png.json")]
    cams = {}
    for i in have:
        c = json.load(open(f"{raw}/{i:03d}.png.json"))
        cams[i] = c["frames"][0] if "frames" in c else c
    if run.grid and have:
        ground = ground_grid.plane(cams.values())
        minor = ground_grid.spacing(cams[have[0]], ground)

    def one(i):
        src = f"{raw}/{i:03d}.png"
        w, h = size_of(src)
        # The model's box is measured from the picture: the camera file's is the box of its bounding
        # box, which is looser than a long model seen at an angle.
        x0, y0, x1, y1 = alpha_box(src)
        for name, (W, H, f0, f1, hmax) in sets.JET["framing"].items():
            s = min((f0 + (f1 - f0) * ss(i / (N - 1))) * W / (x1 - x0), hmax * H / (y1 - y0), 1.0)
            cw, ch = round(W / s / 2) * 2, round(H / s / 2) * 2
            window = (round((x0 + x1) / 2 - cw / 2), round((y0 + y1) / 2 - ch / 2), cw, ch)
            os.makedirs(f"{run.work}/framed/{name}", exist_ok=True)
            flatten(src, f"{run.work}/framed/{name}/frame_{i:04d}.png", window, (W, H),
                    ground_grid.svg(cams[i], (w, h), window, ground, minor) if run.grid else None)
    with ThreadPoolExecutor(6) as ex:
        list(ex.map(one, have))
    print(f"jet: {len(have)} of {N} frames rendered and framed; now: encode")


# ---- encoding -------------------------------------------------------------------
def encode(run):
    """Framed masters -> <media>/seq/<set>/
         d/NNN.avif  wide screens (AVIF: about half the bytes of WebP)
         w/NNN.webp  Safari on a wide screen (its AVIF decode is too slow to scrub)
         m/NNN.webp  phones: every second frame
         x/NNN.avif  the jet only: a 1920 set for sharp wide screens
       and each scene's first frame into static/seq/, which is in the tree."""
    jobs = []

    def job(src, dst, size, kind):
        if newer(dst, src):
            return
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        codec = (["-c:v", "libaom-av1", "-still-picture", "1", "-crf", "26", "-cpu-used", "5", "-pix_fmt", "yuv444p"] if kind == "avif"
                 else ["-c:v", "libwebp", "-quality", str(kind), "-compression_level", "6", "-preset", "picture"])
        jobs.append(["-i", src, "-vf", f"scale={size}:-2:flags=lanczos", "-frames:v", "1", *codec, dst])
    for seq, (wide, narrow, n, xl) in sets.ENCODE.items():
        fw = sorted(glob.glob(f"{run.work}/framed/{wide}/frame_*.png"))
        fn = sorted(glob.glob(f"{run.work}/framed/{narrow}/frame_*.png"))
        if len(fw) != n or len(fn) != n:
            print(f"seq {seq}: {len(fw)}/{n} frames, skipped")
            continue
        out = f"{run.media}/seq/{seq}"
        for i, f in enumerate(fw):
            job(f, f"{out}/d/{i:03d}.avif", 1440, "avif")
            job(f, f"{out}/w/{i:03d}.webp", 1280, 82)
            if xl:
                job(f, f"{out}/x/{i:03d}.avif", 1920, "avif")
        nm = n // 2
        for i in range(nm):
            job(fn[round(i * (n - 1) / (nm - 1))], f"{out}/m/{i:03d}.webp", 960, 82)
    with ThreadPoolExecutor(8) as ex:
        list(ex.map(lambda j: ffmpeg(*j), jobs))
    for seq in sets.ENCODE:
        for v, ext in (("d", "avif"), ("m", "webp")):
            src = f"{run.media}/seq/{seq}/{v}/000.{ext}"
            if os.path.exists(src):
                os.makedirs(f"{run.static}/seq/{seq}/{v}", exist_ok=True)
                with open(src, "rb") as a, open(f"{run.static}/seq/{seq}/{v}/000.{ext}", "wb") as b:
                    b.write(a.read())
    print(f"encoded {len(jobs)} pictures into {run.media}; now: lock {run.media}")


def lock(run):
    """media.lock.json from a media directory: what `assemble` will accept."""
    media = os.path.abspath(os.path.join(os.environ.get("BUILD_WORKING_DIRECTORY", "."), run.args[0])) if run.args else run.media
    sha = lambda p: hashlib.sha256(open(p, "rb").read())
    out = {"sets": {}, "files": {}}
    for seq in sorted(os.listdir(f"{media}/seq")):
        for v in sorted(os.listdir(f"{media}/seq/{seq}")):
            d = f"{media}/seq/{seq}/{v}"
            if not os.path.isdir(d):
                continue
            h, files = hashlib.sha256(), sorted(os.listdir(d))
            for f in files:
                h.update(f.encode() + b"\0" + sha(f"{d}/{f}").digest())
            out["sets"][f"seq/{seq}/{v}"] = {"count": len(files), "bytes": sum(os.path.getsize(f"{d}/{f}") for f in files), "sha256": h.hexdigest()}
    for f in sets.RECORDING:
        p = f"{media}/{f}"
        if not os.path.exists(p):
            sys.exit(f"{p}: the recording is not re-made by this tool; copy it from the published site")
        out["files"][f] = {"bytes": os.path.getsize(p), "sha256": sha(p).hexdigest()}
    with open(f"{run.site}/media.lock.json", "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")
    print("wrote site/media.lock.json")


# ---- vector drawings ------------------------------------------------------------
def drawings(run):
    """Every drawing on the page, from its script: the file as the CLI writes it (<name>.raw.svg, .pdf, .dxf)
    and the copy the page sets inline (<name>.svg: no colours of its own, so the stylesheet colours it)."""
    out = f"{run.static}/draw"
    tmp = f"{run.work}/drawings"
    os.makedirs(tmp, exist_ok=True)

    def draw(script, view, name, extra, formats=("svg", "pdf", "dxf")):
        for fmt in formats:
            dst = f"{out}/{name}.raw.svg" if fmt == "svg" else f"{out}/{name}.{fmt}"
            unwrap(run.cli("op", "Drawing", "--doc", run.script(script), "--view", view, "--format", fmt, *extra, capture=True), dst)

    def theme(tool, name, *args, raw=None):
        subprocess.run([sys.executable, f"{HERE}/{tool}", raw or f"{out}/{name}.raw.svg", f"{out}/{name}.svg", *args], check=True)
    # `drawings sheet-thumbscrew ...` redraws only the named ones; "cabin" names the plan sheets
    for name, d in sets.DRAWINGS.items():
        if run.args and name not in run.args:
            continue
        draw(d["script"], d["view"], name, d["flags"], d.get("formats", ("svg",)))
        args = ["--prefix", name, "--title", d["title"], *d.get("theme", [])]
        if "frame" in d:
            args += ["--frame", d["frame"], "--size", "1500"]
        if "pair" in d:
            # A line view and a path-traced picture of the same model from the same camera, both parallel
            # projections, the picture covering exactly the drawing's frame.
            model, az, el, tol, px = d["pair"]
            png = f"{tmp}/{name}-{px}.png"
            if not os.path.exists(png):
                # What has no flag: the parallel projection, and the page's colour in linear light. At
                # this size the picture needs no supersampling.
                req = {"camera": {"orthographic": True}, "background": {"r": 0.0018211, "g": 0.0030353, "b": 0.0027317}}
                i = PAIR_LOOK.index("--supersample")
                look = PAIR_LOOK[:i + 1] + ["1"] + PAIR_LOOK[i + 2:]
                unwrap(run.cli("op", "Render", "--doc", run.script(model), "--width", px, "--height", px, "--threads", run.threads, "--tolerance", tol,
                               "--view", f"az={az},el={el}", *look, "--json", json.dumps(req), capture=True), png)
            ffmpeg("-f", "lavfi", "-i", f"color=c={PAGE}:s={px}x{px}", "-i", png, "-filter_complex", "[0][1]overlay", "-frames:v", "1",
                   "-c:v", "libwebp", "-quality", "82", f"{out}/{name}-{px}.webp")
        theme("theme_drawing.py", name, *args)
        print("drawing", name, flush=True)
    if run.args and "cabin" not in run.args:
        print("drawings done")
        return
    # The cabin: the plan as the script stands, the same sheet at five widths of the living room (all in
    # the widest one's viewBox, so the page can swap them in place), a section and an elevation.
    plan = sets.PLAN["script"]
    draw(plan, "sheet:A1", "plan", [])
    widths = sets.PLAN["widths"]
    raws = []
    for i, w in enumerate(widths):
        raw = f"{tmp}/plan-s{i}.raw.svg"
        unwrap(run.cli("op", "Drawing", "--doc", run.script(plan), "--view", "sheet:A1", "--format", "svg", "--param", f"living_w={w}", capture=True), raw)
        raws.append(raw)
    widest = f"{tmp}/plan-wide.svg"
    subprocess.run([sys.executable, f"{HERE}/theme_plan.py", raws[-1], widest, "--title", "Cabin floor plan"], check=True)
    for i, w in enumerate(widths):
        subprocess.run([sys.executable, f"{HERE}/theme_plan.py", raws[i], f"{out}/plan-s{i}.svg", "--title", f"Cabin floor plan, living room {w:g} ft wide", "--viewbox-of", widest], check=True)
    for sheet, name, title in (("A2", "section", "Section through living room and kitchen"), ("A3", "elevation", "South elevation")):
        raw = f"{tmp}/{name}.raw.svg"
        unwrap(run.cli("op", "Drawing", "--doc", run.script(plan), "--view", f"sheet:{sheet}", "--format", "svg", capture=True), raw)
        subprocess.run([sys.executable, f"{HERE}/theme_plan.py", raw, f"{out}/{name}.svg", "--prefix", name, "--title", title], check=True)
    print("drawings done")


# ---- the building sections ---------------------------------------------------------
def buildings(run):
    """The cabin, the lot, the shadow study and the solved rooms as raster renders (seconds each, on the page's colour),
    one sheet of the house set as a themed drawing beside the whole set as the CLI writes it, and the rooms
    the solver chose, read from what the script prints."""
    tmp = f"{run.work}/buildings"
    os.makedirs(tmp, exist_ok=True)
    for name, (script, extra) in sets.BUILDINGS.items():
        master = f"{tmp}/{name}.png"
        run.cli("render", run.script(script), "--background", "#" + PAGE[2:], "--no-grid", "--supersample", "2", *extra, "-o", master)
        for w, q in ((1200, 84), (720, 80)):
            ffmpeg("-i", master, "-vf", f"scale={w}:-2:flags=lanczos", "-frames:v", "1", "-c:v", "libwebp", "-quality", q, "-compression_level", "6", "-preset", "picture",
                   f"{run.static}/media/{name}-{w}.webp")
        print("building", name, "x".join(str(v) for v in size_of(master)), flush=True)
    d = sets.SET
    raw = f"{run.static}/draw/{d['name']}.raw.svg"
    unwrap(run.cli("op", "Drawing", "--doc", run.script(d["script"]), "--view", d["view"], "--format", "svg", capture=True), raw)
    subprocess.run([sys.executable, f"{HERE}/theme_plan.py", raw, f"{run.static}/draw/{d['name']}.svg", "--prefix", d["prefix"], "--title", d["title"]], check=True)
    run.cli("export", run.script(d["script"]), "--run-exports", "--out-dir", tmp)
    os.replace(f"{tmp}/{d['pdf']}", f"{run.static}/draw/{d['pdf']}")
    print("drawing", d["name"], flush=True)
    # "living: 14.00 x 28.12 ft at (0.00, 0.00)" and "plate 28.125 ft deep, 0.0 of the 1,350 sq ft cap left"
    out = subprocess.run([run.cadpole, "eval", run.script(sets.LAYOUT)], capture_output=True, text=True, check=True)
    text = out.stdout + out.stderr
    rooms = [{"name": m[0], "w": float(m[1]), "d": float(m[2]), "x": float(m[3]), "y": float(m[4])}
             for m in re.findall(r"print: (\w+): ([\d.]+) x ([\d.]+) ft at \(([\d.]+), ([\d.]+)\)", text)]
    plate = re.search(r"print: plate ([\d.]+) ft deep, ([\d.]+) of the ([\d,]+) sq ft cap left", text)
    solve = re.search(r"optimize .*constraints=(\d+).*status=(\w+) variables=(\d+)", text)
    if not rooms or not plate or not solve:
        sys.exit("the layout script printed something else:\n" + text[-1500:])
    with open(f"{run.site}/content/layout.json", "w") as f:
        json.dump({"rooms": rooms, "depth": float(plate[1]), "left": float(plate[2]), "cap": int(plate[3].replace(",", "")),
                   "constraints": int(solve[1]), "variables": int(solve[3]), "status": solve[2]}, f, indent=1)
        f.write("\n")
    print("wrote site/content/layout.json")


def main():
    argv = sys.argv[1:]
    cmd = next((a for a in argv if a in ("stills", "frames", "jet", "drawings", "buildings", "encode", "lock", "help")), "help")
    if cmd == "help":
        print(__doc__)
        return
    argv.remove(cmd)
    {"stills": stills, "frames": frames, "jet": jet, "drawings": drawings, "buildings": buildings, "encode": encode, "lock": lock}[cmd](Run(argv))


main()
