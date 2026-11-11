"""@cadpole/building.star -- architecture massing: levels, floor plates, roofs, decks, stairs, areas.

    load("@cadpole/building.star", "ft", "inch", "ft_in", "level", "floor",
         "hip_roof", "gable_roof", "shed_roof", "cupola", "deck", "stair",
         "garage_door", "elevator_shaft", "floor_area", "gross_floor_area",
         "lot_coverage", "footprint_of", "straight_skeleton", "roof_profile",
         "height_envelope", "height_check", "floor_area_cap")

Every length is in millimetres, as everywhere in Cadpole: `ft(45)` is
`45 * 304.8`, a plain number, so `ft(45) + inch(6)` just works. How a length
is *written back* -- `45'-6"` in `cadpole inspect`, on a drawing, in the web
UI -- is the document's `units(units.ft_in)` (the display convention),
not this library's business. The library records nothing when
loaded and every body-building function takes `name=` and forwards it.

## Conventions

* A **footprint** is a `Sketch` on XY (one loop) or a list of `(x, y)`
  points; it is read counter-clockwise whichever way it was drawn.
* A **level** is data: `level("L1", elevation, height)`. A **floor** is the
  mass on it: `floor(footprint, z, height)` is a plain box (`solid=True`)
  or, with `solid=False`, a slab with walls round it -- one exact body
  either way.
* **Pitch** is rise over run, a plain number: `pitch=6/12` or `pitch(6)`
  for the carpenter's `6:12`; `pitch("6:12")` reads the string.
* A roof stands on its **eave polygon**: the footprint offset outward by
  `overhang` (mitred corners), at `eave_z`. `fascia=` (default 0) is the
  vertical face at the eave; with it the eave is a small wall rather than
  a knife edge, which is also what makes a gable or shed on an irregular
  footprint an exact body (below).
* Code limits are parameters, and refusals are errors that name the
  number: a stair whose rise exceeds `code`'s is `fail`ed, not warned.

## How a roof is built

A hip roof's surface is the **straight skeleton** of its eave polygon
lifted by the pitch: every eave edge owns one roof plane through it at
the pitch, and the planes meet on the skeleton's arcs (hips at convex
corners, valleys at reflex ones, ridges where two edges' wavefronts meet).
`straight_skeleton(points)` computes it here, in Starlark: the wavefront
simulation of Aichholzer et al. -- every vertex moves along its bisector
so both its edges advance inward at unit speed, `u = (n_a + n_b) /
(1 + n_a . n_b)`; an **edge event** (an edge shrinks to nothing) merges its
two vertices, a **split event** (a reflex vertex runs into an edge) cuts
the wavefront in two, and a wavefront that has flattened onto a line is a
ridge. A node at time `t` is `t * pitch` high, because the offset of an
edge by `t` is where its plane has risen `t * pitch`.

The solid is then built without a loft's degenerate ridge section:

* a **convex** footprint whose skeleton nodes are all three-way (a
  rectangle, a trapezoid, an irregular pentagon): a prism over the eave
  polygon *offset outward by a margin*, cut by every edge's plane in turn
  (`intersect` with a half-space, a rotated box). Because the eave lines
  run through the middle of the oversized prism's bottom face rather than
  along its edges, every cut crosses a face transversally -- the exact
  kernel refuses a plane that contains an edge of what it cuts (the
  tangent case) -- and the margin falls away with the first cut. The cuts
  run on the **mesh kernel** by default, so the roof is a mesh body whose
  faces are the skeleton's planes and whose volume is the closed form
  (`V = W h (3 L - W) / 6` on a `W x L` rectangle, `L >= W`, ridge height
  `h = pitch * W / 2`, to the mesh's f32). `kernel="brep"` asks for the
  exact cuts instead, and they close on many rectangles -- 20 x 36 ft from
  4:12 to 18:12 -- but not on others: at 30 x 15 ft and 6:12, or 20 x 36 ft
  with a 2 ft overhang at 4:12, truck panics on the third cut ("this shell
  is not oriented and closed"), which the native kernel catches and
  retries on the mesh kernel while the wasm engine, which cannot unwind,
  traps; on a trapezoid at 12:12 the exact result is a
  body the tessellator cannot close, an error rather than a refusal. Which
  side of that line a rectangle falls on is not something a script can
  stand on, so the exact cuts are an opt-in for a native run, never the
  default. A `fascia` under a hip roof is trimmed to the footprint's prism
  the same way;
* a footprint whose skeleton is a **single apex** (a square, any regular
  polygon): `loft([eave polygon, apex])`, exact, the loft's tip section;
* anything else -- a reflex corner, or four planes through one node: one
  piece per skeleton face (the prism over the face, extended past its
  eave edge, cut by its plane), the pieces overlapping by half a
  millimetre across the arcs, cut and united on the **mesh kernel**, which
  merges pieces that meet face to face where the exact kernel's glue
  refuses a ridge shared by four faces (and whose cut of a piece always
  tessellates, where truck's exact cut of one of an L's six pieces left
  four open edges). The result is a mesh body whose faces are the same
  planes; its volume is exact for planar faces, and its ridge reads up to
  `0.5 mm * pitch * sqrt(2)` high where a piece's plane runs past the arc.

A gable is the two-plane case; on a rectangle it is one `extrude` of the
triangular profile along the ridge, and on any other footprint the prism
cut by two half-spaces. A shed roof is one plane. Both meet the eave along
an *edge* of the footprint's prism, so with `fascia=0` the cut is tangent
and runs on the mesh kernel; give them a fascia (`inch(6)` is a fascia
board) and they are exact.

## Areas

`floor_area(body, z)` is the plan area of the body cut `cut` (4 ft, where
a plan is drawn) above the level, *restricted to where the ceiling is at
least* `exclude_under` (5 ft) above it: `section(z + cut) intersect
section(z + exclude_under)` -- the rule zoning applies to a room under a
sloped ceiling, computed on the geometry rather than assumed. A box mass
gives its footprint; an attic under a hip roof gives the part of it a
person can stand in. `gross_floor_area(levels)` sums it per level and in
total; `lot_coverage(bodies, lot)` unions the bodies' footprints over the
lot.

## Height over grade, with averaging

`height_envelope(grade, roof, limit=ft(30), cap=ft(40), rear=(ft(8),
ft(32)))` samples a grade profile and a roof profile along the lot depth
and compares the roof against the limit line `grade + limit`: it reports
the area of the building above the line and the unbuilt area below it,
and *passes* when the excess is no more than the deficit (the averaging
rule), nothing exceeds the cap, and the rearmost strip keeps to its own
limit. `height_check` is the same without averaging: nothing over the line.
`floor_area_cap(...)` is the companion for floor area: the buildable
plate (the lot between the front setback and the rear-yard line, the lot's
full width) times the floors, less a deduction (650 sq ft), against what
the floors build.
"""

_MARGIN_FRACTION = 0.05  # the oversized prism's margin, as a fraction of the roof's extent
_PIECE_OVERLAP = 0.5  # mm: how far the general case's pieces overlap across a skeleton arc
_SKELETON_EPS = 1e-7  # relative to the footprint's extent
_MAX_EVENTS_PER_VERTEX = 6

# -------------------------------------------------------------------- units


def mm(x):
    """`x` millimetres, as a float."""
    return float(x)


def cm(x):
    """`x` centimetres in millimetres."""
    return 10.0 * x


def m(x):
    """`x` metres in millimetres."""
    return 1000.0 * x


def inch(x):
    """`x` inches in millimetres: `inch(6)` is 152.4."""
    return 25.4 * x


def ft(x, inches=0):
    """`x` feet (and `inches`) in millimetres: `ft(45)` is 13716, `ft(45, 6)` is 13868.4."""
    return 304.8 * x + 25.4 * inches


def ft_in(feet, inches):
    """Feet and inches in millimetres: `ft_in(45, 6)` is `45'-6"`."""
    return 304.8 * feet + 25.4 * inches


def sq_ft(area_mm2):
    """An area in mm² as square feet, for a figure zoning reads."""
    return area_mm2 / (304.8 * 304.8)


def pitch(rise, run=12):
    """Rise over run as a plain number: `pitch(6)` is a 6:12 roof (0.5), `pitch("6:12")` the same,
    `pitch(30, "deg")` a 30 degree one."""
    if type(rise) == "string":
        parts = rise.split(":")
        if len(parts) != 2:
            _fail("pitch", "%r is not a pitch; write it as \"6:12\"" % rise)
        return float(parts[0].strip()) / float(parts[1].strip())
    if run == "deg":
        return math.tan(rise)
    return float(rise) / float(run)


# ------------------------------------------------------------------ helpers


