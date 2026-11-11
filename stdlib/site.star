"""@cadpole/site.star -- site modelling: a local frame from latitude and longitude, parcels with named sides, setback and rear-yard lines, a grade profile along a lot, a zoning envelope above grade, cut and fill for a pad, neighbour masses.

    load("@cadpole/site.star", "ft", "inch", "to_ft", "to_mm",
         "local_frame", "latlon_to_local", "local_to_latlon",
         "parcel", "setbacks", "rear_yard", "lot_point",
         "profile_points", "grade_profile",
         "envelope_above_grade", "cut_fill", "footprint_mass", "polygon_area")

The kernel is millimetres: `ft(45)` is 13716 mm, `to_ft(13716)` is 45, and
every body-making function here takes millimetres (how a length is written
back -- `45'-6"` on a sheet -- is `drawing(units="ft-in")`'s business). A
local frame carries its own unit (`units="ft"` by default) for what surveys
and parcel maps give -- `latlon_to_local` answers in it and
`to_mm(frame, points)` converts a list of shots, or a `(depth, z)` profile,
for `terrain()`. The library records nothing when loaded and declares no
parameters; every body-building function takes `name=` and forwards it.

## The data shapes this serves

* **Parcel corners as lat/lon** (a city's open-data parcel polygons): `parcel(frame, corners)`.
* **A surveyed profile along the lot** (a section the surveyor shot down
  the depth, `(depth, z)` pairs from the front line) with **the neighbours'
  LiDAR ground samples** (`(x, y, z)` in the frame): `profile_points(lot,
  to_mm(frame, profile))` places the profile on the lot's side lines and
  centreline, `to_mm(frame, samples)` converts the rest, and their
  concatenation is `terrain(points, ...)`'s argument.
* **Spot elevations** (a survey's hundreds of shots, a LiDAR footprint's
  ground min/median/max): `to_mm(frame, shots)` then `terrain(points, ...)`.
* **Contour polylines** (a topo sheet, a DXF): `terrain_from_contours(...)`.
* **Building footprints with a height** (LiDAR building outlines with
  ground and roof elevations): `footprint_mass(frame, polygon, ground_z, height)`.

## Lines on the lot

`parcel(...)` names the lot's sides `front`, `rear` and `side[k]`;
`setbacks(lot, front=, rear=, sides=)` moves each inward and clips;
`rear_yard(lot, depth)` is the line parallel to the front `depth` in from
it (a planning code's "45 % of the lot depth") with the plan area in front
of it; `lot_point(lot, across, depth)` is the site point `across` along the
front and `depth` in from it, the lot's own coordinates on the site's; and
`grade_profile(terrain, lot)` reads the ground along the depth as `(depth,
z)` pairs -- what `@cadpole/building.star`'s `height_envelope(grade, roof)`
takes as `grade`, and what a section plots.

## The frame

`local_frame(lat0, lon0, rotation_deg=0, units="ft")` is a tangent plane
at `(lat0, lon0)`: x east, y north, turned counter-clockwise by
`rotation_deg` so a lot can stand upright (a lot whose depth runs at
bearing 345 degrees is upright at `rotation_deg=15`). Metres per degree
come from the usual spheroid series, so at 38 degrees one arc-second is
101.16 ft of latitude and 80.05 ft of longitude; across a city block the
error against a projected survey is millimetres. It is not a map
projection: use it for a site, not a county.

## The datum trap

LiDAR and most public elevation data are NAVD88; older surveys and city
benchmarks are often a local datum, several feet off. Every elevation in one model must be on one datum: add
the difference to the survey's numbers (or subtract it from the LiDAR's)
before they meet, and say which in a comment.

## Envelopes

`envelope_above_grade(terrain, area, height, cap=, base=, tolerance=,
name=)` is the rule "no higher than `height` above grade at every point,
and never above `cap`": the terrain raised by `height` (`Terrain.offset_up`,
exact), over the plan `area`, under the plane `z = cap`, as one body from
`base` up. When the area is a rectangle on the site's axes -- a lot in its
own frame -- the body is `Terrain.clip(rect).body()`, the surface over
exactly that rectangle with no boolean, so a fitted terrain's envelope is
an exact B-rep; any other area is the slab intersected with the area's
prism. The cap is applied only where it bites (`Terrain.z_range` says), and
a cap that bites a fitted surface runs on the mesh kernel at the document
tolerance -- the exact kernel's trimmed patch is what the tessellator
cannot close. Subtract the envelope from a massing and what is left is what
breaks the rule; `intersect` it with the massing and you have the buildable
part.

## Cut and fill

`cut_fill(terrain, area, level, base=, name=)` is the earthwork for a level
pad: within the area, the ground above `level` is the `cut` body and the
space between the ground and `level` where the ground is below it is the
`fill` body, each `None` where there is nothing to do, with their volumes.
Both are mesh booleans (a TIN is what a surveyor's software computes
earthwork from; a fitted surface is tessellated at the document tolerance
first), so pass the TIN.
"""

