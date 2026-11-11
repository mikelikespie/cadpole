#!/usr/bin/env python3
"""Restyle a sheet SVG written by the cadpole CLI for inlining in a dark page.

usage: theme.py in.raw.svg out.svg [--prefix plan] [--title "Cabin floor plan"]
                [--viewbox-of other.svg]

What it does, and nothing else:
  * drops the sheet frame and title column (layer G-ANNO-TTLB and after)
  * maps every colour the CLI wrote to a class; colours then come from CSS
    custom properties with dark-palette fallbacks, inks from currentColor
  * plan sheets: turns the wall rectangles minus rooms and openings into one
    outline path, so rooms and openings are see-through on any page colour
  * crops the viewBox to what is left, removes width/height, rounds numbers,
    prefixes every id and class
The geometry is the CLI's; no line is added that the CLI did not draw.
"""
import re
import sys
from html import escape, unescape

KIND = {"#f7ebd3": "living", "#f6dcc0": "kitchen", "#e6dff0": "bed",
        "#d7e7f3": "bath", "#ebe9e4": "entry", "#e0ecd5": "deck"}
WALL, OPENING, WHITE = "#4a4540", "#f4f1ea", "#ffffff"


def attrs(tag):
    return dict(re.findall(r'([\w-]+)="([^"]*)"', tag))


def num(v):
    s = ("%.2f" % v).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def points(d):
    n = [float(x) for x in re.findall(r"-?\d+\.?\d*(?:e-?\d+)?", d)]
    return list(zip(n[0::2], n[1::2]))


def redo(d):
    return re.sub(r"-?\d+\.?\d*(?:e-?\d+)?", lambda m: num(float(m.group(0))), d).replace(" Z", "Z")


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


def outline(cells, xs, ys):
    """Boundary loops of a set of grid cells, as one path."""
    edges = set()
    for (i, j) in cells:
        for e in (((i, j), (i + 1, j)), ((i + 1, j), (i + 1, j + 1)),
                  ((i + 1, j + 1), (i, j + 1)), ((i, j + 1), (i, j))):
            if (e[1], e[0]) in edges:
                edges.remove((e[1], e[0]))
            else:
                edges.add(e)
    nxt = {}
    for a, b in edges:
        nxt.setdefault(a, []).append(b)
    out = []
    while nxt:
        start = min(nxt)
        loop, cur = [start], start
        while True:
            b = nxt[cur].pop()
            if not nxt[cur]:
                del nxt[cur]
            if b == start:
                break
            loop.append(b)
            cur = b
        keep = []
        for k, p in enumerate(loop):
            a, c = loop[k - 1], loop[(k + 1) % len(loop)]
            if not ((a[0] == p[0] == c[0]) or (a[1] == p[1] == c[1])):
                keep.append(p)
        out.append("M" + "L".join("%s %s" % (num(xs[i]), num(ys[j])) for i, j in keep) + "Z")
    return "".join(out)


