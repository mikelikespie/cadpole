"""@cadpole/print.star -- printing a model in pieces: registration pegs, story bands, the bed.

    load("@cadpole/print.star", "pegs", "peg_sites", "story_bands", "drop_to_bed", "is_box", "teardrop", "PEG")

A building printed one story at a time is a stack of pieces that has to go
back together where it came apart. This module makes the pieces and the
features that line them up:

* `pegs(lower, upper, ...)` adds registration pegs on the top of one piece
  and matching sockets under the next, where the two meet;
* `peg_sites(lower, upper, ...)` is where they can go -- on material under,
  in material over, with a wall round every socket;
* each side of either can be one body or a list -- a piece printed as a body
  per filament -- judged as the union of its bodies, and `column=` stands a
  key column in a room corner under a peg where the walls are thinner than
  the peg;
* `story_bands(parts, levels, ...)` cuts a model into horizontal bands,
  joins each into one printable solid (`union(weld=)`), pegs every
  band to the next and drops each onto the bed;
* `drop_to_bed(body)` moves a body so its lowest point is z = 0;
* `teardrop(d, length, ...)` is the tool that cuts a horizontal hole with
  a 45 degree peaked top, the shape that prints without support where a
  round bore wider than about 7 mm sags (`print_check`'s
  `horizontal_hole`, `assert_support_free`).

Every length is in the model's units at the moment of the call -- the
peg sizes are millimetres because pegs are made after a model drawn in feet
has been scaled to the print, which `story_bands(scale=)` does first.

## The fit (FDM, 0.4 mm nozzle)

`PEG` holds the defaults, each a parameter of `pegs()`:

* `size` 3.0: the peg's side (or diameter). A peg narrower than about six
  extrusions (2.4 mm at a 0.4 mm nozzle) is mostly perimeter and snaps off.
* `height` 2.0: how far it reaches into the piece above -- ten 0.2 mm
  layers, enough to stop the pieces sliding.
* `clearance` 0.15 per side between peg and socket. A 0.4 mm nozzle lays
  extrusions 0.42-0.45 mm wide and a vertical hole prints 0.1-0.15 mm
  undersize (the numbers behind `print_check`'s `min_gap`), so a socket cut 0.15 mm bigger each side
  is a slip fit on a tuned printer; 0.1 is a press fit, 0.25 a loose one.
* `depth_clearance` 0.3: the socket is this much deeper than the peg is
  tall, so the peg never bottoms out and the two pieces sit flush.
* `chamfer` 0.4: the top 0.4 mm of the peg is narrower by 0.4 mm a side --
  a lead-in, stepped rather than sloped so a square peg stays an
  axis-aligned box (two layers at 0.2 mm: the slicer prints it as a slope).
* `min_wall` 0.8: material left round every socket -- two perimeters.
* `shape` "square" (boxes, which join exactly with a box-built model) or
  "round" (pins).
"""

PEG = {
    "size": 3.0,
    "height": 2.0,
    "clearance": 0.15,
    "depth_clearance": 0.3,
    "chamfer": 0.4,
    "min_wall": 0.8,
    "shape": "square",
}

def is_box(b, rel = 1e-6):
    """Whether body `b` is an axis-aligned box: six faces and the volume of
    its bounding box. What `union(weld=)` joins on its grid."""
    lo, hi = b.bbox()
    v = (hi[0] - lo[0]) * (hi[1] - lo[1]) * (hi[2] - lo[2])
    return len(b.faces()) == 6 and v > 0 and abs(b.volume() - v) <= rel * v

def drop_to_bed(body, name = None):
    """`body` moved straight down (or up) so its lowest point is z = 0, where
    a print starts. Returns the moved body; the original is consumed."""
    lo, _ = body.bbox()
    return translate(body, (0, 0, -lo[2]), name = name)