FT = 304.8
INCH = 25.4
M = 1000.0

_UNIT_MM = {"ft": FT, "in": INCH, "m": M, "mm": 1.0}


def ft(x):
    """`x` feet as millimetres."""
    return x * FT


def inch(x):
    """`x` inches as millimetres."""
    return x * INCH


def to_ft(mm):
    """Millimetres as feet."""
    return mm / FT


def _m_per_deg_lat(lat):
    # Metres per degree of latitude on the WGS84 spheroid, the standard
    # three-term series (Cadpole's `math` takes degrees).
    return 111132.954 - 559.822 * math.cos(2 * lat) + 1.175 * math.cos(4 * lat)


def _m_per_deg_lon(lat):
    return 111412.84 * math.cos(lat) - 93.5 * math.cos(3 * lat) + 0.118 * math.cos(5 * lat)


def local_frame(lat0, lon0, rotation_deg=0, units="ft"):
    """A local tangent frame at `(lat0, lon0)`: x east, y north, turned
    counter-clockwise by `rotation_deg`, in `units` ("ft", "m", "in" or
    "mm"). A dict; `latlon_to_local`, `parcel`, `footprint_mass` and
    `to_mm` take it."""
    if units not in _UNIT_MM:
        fail("local_frame: units must be one of ft, m, in, mm, got %r" % units)
    if lat0 < -90 or lat0 > 90 or lon0 < -180 or lon0 > 180:
        fail("local_frame: (%s, %s) is not a latitude and longitude in degrees" % (lat0, lon0))
    return {
        "lat0": lat0,
        "lon0": lon0,
        "rotation_deg": rotation_deg,
        "units": units,
        "unit_mm": _UNIT_MM[units],
        "m_per_deg_lat": _m_per_deg_lat(lat0),
        "m_per_deg_lon": _m_per_deg_lon(lat0),
    }


def latlon_to_local(frame, lat, lon):
    """`(x, y)` of a point in the frame's units."""
    scale = 1000.0 / frame["unit_mm"]
    e = (lon - frame["lon0"]) * frame["m_per_deg_lon"] * scale
    n = (lat - frame["lat0"]) * frame["m_per_deg_lat"] * scale
    c = math.cos(frame["rotation_deg"])
    s = math.sin(frame["rotation_deg"])
    return (e * c + n * s, -e * s + n * c)


def local_to_latlon(frame, x, y):
    """`(lat, lon)` of a point given in the frame's units: the inverse of `latlon_to_local`."""
    c = math.cos(frame["rotation_deg"])
    s = math.sin(frame["rotation_deg"])
    e = x * c - y * s
    n = x * s + y * c
    scale = frame["unit_mm"] / 1000.0
    return (frame["lat0"] + n * scale / frame["m_per_deg_lat"], frame["lon0"] + e * scale / frame["m_per_deg_lon"])


def to_mm(frame, points):
    """Points `(x, y)` or `(x, y, z)` in the frame's units as millimetres:
    what `terrain(...)`, `sketch().polygon(...)` and every body take."""
    u = frame["unit_mm"]
    out = []
    for p in points:
        if len(p) == 3:
            out.append((p[0] * u, p[1] * u, p[2] * u))
        elif len(p) == 2:
            out.append((p[0] * u, p[1] * u))
        else:
            fail("to_mm: a point is (x, y) or (x, y, z), got %r" % (p,))
    return out


def polygon_area(points):
    """The signed area of a polygon (positive counter-clockwise), in the
    units of its coordinates squared."""
    a = 0.0
    n = len(points)
    for i in range(n):
        p = points[i]
        q = points[(i + 1) % n]
        a += p[0] * q[1] - q[0] * p[1]
    return a / 2.0