def main(argv):
    src, dst = argv[1], argv[2]
    opt = {"--prefix": "plan", "--title": "Drawing", "--viewbox-of": ""}
    for k in range(3, len(argv) - 1, 2):
        opt[argv[k]] = argv[k + 1]
    p = opt["--prefix"]

    group, stop, seen_window, knocked = {}, False, False, False
    walls, holes = [], []
    rooms, labels = [], []   # (kind, rect) / (x, y, text)
    paths = {}               # class -> [d]
    order = []
    widths = {}
    texts = []               # (class, attrs, string)
    boxes = []

    def add(cls, d, width=None):
        if cls not in paths:
            paths[cls] = []
            order.append(cls)
        paths[cls].append(redo(d))
        if width:
            widths.setdefault(cls, width)
        pts = points(d)
        boxes.append((min(x for x, _ in pts), min(y for _, y in pts),
                      max(x for x, _ in pts), max(y for _, y in pts)))

    for line in open(src, encoding="utf-8"):
        t = line.strip()
        if t.startswith("<g "):
            group = attrs(t)
            group["layer"] = group.get("id") or group.get("class")
            group["first"] = "id" in group
            if group.get("id") == "G-ANNO-TTLB":
                stop = True
            continue
        if stop:
            continue
        if t.startswith("<path "):
            a = attrs(t)
            d = a["d"]
            fill = a.get("fill", group.get("fill", "none"))
            stroke = a.get("stroke", group.get("stroke", "none"))
            sw = a.get("stroke-width", group.get("stroke-width"))
            layer = group["layer"]
            if layer == "A-ANNO-DIMS":
                add(p + "-dim", d, sw)
            elif layer == "M-VIEW":
                add(p + "-line", d, sw)
            elif layer == "M-SECT-PATT":
                # A filled cut region (a plan's poché) or a hatch stroke (a section's earth and wall hatch).
                add(p + ("-cut" if fill != "none" else "-hatch"), d, None if fill != "none" else sw)
            elif layer == "M-SECT":
                add(p + "-cutline", d, sw)
            elif fill != "none":
                pts = points(d)
                r = (min(x for x, _ in pts), min(y for _, y in pts),
                     max(x for x, _ in pts), max(y for _, y in pts))
                if fill == WALL:
                    walls.append(r)
                    boxes.append(r)
                elif fill in KIND:
                    if group["first"]:      # a room; the later ones are label backings
                        rooms.append((KIND[fill], r, d))
                        holes.append(r)
                elif fill == OPENING:
                    holes.append(r)
                elif fill == WHITE:
                    if not walls and not rooms:
                        # A sheet with no plan walls: white is a knock-out (an elevation's ground mask) and
                        # is painted in the card's colour, over what was drawn before it.
                        add(p + "-knock", d)
                        knocked = True
                    elif not seen_window:
                        holes.append(r)     # the gap a window leaves in the wall
                    # fixture backings are dropped: the outlines carry them
                else:
                    add(p + "-fill", d)
            elif "stroke-dasharray" in a:
                add(p + "-deck-edge", d, sw)
            elif stroke == "#6f6a62":
                seen_window = True
                add(p + "-window", d, sw)
            elif stroke == "#5b6f80":
                seen_window = True
                add(p + "-glass", d, sw)
            elif stroke == "#1f6fd1":
                add(p + "-glass-egress", d, sw)
            elif stroke == "#5d6670":
                add(p + "-fixture", d, "0.2")
            elif stroke == "#9a948a":
                add(p + "-furniture", d, sw)
            elif stroke == "#2b2a28":
                add(p + ("-door-swing" if sw == "0.18" else "-door"), d, sw)
            else:
                # Lines after a knock-out are drawn over it, so they get a class of their own, later in the file.
                add(p + ("-line-over" if knocked else "-line"), d, sw)
        elif t.startswith("<text "):
            a = attrs(t[:t.index(">")])
            s = unescape(re.search(r">([^<]*)<", t).group(1))
            fill = a.get("fill")
            if fill == "#2b2a28":
                cls = p + "-label"
                labels.append((float(a["x"]), float(a["y"]), s))
            elif fill == "#6f6a62":
                cls = p + ("-area" if "sq ft" in s else "-size")
            elif fill == "#3c3c3c":
                cls = p + "-dimtext"
            elif fill == "#5d6670":
                cls = p + "-note"
            else:
                cls = p + "-title"
            texts.append((cls, a, s))
            x, y, fs = float(a["x"]), float(a["y"]), float(a["font-size"])
            wd = 0.62 * fs * len(s)
            x0 = x - wd / 2 if a.get("text-anchor") == "middle" else x
            if "transform" in a:
                boxes.append((x - fs, y - wd / 2, x + 0.3 * fs, y + wd / 2))
            else:
                boxes.append((x0, y - fs, x0 + wd, y + 0.3 * fs))

    out = []
    # rooms, each with the name the CLI printed inside it
    for kind, r, d in rooms:
        name = [s for (x, y, s) in labels if r[0] < x < r[2] and r[1] < y < r[3]]
        rid = ' id="%s-room-%s"' % (p, slug(name[0])) if name else ""
        out.append('<path%s class="%s-room %s-room-%s" d="%s"/>' % (rid, p, p, kind, redo(d)))
        boxes.append(r)
    if walls:
        xs = sorted(set(round(v, 3) for r in walls + holes for v in (r[0], r[2])))
        ys = sorted(set(round(v, 3) for r in walls + holes for v in (r[1], r[3])))

        def inside(rs, cx, cy):
            return any(r[0] < cx < r[2] and r[1] < cy < r[3] for r in rs)
        cells = set()
        for i in range(len(xs) - 1):
            for j in range(len(ys) - 1):
                cx, cy = (xs[i] + xs[i + 1]) / 2, (ys[j] + ys[j + 1]) / 2
                if inside(walls, cx, cy) and not inside(holes, cx, cy):
                    cells.add((i, j))
        out.append('<path id="%s-walls" class="%s-wall" fill-rule="evenodd" d="%s"/>' % (p, p, outline(cells, xs, ys)))
    for cls in order:
        ds = paths[cls]
        if cls.endswith("-cut") or cls.endswith("-fill") or cls.endswith("-knock"):
            out.append('<path class="%s" fill-rule="evenodd" d="%s"/>' % (cls, "".join(ds)))
        else:
            out.append('<path class="%s" d="%s"/>' % (cls, "".join(ds)))
    for cls, a, s in texts:
        x, y = float(a["x"]), float(a["y"])
        extra = ""
        if a.get("text-anchor", "start") != "start":
            extra += ' text-anchor="%s"' % a["text-anchor"]
        if "transform" in a:
            extra += ' transform="rotate(-90 %s %s)"' % (num(x), num(y))
        out.append('<text class="%s" x="%s" y="%s" font-size="%s"%s>%s</text>'
                   % (cls, num(x), num(y), num(float(a["font-size"])), extra, escape(s, quote=False)))

    pad = 2.0
    bx = (min(b[0] for b in boxes) - pad, min(b[1] for b in boxes) - pad,
          max(b[2] for b in boxes) + pad, max(b[3] for b in boxes) + pad)
    vb = "%s %s %s %s" % (num(bx[0]), num(bx[1]), num(bx[2] - bx[0]), num(bx[3] - bx[1]))
    if opt["--viewbox-of"]:
        vb = re.search(r'viewBox="([^"]*)"', open(opt["--viewbox-of"]).read()).group(1)

    q = ".%s-drawing" % p
    css = [
        "%s{color:var(--plan-ink,#e8efe9)}" % q,
        "%s path{fill:none;stroke:currentColor;stroke-linecap:round;stroke-linejoin:round}" % q,
        "%s text{fill:currentColor;stroke:none;font-family:inherit}" % q,
        "%s .%s-wall{fill:var(--plan-wall,currentColor);stroke:none}" % (q, p),
        "%s .%s-room{stroke:none;fill:var(--plan-accent,#6fbfa8);fill-opacity:.09}" % (q, p),
        "%s .%s-room-living,%s .%s-room-kitchen{fill:var(--plan-warm,#f0c987)}" % (q, p, q, p),
        "%s .%s-room-kitchen,%s .%s-room-bath{fill-opacity:.15}" % (q, p, q, p),
        "%s .%s-room-entry,%s .%s-room-deck{fill:var(--plan-muted,#8fa39a)}" % (q, p, q, p),
        "%s .%s-deck-edge{stroke:var(--plan-muted,#8fa39a);stroke-dasharray:1.2 .8}" % (q, p),
        "%s .%s-window{stroke:var(--plan-muted,#8fa39a)}" % (q, p),
        "%s .%s-glass{stroke:var(--plan-accent,#6fbfa8)}" % (q, p),
        "%s .%s-glass-egress{stroke:var(--plan-warm,#f0c987)}" % (q, p),
        "%s .%s-fixture{stroke:var(--plan-muted,#8fa39a)}" % (q, p),
        "%s .%s-furniture{stroke:var(--plan-furniture,#c4b493)}" % (q, p),
        "%s .%s-dim{stroke:var(--plan-muted,#8fa39a)}" % (q, p),
        "%s .%s-cut{stroke:none;fill:var(--plan-muted,#8fa39a);fill-opacity:.55}" % (q, p),
        "%s .%s-hatch{stroke:var(--plan-muted,#8fa39a);stroke-opacity:.7}" % (q, p),
        "%s .%s-knock{stroke:none;fill:var(--bg2,#0b1110)}" % (q, p),
        "%s .%s-label,%s .%s-title{font-weight:700}" % (q, p, q, p),
        "%s .%s-area,%s .%s-size,%s .%s-note{fill:var(--plan-muted,#8fa39a)}" % (q, p, q, p, q, p),
    ]
    for cls in order:
        if cls in widths:
            css.append("%s .%s{stroke-width:%s}" % (q, cls, widths[cls]))
    used = " ".join(out)
    css = [c for c in css[:3]] + [c for c in css[3:] if any(("%s-%s" % (p, m)) in used for m in re.findall(r"\.%s-([a-z-]+)" % p, c))]
    title = escape(opt["--title"], quote=False)
    svg = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="%s" class="%s-drawing" role="img" aria-labelledby="%s-title">\n'
           '<title id="%s-title">%s</title>\n<style>%s</style>\n%s\n</svg>\n'
           % (vb, p, p, p, title, "".join(css), "\n".join(out)))
    open(dst, "w", encoding="utf-8").write(svg)
    print("%s viewBox %s, %d bytes" % (dst, vb, len(svg.encode())))


if __name__ == "__main__":
    main(sys.argv)