def teardrop(d, length, at = (0, 0, 0), axis = "y", segments = 48, name = "teardrop"):
    """The tool for a horizontal hole `d` across that prints without
    support: a bore along `axis` ("x" or "y"), `length` long and centred on
    `at`, whose top is two 45 degree flats meeting `d / sqrt(2)` above the
    centre instead of a round ceiling. Subtract it from the part.

    A slicer bridges the round top of a horizontal bore up to about 7 mm
    across; past that the top sags into the hole. Two 45 degree slopes are
    self-supporting at any size, and the bore below them is the full circle,
    so a pin or a screw still bears on it. The profile is a polygon of
    `segments` steps round the circle, every vertex on it, so the hole is a
    chord error under `d` and never over."""
    if not d > 0 or not length > 0:
        fail("teardrop: d and length must be positive, got %s and %s" % (d, length))
    if axis not in ("x", "y"):
        fail("teardrop: axis is \"x\" or \"y\" (a horizontal hole), got %s" % repr(axis))
    r = d / 2.0
    n = max(8, segments)
    pts = [(r * math.cos(135.0 + 270.0 * i / n), r * math.sin(135.0 + 270.0 * i / n)) for i in range(n + 1)]
    pts.append((0.0, r * math.sqrt(2.0)))
    # On XZ: sketch x is world x, sketch y world z, extruded along -y.
    tool = extrude(sketch(XZ, name = name + "_profile").polygon(pts), length, name = name + "_bore")
    lo, hi = tool.bbox()
    tool = translate(tool, (at[0], at[1] - (lo[1] + hi[1]) / 2.0, at[2]), name = name if axis == "y" else name + "_y")
    if axis == "x":
        tool = rotate(tool, 90, axis = (0, 0, 1), origin = at, name = name)
    return tool

def _opts(overrides):
    o = dict(PEG)
    for k, v in overrides.items():
        if k not in o:
            fail("pegs: unknown option %s; the options are %s" % (repr(k), ", ".join(sorted(o.keys()))))
        o[k] = v
    if o["shape"] not in ("square", "round"):
        fail("pegs: shape is \"square\" or \"round\", got %s" % repr(o["shape"]))
    for k in ("size", "height"):
        if not o[k] > 0:
            fail("pegs: %s is a positive length, got %s" % (k, o[k]))
    for k in ("clearance", "depth_clearance", "chamfer", "min_wall"):
        if o[k] < 0:
            fail("pegs: %s is a length, 0 or more, got %s" % (k, o[k]))
    if o["chamfer"] * 2 >= o["size"] or o["chamfer"] >= o["height"]:
        fail("pegs: a chamfer of %s leaves nothing of a %s x %s peg" % (o["chamfer"], o["size"], o["height"]))
    return o

def _as_list(b):
    """A body or a list of bodies, as a list."""
    return b if type(b) == "list" else [b]

def _section(body, z, on):
    """The loops where the horizontal plane at `z` cuts `body`, in world x, y."""
    return body.section(plane(origin = (0, 0, z)), on = on).points()

def _cuts(bodies, z, on):
    """Where the horizontal plane at `z` cuts each of `bodies`: one set of
    loops per body. A side of a joint is the union of its bodies' material,
    judged point by point below -- never a 2D boolean of the cuts,
    which can fill a room in."""
    return [_section(b, z, on) for b in bodies]

def _inside(loops, x, y):
    """Even-odd: is (x, y) inside the material the loops bound?"""
    n = 0
    for loop in loops:
        k = len(loop)
        for i in range(k):
            (ax, ay), (bx, by) = loop[i], loop[(i + 1) % k]
            if (ay > y) != (by > y):
                t = ax + (y - ay) * (bx - ax) / (by - ay)
                if t > x:
                    n += 1
    return n % 2 == 1

def _in(cuts, x, y):
    """Is (x, y) in the material of any of the bodies `cuts` are of?"""
    for loops in cuts:
        if _inside(loops, x, y):
            return True
    return False

def _clip_segment(ax, ay, bx, by, x0, y0, x1, y1):
    """The part of the segment a-b inside the open rectangle (x0, x1) x (y0,
    y1), as parameters `(t0, t1)` along it, or None: Liang-Barsky."""
    dx, dy = bx - ax, by - ay
    t0, t1 = 0.0, 1.0
    for p, q in ((-dx, ax - x0), (dx, x1 - ax), (-dy, ay - y0), (dy, y1 - ay)):
        if p == 0:
            if q <= 0:
                return None
        elif p < 0:
            t0 = max(t0, q / p)
        else:
            t1 = min(t1, q / p)
    return (t0, t1) if t0 < t1 else None