def _ccw(points, front):
    """The polygon counter-clockwise, and the edge index `front` carried
    over the reversal (edge k of a list is edge n-2-k of the reversed one)."""
    if polygon_area(points) < 0:
        n = len(points)
        return (list(reversed(points)), (n - 2 - front) % n)
    return (points, front)


def _named(name, suffix=""):
    """The `name=` keyword to forward, as kwargs: `{"name": name + suffix}`,
    or nothing for an unnamed call -- so the auto-numbered ids never collide
    and no native is handed an explicit `None`, which every one refuses."""
    if name == None:
        return {}
    return {"name": name + suffix}


def _lot(mm, front, name):
    n = len(mm)
    if n < 3:
        fail("parcel: a lot needs at least 3 corners, got %d" % n)
    if front < 0 or front >= n:
        fail("parcel: front=%d is not an edge of a %d-corner lot" % (front, n))
    mm, front = _ccw(mm, front)
    edges = [(mm[i], mm[(i + 1) % n]) for i in range(n)]
    fa, fb = edges[front]
    dx = fb[0] - fa[0]
    dy = fb[1] - fa[1]
    length = math.sqrt(dx * dx + dy * dy)
    if length <= 0:
        fail("parcel: the front edge has no length")
    # The inward normal of a counter-clockwise polygon is to the left of
    # the edge; the rear is the edge whose midpoint is deepest along it.
    nx = -dy / length
    ny = dx / length
    depth = -1.0
    rear = -1
    for k in range(n):
        if k == front:
            continue
        mx = (edges[k][0][0] + edges[k][1][0]) / 2.0
        my = (edges[k][0][1] + edges[k][1][1]) / 2.0
        d = (mx - fa[0]) * nx + (my - fa[1]) * ny
        if d > depth:
            depth = d
            rear = k
    names = {}
    lines = {}
    side = 0
    for j in range(n):
        k = (front + j) % n
        if k == front:
            nm = "front"
        elif k == rear:
            nm = "rear"
        else:
            nm = "side[%d]" % side
            side += 1
        names[k] = nm
        lines[nm] = edges[k]
    return {
        "sketch": sketch(XY, **_named(name)).polygon(mm),
        "corners": mm,
        "lines": lines,
        "edge_names": [names[k] for k in range(n)],
        # `extrude(lot["sketch"])` names its walls `<body>/side[0.k]` by
        # edge: the suffix of each named side's face.
        "faces": {names[k]: "side[0.%d]" % k for k in range(n)},
        "front_index": front,
        "rear_index": rear,
        "area": polygon_area(mm),
        "width": length,
        "depth": depth,
        "name": name,
    }


def parcel(frame, corners, front=0, latlon=True, name=None):
    """A lot from its recorded corners, in order round the lot either way:
    `(lat, lon)` pairs (the default), or `(x, y)` in the frame's units with
    `latlon=False`. `front` is the index of the street side: the edge from
    `corners[front]` to the next corner. Returns a dict: `sketch` (the lot
    on XY, mm, counter-clockwise), `corners` (mm), `lines` (`front`, `rear`
    -- the edge deepest from the front -- and `side[0]`, `side[1]`, ... in
    order round from the front, each a pair of mm points), `edge_names`,
    `faces` (the `side[0.k]` suffix each named side gets when the sketch is
    extruded), `front_index`, `rear_index`, `area` (mm²), `width` (the
    front's length, mm) and `depth` (front line to the rear's midpoint, mm)."""
    if latlon:
        pts = [latlon_to_local(frame, c[0], c[1]) for c in corners]
    else:
        pts = [(c[0], c[1]) for c in corners]
    return _lot(to_mm(frame, pts), front, name)


def _clip(poly, a, b, d):
    """Sutherland-Hodgman: the part of `poly` on the inside of the line
    a -> b (its left, for a counter-clockwise polygon), moved in by `d`."""
    dx = b[0] - a[0]
    dy = b[1] - a[1]
    length = math.sqrt(dx * dx + dy * dy)
    nx = -dy / length
    ny = dx / length
    out = []
    n = len(poly)
    for i in range(n):
        p = poly[i]
        q = poly[(i + 1) % n]
        sp = (p[0] - a[0]) * nx + (p[1] - a[1]) * ny - d
        sq = (q[0] - a[0]) * nx + (q[1] - a[1]) * ny - d
        if sp >= 0:
            out.append(p)
        if (sp >= 0) != (sq >= 0):
            t = sp / (sp - sq)
            out.append((p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])))
    return out