def _fail(where, msg):
    fail("building.%s: %s" % (where, msg))


def _named(name, suffix):
    """`name=` for a sub-feature, or nothing when the caller gave no name."""
    return {"name": name + suffix} if name else {}


def _mm_in(v):
    """A length for a refusal: `203.2 mm (8.0")`. (Starlark's `%` takes no precision, so the
    rounding is `math.round`'s.)"""
    return '%s mm (%s")' % (math.round(v, 3), math.round(v / 25.4, 2))


def _positive(where, what, v):
    if type(v) != "int" and type(v) != "float":
        _fail(where, "%s must be a number (got %s)" % (what, repr(v)))
    if v <= 0:
        _fail(where, "%s must be positive (got %s)" % (what, v))
    return float(v)


def _number(where, what, v):
    if type(v) != "int" and type(v) != "float":
        _fail(where, "%s must be a number (got %s)" % (what, repr(v)))
    return float(v)


def _sub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def _add(a, b):
    return (a[0] + b[0], a[1] + b[1])


def _mul(a, s):
    return (a[0] * s, a[1] * s)


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1]


def _cross(a, b):
    return a[0] * b[1] - a[1] * b[0]


def _len(a):
    return math.hypot(a[0], a[1])


def _unit(a):
    l = _len(a)
    return (a[0] / l, a[1] / l) if l > 0 else (0.0, 0.0)


def _left(d):
    """The normal to the left of direction `d`: inward for a counter-clockwise polygon."""
    return (-d[1], d[0])


def _signed_area(pts):
    s = 0.0
    n = len(pts)
    for i in range(n):
        a = pts[i]
        b = pts[(i + 1) % n]
        s += a[0] * b[1] - a[1] * b[0]
    return s / 2.0


def polygon_area(points):
    """The area of a polygon given as `(x, y)` points, whichever way round."""
    return abs(_signed_area(_polygon("polygon_area", points)))


def _polygon(where, footprint):
    """A footprint as a clean counter-clockwise point list: a Sketch's first loop, or points.
    A repeated closing point and coincident consecutive points are dropped."""
    if type(footprint) == "Sketch":
        loops = footprint.points()
        if len(loops) != 1:
            _fail(where, "the footprint sketch %s must have exactly one loop (it has %d)" % (footprint.id, len(loops)))
        pts = loops[0]
    elif type(footprint) == "list" or type(footprint) == "tuple":
        pts = [(float(p[0]), float(p[1])) for p in footprint]
    else:
        _fail(where, "the footprint must be a Sketch or a list of (x, y) points, got %s" % type(footprint))
    clean = []
    for p in pts:
        if not clean or _len(_sub(p, clean[-1])) > 1e-9:
            clean.append((float(p[0]), float(p[1])))
    if len(clean) > 1 and _len(_sub(clean[0], clean[-1])) <= 1e-9:
        clean = clean[:-1]
    if len(clean) < 3:
        _fail(where, "a footprint needs at least 3 distinct points, got %d" % len(clean))
    if _signed_area(clean) < 0:
        clean = list(reversed(clean))
    return clean


def _extent(pts):
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return (min(xs), min(ys), max(xs), max(ys))


def _is_convex(pts, eps):
    n = len(pts)
    for i in range(n):
        a = pts[i]
        b = pts[(i + 1) % n]
        c = pts[(i + 2) % n]
        if _cross(_sub(b, a), _sub(c, b)) < -eps:
            return False
    return True


def _is_axis_rect(pts, eps):
    """Four corners on the corners of their own bounding box."""
    if len(pts) != 4:
        return False
    x0, y0, x1, y1 = _extent(pts)
    corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    for p in pts:
        hit = False
        for c in corners:
            if _len(_sub(p, c)) <= eps:
                hit = True
        if not hit:
            return False
    return True


def _clean(pts):
    """A point list without coincident consecutive points or a repeated closing point, in
    the order given: a profile whose corners have merged at `fascia=0` becomes the triangle
    it is, instead of a polygon with a zero-length edge the tessellator drops a face over."""
    out = []
    for p in pts:
        if not out or _len(_sub(p, out[-1])) > 1e-9:
            out.append(p)
    if len(out) > 1 and _len(_sub(out[0], out[-1])) <= 1e-9:
        out = out[:-1]
    return out


def _line_intersection(p, d, q, e):
    """The point where the line through `p` along `d` meets the line through `q` along `e`,
    or None when parallel."""
    den = _cross(d, e)
    if abs(den) < 1e-12:
        return None
    t = _cross(_sub(q, p), e) / den
    return _add(p, _mul(d, t))


def offset_polygon(points, distance):
    """A polygon's edges each moved outward by `distance` (inward when negative), corners mitred:
    every vertex is where its two shifted edge lines meet. Right for any simple polygon as long
    as no edge is shorter than the offset -- a footprint grown by an overhang, a slab inset for
    its posts. For the general case (self-intersections cut away, splits) use `Sketch.offset`."""
    pts = _polygon("offset_polygon", points)
    n = len(pts)
    out = []
    for i in range(n):
        prev = pts[(i - 1) % n]
        cur = pts[i]
        nxt = pts[(i + 1) % n]
        d0 = _unit(_sub(cur, prev))
        d1 = _unit(_sub(nxt, cur))
        # Outward is to the right of a counter-clockwise edge.
        q0 = _sub(cur, _mul(_left(d0), distance))
        q1 = _sub(cur, _mul(_left(d1), distance))
        x = _line_intersection(q0, d0, q1, d1)
        out.append(x if x != None else q1)
    return out


def _bbox_points(pts):
    x0, y0, x1, y1 = _extent(pts)
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def _sketch_of(where, pts, z, name):
    return sketch(XY.offset(z), **_named(name, "_outline")).polygon(pts)


# --------------------------------------------------------- straight skeleton


