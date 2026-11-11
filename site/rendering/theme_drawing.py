#!/usr/bin/env python3
"""theme.py: make a drawing SVG from the CLI inline-able and themeable.

usage: theme.py <in.raw.svg> <out.svg> --prefix ID [--title TEXT]
                [--frame X,Y,W,H --size N]   map this square of the sheet to 0..N
                [--only vis,hid,...]         keep only these line kinds
                [--weights vis=2.2,hid=1.1]  stroke widths, output units
                [--dash A,B]                 hidden-line dashes, output units
                [--places N]                 decimals kept in coordinates (2)
                [--cut-dashes 1]             hidden lines cut into their dashes in the file,
                                             not dashed by the renderer
                [--tiles MM]                 one path per MM square of the sheet, not one
                                             per line kind, so a renderer zoomed into one
                                             corner can skip every path off screen

The CLI writes black ink on fixed paper: one <g> per layer with a stroke
colour and a width in mm. This rewrites it with no colours at all:

  layer         class       what it is
  M-VIEW        cad-vis     visible edges and outlines
  M-VIEW-HIDD   cad-hid     hidden edges (dashed)
  M-SECT        cad-cut     the outline of a section cut
  M-SECT-PATT   cad-hatch   the hatch on a cut face
  A-ANNO-DIMS   cad-dim     dimension and extension lines, and their text
  A-ANNO-SYMB   cad-sym     centre lines, cutting planes, view title rings
  G-ANNO-TTLB   cad-frame   the sheet border and title block rules
  A-ANNO-TEXT   cad-text    view labels and title block text

Every stroke and text fill is currentColor, so the drawing takes the
colour of the element it is inlined in; `.cad-hid { stroke: ... }` and so
on recolour one kind of line. The root has a viewBox and no width or
height, there is no background, ids are prefixed, numbers are rounded.
Hidden lines are painted first so a visible edge always covers a dash
lying under it. Nothing is added or removed from the geometry except by
--only. Standard library only.
"""
import re, sys
import xml.etree.ElementTree as ET

NS = "{http://www.w3.org/2000/svg}"
KINDS = {  # layer -> (kind, paint order, stroke-opacity)
    "M-SECT-PATT": ("hatch", 0, 0.55),
    "M-VIEW-HIDD": ("hid", 1, 0.6),
    "M-VIEW": ("vis", 2, None),
    "M-SECT": ("cut", 3, None),
    "A-ANNO-DIMS": ("dim", 4, 0.75),
    "A-ANNO-SYMB": ("sym", 4.5, None),
    "G-ANNO-TTLB": ("frame", 5, None),
    "A-ANNO-TEXT": ("text", 6, None),
}


def dashes(d, on, off):
    """A polyline path cut into its dashes, as geometry: each dash a short line of its own."""
    out = []
    for sub in re.findall(r"M[^M]*", d):
        pts = [(float(x), float(y)) for x, y in re.findall(r"(-?[\d.]+) (-?[\d.]+)", sub)]
        if sub.rstrip().endswith("Z"):
            pts.append(pts[0])
        drawing, left, run = True, on, [pts[0]]
        for a, b in zip(pts, pts[1:]):
            seg = ((b[0] - a[0]) ** 2 + (b[1] - a[1]) ** 2) ** 0.5
            t = 0.0
            while seg - t > left:
                t += left
                p = (a[0] + (b[0] - a[0]) * t / seg, a[1] + (b[1] - a[1]) * t / seg)
                if drawing:
                    run.append(p)
                    out.append(run)
                else:
                    run = [p]
                drawing, left = not drawing, (on if not drawing else off)
            left -= seg - t
            if drawing:
                run.append(b)
        if drawing and len(run) > 1:
            out.append(run)
    return "".join("M" + "L".join("%s %s" % (num(x, 3), num(y, 3)) for x, y in r) for r in out)