def setbacks(lot, front=0, rear=0, sides=0, name=None):
    """The buildable plan area of a `parcel(...)`: its polygon clipped by
    each side's line moved inward by that side's setback (mm; `sides`
    applies to every side that is not the front or the rear). Returns a
    dict: `sketch` (XY, mm), `corners`, `lines` (the setback lines, named as
    the lot's sides are) and `area` (mm²). Fails when nothing is left."""
    pts = lot["corners"]
    n = len(pts)
    names = lot["edge_names"]
    poly = list(pts)
    lines = {}
    for k in range(n):
        nm = names[k]
        if nm == "front":
            d = front
        elif nm == "rear":
            d = rear
        else:
            d = sides
        a = pts[k]
        b = pts[(k + 1) % n]
        dx = b[0] - a[0]
        dy = b[1] - a[1]
        length = math.sqrt(dx * dx + dy * dy)
        nx = -dy / length
        ny = dx / length
        lines[nm] = ((a[0] + nx * d, a[1] + ny * d), (b[0] + nx * d, b[1] + ny * d))
        poly = _clip(poly, a, b, d)
        if len(poly) < 3:
            fail("setbacks: the setbacks leave no buildable area (the %s line at %s mm closed it)" % (nm, d))
    return {
        "sketch": sketch(XY, **_named(name)).polygon(poly),
        "corners": poly,
        "lines": lines,
        "area": polygon_area(poly),
        "name": name,
    }


def _front_axes(lot):
    """The lot's front line's first corner, the unit vector along the front
    and the unit inward normal: the lot's `across` and `depth` axes."""
    a, b = lot["lines"]["front"]
    dx = b[0] - a[0]
    dy = b[1] - a[1]
    length = math.sqrt(dx * dx + dy * dy)
    return (a, (dx / length, dy / length), (-dy / length, dx / length))


def lot_point(lot, across, depth):
    """The site point `across` mm along the front line from its first corner
    and `depth` mm in from it, as `(x, y)` mm: the lot's own coordinates on
    the site's, whichever way the lot faces."""
    a, t, n = _front_axes(lot)
    return (a[0] + t[0] * across + n[0] * depth, a[1] + t[1] * across + n[1] * depth)


def profile_points(lot, profile, across=None):
    """Spot elevations from a surveyed profile along the lot's depth: each
    `(depth, z)` of `profile` (mm; `depth` from the front line) placed at
    every `across` position (mm along the front; default the two side lines
    and the centreline), as `(x, y, z)` mm points for `terrain(...)`. Add
    the neighbours' LiDAR ground samples to the list and call
    `terrain(points, ...)`; with the default `across` a lot's ground is
    level across and the samples beyond the side lines carry the rest."""
    if across == None:
        across = [0.0, lot["width"] / 2.0, lot["width"]]
    if len(profile) < 2:
        fail("profile_points: a profile needs at least 2 (depth, z) pairs, got %d" % len(profile))
    out = []
    for p in profile:
        if len(p) != 2:
            fail("profile_points: a profile is (depth, z) pairs, got %r" % (p,))
        for u in across:
            x, y = lot_point(lot, u, p[0])
            out.append((x, y, p[1]))
    return out


def grade_profile(terrain, lot, across=None, step=None, start=None, end=None):
    """The ground along the lot's depth: `(depth, z)` pairs, mm, read off the
    terrain at `across` (mm along the front; default the centreline) every
    `step` (default 1 ft) from `start` (0, the front line) to `end` (the
    lot's depth). What `@cadpole/building.star`'s `height_envelope(grade,
    roof)` takes as `grade`, and what a section plots; fails where the line
    leaves the surface."""
    u = lot["width"] / 2.0 if across == None else across
    st = ft(1) if step == None else step
    v0 = 0.0 if start == None else start
    v1 = lot["depth"] if end == None else end
    if st <= 0:
        fail("grade_profile: step must be positive, got %s" % st)
    if v1 <= v0:
        fail("grade_profile: end %s mm is not beyond start %s mm" % (v1, v0))
    count = max(1, int(math.ceil((v1 - v0) / st - 1e-9)))
    out = []
    for k in range(count + 1):
        v = v0 + (v1 - v0) * k / count
        x, y = lot_point(lot, u, v)
        out.append((v, terrain.z_at(x, y)))
    return out