def _crosses(cuts, x0, y0, x1, y1):
    """Does the boundary of the bodies' material pass through the open
    rectangle (x0, x1) x (y0, y1)? With one body, any loop edge through it;
    with several, an edge through it with air on one side somewhere along
    it -- where two bodies meet, a wall of one filament on a floor of
    another, there is material on both sides and no boundary."""
    one = len(cuts) == 1
    d = 1e-3 * min(x1 - x0, y1 - y0)
    for loops in cuts:
        for loop in loops:
            k = len(loop)
            for i in range(k):
                (ax, ay), (bx, by) = loop[i], loop[(i + 1) % k]
                span = _clip_segment(ax, ay, bx, by, x0, y0, x1, y1)
                if span == None:
                    continue
                if one:
                    return True
                length = math.hypot(bx - ax, by - ay)
                if length == 0:
                    continue
                nx, ny = -(by - ay) / length * d, (bx - ax) / length * d
                t0, t1 = span
                for f in (0.1, 0.3, 0.5, 0.7, 0.9):
                    t = t0 + (t1 - t0) * f
                    px, py = ax + (bx - ax) * t, ay + (by - ay) * t
                    if not (_in(cuts, px + nx, py + ny) and _in(cuts, px - nx, py - ny)):
                        return True
    return False

def _square_in(cuts, x, y, h):
    """Is the square of half-side `h` centred on (x, y) wholly inside the
    material: its centre inside and no boundary through it."""
    e = h * 1e-6
    return _in(cuts, x, y) and not _crosses(cuts, x - h + e, y - h + e, x + h - e, y + h - e)

def _touches(loops, x, y, h):
    """Does any of one body's material reach into the square of half-side
    `h` centred on (x, y)?"""
    e = h * 1e-6
    return _inside(loops, x, y) or _crosses([loops], x - h + e, y - h + e, x + h - e, y + h - e)

def _socket_fits(upper_bottom, upper_deep, x, y, o):
    """A socket at (x, y) has `min_wall` of material all round it in the
    piece above, and that piece still has material over its floor."""
    hs = o["size"] / 2.0 + o["clearance"]
    return _square_in(upper_bottom, x, y, hs + o["min_wall"]) and _square_in(upper_deep, x, y, hs)

def _fits(lower_top, upper_bottom, upper_deep, x, y, o):
    return _square_in(lower_top, x, y, o["size"] / 2.0) and _socket_fits(upper_bottom, upper_deep, x, y, o)

def _interface(lowers, uppers, z):
    if z != None:
        return z
    top = max([b.bbox()[1][2] for b in lowers])
    bottom = min([b.bbox()[0][2] for b in uppers])
    gap = abs(top - bottom)
    if gap > 1e-6 * max(1.0, abs(bottom)):
        fail(("pegs: the pieces do not meet: the lower one's top is at z = %s and the upper one's bottom at " +
              "z = %s; make the pegs while the pieces still stand where the model has them (story_bands does), " +
              "or pass z=") % (top, bottom))
    return bottom

def _bottom(lowers):
    return min([b.bbox()[0][2] for b in lowers])

def _check_column(column, o):
    if not column > 0:
        fail("pegs: column is the side of a key column, a positive length, got %s" % (column,))
    if column < o["size"]:
        fail("pegs: a key column %s across is narrower than the %s peg it carries; make column at least size" %
             (column, o["size"]))

def _l_corner(cuts, vx, vy, sx, sy, c, d, room):
    """At (vx, vy), the corner of a room that lies towards (sx, sy): is there
    material behind both sides of a `c`-wide square standing in that corner,
    all along each -- the L of two walls meeting, whatever their thickness --
    and, with `room`, air just off the corner?"""
    if room and _in(cuts, vx + sx * d, vy + sy * d):
        return False
    for t in (d, c / 2.0, c - d):
        if not _in(cuts, vx + sx * t, vy - sy * d):
            return False
        if not _in(cuts, vx - sx * d, vy + sy * t):
            return False
    return True