def straight_skeleton(points):
    """The straight skeleton of a simple polygon: the arcs its edges' inward wavefront traces.

    Returns a dict: `points` (the polygon, counter-clockwise), `arcs` -- a list of
    `(a, b, t_a, t_b, (edge_i, edge_j))`, each a straight piece from `a` (reached at wavefront
    time `t_a`) to `b` (at `t_b`) bordering the faces of the two edges named -- `faces`, one
    polygon per edge as a list of `(x, y, t)` starting with the edge's own two corners (at
    `t = 0`), `nodes` (every interior vertex, `(x, y, t)`), and `height`, the largest `t`: the
    ridge, in the polygon's units of distance. At pitch `p` a point at time `t` is `p * t` high.
    Edge `i` runs from `points[i]` to `points[i + 1]`.

    Every event is handled: an edge shrinking to nothing, a reflex vertex splitting the
    wavefront, and a wavefront collapsing onto a line (the ridge of a rectangle, the whole of an
    L). Simultaneous events -- a rectangle's two ends, a square's four -- are taken one at a
    time at the same instant and the loop is closed as soon as it has no area left."""
    pts = _polygon("straight_skeleton", points)
    n = len(pts)
    x0, y0, x1, y1 = _extent(pts)
    scale = max(x1 - x0, y1 - y0)
    eps = _SKELETON_EPS * scale
    dirs = []
    normals = []
    for i in range(n):
        d = _unit(_sub(pts[(i + 1) % n], pts[i]))
        dirs.append(d)
        normals.append(_left(d))

    def velocity(ep, en):
        na = normals[ep]
        nb = normals[en]
        den = 1.0 + _dot(na, nb)
        if den < 1e-9:
            return None
        return _mul(_add(na, nb), 1.0 / den)

    def is_reflex(ep, en):
        return _cross(dirs[ep], dirs[en]) < -1e-9

    def vertex(p, t0, ep, en):
        return {"p": p, "t0": t0, "u": velocity(ep, en), "ep": ep, "en": en, "reflex": is_reflex(ep, en)}

    def pos(v, t):
        if v["u"] == None:
            return v["p"]
        return _add(v["p"], _mul(v["u"], t - v["t0"]))

    arcs = []

    def trace(v, x, t):
        if _len(_sub(v["p"], x)) > eps:
            arcs.append((v["p"], x, v["t0"], t, (v["ep"], v["en"])))

    loops = [[vertex(pts[i], 0.0, (i - 1) % n, i) for i in range(n)]]
    finished = []

    def loop_area(loop, t):
        return _signed_area([pos(v, t) for v in loop])

    def finalize(loop, t):
        """A wavefront with no area left: every vertex ends here, and the segments between
        distinct positions are ridges, each traversed once in each direction."""
        ps = [pos(v, t) for v in loop]
        for k in range(len(loop)):
            trace(loop[k], ps[k], t)
        ridges = {}
        for k in range(len(loop)):
            a = ps[k]
            b = ps[(k + 1) % len(loop)]
            if _len(_sub(a, b)) <= eps:
                continue
            key = _pair_key(a, b, eps)
            if key in ridges:
                ridges[key]["edges"].append(loop[k]["en"])
            else:
                ridges[key] = {"a": a, "b": b, "edges": [loop[k]["en"]]}
        for key in ridges:
            r = ridges[key]
            e = r["edges"]
            arcs.append((r["a"], r["b"], t, t, (e[0], e[1] if len(e) > 1 else e[0])))

    def merge_zero_edges(loop, t):
        """Consecutive vertices that coincide at `t` are one vertex."""
        for _ in range(len(loop)):
            if len(loop) < 2:
                return loop
            done = True
            for k in range(len(loop)):
                a = loop[k]
                b = loop[(k + 1) % len(loop)]
                if _len(_sub(pos(a, t), pos(b, t))) <= eps:
                    x = pos(a, t)
                    trace(a, x, t)
                    trace(b, x, t)
                    w = vertex(x, t, a["ep"], b["en"])
                    rest = [loop[j] for j in range(len(loop)) if j != k and j != (k + 1) % len(loop)]
                    loop = [w] + rest[k:] + rest[:k] if k < len(rest) else [w] + rest
                    done = False
                    break
            if done:
                return loop
        return loop

    t_now = 0.0
    for _ in range(_MAX_EVENTS_PER_VERTEX * n + 16):
        if not loops:
            break
        best = None
        for li in range(len(loops)):
            loop = loops[li]
            m_ = len(loop)
            for i in range(m_):
                a = loop[i]
                b = loop[(i + 1) % m_]
                if a["u"] == None or b["u"] == None:
                    continue
                d = dirs[a["en"]]
                g0 = _dot(_sub(pos(b, t_now), pos(a, t_now)), d)
                rate = _dot(_sub(b["u"], a["u"]), d)
                if rate < -1e-12:
                    tc = t_now + g0 / (-rate)
                    if tc >= t_now - eps and (best == None or tc < best[0] - eps or (abs(tc - best[0]) <= eps and best[1] != "edge")):
                        best = (tc, "edge", li, i, 0)
            for i in range(m_):
                r = loop[i]
                if not r["reflex"] or r["u"] == None:
                    continue
                for j in range(m_):
                    if j == i or (j + 1) % m_ == i:
                        continue
                    a = loop[j]
                    b = loop[(j + 1) % m_]
                    if a["u"] == None or b["u"] == None:
                        continue
                    e = a["en"]
                    ne = normals[e]
                    un = _dot(r["u"], ne)
                    if 1.0 - un < 1e-9:
                        continue
                    h0 = _dot(_sub(r["p"], pts[e]), ne) - un * r["t0"]
                    ts = h0 / (1.0 - un)
                    if ts < t_now - eps:
                        continue
                    x = pos(r, ts)
                    pa = pos(a, ts)
                    pb = pos(b, ts)
                    d = dirs[e]
                    s = _dot(_sub(x, pa), d)
                    length = _dot(_sub(pb, pa), d)
                    if s < -eps or s > length + eps:
                        continue
                    if best == None or ts < best[0] - eps:
                        best = (ts, "split", li, i, j)
        if best == None:
            _fail("straight_skeleton", "the wavefront of %d vertices did not close (a self-intersecting or degenerate footprint?)" % n)
        t_now = max(best[0], t_now)
        li = best[2]
        loop = loops[li]
        m_ = len(loop)
        new_loops = []
        if best[1] == "edge":
            i = best[3]
            a = loop[i]
            b = loop[(i + 1) % m_]
            x = _mul(_add(pos(a, t_now), pos(b, t_now)), 0.5)
            trace(a, x, t_now)
            trace(b, x, t_now)
            w = vertex(x, t_now, a["ep"], b["en"])
            rest = [loop[j] for j in range(m_) if j != i and j != (i + 1) % m_]
            # `w` takes the merged pair's place in the cycle: between loop[i - 1] and loop[i + 2].
            new_loops = [[w] + rest[i:] + rest[:i] if i + 1 < m_ else [w] + rest]
        else:
            i = best[3]
            j = best[4]
            r = loop[i]
            x = pos(r, t_now)
            trace(r, x, t_now)
            e = loop[j]["en"]
            r1 = vertex(x, t_now, e, r["en"])
            r2 = vertex(x, t_now, r["ep"], e)
            loop_a = [r1]
            k = (i + 1) % m_
            for _s in range(m_):
                loop_a.append(loop[k])
                if k == j:
                    break
                k = (k + 1) % m_
            loop_b = [r2]
            k = (j + 1) % m_
            for _s in range(m_):
                if k == i:
                    break
                loop_b.append(loop[k])
                k = (k + 1) % m_
            new_loops = [loop_a, loop_b]
        loops = [loops[k] for k in range(len(loops)) if k != li]
        for nl in new_loops:
            nl = merge_zero_edges(nl, t_now)
            if len(nl) <= 2 or abs(loop_area(nl, t_now)) <= eps * scale:
                finalize(nl, t_now)
                finished.append(nl)
            else:
                for v in nl:
                    if v["u"] == None:
                        _fail("straight_skeleton", "two parallel edges met at (%s, %s) inside a wavefront that still has area; the footprint is degenerate there" % (v["p"][0], v["p"][1]))
                loops.append(nl)
    if loops:
        _fail("straight_skeleton", "the wavefront did not finish in %d events" % (_MAX_EVENTS_PER_VERTEX * n + 16))

    nodes = {}
    height = 0.0
    for arc in arcs:
        for p, t in ((arc[0], arc[2]), (arc[1], arc[3])):
            if t > eps:
                nodes[_point_key(p, eps)] = (p[0], p[1], t)
                height = max(height, t)
    faces = []
    for e in range(n):
        a = pts[e]
        b = pts[(e + 1) % n]
        mine = [arc for arc in arcs if arc[4][0] == e or arc[4][1] == e]
        face = [(a[0], a[1], 0.0), (b[0], b[1], 0.0)]
        cur = b
        used = [False for _ in mine]
        for _ in range(len(mine) + 1):
            if _len(_sub(cur, a)) <= eps and len(face) > 2:
                break
            found = False
            for k in range(len(mine)):
                if used[k]:
                    continue
                arc = mine[k]
                for p, q, tq in ((arc[0], arc[1], arc[3]), (arc[1], arc[0], arc[2])):
                    if not found and _len(_sub(p, cur)) <= eps:
                        used[k] = True
                        cur = q
                        if _len(_sub(q, a)) > eps:
                            face.append((q[0], q[1], tq))
                        found = True
            if not found:
                _fail("straight_skeleton", "the face of edge %d does not close (reached (%s, %s))" % (e, cur[0], cur[1]))
        faces.append(face)
    return {"points": pts, "arcs": arcs, "faces": faces, "nodes": nodes.values(), "height": height}


def _point_key(p, eps):
    q = max(eps, 1e-9) * 4.0
    return (math.round(p[0] / q), math.round(p[1] / q))


def _pair_key(a, b, eps):
    ka = _point_key(a, eps)
    kb = _point_key(b, eps)
    return (ka, kb) if ka <= kb else (kb, ka)


# -------------------------------------------------------------------- roofs


def _roof_common(where, footprint, pitch_, overhang, eave_z, fascia):
    p = _positive(where, "pitch", pitch_)
    overhang = _number(where, "overhang", overhang)
    eave_z = _number(where, "eave_z", eave_z)
    fascia = _number(where, "fascia", fascia)
    if fascia < 0:
        _fail(where, "fascia must not be negative (got %s)" % fascia)
    pts = _polygon(where, footprint)
    if overhang != 0:
        pts = offset_polygon(pts, overhang)
    return p, pts, eave_z, fascia


def _halfspace_below(a, d, p, z, size, name):
    """A big box whose top face is the roof plane through the eave line at `a` along `d`, rising
    inward (to the left of `d`) at pitch `p`, `z` high at the eave: everything below the plane."""
    n = _left(d)
    s = math.sqrt(1.0 + p * p)
    normal = (-p * n[0] / s, -p * n[1] / s, 1.0 / s)
    # A cube centred on the origin (`box` centres x and y only), turned about the eave's
    # direction so its top face tilts inward at the pitch (`math.atan` is degrees), then moved
    # so that face passes through the eave at `z`.
    tool = box(size, size, size, at=(-size / 2.0, -size / 2.0, -size / 2.0), **_named(name, "_tool"))
    tool = tool.rotate(math.atan(p), axis=(d[0], d[1], 0.0))
    return tool.translate((a[0] - size / 2.0 * normal[0], a[1] - size / 2.0 * normal[1], z - size / 2.0 * normal[2]))