def rear_yard(lot, depth, name=None):
    """The rear-yard line `depth` mm in from the front line (a planning
    code's "45 % of the lot depth"), and the plan area in front of it:
    the dict `setbacks` returns -- `sketch` (XY, mm), `corners`, `lines`
    (the lot's sides as named, `rear` now the rear-yard line) and `area`
    (mm²) -- plus `line`, the rear-yard line as a pair of mm points, and
    `depth`. Fails when the line is not inside the lot."""
    if depth <= 0 or depth >= lot["depth"]:
        fail("rear_yard: a line %s mm from the front is not inside a lot %s mm deep" % (depth, lot["depth"]))
    a, t, n = _front_axes(lot)
    b = lot["lines"]["front"][1]
    la = (a[0] + n[0] * depth, a[1] + n[1] * depth)
    lb = (b[0] + n[0] * depth, b[1] + n[1] * depth)
    # The front's side of the line is the left of lb -> la.
    poly = _clip(lot["corners"], lb, la, 0)
    if len(poly) < 3:
        fail("rear_yard: the line %s mm from the front leaves no area in front of it" % depth)
    lines = {}
    for nm in lot["edge_names"]:
        if nm != "rear":
            lines[nm] = lot["lines"][nm]
    lines["rear"] = (la, lb)
    return {
        "sketch": sketch(XY, **_named(name)).polygon(poly),
        "corners": poly,
        "lines": lines,
        "line": (la, lb),
        "depth": depth,
        "area": polygon_area(poly),
        "name": name,
    }


def _area_sketch(area, what):
    if type(area) == "Sketch":
        return area
    if type(area) == "dict":
        return area["sketch"]
    if type(area) == "list" or type(area) == "tuple":
        return sketch(XY).polygon(area)
    fail("%s: the area is a Sketch, a parcel or setbacks dict, or a list of (x, y) mm points, got %s" % (what, type(area)))


def _area_corners(area):
    """The area's polygon as mm points, or None for a Sketch (which the
    terrain's own queries take as it is)."""
    if type(area) == "dict":
        return area["corners"]
    if type(area) == "list" or type(area) == "tuple":
        return list(area)
    return None


def _rectangle_extent(corners):
    """`((x0, y0), (x1, y1))` when the polygon is a rectangle on the site's
    axes -- four corners, every edge along x or along y -- else None."""
    if corners == None or len(corners) != 4:
        return None
    xs = [p[0] for p in corners]
    ys = [p[1] for p in corners]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    if x1 - x0 <= 0 or y1 - y0 <= 0:
        return None
    eps = 1e-6 * max(x1 - x0, y1 - y0)
    for i in range(4):
        p = corners[i]
        q = corners[(i + 1) % 4]
        if abs(p[0] - q[0]) > eps and abs(p[1] - q[1]) > eps:
            return None
        if (abs(p[0] - x0) > eps and abs(p[0] - x1) > eps) or (abs(p[1] - y0) > eps and abs(p[1] - y1) > eps):
            return None
    return ((x0, y0), (x1, y1))


def _range_over(terrain, area):
    corners = _area_corners(area)
    return terrain.z_range(area if corners == None else corners)