def _corner_sites(top, mid, c, d):
    """Every inside corner of the walls in `top` (the lower piece at the
    joint) where a key column `c` across stands against both walls, and the
    walls are there at mid-height too (`mid`) -- not beside a door or in
    front of a window: the column's centres, in loop order."""
    out = []
    for loop in [loop for loops in top for loop in loops]:
        for (vx, vy) in loop:
            for sx in (-1, 1):
                for sy in (-1, 1):
                    if _l_corner(top, vx, vy, sx, sy, c, d, True) and _l_corner(mid, vx, vy, sx, sy, c, d, False):
                        out.append((vx + sx * c / 2.0, vy + sy * c / 2.0))
    return out

def _column_at(top, mid, x, y, c, d):
    """The corner a key column centred on (x, y) stands in: `(vx, vy, sx,
    sy)`, or None."""
    for sx in (-1, 1):
        for sy in (-1, 1):
            vx, vy = x - sx * c / 2.0, y - sy * c / 2.0
            if _l_corner(top, vx, vy, sx, sy, c, d, True) and _l_corner(mid, vx, vy, sx, sy, c, d, False):
                return (vx, vy, sx, sy)
    return None

def peg_sites(lower, upper, count = 4, z = None, column = None, **options):
    """Up to `count` places where a peg fits between `lower` and `upper`,
    which meet on the plane z = `z` (default: where `upper` starts and
    `lower` ends), as `(x, y)` pairs. Each side is a body or a list of
    bodies -- a piece printed as one body per filament -- judged as one: the
    union of its bodies' material.

    A place fits when the peg's footprint is wholly on `lower`'s material
    just under the plane, the socket's footprint plus `min_wall` all round
    is wholly in `upper`'s material just over it, and `upper` still has
    material over the socket's floor, so the socket does not break through.
    The candidates are the corners of both pieces' outlines on the plane,
    stepped in by the footprint -- the inside and outside corners of walls,
    where a key belongs.

    `column=` (a side length, at least `size`) also offers the room corners:
    wherever two walls of `lower` meet, with their faces running on at least
    `column` from the corner on both sides at the joint and at mid-height
    (no door or window beside it), a key column `column` across standing in
    the corner carries the peg, and `pegs(..., column=)` builds it. That is
    what lets a peg stand where the walls are thinner than the peg -- 1.2 to
    2.2 mm at 1:96, where a 2.4 mm peg finds no wall top wide enough.

    The places are spread out by farthest-point selection, so three pegs
    land as far apart as the plan allows. Deterministic: the same pieces
    give the same sites. Options are `PEG`'s."""
    o = _opts(options)
    lowers, uppers = _as_list(lower), _as_list(upper)
    z = _interface(lowers, uppers, z)
    h = o["size"] / 2.0
    hs = h + o["clearance"]
    eps = max(1e-4, o["size"] * 1e-3)
    on = plane(origin = (0, 0, z))
    top = _cuts(lowers, z - eps, on)
    bottom = _cuts(uppers, z + eps, on)
    deep = _cuts(uppers, z + o["height"] + o["depth_clearance"] + eps, on)
    cands = []
    # Stepped in a tenth more than the wall needs, so a socket by an outer
    # corner leaves a wall `print_check` does not call thin.
    for cuts, step in ((top, h), (bottom, hs + 1.1 * o["min_wall"])):
        for loop in [loop for loops in cuts for loop in loops]:
            for (px, py) in loop:
                for sx in (-1, 1):
                    for sy in (-1, 1):
                        cands.append((px + sx * step, py + sy * step))
    seen = {}
    good = []
    for (x, y) in sorted(cands):
        if (x, y) in seen:
            continue
        seen[(x, y)] = True
        if _fits(top, bottom, deep, x, y, o):
            good.append((x, y))
    if column != None:
        _check_column(column, o)
        mid = _cuts(lowers, (_bottom(lowers) + z) / 2.0, on)
        for (x, y) in sorted(_corner_sites(top, mid, column, eps)):
            if (x, y) in seen:
                continue
            seen[(x, y)] = True
            if _socket_fits(bottom, deep, x, y, o):
                good.append((x, y))
    if not good:
        return []

    # Farthest-point selection, from the site farthest from their centre.
    cx, cy = 0.0, 0.0
    for p in good:
        cx += p[0] / len(good)
        cy += p[1] / len(good)
    def d2(a, b):
        return (a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])
    first = good[0]
    for p in good:
        if d2(p, (cx, cy)) > d2(first, (cx, cy)):
            first = p
    chosen = [first]
    near = [d2(p, first) for p in good]
    for _ in range(count - 1):
        best, bi = -1.0, -1
        for i, p in enumerate(good):
            if near[i] > best:
                best, bi = near[i], i
        # Two pegs must not crowd each other's sockets.
        if bi < 0 or best < (2 * (hs + o["min_wall"])) * (2 * (hs + o["min_wall"])):
            break
        chosen.append(good[bi])
        for i, p in enumerate(good):
            near[i] = min(near[i], d2(p, good[bi]))
    return chosen