def _prism(pts, z0, z1, name, suffix):
    return extrude(sketch(XY.offset(z0), **_named(name, suffix + "_outline")).polygon(pts), z1 - z0, **_named(name, suffix))


def hip_roof(footprint, *, pitch, overhang=0, eave_z=0, fascia=0, ridge=None, kernel=None, name=None):
    """A hip roof over `footprint`: every eave edge carries a plane at `pitch`, meeting on the
    straight skeleton (hips, valleys, ridges). `overhang` grows the footprint to the eave line,
    `eave_z` is the eave height, `fascia` the vertical face at the eave (0: a knife edge).
    `ridge=(p0, p1)` on a rectangle places the ridge's plan endpoints yourself: the end planes
    then pass through the end eaves and the ridge ends, whatever pitch that makes them. Returns
    one body: an exact loft on a footprint with a single apex (a square, a regular polygon),
    otherwise a mesh body whose faces are the skeleton's planes, built on the mesh kernel (the
    module docstring says why the exact cuts are not the default). `kernel="brep"` asks for
    them anyway, on a native run only. Names: `<name>` (the roof), `<name>_blank`,
    `<name>_outline`, `<name>_cut<i>`, `<name>_tool<i>`, `<name>_face<k>`, `<name>_piece<k>`,
    `<name>_trim`."""
    where = "hip_roof"
    p, pts, eave_z, fascia = _roof_common(where, footprint, pitch, overhang, eave_z, fascia)
    n = len(pts)
    x0, y0, x1, y1 = _extent(pts)
    scale = max(x1 - x0, y1 - y0)
    eps = _SKELETON_EPS * scale
    sk = straight_skeleton(pts)
    h_max = sk["height"] * p
    margin = max(_MARGIN_FRACTION * scale, 10.0)
    size = 4.0 * (scale + h_max + margin + fascia)
    z_eave = eave_z + fascia

    if ridge != None:
        if len(pts) != 4:
            _fail(where, "ridge= is for a four-sided footprint; this one has %d sides" % len(pts))
        return _hip_with_ridge(pts, p, eave_z, fascia, ridge, size, "mesh" if kernel == None else kernel, name)

    apex_only = len(sk["nodes"]) == 1
    three_way = True
    for node in sk["nodes"]:
        count = 0
        for arc in sk["arcs"]:
            for q in (arc[0], arc[1]):
                if _len(_sub(q, (node[0], node[1]))) <= eps:
                    count += 1
        if count > 3:
            three_way = False

    if apex_only:
        node = sk["nodes"][0]
        base = sketch(XY.offset(z_eave), **_named(name, "_outline")).polygon(pts)
        if fascia > 0:
            roof = loft([base, (node[0], node[1], z_eave + node[2] * p)], **_named(name, "_shell"))
            # The band under the eave overlaps the pyramid's base by a hair: a crossing, not
            # a touch, so the union is a kernel boolean.
            band = _prism(pts, eave_z, z_eave + 1.0, name, "_fascia")
            return union(band, roof, **_named(name, ""))
        return loft([base, (node[0], node[1], z_eave + node[2] * p)], **_named(name, ""))

    if _is_convex(pts, eps) and three_way:
        outer = offset_polygon(pts, margin)
        body = _prism(outer, eave_z, z_eave + h_max + margin, name, "_blank")
        # On the mesh kernel unless `kernel=` says: the exact cuts close on many
        # rectangles and panic in truck on others (30 x 15 ft at 6:12; the module
        # docstring has the list), and a truck panic is a trap on wasm.
        k = kernel if kernel != None else "mesh"
        for i in range(n):
            a = pts[i]
            d = dirs_of(pts, i)
            tool = _halfspace_below(a, d, p, z_eave, size, _suffix(name, "_tool%d" % i))
            last = i == n - 1 and fascia == 0
            body = intersect(body, tool, **_kernel(k, _named(name, "" if last else "_cut%d" % i)))
        if fascia > 0:
            # The oversized blank still reaches past the eave below the fascia line: trim it
            # to the footprint's prism. The prism's vertical edges pass through the corners
            # where the hips cross the fascia line -- a vertex on an edge, which the exact
            # kernel refuses.
            body = intersect(body, _prism(pts, eave_z - margin, z_eave + h_max + 2 * margin, name, "_trim"), kernel=k, **_named(name, ""))
        return body

    pieces = []
    k = "mesh" if kernel == None else kernel
    for e in range(n):
        face = sk["faces"][e]
        d = dirs_of(pts, e)
        nrm = _left(d)
        # The face grown by half a millimetre across its arcs, so neighbouring pieces
        # overlap in a hair-thin strip instead of touching face to face -- what the mesh
        # kernel asks for; a touching pair pinches to a non-manifold edge. Over the strip the
        # neighbour's plane is a fraction of that above the roof, invisible at any scale.
        poly = offset_polygon([(q[0], q[1]) for q in face], _PIECE_OVERLAP)
        a = poly[0]
        b = poly[1]
        # And pushed outward past its eave along the arcs that end on its corners, so the
        # piece's plane crosses its prism's bottom face rather than lying along an edge.
        last = poly[-1] if len(poly) > 2 else a
        third = poly[2] if len(poly) > 2 else b
        ua = _unit(_sub(a, last))
        ub = _unit(_sub(b, third))
        sa = margin / max(_dot(ua, _mul(nrm, -1.0)), 0.2)
        sb = margin / max(_dot(ub, _mul(nrm, -1.0)), 0.2)
        ext = [_add(a, _mul(ua, sa)), _add(b, _mul(ub, sb))] + poly[2:]
        piece = _prism(ext, eave_z, z_eave + h_max + margin, name, "_face%d" % e)
        a = pts[e]
        tool = _halfspace_below(a, d, p, z_eave, size, _suffix(name, "_tool%d" % e))
        # On the mesh kernel like the union: truck's exact cut of a piece is sometimes a
        # body its tessellator cannot close, and the union would fail there instead.
        pieces.append(intersect(piece, tool, kernel=k, **_named(name, "_piece%d" % e)))
    if fascia > 0:
        roof = union(pieces, kernel=k, **_named(name, "_pieces"))
        return intersect(roof, _prism(pts, eave_z - margin, z_eave + h_max + 2 * margin, name, "_trim"), kernel=k, **_named(name, ""))
    return union(pieces, kernel=k, **_named(name, ""))


def _hip_with_ridge(pts, p, eave_z, fascia, ridge, size, k, name):
    """A four-sided roof whose ridge endpoints are given in plan: the two long planes rise from
    their eaves at `pitch` to the ridge line, the end planes pass through their eave and the
    nearer ridge end. The cuts run on kernel `k` (the mesh kernel by default: the exact third
    cut panics in truck on a centred 20 x 36 ft ridge at 6:12)."""
    where = "hip_roof"
    if len(ridge) != 2:
        _fail(where, "ridge= must be two plan points ((x0, y0), (x1, y1))")
    r0 = (float(ridge[0][0]), float(ridge[0][1]))
    r1 = (float(ridge[1][0]), float(ridge[1][1]))
    rd = _unit(_sub(r1, r0))
    # The ridge is parallel to the two "long" eaves: the pair of edges most parallel to it.
    n = len(pts)
    par = []
    for i in range(n):
        par.append((abs(_dot(dirs_of(pts, i), rd)), i))
    par = sorted(par)
    long_edges = [par[-1][1], par[-2][1]]
    # Ridge height from the first long edge at the pitch.
    a0 = pts[long_edges[0]]
    n0 = _left(dirs_of(pts, long_edges[0]))
    h = p * _dot(_sub(r0, a0), n0)
    if h <= 0:
        _fail(where, "the ridge must lie inside the footprint")
    z_eave = eave_z + fascia
    x0, y0, x1, y1 = _extent(pts)
    scale = max(x1 - x0, y1 - y0)
    margin = max(_MARGIN_FRACTION * scale, 10.0)
    body = _prism(offset_polygon(pts, margin), eave_z, z_eave + h + margin, name, "_blank")
    for i in range(n):
        a = pts[i]
        b = pts[(i + 1) % n]
        d = dirs_of(pts, i)
        nrm = _left(d)
        if i in long_edges:
            # Rises from its eave to the ridge line: the pitch on the far side is whatever
            # reaches the same ridge (a saltbox when the ridge is off centre).
            dist = _dot(_sub(r0, a), nrm)
            if dist <= 0:
                _fail(where, "the ridge must lie inside the footprint (it is outside edge %d)" % i)
            pi = h / dist
        else:
            # The end plane: through the eave line, reaching height h at the nearer ridge end.
            mid = _mul(_add(a, b), 0.5)
            end = r0 if _len(_sub(r0, mid)) < _len(_sub(r1, mid)) else r1
            dist = _dot(_sub(end, a), nrm)
            if dist <= 0:
                _fail(where, "ridge end (%s, %s) is not inside edge %d" % (end[0], end[1], i))
            pi = h / dist
        tool = _halfspace_below(a, d, pi, z_eave, size, _suffix(name, "_tool%d" % i))
        last = i == n - 1 and fascia == 0
        body = intersect(body, tool, kernel=k, **_named(name, "" if last else "_cut%d" % i))
    if fascia > 0:
        body = intersect(body, _prism(pts, eave_z - margin, z_eave + h + 2 * margin, name, "_trim"), kernel=k, **_named(name, ""))
    return body