def envelope_above_grade(terrain, area, height, cap=None, base=None, tolerance=None, name=None):
    """The zoning envelope "no higher than `height` above grade at every
    point": the terrain raised by `height` (mm; `Terrain.offset_up`, exact)
    over the plan `area` (a `parcel`/`setbacks`/`rear_yard` dict, a list of
    mm points, or a Sketch), under the absolute `cap` (mm, a z; default
    none) and standing on the plane `z = base` (default: 10 ft below the
    lowest grade). One body named `name`: a mesh for a TIN, exact for a
    fitted terrain. A rectangular area on the site's axes is
    `Terrain.clip(rect).body()` -- the surface over exactly the rectangle,
    no boolean; any other is the slab intersected with the area's prism.
    The cap is applied only where the raised surface reaches it, and a cap
    that bites a fitted surface runs on the mesh kernel at the document
    tolerance. `tolerance` is a fitted slab's own chord tolerance
    (`Terrain.body(tolerance=)`, default a quarter inch: a site is metres).
    Subtract it from a massing to see what breaks the rule; intersect to
    keep what fits."""
    raised = terrain.offset_up(height)
    b = base
    if b == None:
        b = terrain.z_min - ft(10)
    tol = inch(0.25) if tolerance == None else tolerance
    lo, hi = _range_over(raised, area)
    bites = cap != None and cap < hi
    top = cap if bites else hi + ft(1)
    if top <= b:
        fail("envelope_above_grade: the cap %s mm is not above the base %s mm" % (top, b))
    rect = _rectangle_extent(_area_corners(area))
    fitted = terrain.method == "fit"
    if rect != None and not bites:
        return raised.clip(rect).body(base = b, tolerance = tol, **_named(name))
    if rect != None:
        slab = raised.clip(rect).body(base = b, tolerance = tol, **_named(name, "_grade"))
    else:
        slab = raised.body(base = b, tolerance = tol, **_named(name, "_grade"))
    sk = _area_sketch(area, "envelope_above_grade")
    prism = translate(extrude(sk, top - b, **_named(name, "_prism")), (0, 0, b))
    if fitted:
        return intersect(slab, prism, kernel = kernel.mesh, **_named(name))
    return intersect(slab, prism, **_named(name))


def cut_fill(terrain, area, level, base=None, name=None):
    """Cut and fill for a level pad at `level` (mm, a z) over the plan
    `area` (a `parcel`/`setbacks`/`rear_yard` dict, a list of mm points, or
    a Sketch): `cut` is the ground above the level within the area -- what
    is dug out -- and `fill` the space between the ground and the level
    where the ground is below it, each a Body, or None where there is
    nothing to do (`Terrain.z_range` over the area decides). Returns a dict
    with those, `cut_volume` and `fill_volume` (mm³, 0 for None), and
    `ground`, the slab the booleans ran on, standing on `base` (default
    10 ft below the lowest grade). Mesh booleans: pass the TIN."""
    lo, hi = _range_over(terrain, area)
    b = base
    if b == None:
        b = terrain.z_min - ft(10)
    if level <= b:
        fail("cut_fill: the level %s mm is not above the base %s mm" % (level, b))
    rect = _rectangle_extent(_area_corners(area))
    where = terrain.clip(rect) if rect != None else terrain
    ground = where.body(base = b, **_named(name, "_ground"))
    sk = _area_sketch(area, "cut_fill")
    fitted = terrain.method == "fit"
    cut = None
    fill = None
    eps = 1e-6
    if hi > level + eps:
        above = translate(extrude(sk, hi + ft(1) - level, **_named(name, "_above")), (0, 0, level))
        if fitted:
            cut = intersect(ground, above, kernel = kernel.mesh, **_named(name, "_cut"))
        else:
            cut = intersect(ground, above, **_named(name, "_cut"))
    if lo < level - eps:
        # From a millimetre above the base, so the prism's bottom is inside
        # the ground rather than flush with its base.
        below = translate(extrude(sk, level - b - 1.0, **_named(name, "_below")), (0, 0, b + 1.0))
        if fitted:
            fill = subtract(below, ground, kernel = kernel.mesh, **_named(name, "_fill"))
        else:
            fill = subtract(below, ground, **_named(name, "_fill"))
    return {
        "cut": cut,
        "fill": fill,
        "cut_volume": cut.volume() if cut != None else 0.0,
        "fill_volume": fill.volume() if fill != None else 0.0,
        "ground": ground,
        "level": level,
        "name": name,
    }


def footprint_mass(frame, polygon, ground_z, height, latlon=True, name=None):
    """A neighbour as a mass: its footprint polygon (`(lat, lon)` pairs, or
    `(x, y)` in the frame's units with `latlon=False`) extruded `height`
    mm up from the plane `z = ground_z` mm -- a LiDAR footprint's ground
    elevation and its median or max height. A body named `name`."""
    if latlon:
        pts = [latlon_to_local(frame, p[0], p[1]) for p in polygon]
    else:
        pts = [(p[0], p[1]) for p in polygon]
    mm, _ = _ccw(to_mm(frame, pts), 0)
    if height <= 0:
        fail("footprint_mass: height must be positive, got %s" % height)
    sk = sketch(XY, **_named(name, "_plan")).polygon(mm)
    return translate(extrude(sk, height, **_named(name, "_mass")), (0, 0, ground_z), **_named(name))