def _over(o):
    """How far a peg reaches down into the piece under it, and a socket down
    out of the piece over it: a hair, so no boolean meets two faces exactly
    coplanar and has to guess."""
    return max(1e-4, o["size"] * 1e-3)

def _peg_bodies(x, y, z, o, tag):
    h = o["size"] / 2.0
    c = o["chamfer"]
    v = _over(o)
    if o["shape"] == "round":
        out = [cylinder(h, o["height"] - c + v, at = (x, y, z - v), name = tag + "_pin")]
        if c > 0:
            out.append(cone(h, h - c, c, at = (x, y, z + o["height"] - c), name = tag + "_tip"))
        return out
    out = [box(2 * h, 2 * h, o["height"] - c + v, at = (x - h, y - h, z - v), name = tag + "_peg")]
    if c > 0:
        out.append(box(2 * h - 2 * c, 2 * h - 2 * c, c, at = (x - h + c, y - h + c, z + o["height"] - c), name = tag + "_tip"))
    return out

def _column_body(corner, c, z0, z, v, tag):
    """A key column `c` across standing in `corner` from `z0` to the joint at
    `z`, reaching a hair into the two walls it stands against so it overlaps
    them rather than touching them exactly."""
    vx, vy, sx, sy = corner
    x0, x1 = min(vx - sx * v, vx + sx * c), max(vx - sx * v, vx + sx * c)
    y0, y1 = min(vy - sy * v, vy + sy * c), max(vy - sy * v, vy + sy * c)
    return box(x1 - x0, y1 - y0, z - z0, at = (x0, y0, z0), name = tag + "_column")

def _socket_body(x, y, z, o, tag):
    hs = o["size"] / 2.0 + o["clearance"]
    d = o["height"] + o["depth_clearance"]
    v = _over(o)
    if o["shape"] == "round":
        return cylinder(hs, d + v, at = (x, y, z - v), name = tag + "_socket")
    return box(2 * hs, 2 * hs, d + v, at = (x - hs, y - hs, z - v), name = tag + "_socket")

def _onto_index(lowers, onto):
    """`onto=` as an index into the lower bodies: an index, or one of them."""
    if type(onto) == "int":
        if onto < 0 or onto >= len(lowers):
            fail("pegs: onto=%d is not one of the %d lower bodies" % (onto, len(lowers)))
        return onto
    for i, b in enumerate(lowers):
        if b.id == onto.id:
            return i
    fail("pegs: onto= is not one of the lower bodies")

def _under(own, x, y, h):
    """The index of the body whose material is under (x, y) -- its centre,
    else the most of its corners -- of the bodies' own loops `own`."""
    for i, loops in enumerate(own):
        if _inside(loops, x, y):
            return i
    best, bi = 0, 0
    for i, loops in enumerate(own):
        n = len([1 for (px, py) in ((x - h, y - h), (x + h, y - h), (x - h, y + h), (x + h, y + h)) if _inside(loops, px, py)])
        if n > best:
            best, bi = n, i
    return bi