def dirs_of(pts, i):
    """The unit direction of edge `i` of a point list."""
    return _unit(_sub(pts[(i + 1) % len(pts)], pts[i]))


def _suffix(name, suffix):
    return name + suffix if name else None


def _kernel(kernel, named):
    if kernel != None:
        named = dict(named)
        named["kernel"] = kernel
    return named


def gable_roof(footprint, *, pitch, ridge_axis=None, overhang=0, eave_z=0, fascia=0, kernel=None, name=None):
    """A gable roof: two planes at `pitch` rising from the footprint's two sides parallel to
    `ridge_axis` (`"x"` or `"y"`; the longer side by default) to a ridge down the middle. On an
    axis-aligned rectangle it is one exact `extrude` of the triangular profile; on any other
    footprint the footprint's prism cut by the two planes, exact with a `fascia`, on the mesh
    kernel without one. Names: `<name>`, `<name>_profile`."""
    where = "gable_roof"
    p, pts, eave_z, fascia = _roof_common(where, footprint, pitch, overhang, eave_z, fascia)
    x0, y0, x1, y1 = _extent(pts)
    if ridge_axis == None:
        ridge_axis = "x" if (x1 - x0) >= (y1 - y0) else "y"
    if ridge_axis != "x" and ridge_axis != "y":
        _fail(where, "ridge_axis=%r is not an axis; valid: x, y" % ridge_axis)
    scale = max(x1 - x0, y1 - y0)
    eps = _SKELETON_EPS * scale
    span = (y1 - y0) if ridge_axis == "x" else (x1 - x0)
    h = p * span / 2.0
    if _is_axis_rect(pts, eps):
        if ridge_axis == "x":
            prof = [(y0, 0.0), (y1, 0.0), (y1, fascia), ((y0 + y1) / 2.0, fascia + h), (y0, fascia)]
            if fascia == 0:
                prof = [(y0, 0.0), (y1, 0.0), ((y0 + y1) / 2.0, h)]
            prof = [(q[0], q[1] + eave_z) for q in prof]
            s = sketch(YZ.offset(x0), **_named(name, "_profile")).polygon(prof)
            return extrude(s, x1 - x0, **_named(name, ""))
        prof = [(x0, 0.0), (x1, 0.0), (x1, fascia), ((x0 + x1) / 2.0, fascia + h), (x0, fascia)]
        if fascia == 0:
            prof = [(x0, 0.0), (x1, 0.0), ((x0 + x1) / 2.0, h)]
        prof = [(q[0], q[1] + eave_z) for q in prof]
        # XZ's normal is -Y: the plane at y1, extruded toward y0.
        s = sketch(XZ.offset(-y1), **_named(name, "_profile")).polygon(prof)
        return extrude(s, y1 - y0, **_named(name, ""))
    z_eave = eave_z + fascia
    size = 4.0 * (scale + h + fascia)
    body = _prism(pts, eave_z, z_eave + h + 1.0, name, "_blank")
    if ridge_axis == "x":
        planes = [((x0, y0), (1.0, 0.0)), ((x1, y1), (-1.0, 0.0))]
    else:
        planes = [((x1, y0), (0.0, 1.0)), ((x0, y1), (0.0, -1.0))]
    k = kernel if kernel != None else ("auto" if fascia > 0 else "mesh")
    for i in range(2):
        a, d = planes[i]
        tool = _halfspace_below(a, d, p, z_eave, size, _suffix(name, "_tool%d" % i))
        body = intersect(body, tool, kernel=k, **_named(name, "" if i == 1 else "_cut0"))
    return body


def shed_roof(footprint, *, pitch, direction=(1, 0), overhang=0, eave_z=0, fascia=0, kernel=None, name=None):
    """A shed (single-plane) roof rising along `direction` from its low eave, the footprint's
    extreme against `direction`, at `eave_z + fascia`. An axis-aligned rectangle with an axis
    direction is one exact `extrude`; anything else is the prism cut by the plane, exact with
    a `fascia`, on the mesh kernel without one."""
    where = "shed_roof"
    p, pts, eave_z, fascia = _roof_common(where, footprint, pitch, overhang, eave_z, fascia)
    d = _unit((float(direction[0]), float(direction[1])))
    if _len(d) == 0:
        _fail(where, "direction must be a non-zero (dx, dy)")
    x0, y0, x1, y1 = _extent(pts)
    scale = max(x1 - x0, y1 - y0)
    eps = _SKELETON_EPS * scale
    along = [_dot(q, d) for q in pts]
    lo = min(along)
    run = max(along) - lo
    h = p * run
    axis_aligned = abs(d[0]) < 1e-12 or abs(d[1]) < 1e-12
    if _is_axis_rect(pts, eps) and axis_aligned:
        if abs(d[1]) < 1e-12:
            # Rising along x: the profile in XZ, extruded across y.
            rising = d[0] > 0
            # With `fascia=0` the low corner merges with the eave corner below it: the
            # profile is then a triangle, and `_clean` drops the zero-length edge.
            prof = _clean([(x0, eave_z), (x1, eave_z), (x1, eave_z + fascia + (h if rising else 0.0)), (x0, eave_z + fascia + (0.0 if rising else h))])
            s = sketch(XZ.offset(-y1), **_named(name, "_profile")).polygon(prof)
            return extrude(s, y1 - y0, **_named(name, ""))
        rising = d[1] > 0
        prof = _clean([(y0, eave_z), (y1, eave_z), (y1, eave_z + fascia + (h if rising else 0.0)), (y0, eave_z + fascia + (0.0 if rising else h))])
        s = sketch(YZ.offset(x0), **_named(name, "_profile")).polygon(prof)
        return extrude(s, x1 - x0, **_named(name, ""))
    z_eave = eave_z + fascia
    size = 4.0 * (scale + h + fascia)
    body = _prism(pts, eave_z, z_eave + h + 1.0, name, "_blank")
    # The low eave line: perpendicular to `direction`, through the extreme point; the plane rises
    # to the left of its direction, so the line runs along `direction` turned to the right.
    low = pts[along.index(lo)]
    line_dir = (d[1], -d[0])
    k = kernel if kernel != None else ("auto" if fascia > 0 else "mesh")
    tool = _halfspace_below(low, line_dir, p, z_eave, size, _suffix(name, "_tool"))
    return intersect(body, tool, kernel=k, **_named(name, ""))


def cupola(width, height, *, on, at=None, pitch=1.0, sink=None, depth=None, name=None):
    """A cupola: a `width` x `depth` box `height` tall with a pyramid roof at `pitch`, standing
    on roof body `on` -- its floor `sink` (default a foot) below the roof's ridge so it straddles
    the ridge -- centred at `at` (the roof's plan centre by default). Returns one body, separate
    from the roof. Names: `<name>_walls`, `<name>_roof`, `<name>`."""
    where = "cupola"
    w = _positive(where, "width", width)
    dpt = _positive(where, "depth", depth) if depth != None else w
    h = _positive(where, "height", height)
    p = _positive(where, "pitch", pitch)
    lo, hi = on.bbox()
    if at == None:
        at = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0)
    sink = ft(1) if sink == None else _number(where, "sink", sink)
    z0 = hi[2] - sink
    roof = hip_roof([(at[0] - w / 2.0, at[1] - dpt / 2.0), (at[0] + w / 2.0, at[1] - dpt / 2.0), (at[0] + w / 2.0, at[1] + dpt / 2.0), (at[0] - w / 2.0, at[1] + dpt / 2.0)], pitch=p, eave_z=z0 + h, overhang=inch(3), **_named(name, "_roof"))
    # The roof's eave overhangs the walls and its underside sits on their top: overlap them by
    # a hair so the union is a crossing, not a touch.
    walls = box(w, dpt, h + 1.0, at=(at[0] - w / 2.0, at[1] - dpt / 2.0, z0), **_named(name, "_walls"))
    return union(walls, roof, **_named(name, ""))