def num(v, places=2):
    s = ("%.*f" % (places, v)).rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def main():
    a = sys.argv[1:]
    src, out = a[0], a[1]
    o = {a[i][2:]: a[i + 1] for i in range(2, len(a), 2)}
    prefix = o["prefix"]
    root = ET.parse(src).getroot()
    vb = [float(v) for v in root.get("viewBox").split()]
    if "frame" in o:
        fx, fy, fw, fh = [float(v) for v in o["frame"].split(",")]
        size = float(o.get("size", 1500))
        k, ox, oy, view = size / fw, fx, fy, (0, 0, size, size * fh / fw)
    else:
        k, ox, oy, view = 1.0, vb[0], vb[1], (0, 0, vb[2], vb[3])
    only = set(o["only"].split(",")) if "only" in o else None
    weights = dict(p.split("=") for p in o["weights"].split(",")) if "weights" in o else {}
    places = int(o.get("places", 2))
    tile = float(o["tiles"]) * k if "tiles" in o else 0
    X = lambda v: num((float(v) - ox) * k, places)
    Y = lambda v: num((float(v) - oy) * k, places)

    def path(d):
        t = re.findall(r"[MLAZ]|-?\d+\.?\d*(?:e-?\d+)?", d)
        res, i = [], 0
        while i < len(t):
            if t[i] in "ML":
                res.append("%s%s %s" % (t[i], X(t[i + 1]), Y(t[i + 2])))
                i += 3
            elif t[i] == "A":
                rx, ry = (num(float(v) * k, places) for v in t[i + 1:i + 3])
                res.append("A%s %s %s %s %s %s %s" % (rx, ry, t[i + 3], t[i + 4], t[i + 5], X(t[i + 6]), Y(t[i + 7])))
                i += 8
            elif t[i] == "Z":
                res.append("Z")
                i += 1
            else:
                sys.exit("theme.py: a path command this script does not know: " + d[:60])
        return "".join(res)

    groups = {}  # kind -> {(stroke width (mm), dash (mm) or None): [path data]}
    fills = {}   # kind -> [path data] of filled shapes
    texts = []   # (kind, element)
    for g in root.findall(NS + "g"):
        layer = g.get("id") or g.get("class")
        if layer not in KINDS:
            sys.exit("theme.py: a layer this script does not know: %s" % layer)
        kind = KINDS[layer][0]
        if only and kind not in only:
            continue
        w = float(g.get("stroke-width"))
        dash = g.get("stroke-dasharray") if kind != "hid" else None
        for e in g:
            tag = e.tag.replace(NS, "")
            if tag == "path":
                if e.get("fill", "none") != "none":
                    # an arrowhead: filled in the group's colour, not stroked
                    fills.setdefault(kind, []).append(path(e.get("d")))
                    continue
                groups.setdefault(kind, {}).setdefault((w, dash), []).append(path(e.get("d")))
            elif tag == "text":
                texts.append((kind, e))
            else:
                sys.exit("theme.py: an element this script does not know: " + tag)

    L = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="%s" class="cad-drawing" fill="none" '
         'stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">'
         % " ".join(num(v) for v in view)]
    title = o.get("title") or (root.findtext(NS + "title") or "")
    if title:
        L.append("<title>%s</title>" % title.replace("&", "&amp;").replace("<", "&lt;"))
    order = {v[0]: (v[1], v[2]) for v in KINDS.values()}
    for kind in sorted(groups, key=lambda n: order[n][0]):
        attrs = ""
        if order[kind][1]:
            attrs += ' stroke-opacity="%s"' % order[kind][1]
        cut = None
        if kind == "hid":
            dash = o.get("dash") or "%s,%s" % (num(2 * k), num(1 * k))
            if "cut-dashes" in o:
                cut = [float(v) for v in dash.split(",")]
            else:
                attrs += ' stroke-dasharray="%s"' % dash
        L.append('<g id="%s-%s" class="cad-%s"%s>' % (prefix, kind, kind, attrs))
        for w, dash in sorted(groups[kind], key=lambda wd: (-wd[0], wd[1] or "")):
            sw = weights.get(kind) or num(w * k, 3)
            da = ' stroke-dasharray="%s"' % ",".join(num(float(v) * k) for v in dash.split(",")) if dash else ""
            parts = groups[kind][(w, dash)]
            if cut:
                parts = [dashes(d, *cut) for d in parts]
                parts = [d for d in parts if d]
            if tile:
                # Every polyline is cut where it crosses into another square, so each path's
                # bounds stay within about one square and a renderer can skip, and need not
                # dash, the ones off screen.
                buckets = {}
                at = lambda x, y: (int(float(x) // tile), int(float(y) // tile))
                for d in parts:
                    if "A" in d:
                        x, y = re.match(r"M(-?[\d.]+) (-?[\d.]+)", d).groups()
                        buckets.setdefault(at(x, y), []).append(d)
                        continue
                    for sub in re.findall(r"M[^M]*", d):
                        pts = re.findall(r"(-?[\d.]+) (-?[\d.]+)", sub)
                        closed = sub.rstrip().endswith("Z")
                        if closed:
                            pts.append(pts[0])
                        run, here = [pts[0]], at(*pts[0])
                        for q in pts[1:]:
                            run.append(q)
                            t = at(*q)
                            if t != here:
                                buckets.setdefault(here, []).append("M" + "L".join("%s %s" % r for r in run))
                                run, here = [q], t
                        if len(run) > 1:
                            buckets.setdefault(here, []).append("M" + "L".join("%s %s" % r for r in run))
                runs = [buckets[b] for b in sorted(buckets)]
            else:
                runs = [parts]
            for run in runs:
                L.append('<path stroke-width="%s"%s d="%s"/>' % (sw, da, "".join(run)))
        if kind in fills:
            L.append('<path fill="currentColor" stroke="none" d="%s"/>' % "".join(fills.pop(kind)))
        L.append("</g>")
    by = {}
    for kind, e in texts:
        by.setdefault(kind, []).append(e)
    for kind in sorted(by, key=lambda n: order[n][0]):
        cls = "cad-dim-text" if kind == "dim" else "cad-text"
        L.append('<g id="%s-%s-text" class="%s" fill="currentColor" stroke="none">' % (prefix, kind, cls))
        for e in by[kind]:
            x, y = X(e.get("x")), Y(e.get("y"))
            at = ' x="%s" y="%s" font-size="%s"' % (x, y, num(float(e.get("font-size")) * k))
            if e.get("font-weight"):
                at += ' font-weight="%s"' % e.get("font-weight")
            if e.get("text-anchor", "start") != "start":
                at += ' text-anchor="%s"' % e.get("text-anchor")
            m = re.match(r"rotate\((-?[\d.]+) ", e.get("transform") or "")
            if m:
                at += ' transform="rotate(%s %s %s)"' % (num(float(m.group(1))), x, y)
            elif e.get("transform"):
                sys.exit("theme.py: a text transform this script does not know")
            txt = (e.text or "").replace("&", "&amp;").replace("<", "&lt;")
            L.append("<text%s>%s</text>" % (at, txt))
        L.append("</g>")
    L.append("</svg>")
    open(out, "w").write("\n".join(L) + "\n")


main()