def pegs(lower, upper, at = None, count = 3, z = None, name = "pegs", column = None, onto = None, **options):
    """Register two pieces that stack: pegs standing on `lower`'s top where
    it meets `upper`, and a socket under `upper` over each, a clearance
    bigger. Returns `[lower with its pegs, upper with its sockets]`; the
    bodies changed are consumed.

    Each side is a body or a list of bodies -- a piece printed as one body
    per filament. Every test runs over the union of a side's bodies:
    the peg on the material of all of `lower`, the socket and the wall round
    it in the material of all of `upper`. A peg joins one body of `lower`:
    `onto=` (one of them, or its index), else the one under it; a socket is
    cut from every body of `upper` it reaches. With lists, the return is two
    lists in the order given, the bodies that took nothing returned as they
    were.

    The pieces must still stand where the model has them, meeting on a
    horizontal plane (`z=`, default where `upper` starts). `at=[(x, y),
    ...]` puts the pegs there and checks each fits -- a peg wholly on
    material under the plane, its socket and `min_wall` round it wholly in
    material over it, the socket not breaking through the top -- and fails
    naming the first that does not; without it `count` sites are chosen by
    `peg_sites()`. Every option is `PEG`'s (`size`, `height`, `clearance`,
    `depth_clearance`, `chamfer`, `min_wall`, `shape`), sized for FDM at a
    0.4 mm nozzle.

    `column=` (a side length, at least `size`) stands a key column in a room
    corner under a peg that has no wall top under it: the column runs from
    the bottom of `lower` up to the joint against both walls of the corner,
    joins the body of those walls (or `onto=`), and carries the peg -- a
    corner key, for walls thinner than the peg (`peg_sites()` says where).

    A square peg is boxes, so on a piece joined with `union(weld=)` it is
    unioned exactly; the sockets are subtracted. The new features are named
    `<name>_<k>_peg`, `<name>_<k>_tip`, `<name>_<k>_column`,
    `<name>_<k>_socket`, `<name>_lower` and `<name>_upper` (with lists,
    `<name>_<k>_<i>_socket`, `<name>_lower_<i>` and `<name>_upper_<i>` for
    body `i`)."""
    o = _opts(options)
    single = type(lower) != "list" and type(upper) != "list"
    lowers, uppers = _as_list(lower), _as_list(upper)
    if not lowers or not uppers:
        fail("pegs: no bodies on one side")
    z = _interface(lowers, uppers, z)
    if column != None:
        _check_column(column, o)
    if at == None:
        at = peg_sites(lowers, uppers, count = count, z = z, column = column, **options)
        if len(at) < count:
            fail(("pegs: only %d place%s fit a %s peg with %s of wall round its socket between these pieces; " +
                  "pass count=%d, a smaller size=, column= for key columns in the room corners, or the places as " +
                  "at=[(x, y), ...]") %
                 (len(at), "" if len(at) == 1 else "s", o["size"], o["min_wall"], len(at)))
    if not at:
        fail("pegs: no places given")
    h = o["size"] / 2.0
    hs = h + o["clearance"]
    v = _over(o)
    eps = max(1e-4, o["size"] * 1e-3)
    on = plane(origin = (0, 0, z))
    top = _cuts(lowers, z - eps, on)
    bottom = _cuts(uppers, z + eps, on)
    deep = _cuts(uppers, z + o["height"] + o["depth_clearance"] + eps, on)
    mid = _cuts(lowers, (_bottom(lowers) + z) / 2.0, on) if column != None else []
    places = []
    for (x, y) in at:
        corner = None
        if not _square_in(top, x, y, h):
            if column != None:
                corner = _column_at(top, mid, x, y, column, eps)
            if corner == None:
                fail(("pegs: a %s peg at (%s, %s) does not fit: it needs material under the plane z = %s for " +
                      "the peg%s, and over it for a socket with %s of wall all round that does not break " +
                      "through; peg_sites() lists the places that do") %
                     (o["size"], x, y, z, "" if column == None else " or a room corner for its column", o["min_wall"]))
        if not _socket_fits(bottom, deep, x, y, o):
            fail(("pegs: a %s peg at (%s, %s) does not fit: the piece over the plane z = %s has no room for its " +
                  "socket with %s of wall all round, or the socket would break through; peg_sites() lists the " +
                  "places that do") % (o["size"], x, y, z, o["min_wall"]))
        places.append((x, y, corner))

    # Which lower body each peg joins: onto=, else the one under it -- for a
    # key column, the one whose walls it stands against.
    fixed = _onto_index(lowers, onto) if onto != None else None
    own = top if fixed == None and len(lowers) > 1 else None
    z0 = _bottom(lowers)
    plugs = {}
    holes = []
    for k, (x, y, corner) in enumerate(places):
        tag = "%s_%d" % (name, k)
        if fixed != None:
            i = fixed
        elif own == None:
            i = 0
        elif corner != None:
            vx, vy, sx, sy = corner
            i = _under(own, vx + sx * column / 2.0, vy - sy * eps, eps)
        else:
            i = _under(own, x, y, h)
        add = _peg_bodies(x, y, z, o, tag)
        if corner != None:
            add.append(_column_body(corner, column, z0, z, v, tag))
        plugs[i] = plugs.get(i, []) + add
        holes.append((x, y, tag))
    out_lower = []
    for i, b in enumerate(lowers):
        if i in plugs:
            out_lower.append(union([b] + plugs[i], name = name + "_lower" if single else "%s_lower_%d" % (name, i)))
        else:
            out_lower.append(b)
    # A socket is cut from every body of the upper piece it reaches: just over
    # the plane, or at its own top.
    out_upper = []
    d = o["height"] + o["depth_clearance"]
    for i, b in enumerate(uppers):
        if len(uppers) == 1:
            mine = [_socket_body(x, y, z, o, tag) for (x, y, tag) in holes]
        else:
            low = _section(b, z + eps, on)
            high = _section(b, z + d - eps, on)
            mine = [_socket_body(x, y, z, o, "%s_%d" % (tag, i)) for (x, y, tag) in holes
                    if _touches(low, x, y, hs) or _touches(high, x, y, hs)]
        if mine:
            out_upper.append(subtract([b] + mine, name = name + "_upper" if single else "%s_upper_%d" % (name, i)))
        else:
            out_upper.append(b)
    if single:
        return [out_lower[0], out_upper[0]]
    return [out_lower, out_upper]