# ------------------------------------------------------------ levels, floors


def level(name, elevation, height, *, bodies=None, plan_cut=None):
    """A storey as data: `name`, its floor `elevation`, its `height` (to the next floor), the
    bodies that stand on it (for `gross_floor_area`) and the `plan_cut` height a plan of it is
    drawn at (4 ft). Adds nothing to the model."""
    return {
        "name": name,
        "z": _number("level", "elevation", elevation),
        "height": _positive("level", "height", height),
        "top": _number("level", "elevation", elevation) + _positive("level", "height", height),
        "bodies": list(bodies) if bodies != None else [],
        "plan_cut": ft(4) if plan_cut == None else _number("level", "plan_cut", plan_cut),
    }


def floor(footprint, z, height, *, solid=True, slab=None, walls=None, name=None):
    """The mass of one floor: the footprint extruded from `z` to `z + height`. `solid=True` is a
    box; `solid=False` is a slab (`slab` thick, a foot by default) with walls (`walls` thick,
    6 inches) round it, open at the top -- one exact body, the box minus the room. Names:
    `<name>` (and `<name>_void` for the room)."""
    where = "floor"
    pts = _polygon(where, footprint)
    z = _number(where, "z", z)
    h = _positive(where, "height", height)
    if solid:
        return _prism(pts, z, z + h, name, "")
    mass = _prism(pts, z, z + h, name, "_mass")
    slab = ft(1) if slab == None else _positive(where, "slab", slab)
    walls = inch(6) if walls == None else _positive(where, "walls", walls)
    if slab >= h:
        _fail(where, "the slab (%s) is as thick as the floor is tall (%s)" % (slab, h))
    inner = offset_polygon(pts, -walls)
    void = _prism(inner, z + slab, z + h + 1.0, name, "_void")
    return subtract(mass, void, **_named(name, ""))


# ---------------------------------------------------------- decks and stairs


def deck(footprint, z, thickness, *, posts=None, rail=None, ground=0, name=None):
    """A deck: its slab from `z - thickness` up to `z`, on square posts, with a rail. `posts` is
    `(spacing, size)` -- a post at every corner and at most `spacing` apart along the edges,
    `size` square, inset half a post from the edge, standing from `ground` (a height, or a
    function `(x, y) -> z` such as a terrain's) to the slab's underside. `rail` is `(height,
    thickness)` for a solid rail round the edge. Returns a dict: `slab`, `posts` (a list),
    `rails` (a list), `bodies` (all of them). Names: `<name>`, `<name>_post<k>`, `<name>_rail`."""
    where = "deck"
    pts = _polygon(where, footprint)
    z = _number(where, "z", z)
    t = _positive(where, "thickness", thickness)
    slab = _prism(pts, z - t, z, name, "")
    out = {"slab": slab, "posts": [], "rails": [], "bodies": [slab]}
    if posts != None:
        spacing = _positive(where, "post spacing", posts[0])
        size = _positive(where, "post size", posts[1])
        centres = offset_polygon(pts, -size / 2.0)
        seen = {}
        n = len(centres)
        for i in range(n):
            a = centres[i]
            b = centres[(i + 1) % n]
            length = _len(_sub(b, a))
            k = max(1, int(math.ceil(length / spacing - 1e-9)))
            for j in range(k):
                q = _add(a, _mul(_sub(b, a), float(j) / k))
                key = (math.round(q[0], 3), math.round(q[1], 3))
                if key in seen:
                    continue
                seen[key] = True
                zg = ground(q[0], q[1]) if type(ground) == "function" else _number(where, "ground", ground)
                top = z - t
                if top - zg <= 0:
                    _fail(where, "post %d at (%s, %s) would be %s tall: the deck is below its ground" % (len(out["posts"]), q[0], q[1], top - zg))
                post = box(size, size, top - zg, at=(q[0] - size / 2.0, q[1] - size / 2.0, zg), **_named(name, "_post%d" % len(out["posts"])))
                out["posts"].append(post)
                out["bodies"].append(post)
    if rail != None:
        rh = _positive(where, "rail height", rail[0])
        rt = _positive(where, "rail thickness", rail[1])
        inner = offset_polygon(pts, -rt)
        ring = sketch(XY.offset(z), **_named(name, "_rail_outline")).polygon(pts).polygon(inner)
        r = extrude(ring, rh, **_named(name, "_rail"))
        out["rails"].append(r)
        out["bodies"].append(r)
    return out


_CODES = {
    "irc": {"max_rise": 7.75 * 25.4, "min_run": 10 * 25.4, "min_width": 36 * 25.4},
    "none": {"max_rise": 1e12, "min_run": 0.0, "min_width": 0.0},
}


def stair(from_z, to_z, width, *, run=None, rise=None, landings=None, code="irc", direction=(1, 0), at=(0, 0), name=None):
    """A straight stair from `from_z` up to `to_z`, `width` wide, as a solid massing wedge of
    steps. The riser count is the fewest that keeps every rise at or under `code`'s maximum
    (IRC R311.7.5: 7 3/4" rise, 10" run, 36" wide); `rise=` asks for a particular rise instead,
    `run=` a tread depth (10" by default). `landings=[k, ...]` inserts a landing `width` deep
    after step `k`. The stair climbs along `direction` from `at` (its bottom riser's left
    corner). `code="none"` lifts the checks, or pass a dict `{"max_rise", "min_run",
    "min_width"}`. Refused, naming the number: a rise over the maximum, a run under the
    minimum, a width under the minimum. Returns a dict: `body`, `risers`, `rise`, `run`,
    `length` (the plan length climbed), `top` (the plan point at the top riser's left corner)."""
    where = "stair"
    z0 = _number(where, "from_z", from_z)
    z1 = _number(where, "to_z", to_z)
    total = z1 - z0
    if total <= 0:
        _fail(where, "to_z (%s) must be above from_z (%s)" % (z1, z0))
    w = _positive(where, "width", width)
    limits = _CODES.get(code) if type(code) == "string" else code
    if limits == None:
        _fail(where, "code=%r is not a code; valid: irc, none, or a dict of limits" % code)
    t = inch(10) if run == None else _positive(where, "run", run)
    if rise != None:
        r = _positive(where, "rise", rise)
        count = int(math.round(total / r))
        if count < 1 or abs(count * r - total) > 1e-6:
            _fail(where, "a rise of %s does not divide the %s climb into whole steps" % (r, total))
    else:
        count = int(math.ceil(total / limits["max_rise"] - 1e-9))
        r = total / count
    if r > limits["max_rise"] + 1e-9:
        _fail(where, "a rise of %s is over the %s limit of %s per step" % (_mm_in(r), code, _mm_in(limits["max_rise"])))
    if t < limits["min_run"] - 1e-9:
        _fail(where, "a run of %s is under the %s minimum of %s" % (_mm_in(t), code, _mm_in(limits["min_run"])))
    if w < limits["min_width"] - 1e-9:
        _fail(where, "a width of %s is under the %s minimum of %s" % (_mm_in(w), code, _mm_in(limits["min_width"])))
    after = {}
    for k in landings if landings != None else []:
        after[int(k)] = True
    # The profile: x along the run, y up, from the bottom riser's foot; a solid wedge under
    # the treads down to `from_z`.
    prof = [(0.0, 0.0)]
    x = 0.0
    y = 0.0
    for i in range(count):
        y += r
        prof.append((x, y))
        x += t
        prof.append((x, y))
        if (i + 1) in after and i + 1 < count:
            x += w
            prof.append((x, y))
    prof.append((x, 0.0))
    length = x
    d = _unit((float(direction[0]), float(direction[1])))
    if _len(d) == 0:
        _fail(where, "direction must be a non-zero (dx, dy)")
    # Draw along +X on the XZ plane at y = 0, extrude -Y (the plane's normal) by the width, then
    # turn to `direction` and move to `at`. XZ's normal is -Y, so the body spans y in [-w, 0];
    # shift it to [0, w] so `at` is the bottom riser's left corner looking up the stair.
    s = sketch(XZ, **_named(name, "_profile")).polygon(prof)
    body = extrude(s, w, **_named(name, ""))
    body = body.translate((0.0, w, z0))
    angle = math.atan2(d[1], d[0])
    if abs(angle) > 1e-9:
        body = body.rotate(angle, axis=(0, 0, 1))
    body = body.translate((float(at[0]), float(at[1]), 0.0))
    top = (float(at[0]) + d[0] * length, float(at[1]) + d[1] * length)
    return {"body": body, "risers": count, "rise": r, "run": t, "length": length, "top": top, "width": w}