def scale_body(b, factor, name):
    """`scale(b, factor)` about the origin, under `name`."""
    return scale(b, factor, name = name)

def _clip(b, z0, z1, i, tag, thick = 0.0):
    """The part of body `b` between z0 and z1: a box by arithmetic, anything
    else by a boolean with a block that covers it. A box narrower than
    `thick` across x or y is widened to it about its centre line."""
    lo, hi = b.bbox()
    a, c = max(lo[2], z0), min(hi[2], z1)
    if is_box(b):
        x0, x1, y0, y1 = lo[0], hi[0], lo[1], hi[1]
        if x1 - x0 < thick:
            m = (x0 + x1) / 2.0
            x0, x1 = m - thick / 2.0, m + thick / 2.0
        if y1 - y0 < thick:
            m = (y0 + y1) / 2.0
            y0, y1 = m - thick / 2.0, m + thick / 2.0
        return box(x1 - x0, y1 - y0, c - a, at = (x0, y0, a), name = "%s_b%d" % (tag, i))
    if lo[2] >= z0 and hi[2] <= z1:
        return copy(b, name = "%s_c%d" % (tag, i))
    k = box(hi[0] - lo[0] + 2, hi[1] - lo[1] + 2, z1 - z0, at = (lo[0] - 1, lo[1] - 1, z0), name = "%s_k%d" % (tag, i))
    return intersect(copy(b, name = "%s_s%d" % (tag, i)), k, name = "%s_x%d" % (tag, i))

def story_bands(parts, levels, regions = None, scale = 1.0, weld = 0.01, sliver = None, pegs_per_joint = 0,
                drop = True, name = "story", peg = None, layer = None, thicken = None):
    """Cut `parts` (a list of bodies) into horizontal bands and make each a
    printable piece. Returns a list of `(label, body)` in stack order, region
    by region: `"main.0"`, `"main.1"`, ..., then each region's.

    `levels` are the cut heights in the model's units, lowest first: n + 1
    levels make n bands. `regions` is a list of `(label, ((x0, y0), (x1,
    y1)), levels)`: a part whose plan centre is inside the rectangle is cut at
    that region's levels and stacked as its own pieces -- a wing whose floors
    do not line up with the main house's, behind a joint. The rest are
    `"main"`.

    Every part is clipped to each band it reaches (a box by arithmetic, not a
    boolean; anything else intersected with a block), and a clipped piece
    thinner than `sliver` (default: a hundredth of the thinnest band) is
    dropped -- the sliver a wall makes poking a hair past a cut. Then, in
    order: everything is multiplied by `scale` (`304.8 / 96` takes a model
    in feet to 1:96 in millimetres); each band is joined with `union(weld=)`
    (`weld` in the scaled units; `weld=None` leaves each band a list of
    parts, for `export(..., name=)`); with `pegs_per_joint`, each band is
    pegged to the one above with `pegs()` (`peg=` a dict of `PEG`'s
    options); and with `drop` each piece is moved onto the bed. A band with
    nothing in it is left out of the list. `layer=` puts the pieces on a
    display layer, so `cadpole check --layer` and `cadpole export --layer` take
    them and nothing of the model they were cut from.

    `thicken=` (in the scaled units: millimetres at a print scale) widens
    every box narrower than it across x or y -- a 3-in mullion, a glass
    pane, a baluster -- to that width about its centre line before the
    join, so it prints rather than vanishing: `thicken=0.8` is two
    perimeters at a 0.4 mm nozzle. Only boxes, and only in plan; a body
    that is not a box is left as it is."""
    peg = peg or {}
    if len(levels) < 2:
        fail("story_bands: levels needs two heights or more, got %s" % (levels,))
    for k in range(len(levels) - 1):
        if not levels[k] < levels[k + 1]:
            fail("story_bands: levels go up: %s then %s" % (levels[k], levels[k + 1]))
    groups = [("main", None, levels)]
    for r in regions or []:
        label, rect, lv = r
        if len(lv) < 2:
            fail("story_bands: region %s needs two levels or more" % label)
        groups.append((label, rect, lv))
    thin = min([levels[k + 1] - levels[k] for k in range(len(levels) - 1)])
    sliver = sliver if sliver != None else thin / 100.0
    thick = thicken / scale if thicken else 0.0

    def region_of(b):
        lo, hi = b.bbox()
        cx, cy = (lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0
        for label, rect, lv in groups[1:]:
            (x0, y0), (x1, y1) = rect
            if cx >= x0 and cx <= x1 and cy >= y0 and cy <= y1:
                return label
        return "main"

    owned = {}
    for b in parts:
        owned.setdefault(region_of(b), []).append(b)

    out = []
    for label, rect, lv in groups:
        bands = []
        for k in range(len(lv) - 1):
            z0, z1 = lv[k], lv[k + 1]
            tag = "%s_%s_%d" % (name, label, k)
            pieces = []
            for i, b in enumerate(owned.get(label, [])):
                lo, hi = b.bbox()
                if min(hi[2], z1) - max(lo[2], z0) < sliver:
                    continue
                p = _clip(b, z0, z1, i, tag, thick)
                if scale != 1.0:
                    p = scale_body(p, scale, "%s_m%d" % (tag, i))
                pieces.append(p)
            if not pieces:
                continue
            body = union(pieces, weld = weld, name = tag) if weld != None else pieces
            bands.append([k, z0 * scale, body])
        if pegs_per_joint > 0 and weld != None:
            for j in range(len(bands) - 1):
                if bands[j + 1][0] != bands[j][0] + 1:
                    continue
                where = "%s.%d and %s.%d" % (label, bands[j][0], label, bands[j + 1][0])
                sites = peg_sites(bands[j][2], bands[j + 1][2], count = pegs_per_joint, **peg)
                if len(sites) < pegs_per_joint:
                    # Pegged with what fits, and said: a joint with no room
                    # for a peg is the model's (walls too thin for the peg,
                    # or a floor too thin for its socket), not a reason to
                    # make nothing.
                    print(("story_bands: room for %d of %d pegs between %s; a smaller peg (peg={\"size\": ...}) " +
                           "or a thicker floor over the joint makes more") % (len(sites), pegs_per_joint, where))
                if not sites:
                    continue
                lower, upper = pegs(bands[j][2], bands[j + 1][2], at = sites,
                                    name = "%s_%s_pegs%d" % (name, label, j), **peg)
                bands[j][2], bands[j + 1][2] = lower, upper
        for k, zb, body in bands:
            dz, end = (-zb, "_bed") if drop else (0.0, "_piece")
            if weld != None:
                body = translate(body, (0, 0, dz), name = "%s_%s_%d%s" % (name, label, k, end), layer = layer)
            else:
                body = [translate(p, (0, 0, dz), name = p.name + end, layer = layer) for p in body]
            out.append(("%s.%d" % (label, k), body))
    return out