def garage_door(mass, width, height, *, at, facing=(0, -1), recess=None, name=None):
    """Cut a garage door into a mass as a recessed panel: `width` wide, `height` tall from the
    mass's bottom, centred at plan point `at` on the face whose outward normal is `facing`,
    `recess` deep (4 inches). Returns the mass with the recess. Names: `<name>` (the cut),
    `<name>_door` (the cutter)."""
    where = "garage_door"
    w = _positive(where, "width", width)
    h = _positive(where, "height", height)
    rc = inch(4) if recess == None else _positive(where, "recess", recess)
    f = _unit((float(facing[0]), float(facing[1])))
    if _len(f) == 0:
        _fail(where, "facing must be a non-zero (dx, dy)")
    lo, hi = mass.bbox()
    # A slab centred on the face, half in and half out, from below the floor to `height`.
    cutter = box(w, 2 * rc, h + 1.0, center=True, at=(0.0, 0.0, -1.0), **_named(name, "_door"))
    # The cutter's thin axis is +y; turn it onto `facing` (the cutter is symmetric, so either
    # sign of the normal is the same box). `math.atan2` is degrees.
    angle = math.atan2(f[1], f[0]) - 90.0
    if abs(angle) > 1e-9:
        cutter = cutter.rotate(angle, axis=(0, 0, 1))
    cutter = cutter.translate((float(at[0]), float(at[1]), lo[2]))
    return subtract(mass, cutter, **_named(name, ""))


def elevator_shaft(width, depth, from_z, to_z, *, overrun=None, pit=None, at=(0, 0), name=None):
    """An elevator shaft `width` x `depth` in plan with its corner at `at`, from the lowest
    floor `from_z` less the `pit` (4 ft) to the top floor `to_z` plus the `overrun` (42 in,
    a hydraulic/MRL residential figure; a traction machine room wants more). One box."""
    where = "elevator_shaft"
    w = _positive(where, "width", width)
    d = _positive(where, "depth", depth)
    z0 = _number(where, "from_z", from_z)
    z1 = _number(where, "to_z", to_z)
    if z1 <= z0:
        _fail(where, "to_z (%s) must be above from_z (%s)" % (z1, z0))
    ov = inch(42) if overrun == None else _number(where, "overrun", overrun)
    pt = ft(4) if pit == None else _number(where, "pit", pit)
    return box(w, d, (z1 + ov) - (z0 - pt), at=(float(at[0]), float(at[1]), z0 - pt), **_named(name, ""))


# --------------------------------------------------------------------- areas


def floor_area(body, z, *, cut=None, exclude_under=None, name=None):
    """The floor area of `body` on the level at `z`, in mm²: the plan cut `cut` (4 ft) above the
    level, restricted to where the ceiling is at least `exclude_under` (5 ft) above it -- the
    part of an attic under a sloped roof that counts. Zero when the body has no material at
    either height. Records the two sections and their intersection as sketches."""
    where = "floor_area"
    z = _number(where, "z", z)
    cut = ft(4) if cut == None else _positive(where, "cut", cut)
    head = ft(5) if exclude_under == None else _positive(where, "exclude_under", exclude_under)
    plan = body.section(XY.offset(z + cut), on=XY, **_named(name, "_plan"))
    if plan.loops == 0:
        return 0.0
    if head <= cut:
        return plan.area()
    room = body.section(XY.offset(z + head), on=XY, **_named(name, "_headroom"))
    if room.loops == 0:
        return 0.0
    usable = plan.intersect(room, **_named(name, ""))
    return usable.area()


def gross_floor_area(levels, *, cut=None, exclude_under=None):
    """Gross floor area per level and in total, in mm²: for every level (from `level(...)`),
    the `floor_area` of each of its bodies at the level's elevation, summed. Returns a dict:
    `levels` (a list of `{"name", "area"}` in order), `by_level` (name to area) and `total`."""
    rows = []
    by = {}
    total = 0.0
    for lv in levels:
        area = 0.0
        for b in lv["bodies"]:
            area += floor_area(b, lv["z"], cut=cut if cut != None else lv["plan_cut"], exclude_under=exclude_under)
        rows.append({"name": lv["name"], "area": area})
        by[lv["name"]] = area
        total += area
    return {"levels": rows, "by_level": by, "total": total}


def footprint_of(bodies, *, name=None):
    """The plan footprint of one or more bodies as a Sketch on XY: each body cut just above its
    lowest point, the cuts united."""
    out = None
    for b in bodies if type(bodies) == "list" else [bodies]:
        lo, hi = b.bbox()
        s = b.section(XY.offset(lo[2] + min(25.0, (hi[2] - lo[2]) / 2.0)), on=XY)
        if s.loops == 0:
            continue
        out = s if out == None else out.union(s)
    if out == None:
        _fail("footprint_of", "no body has a footprint")
    return out


def lot_coverage(bodies, lot):
    """How much of `lot` (a footprint) the bodies cover: a dict of `area` (the union of their
    footprints, mm²), `lot_area` and `ratio`."""
    lot_pts = _polygon("lot_coverage", lot)
    lot_area = abs(_signed_area(lot_pts))
    fp = footprint_of(bodies)
    area = fp.area()
    return {"area": area, "lot_area": lot_area, "ratio": area / lot_area}


def floor_area_cap(lot_width, lot_depth, *, front_setback, rear_yard, floors, deduction=None, built=None):
    """The floor-area cap of a buildable envelope, the way a zoning threshold states it: the
    **plate** is the lot between the front setback and the rear-yard line at the lot's full
    width, `(lot_depth - front_setback - rear_yard) * lot_width`; the **envelope** is the
    plate times `floors`; the **cap** is the envelope less `deduction` (650 sq ft). `built` is
    what the floors build: a list of areas in mm², or of `{"area": ...}` rows as
    `gross_floor_area(...)["levels"]` returns them. Returns a dict: `plate`, `envelope`, `cap`,
    `built` (the sum) and `remaining` (`cap - built`), all mm², `ratio` (`built / cap`) and
    `passes` (`built <= cap`). Refused, naming the number: a plate of no depth."""
    where = "floor_area_cap"
    w = _positive(where, "lot_width", lot_width)
    dpt = _positive(where, "lot_depth", lot_depth)
    front = _number(where, "front_setback", front_setback)
    rear = _number(where, "rear_yard", rear_yard)
    n = _positive(where, "floors", floors)
    ded = 650.0 * 304.8 * 304.8 if deduction == None else _number(where, "deduction", deduction)
    plate_depth = dpt - front - rear
    if plate_depth <= 0:
        _fail(where, "the front setback (%s) and the rear yard (%s) leave no plate on a lot %s deep" % (front, rear, dpt))
    plate = plate_depth * w
    envelope = plate * n
    cap = envelope - ded
    total = 0.0
    for b in built if built != None else []:
        total += _number(where, "a built area", b["area"] if type(b) == "dict" else b)
    return {
        "plate": plate,
        "envelope": envelope,
        "cap": cap,
        "built": total,
        "remaining": cap - total,
        "ratio": total / cap if cap > 0 else 0.0,
        "passes": total <= cap + 1e-9,
    }


# ---------------------------------------------------------- height envelopes


def _interp(profile, v):
    """Linear interpolation of a sorted `(v, z)` profile, held flat beyond its ends."""
    if v <= profile[0][0]:
        return profile[0][1]
    if v >= profile[-1][0]:
        return profile[-1][1]
    for i in range(len(profile) - 1):
        a = profile[i]
        b = profile[i + 1]
        if v >= a[0] and v <= b[0]:
            if b[0] - a[0] <= 1e-12:
                return max(a[1], b[1])
            return a[1] + (b[1] - a[1]) * (v - a[0]) / (b[0] - a[0])
    return profile[-1][1]


def _by_v(q):
    return q[0]


def _profile(where, what, profile):
    if type(profile) != "list" and type(profile) != "tuple":
        _fail(where, "%s must be a list of (v, z) points" % what)
    # Sorted by `v` alone, and stably: a vertical step is two points at one `v` whose order
    # (lower first going up, higher first going down) is the profile's own.
    pts = sorted([(float(q[0]), float(q[1])) for q in profile], key=_by_v)
    if len(pts) < 2:
        _fail(where, "%s needs at least two points" % what)
    return pts


def roof_profile(body, *, axis=(1, 0), through=None, step=None):
    """The top of `body` along a line in plan: a `(v, z)` list, `v` the distance along `axis`
    from `through` (the body's plan centre by default), from a vertical section through the
    body: a point every `step` (6 in) and one at every vertex of the section, a vertical
    edge -- a parapet, a setback -- as two points at one `v`, so `height_envelope` integrates
    the body's own breakpoints exactly and its worst point is a corner, not the sample
    before it."""
    where = "roof_profile"
    d = _unit((float(axis[0]), float(axis[1])))
    if _len(d) == 0:
        _fail(where, "axis must be a non-zero (dx, dy)")
    st = inch(6) if step == None else _positive(where, "step", step)
    if through == None:
        lo, hi = body.bbox()
        through = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0)
    plane_ = plane(origin=(float(through[0]), float(through[1]), 0.0), normal=(d[1], -d[0], 0.0), x_dir=(d[0], d[1], 0.0))
    cut = body.section(plane_)
    loops = cut.points()
    if not loops:
        _fail(where, "the section along the axis misses %s" % body)
    segs = []
    for lp in loops:
        for i in range(len(lp)):
            a = lp[i]
            b = lp[(i + 1) % len(lp)]
            segs.append((a, b))
    eps = 1e-6
    vs = sorted(dict([(q[0], True) for lp in loops for q in lp]).keys())
    v0 = vs[0]
    v1 = vs[-1]
    count = max(1, int(math.ceil((v1 - v0) / st - 1e-9)))
    grid = [v0 + (v1 - v0) * k / count for k in range(count + 1)]
    breaks = []
    for v in sorted(vs + grid):
        if breaks and v - breaks[-1] <= eps:
            continue
        breaks.append(v)
    out = []
    for v in breaks:
        left = _top_at(segs, v, -1, eps)
        right = _top_at(segs, v, 1, eps)
        if left == None and right == None:
            continue
        if left == None:
            left = right
        if right == None:
            right = left
        out.append((v, left))
        if abs(right - left) > eps:
            out.append((v, right))
    return out


def _top_at(segs, v, side, eps):
    """The highest z of the section's segments at `v`, over the segments that continue to
    `side` of it (-1 left, +1 right) -- so a vertex on a vertical edge has a height either
    side, and the vertical edge itself counts for neither. None when no segment does."""
    top = None
    for a, b in segs:
        lo = min(a[0], b[0])
        hi = max(a[0], b[0])
        if hi - lo <= eps or v < lo - eps or v > hi + eps:
            continue
        if (side < 0 and v <= lo + eps) or (side > 0 and v >= hi - eps):
            continue
        zz = a[1] + (b[1] - a[1]) * (v - a[0]) / (b[0] - a[0])
        if top == None or zz > top:
            top = zz
    return top


def height_envelope(grade, roof, *, limit=None, cap=None, rear=None, step=None):
    """The height-over-grade envelope with averaging, as hillside height rules measure it.
    `grade` and `roof` are `(v, z)` profiles along the same axis (the lot depth, `v` from the
    front); `roof` may be a body, sampled along x through the origin (use `roof_profile` for
    another line). The limit line is `grade + limit` (30 ft) at every point; the building
    `passes` when the area of its profile above the line is no more than the area of the
    line's envelope it leaves unbuilt below (`over_area <= under_area`), no point exceeds
    `cap` (40 ft) over grade, and, when `rear=(depth, height)` is given (for example
    `(ft(8), ft(32))`), no point in the rearmost `depth` of the building exceeds `height` over
    grade. Returns a dict: `passes`,
    `over_area`, `under_area` (mm²), `max_over` and `max_over_at` (the worst point), `cap_ok`,
    `rear_ok`, `limit_line`, `cap_line` and `rear_line` (`(v, z)` lists to draw), and `samples`
    (`(v, grade, roof, limit)` every `step`, 6 in)."""
    where = "height_envelope"
    g = _profile(where, "grade", grade)
    r = roof_profile(roof) if type(roof) == "Body" else _profile(where, "roof", roof)
    lim = ft(30) if limit == None else _positive(where, "limit", limit)
    cp = ft(40) if cap == None else _positive(where, "cap", cap)
    st = inch(6) if step == None else _positive(where, "step", step)
    v0 = r[0][0]
    v1 = r[-1][0]
    if v1 <= v0:
        _fail(where, "the roof profile has no length along the axis")
    rear_depth = _positive(where, "rear depth", rear[0]) if rear != None else 0.0
    rear_h = _positive(where, "rear height", rear[1]) if rear != None else 0.0

    # Both profiles are piecewise linear, so their difference is too, and its
    # positive and negative parts integrate exactly interval by interval
    # between the union of their breakpoints -- a vertical step in the roof
    # (a parapet, a setback) is two breakpoints at one `v` and contributes
    # nothing to the area, where a sampled sum would put half a step's worth
    # on the wrong side of the line.
    breaks = sorted(dict([(q[0], True) for q in g if q[0] >= v0 and q[0] <= v1] + [(q[0], True) for q in r]).keys())
    over = 0.0
    under = 0.0
    max_over = None
    max_at = None
    cap_ok = True
    rear_ok = True
    for i in range(len(breaks) - 1):
        va = breaks[i]
        vb = breaks[i + 1]
        if vb - va <= 1e-9:
            continue
        mid = (va + vb) / 2.0
        ga, gb = _segment_values(g, mid, va, vb)
        ra, rb = _segment_values(r, mid, va, vb)
        ea = ra - ga - lim
        eb = rb - gb - lim
        for e, v in ((ea, va), (eb, vb)):
            if max_over == None or e > max_over:
                max_over = e
                max_at = v
        if max(ra - ga, rb - gb) > cp + 1e-9:
            cap_ok = False
        if rear != None and vb > v1 - rear_depth + 1e-9:
            # The part of this interval inside the rear strip.
            wa = max(va, v1 - rear_depth)
            hz_a = _lerp(va, ra - ga, vb, rb - gb, wa)
            if max(hz_a, rb - gb) > rear_h + 1e-9:
                rear_ok = False
        if ea >= 0 and eb >= 0:
            over += (ea + eb) / 2.0 * (vb - va)
        elif ea <= 0 and eb <= 0:
            under += -(ea + eb) / 2.0 * (vb - va)
        else:
            root = va + (vb - va) * ea / (ea - eb)
            if ea > 0:
                over += ea / 2.0 * (root - va)
                under += -eb / 2.0 * (vb - root)
            else:
                under += -ea / 2.0 * (root - va)
                over += eb / 2.0 * (vb - root)

    # The lines to draw, sampled every `step`.
    samples = []
    limit_line = []
    cap_line = []
    rear_line = []
    count = max(1, int(math.ceil((v1 - v0) / st - 1e-9)))
    for k in range(count + 1):
        v = v0 + (v1 - v0) * k / count
        gz = _interp(g, v)
        rz = _interp(r, v)
        samples.append((v, gz, rz, gz + lim))
        limit_line.append((v, gz + lim))
        cap_line.append((v, gz + cp))
        if rear != None and v >= v1 - rear_depth - 1e-9:
            rear_line.append((v, gz + rear_h))
    return {
        "passes": over <= under + 1e-9 and cap_ok and rear_ok,
        "over_area": over,
        "under_area": under,
        "max_over": max_over,
        "max_over_at": max_at,
        "cap_ok": cap_ok,
        "rear_ok": rear_ok,
        "limit_line": limit_line,
        "cap_line": cap_line,
        "rear_line": rear_line,
        "samples": samples,
    }


def _lerp(va, za, vb, zb, v):
    if vb - va <= 1e-12:
        return max(za, zb)
    return za + (zb - za) * (v - va) / (vb - va)


def _segment_values(profile, mid, va, vb):
    """The values at `va` and `vb` of the profile's own segment through `mid`: its line, not the
    held-flat ends or the max-at-a-step rule of `_interp`."""
    if mid <= profile[0][0]:
        return (profile[0][1], profile[0][1])
    if mid >= profile[-1][0]:
        return (profile[-1][1], profile[-1][1])
    for i in range(len(profile) - 1):
        a = profile[i]
        b = profile[i + 1]
        if mid >= a[0] and mid < b[0] and b[0] - a[0] > 1e-12:
            return (_lerp(a[0], a[1], b[0], b[1], va), _lerp(a[0], a[1], b[0], b[1], vb))
    return (profile[-1][1], profile[-1][1])


def height_check(grade, roof, *, limit=None, step=None):
    """The plain height-over-grade check, no averaging: `passes` when no point of the roof
    profile is more than `limit` (30 ft) above the grade under it. Returns `passes`,
    `max_over`, `max_over_at`, `limit_line` and `samples`, as `height_envelope` does."""
    env = height_envelope(grade, roof, limit=limit, step=step, cap=1e12)
    return {
        "passes": env["max_over"] <= 1e-9,
        "max_over": env["max_over"],
        "max_over_at": env["max_over_at"],
        "limit_line": env["limit_line"],
        "samples": env["samples"],
    }
