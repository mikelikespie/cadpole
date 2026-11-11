"""@cadpole/plan_solve.star -- floor plans as mixed-integer programs on the `opt` builder.

    load("@cadpole/plan_solve.star", "rect", "wall", "touch", "hold", "annex", "face", "face_ref",
         "floor", "room", "shared", "court", "rule", "stacks", "structure", "not_over", "problem",
         "solve", "options", "solve_floors", "score", "ftin",
         "build", "run", "read", "row", "var", "binary", "objective_term", "room_vars", "layout_of",
         "arrangement_of", "relations")

A free-rectangle floor model. Every room is an axis-aligned rectangle (inside faces) -- x (its west face, u), y (its
south face, v), w and d, in feet -- and every rule is a linear row over those and binaries. The one product the
rules need, a 0/1 bit times a length, is four exact rows. Nothing here multiplies two variables and there is no
callback. The library records nothing when loaded; `solve()` and friends make one `opt.problem` each, named
from `name=` (or `plan_solve_<problem name>`), so a second solve of one problem in a document needs its own
`name=` (two features may not share a name).

## Units and frame

Feet in, feet out: the plan frame of `@cadpole/plan.star` (u east, v north, rects as `(u0, u1, v0, v1)` on inside
faces). The solver works in feet too (model in units near 1; in mm an area is 1e7 beside a 762-mm door).

## A problem is data

    F = floor("A", zones={"A": rect(0.5, 30.5, 0.5, 30.5)}, obstacles={"stair": rect(0.5, 10.5, 0.5, 7.0)},
              fixed={"lobby": rect(10.5, 14.5, 0.5, 7.0)}, roots=["lobby"],
              egress=[wall("W", 0.5, (10.0, 30.5)), wall("N", 30.5, (0.5, 30.5))])
    R = [room("hall", floor="A", kind="hall", value=0.2, w=(3, 30), d=(3, 30), opens_to=["lobby"]),
         room("bed1", floor="A", kind="bed", area=(100, 150), w=(9.5, 15), d=(9.5, 15), aspect=1.4,
              opens_to=["hall", "lobby"])]
    P = problem("toy", floors=[F], rooms=R, sym=[("A", "bed1", "bed2")])
    plan = solve(P, time_limit=30)          # a plan value (@cadpole/plan.star's format); meta["plan_solve"] has the numbers

`floor()`, `room()`, `problem()` and the rest return plain dicts, so a problem can be changed by copying
(plan_ask.star's `apply()` does) and rebuilt for every variant a question needs.

## The rules (each a named row, "<family>:<floor>:<room>:<what>")

| family | what it holds |
| --- | --- |
| zone | a room inside one of its allowed zones (one binary per zone, big-M rows to the zone's faces) |
| pair | two rooms apart, with the wall between them: one binary per possible side, sum >= 1; M per pair from the bounds; a room and a fixed room the same (drawn to its inside faces, it keeps a room's gap), a room and an obstacle with none (drawn with its walls) |
| area, size | program areas and dimensions (`area=`, `w=`, `d=`) |
| R304 | habitable rooms >= 70 sq ft and >= 7 ft both ways (read on the main piece of an L-shaped room) |
| R311 | halls >= 36 in both ways (`hall_min=` an exception, flagged in the row's name) |
| R307 | fixture minimums either way round (`orient=(a, b)`, one binary) |
| R310 | a sleeping room's edge on an escape wall, overlapping it by a 3-ft window |
| shape | aspect: long side <= `aspect` x short side |
| contain | rects a room must hold (`holds=`), and a floor's must-holds (`hold()`: inside one of several rooms) |
| touch | a room's edge on a line over a length (`touch()`: the kitchen on the deck glass) |
| annex | an L-shaped room's second rectangle: flush on one wall, open to it, >= 3 ft both ways or nothing |
| door, tree | each room opens onto one parent through >= 2'-6" of shared wall, across the wall the pair rows keep (a fixed room's too); depth(child) >= depth(parent) + 1 |
| sym | interchangeable rooms in one order (bed 1 north of bed 2): one of each mirror pair |
| value | value curves past a knee (sq ft past 220 worth 0.3) |
| plumb | wet rooms on full-height stacks (within 0.6 ft); no stack through the middle of a dry room, the stair or lift |
| struct | bearing lines a floor, spans <= 20 ft; a crossed room needs a beam; lines over lines or a transfer |
| acoustic | `not_over`: a room on one floor not over rooms on the floor below |
| owner | the owner's rules (`rule()`), hard |
| def | definitions (a width's bits, a product's four rows, an annex parked): never dropped, never reported |

Exact area: one side is a binary expansion, w = w0 + st * sum 2^k b_k (st <= `step`, chosen so both ends are
reachable), and each b_k * d is four exact rows, so area = w0 d + st sum 2^k p_k exactly. A pinned side (or a
pinned room) makes the area linear with no bits.

## Modes

- `mode="exact"` (the default): exact areas, `step=` 0.25 ft (3 in).
- `mode="proxy"`: areas by secant cuts and a tangent-plane objective (approximate, several times faster).
- `mode="fast"`: proxy for the arrangement, then exact sizes with it fixed (two stages: 1% short).
- `mode="grid"`: walls on the floor's column lines (`columns=`): exact, fast, aligned, 5-7% of score. A line on a
  fixed room's face is its inside face: a room beside it may stand its whole wall off the line.

## `fix=` and `drop=`

`fix=` is emulated: a binary named in `fix={name: 0 or 1}` is built as that constant, and the rows it gates
fold or vanish, so a fully fixed arrangement is a pure LP (with real duals). `drop=` (families to leave out)
rebuilds without them.

The probe (the pure function the stdlib purity test loads) is `ftin`.
"""

load("@cadpole/plan.star", _plan_normalize = "normalize")

P_PART = 0.4  # ft: a partition between rooms
DOOR = 2.5  # ft of shared wall a door needs (a 2'-4" leaf and its jambs)
WINDOW = 3.0  # ft of exterior wall an escape window takes (R310)
HALL_MIN = 3.0  # ft: R311.6's 36 in
EXT_WALL = 0.5  # ft: an exterior wall (a court's)
_TOL_ROW = 1e-6  # ft or sq ft: a constant row closer than this to its bound holds
_SIDES = ["W", "E", "S", "N"]
# families whose binding rows are bookkeeping (an equality, a chosen side): left out of meta["binding"]
_QUIET = ["def", "pair", "door", "tree", "sym", "value", "annex", "flip"]

_OBSTACLE_KINDS = ["stair", "lift", "lobby", "landing", "mech", "closet", "storage", "garage", "deck", "court", "well"]
_LIVING = ["living", "dining", "family", "den", "study", "office", "play", "media", "guest", "bed"]
_CIRC = ["hall", "entry", "foyer", "landing", "lobby"]
_WET = ["kitchen", "bath", "powder", "wc", "laundry"]
_VALUE = {
    "bath": 0.9, "powder": 0.8, "wc": 0.8, "laundry": 0.6, "pantry": 0.6, "closet": 0.6, "storage": 0.6,
    "workshop": 0.6, "mud": 0.6, "mech": 0.3, "hall": 0.2, "entry": 0.3, "foyer": 0.3, "landing": 0.3,
    "lobby": 0.2, "stair": 0.0, "lift": 0.0, "garage": 0.1, "deck": 0.0, "court": 0.0, "well": 0.0,
    "other": 0.5, "kitchen": 1.0,
}

# ------------------------------------------------------------------------------------------------ pure helpers

def ftin(x):
    """A length in feet as feet-inches, to the nearest inch: 5.75 -> 5'-9"."""
    n = int(math.floor(abs(x) * 12.0 + 0.5))
    return "%s%d'-%d\"" % ("-" if x < 0 else "", n // 12, n % 12)

def _f(x, spec = ".2f"):
    return format(x, spec)

def _isnum(x):
    t = type(x)
    return t == "float" or t == "int"

def _sub(a, b):
    """a - b for numbers and expressions alike (starlark has no reflected subtraction)."""
    if _isnum(a):
        return -b + a
    return a - b

def _sum(xs):
    """A sum of numbers and expressions: a number when every item is one."""
    c = 0.0
    es = []
    for x in xs:
        if _isnum(x):
            c += x
        else:
            es.append(x)
    if not es:
        return c
    e = opt.sum(es)
    return e + c if c != 0.0 else e

def _dot(cs, xs):
    c = 0.0
    kk = []
    es = []
    for i in range(len(xs)):
        x = xs[i]
        k = cs[i]
        if _isnum(x):
            c += k * x
        elif k != 0.0:
            kk.append(k)
            es.append(x)
    if not es:
        return c
    e = opt.dot(kk, es)
    return e + c if c != 0.0 else e

def _one_minus(b):
    return -b + 1

def _w(r):
    return r[1] - r[0]

def _d(r):
    return r[3] - r[2]

def _area(r):
    return (r[1] - r[0]) * (r[3] - r[2])

def _overlaps(a, b, tol = 1e-9):
    return min(a[1], b[1]) - max(a[0], b[0]) > tol and min(a[3], b[3]) - max(a[2], b[2]) > tol

def _bbox(rs):
    return (min([r[0] for r in rs]), max([r[1] for r in rs]), min([r[2] for r in rs]), max([r[3] for r in rs]))

def _feature_name(s):
    out = []
    for ch in s.elems():
        out.append("_" if ch in (":", "#", "/", " ") else ch)
    return "".join(out)

def _rect_in_union(r, rects, tol = 1e-6):
    """Whether the rect lies inside the union of rects: split it on every face that crosses it, test each cell."""
    us = [r[0], r[1]]
    vs = [r[2], r[3]]
    for q in rects:
        for u in (q[0], q[1]):
            if r[0] < u and u < r[1]:
                us.append(u)
        for v in (q[2], q[3]):
            if r[2] < v and v < r[3]:
                vs.append(v)
    us = sorted(us)
    vs = sorted(vs)
    for i in range(len(us) - 1):
        for j in range(len(vs) - 1):
            cu = (us[i] + us[i + 1]) / 2.0
            cv = (vs[j] + vs[j + 1]) / 2.0
            if us[i + 1] - us[i] < tol or vs[j + 1] - vs[j] < tol:
                continue
            inside = False
            for q in rects:
                if q[0] - tol <= cu and cu <= q[1] + tol and q[2] - tol <= cv and cv <= q[3] + tol:
                    inside = True
                    break
            if not inside:
                return False
    return True

# ---------------------------------------------------------------------------------------------- constructors

def rect(u0, u1, v0, v1):
    """A rectangle on inside faces, (u0, u1, v0, v1) in feet."""
    if u1 < u0 or v1 < v0:
        fail("plan_solve.rect: (%s, %s, %s, %s) runs backwards" % (u0, u1, v0, v1))
    return (float(u0), float(u1), float(v0), float(v1))

def face(name, *, min, max):
    """A moving envelope face: one variable (feet) the floors' zones and touch lines may follow (`face_ref`)."""
    return {"name": name, "min": float(min), "max": float(max)}

def face_ref(name, offset = 0.0):
    """A coordinate that is a moving face plus an offset (the glass's outside face less its 0.5-ft wall)."""
    return {"face": name, "offset": float(offset)}

def wall(side, at, span, *, source = "side"):
    """An exterior wall a sleeping room may take its escape window on (R310): the line u = at (W or E) or v = at
    (S or N), over span = (lo, hi). `source` is what the window looks onto (a plan window's `source`)."""
    if side not in _SIDES:
        fail("plan_solve.wall: side is W, E, S or N, got %r" % side)
    return {"side": side, "at": at, "span": (float(span[0]), float(span[1])), "source": source}

def touch(side, at, span, length):
    """A room's `side` edge on the line at `at`, overlapping span = (lo, hi) by `length` feet."""
    if side not in _SIDES:
        fail("plan_solve.touch: side is W, E, S or N, got %r" % side)
    return {"side": side, "at": at, "span": (float(span[0]), float(span[1])), "length": float(length)}

def hold(r, among):
    """A floor's must-hold: the rect lies inside one of the rooms `among` (the lift's landing, the car)."""
    return {"rect": r, "among": list(among)}

def annex(*, side = None, w = (0.0, 16.0), d = (0.0, 6.0), min = 3.0):
    """An L-shaped room's second rectangle: flush on the main room's `side` wall (None: any), open to it, at
    least `min` feet both ways if it exists, else nothing (w = d = 0)."""
    return {"side": side, "w": (float(w[0]), float(w[1])), "d": (float(d[0]), float(d[1])), "min": float(min)}

def floor(key, *, zones, name = None, z = 0.0, ceiling = None, obstacles = None, fixed = None, roots = None,
          egress = None, part = P_PART, zone_walls = None, columns = None, must_hold = None, faces = None):
    """One floor: its zones (name -> rect; their union is the floor), obstacles (name -> rect, or a dict with
    rect=, kind=, core=: nothing goes there; drawn with their walls, so rooms may meet them), fixed rooms doors may
    open onto (name -> rect or dict; drawn to their inside faces, so a room keeps its partition off one unless it is
    `open_to` it), the door tree's roots (fixed room names at depth 0), escape walls (`wall()`), the partition,
    walls between zones ((zone_a, zone_b, t)), column lines (u, for mode "grid"), must-holds (`hold()`, or a list
    of alternatives) and moving faces ({(zone, side): face_ref(...)}, the zone's rect at the face's outermost
    position). A must-hold rect or a touch line on a fixed room's face needs its room open to it, or a partition off."""
    def _named(dd, what):
        out = {}
        for k, v in (dd or {}).items():
            guess = k if k in _OBSTACLE_KINDS else what
            if type(v) == "dict":
                out[k] = {"rect": v["rect"], "kind": v.get("kind", guess), "core": v.get("core"), "name": v.get("name", k)}
            else:
                out[k] = {"rect": v, "kind": guess, "core": None, "name": k}
        return out
    return {
        "key": key, "name": name or key, "z": float(z), "ceiling": ceiling, "zones": dict(zones),
        "obstacles": _named(obstacles, "other"), "fixed": _named(fixed, "lobby"), "roots": list(roots or []),
        "egress": list(egress or []), "part": float(part), "zone_walls": list(zone_walls or []),
        "columns": columns, "must_hold": list(must_hold or []), "faces": dict(faces or {}),
    }

def room(key, *, floor, kind = "other", name = None, area = None, w = None, d = None, aspect = None, orient = None,
         value = None, knee = None, sleeping = None, habitable = None, wet = None, circulation = None, dry = None,
         opens_to = None, open_to = None, zones = None, holds = None, touches = None, annex = None,
         door_gap = None, hall_min = None, tie = 1.0, fixed = None, kept = False, unit = "main", ref = None,
         core = None, furnish = None):
    """One room (see the module doc). `area=(lo, hi)` sq ft (hi None for none), `w=`/`d=` (lo, hi) ft,
    `aspect` the long side over the short at most, `orient=(a, b)` w >= a and d >= b or turned (R307),
    `value` per sq ft (by kind if None), `knee=(sq_ft, value2)` the value past a size, `opens_to` the rooms or
    fixed rooms its door may open onto, `open_to` rooms or fixed rooms with no partition between, `zones` the zones it may
    sit in (None: any), `holds` rects it must contain, `touches` lines it must run along, `annex` its L-piece,
    `door_gap` the wall its doors pass (a concrete wall), `hall_min` an exception to R311.6's 36 in, `fixed` a
    rect (a fixed room), `kept` an existing room exempt from design rules (it must be fixed)."""
    if kept and fixed == None:
        fail("plan_solve.room %r: a kept room is fixed: give fixed=rect(...)" % key)
    lv = kind in _LIVING
    ar = area or (0.0, None)
    return {
        "key": key, "name": name or key, "floor": floor, "kind": kind,
        "area": (float(ar[0]), (float(ar[1]) if ar[1] != None else None)),
        "w": (float((w or (3.0, 40.0))[0]), float((w or (3.0, 40.0))[1])),
        "d": (float((d or (3.0, 40.0))[0]), float((d or (3.0, 40.0))[1])),
        "aspect": aspect, "orient": orient,
        "value": float(value) if value != None else (1.0 if lv else _VALUE.get(kind, 0.5)),
        "knee": knee,
        "sleeping": sleeping if sleeping != None else (kind in ("bed", "guest")),
        "habitable": habitable if habitable != None else (lv or kind == "kitchen"),
        "wet": wet if wet != None else (kind in _WET),
        "circulation": circulation if circulation != None else (kind in _CIRC),
        "dry": dry if dry != None else (lv or kind in _CIRC),
        "opens_to": list(opens_to or []), "open_to": list(open_to or []),
        "zones": list(zones) if zones != None else None,
        "holds": list(holds or []), "touches": list(touches or []), "annex": annex,
        "door_gap": door_gap, "hall_min": hall_min, "tie": float(tie), "fixed": fixed, "kept": kept,
        "unit": unit, "ref": ref, "core": core, "furnish": furnish,
        "annex_of": None, "annex_side": None, "annex_min": 3.0, "shared": None, "gap": None,
    }

def shared(key, *, floors, w, d, kind = "stair", name = None, zones = None, gap = None):
    """One footprint on several floors: the same rectangle (its size fixed at w x d, its place free) on every
    floor listed -- one stair for the house, a chase. Rooms keep clear of it with their partition (or `gap`)."""
    return {"key": key, "floors": list(floors), "w": float(w), "d": float(d), "kind": kind, "name": name or key,
            "zones": zones, "gap": gap, "court": False, "area": None}

def court(key, *, base, floors, w = (6.0, 20.0), d = (6.0, 20.0), area = None, zones = None, top = None,
          reaches = None, kind = "court", rect = None):
    """A court or light well: one rectangle from its base floor up through `floors` (base first), open to the
    sky at `top` (an elevation, for the plan value). Rooms keep an exterior wall (0.5 ft) off it. With `rect` it is
    given (a slot or court of the site, outside the floors' zones): written to the plan as it is, so windows may
    name it as their source ("slot:<key>")."""
    return {"key": key, "floors": list(floors), "base": base, "w": (float(w[0]), float(w[1])),
            "d": (float(d[0]), float(d[1])), "kind": kind, "name": key, "zones": zones, "gap": EXT_WALL,
            "court": True, "area": area, "top": top, "reaches": reaches, "rect": rect}

def rule(floor, room, what, *, min = None, max = None, equals = None, family = "owner", why = None, rect = None,
         point = None, reach = None, side = None, at = None, span = None, length = None):
    """A hard rule on one room, family "owner" by default. `what`:
    - "x0", "x1", "y0", "y1" (its west, east, south, north face), "w", "d", "area": min=/max=/equals=;
    - "holds": the room contains `rect`;
    - "near": the room's rect grown by `reach` holds `point` (the mech by the ejector);
    - "clear": the room keeps out of `rect` (nothing on the deck glass);
    - "touch": the room's `side` edge on the line `at` over `span` by `length`.
    `why` cites where the rule comes from."""
    return {"floor": floor, "room": room, "what": what, "min": min, "max": max, "equals": equals,
            "family": family, "why": why, "rect": rect, "point": point, "reach": reach, "side": side, "at": at,
            "span": span, "length": length}

def stacks(*, count = 6, cost = 15.0, reach = 0.6, region = None, drain_below = None, avoid = None, big = 60.0):
    """Plumbing: up to `count` full-height stacks (u, v), each used one costing `cost` (sq ft of value); every
    wet room above the slab (`drain_below`: floors whose wet rooms drain under the slab) holds one within
    `reach` of its rect (in its wall); a used stack never runs through the middle of a dry room or an obstacle
    named in `avoid` (default the stair and the lift) on any floor."""
    return {"count": count, "cost": float(cost), "reach": float(reach), "region": region,
            "drain_below": list(drain_below or []), "avoid": list(avoid if avoid != None else ["stair", "lift"]),
            "big": float(big)}

def structure(*, span = 20.0, lines = 2, beam_cost = 1.5, clear_span_cost = 6.0, transfer_cost = 40.0,
              floors = None, extent = None, clear_span = None, min_gap = 4.0, margin = 3.0, big = 60.0):
    """Bearing lines: `lines` a floor (on `floors`, the ones that carry a floor above), joists east-west,
    consecutive spans <= `span`; each room is west of, east of or crossed by each line, and a crossed room needs
    a flush beam over its depth (`beam_cost` sq ft per ft; rooms or kinds in `clear_span` -- the garage, no
    posts in the car's space -- `clear_span_cost`); a line not over the same line below costs `transfer_cost`.
    `extent` {floor: (u0, u1)}: the outer walls the spans run from."""
    return {"span": float(span), "lines": lines, "beam_cost": float(beam_cost),
            "clear_span_cost": float(clear_span_cost), "transfer_cost": float(transfer_cost), "floors": floors,
            "extent": dict(extent or {}), "clear_span": list(clear_span if clear_span != None else ["garage"]),
            "min_gap": float(min_gap), "margin": float(margin), "big": float(big)}

def not_over(floor, room, below, rooms, *, big = 60.0):
    """Acoustics: `room` on `floor` is not over any of `rooms` on the floor `below` (plan non-overlap)."""
    return {"floor": floor, "room": room, "below": below, "rooms": list(rooms), "big": float(big)}

def problem(name, *, floors, rooms, stacks = None, structure = None, not_over = None, faces = None, shared = None,
            courts = None, rules = None, sym = None, hooks = None, site = None, source = None, cantilever = None,
            drop = None):
    """A floor-plan problem: floors (bottom up), rooms, and the house's cross-floor rules. `sym`: (floor, a, b)
    pairs of interchangeable rooms (a's centre north of or level with b's). `hooks`: functions hook(M) that add
    their own rows to a model as it is built (`row()`, `var()`, `binary()`, `objective_term()`), for rules
    this library does not have. `cantilever`: each floor inside the one below grown by this many feet (2.0), or
    {floor: feet} for the floors that stand on the one below (not a basement's).
    `drop`: rule families (or "family:floor", "family:floor:room") left out of every model of it."""
    keys = {}
    for F in floors:
        if F["key"] in keys:
            fail("plan_solve.problem %r: floor %r twice" % (name, F["key"]))
        keys[F["key"]] = True
    seen = {}
    for r in rooms:
        if r["floor"] not in keys:
            fail("plan_solve.problem %r: room %r is on floor %r, which is not among %s" % (name, r["key"], r["floor"], list(keys)))
        k = (r["floor"], r["key"])
        if k in seen:
            fail("plan_solve.problem %r: room %r twice on floor %r" % (name, r["key"], r["floor"]))
        seen[k] = True
    return {
        "name": name, "floors": list(floors), "rooms": list(rooms), "stacks": stacks, "structure": structure,
        "not_over": list(not_over or []), "faces": list(faces or []), "shared": list(shared or []),
        "courts": list(courts or []), "rules": list(rules or []), "sym": list(sym or []), "hooks": list(hooks or []),
        "site": site, "source": source, "cantilever": cantilever, "drop": list(drop or []),
    }

# ------------------------------------------------------------------------------------------- the model kit

def _dropped(M, fam, fl, rm):
    D = M["drop"]
    if not D:
        return False
    return fam in D or (fam + ":" + fl) in D or (fam + ":" + fl + ":" + rm) in D

def row(M, family, fl, rm, what, e, *, min = None, max = None, equals = None):
    """Add the rule min <= e <= max (or e == equals), named "<family>:<floor>:<room>:<what>". A rule whose
    family is dropped is left out; a constant rule is checked here: one that fails is listed in M["broken"]
    and, unless the model was built with broken="skip", added as an empty row that cannot hold."""
    if family != "def" and _dropped(M, family, fl, rm):
        return None
    lo = min
    hi = max
    if equals != None:
        lo = equals
        hi = equals
    name = "%s:%s:%s:%s" % (family, fl, rm, what)
    if name in M["names"]:
        k = M["names"][name] + 1
        M["names"][name] = k
        name = "%s (%d)" % (name, k)
    else:
        M["names"][name] = 1
    sh = M["shift"].get(name)
    if sh != None and not (lo != None and hi != None and lo == hi):
        # the rule relaxed by `sh` (its upper bound up, or its lower bound down): a re-solve's finite difference
        if hi != None:
            hi = hi + sh
        else:
            lo = lo - sh
        M["shifted"].append(name)
    if _isnum(e):
        bad = (lo != None and e < lo - _TOL_ROW) or (hi != None and e > hi + _TOL_ROW)
        if bad:
            M["broken"].append({"rule": name, "family": family, "floor": fl, "room": rm, "value": e, "min": lo, "max": hi})
            if M["broken_mode"] != "skip":
                M["p"].constrain(opt.sum([]), min = 1.0, name = name)
                M["count"]["rows"] += 1
        return None
    # a family built untagged (build(untag=)) is one group, last, to the builder's conflict(): held, as a background
    tg = {} if family in M.get("untag", {}) else None
    if lo != None and hi != None and lo == hi:
        M["p"].constrain(e, equals = lo, name = name, tags = tg)
    else:
        M["p"].constrain(e, min = lo, max = hi, name = name, tags = tg)
    M["count"]["rows"] += 1
    M["fams"][family] = M["fams"].get(family, 0) + 1
    u = family + ":" + fl
    if u not in M["units"]:
        M["units"][u] = {}
    M["units"][u][rm] = M["units"][u].get(rm, 0) + 1
    return name

def binary(M, name, *, arr = False):
    """A binary, or the constant M["fix"] gives it (0 or 1): the emulated `fix=`."""
    if name in M["fix"]:
        return 1 if M["fix"][name] > 0.5 else 0
    if name in M["bins"]:
        fail("plan_solve: binary %r declared twice" % name)
    b = M["p"].binary(name)
    M["bins"][name] = b
    M["count"]["binaries"] += 1
    if arr:
        M["arr"].append(name)
    return b

def var(M, name, min, max):
    """A continuous variable, or a number when min == max."""
    if min != None and max != None and max - min <= 1e-12:
        return min
    M["count"]["vars"] += 1
    return M["p"].var(name, min = min, max = max)

def objective_term(M, e):
    """Add a term to the objective (it is maximized)."""
    M["obj"].append(e)

def room_vars(M, fl, key):
    """A room's x, y, w, d (variables or numbers) and their bounds, as a dict."""
    return M["V"][fl][key]

def _val(sol, x):
    if _isnum(x):
        return x
    return sol.value(x)

# ---------------------------------------------------------------------------------------------- the floors

def _floor_rooms(spec, fl, shared_list):
    """The floor's rooms: main rooms in order, then their annex pieces, then shared footprints and courts."""
    mains = []
    pieces = []
    for r in spec["rooms"]:
        if r["floor"] != fl:
            continue
        mains.append(r)
        pcs = r.get("pieces")
        if not pcs and r.get("annex") != None:
            a = r["annex"]
            pcs = [{"key": r["key"] + "_a", "of": r["key"], "side": a["side"], "w": a["w"], "d": a["d"], "min": a["min"]}]
        for a in (pcs or []):
            q = dict(r)
            q.update({
                "key": a["key"], "name": r["name"] + " (its L)", "kind": "annex", "w": a["w"], "d": a["d"],
                "area": (0.0, None), "aspect": None, "orient": None, "knee": None, "sleeping": False,
                "habitable": False, "wet": False, "circulation": False, "opens_to": [], "open_to": [],
                "holds": [], "touches": [], "annex": None, "pieces": None, "hall_min": None, "fixed": None,
                "kept": False, "annex_of": a["of"], "annex_side": a["side"], "annex_min": a["min"], "ref": None,
            })
            pieces.append(q)
    extra = []
    for s in shared_list:
        if fl in s["floors"]:
            q = room(s["key"], floor = fl, kind = s["kind"], name = s["name"], value = 0.0, zones = s["zones"],
                     w = (s["w"], s["w"]) if not s["court"] else s["w"], d = (s["d"], s["d"]) if not s["court"] else s["d"],
                     habitable = False, sleeping = False, circulation = False, wet = False, dry = True)
            q["shared"] = s["key"]
            q["gap"] = s["gap"]
            if s["court"] and s["area"] != None:
                q["area"] = (float(s["area"][0]), (float(s["area"][1]) if s["area"][1] != None else None))
            extra.append(q)
    return mains + pieces + extra

def _allowed(F, r):
    return r["zones"] if r["zones"] != None else list(F["zones"].keys())

def _coord(M, c):
    """A number, or a face_ref resolved to its variable plus offset."""
    if _isnum(c):
        return c
    fv = M["faces"].get(c["face"])
    if fv == None:
        fail("plan_solve: no face named %r (have %s)" % (c["face"], list(M["faces"].keys())))
    return fv + c["offset"]

def _zone_face(M, F, zname, side):
    ref = F["faces"].get((zname, side))
    if ref == None:
        return None
    return _coord(M, ref)

def _fixed_v(f):
    w = f[1] - f[0]
    d = f[3] - f[2]
    return {"x": f[0], "y": f[2], "w": w, "d": d, "xlo": f[0], "xhi": f[0], "ylo": f[2], "yhi": f[2],
            "wlo": w, "whi": w, "dlo": d, "dhi": d, "fixed": True}

def _dropped_in(D, fam, fl, rm):
    return bool(D) and (fam in D or (fam + ":" + fl) in D or (fam + ":" + fl + ":" + rm) in D)

def _tight_bounds(r, bb, hm, has_annex, dropped):
    """A room's width and depth bounds (wlo, whi, dlo, dhi): its program's, the code's minimums and its zones', less
    the families `dropped(family)` leaves out."""
    r304 = r["habitable"] and not dropped("R304")
    r311 = r["circulation"] and not dropped("R311")
    sz = not dropped("size")
    wlo = max(r["w"][0] if sz else 0.0, 7.0 if r304 else 0.0, hm if r311 else 0.0)
    dlo = max(r["d"][0] if sz else 0.0, 7.0 if (r304 and not has_annex) else 0.0, hm if r311 else 0.0)
    whi = min(r["w"][1] if sz else 1e9, bb[1] - bb[0])
    dhi = min(r["d"][1] if sz else 1e9, bb[3] - bb[2])
    return (wlo, whi, dlo, dhi)

def _lattice(lo, hi, step):
    """The widths a room may take in the problem as posed: lo + k * st, k = 0 .. 2^nb - 1, with st as near `step` as
    reaches hi exactly. Returns (lo, st)."""
    span = max(hi - lo, 0.0)
    nb = _nbits(span, step) if span > 1e-9 else 0
    return (lo, span / ((1 << nb) - 1) if nb > 0 else step)

def _vars(M, fl):
    F = M["F"][fl]
    for r in M["rooms"][fl]:
        k = r["key"]
        pr = M["pin"].get(fl, {}).get(k)
        f = r["fixed"] if r["fixed"] != None else pr
        if f != None:
            M["V"][fl][k] = _fixed_v(f)
            continue
        if r["shared"] != None and r["shared"] in M["shared_v"]:
            M["V"][fl][k] = M["shared_v"][r["shared"]]
            continue
        zs = [F["zones"][z] for z in _allowed(F, r)]
        bb = _bbox(zs)
        hm = r["hall_min"] if r["hall_min"] != None else HALL_MIN
        has_annex = len(M["annexes"][fl].get(k, [])) > 0
        wlo, whi, dlo, dhi = _tight_bounds(r, bb, hm, has_annex, lambda fam: _dropped(M, fam, fl, k))
        # the width's lattice is the problem's own (its tight bounds, only its own drop= applied), so every model of
        # one problem -- tight or loose, with families left out -- puts the room's width on the same points: a model
        # with fewer rules is then a true relaxation of the problem, which explain's filters depend on
        gb = _tight_bounds(r, bb, hm, has_annex, lambda fam: _dropped_in(M["spec_drop"], fam, fl, k))
        g0, gst = _lattice(gb[0], max(gb[1], gb[0]), M["step"])
        if M["bounds"] == "loose":
            # every size limit a row of its own family, nothing in the bounds: so a solve's drop= can leave it out
            wlo = 0.0
            dlo = 0.0
            whi = bb[1] - bb[0]
            dhi = bb[3] - bb[2]
        if r["shared"] != None:
            wlo = r["w"][0]
            dlo = r["d"][0]
            whi = r["w"][1]
            dhi = r["d"][1]
            g0, gst = _lattice(wlo, max(whi, wlo), M["step"])
        if wlo > whi + 1e-9 or dlo > dhi + 1e-9:
            row(M, "size", fl, k, "fits its zones (w %s..%s, d %s..%s)" % (_f(wlo), _f(whi), _f(dlo), _f(dhi)), 0.0, min = 1.0)
            whi = max(whi, wlo)
            dhi = max(dhi, dlo)
        nm = (r["shared"] if r["shared"] != None else "%s:%s" % (fl, k))
        x = var(M, nm + ".x", bb[0], bb[1] - wlo)
        y = var(M, nm + ".y", bb[2], bb[3] - dlo)
        w = var(M, nm + ".w", wlo, whi)
        d = var(M, nm + ".d", dlo, dhi)
        v = {"x": x, "y": y, "w": w, "d": d, "xlo": bb[0], "xhi": bb[1] - wlo, "ylo": bb[2], "yhi": bb[3] - dlo,
             "wlo": wlo, "whi": whi, "dlo": dlo, "dhi": dhi, "fixed": False, "g0": g0, "gst": gst}
        M["V"][fl][k] = v
        if r["shared"] != None:
            M["shared_v"][r["shared"]] = v

def _ends(M, fl, k, R = None):
    if R != None:
        return (R[0], R[1], R[2], R[3])
    v = M["V"][fl][k]
    return (v["x"], v["x"] + v["w"], v["y"], v["y"] + v["d"])

def _range(M, fl, k, R = None):
    """(x lo, x hi, x+w lo, x+w hi, y lo, y hi, y+d lo, y+d hi); a fixed rect's is loose."""
    if R != None:
        return (R[0], R[1], R[0], R[1], R[2], R[3], R[2], R[3])
    v = M["V"][fl][k]
    return (v["xlo"], v["xhi"], v["xlo"] + v["wlo"], v["xhi"] + v["whi"], v["ylo"], v["yhi"], v["ylo"] + v["dlo"], v["yhi"] + v["dhi"])

def _zones(M, fl):
    F = M["F"][fl]
    Mu = M["Mu"][fl]
    Mv = M["Mv"][fl]
    for r in M["rooms"][fl]:
        if r["fixed"] != None:
            continue
        k = r["key"]
        v = M["V"][fl][k]
        zn = _allowed(F, r)
        if len(zn) == 1:
            z = F["zones"][zn[0]]
            fe = _zone_face(M, F, zn[0], "E")
            fn = _zone_face(M, F, zn[0], "N")
            ex = v["x"] + v["w"]
            ey = v["y"] + v["d"]
            row(M, "zone", fl, k, "in %s (E)" % zn[0], _sub(ex, fe) if fe != None else ex, max = 0.0 if fe != None else z[1])
            row(M, "zone", fl, k, "in %s (N)" % zn[0], _sub(ey, fn) if fn != None else ey, max = 0.0 if fn != None else z[3])
            if v["fixed"]:
                row(M, "zone", fl, k, "in %s (W)" % zn[0], v["x"], min = z[0])
                row(M, "zone", fl, k, "in %s (S)" % zn[0], v["y"], min = z[2])
            M["zone_of"][fl][k] = {zn[0]: None}
            continue
        bs = {}
        for zname in zn:
            z = F["zones"][zname]
            b = binary(M, "%s:%s@%s" % (fl, k, zname), arr = True)
            bs[zname] = b
            fe = _zone_face(M, F, zname, "E")
            fn = _zone_face(M, F, zname, "N")
            relax_u = Mu * _one_minus(b)
            relax_v = Mv * _one_minus(b)
            row(M, "zone", fl, k, "in %s (W)" % zname, v["x"] + relax_u, min = z[0])
            ex = _sub(v["x"] + v["w"], relax_u)
            row(M, "zone", fl, k, "in %s (E)" % zname, _sub(ex, fe) if fe != None else ex, max = 0.0 if fe != None else z[1])
            row(M, "zone", fl, k, "in %s (S)" % zname, v["y"] + relax_v, min = z[2])
            ey = _sub(v["y"] + v["d"], relax_v)
            row(M, "zone", fl, k, "in %s (N)" % zname, _sub(ey, fn) if fn != None else ey, max = 0.0 if fn != None else z[3])
        row(M, "zone", fl, k, "one zone", _sum(list(bs.values())), equals = 1.0)
        M["zone_of"][fl][k] = bs

# --------------------------------------------------------------------------------------------------- areas

def _prod(M, fl, name, b, d, dlo, dhi):
    """q = b * d exactly (b binary, d in [dlo, dhi]): the four rows; folds when b or d is a number."""
    if _isnum(b):
        return d if b == 1 else 0.0
    if _isnum(d):
        return d * b
    q = var(M, name, 0.0, dhi)
    rm = name[len(fl) + 1:] if name.startswith(fl + ":") else name.replace(":", "_")
    row(M, "def", fl, rm, "<= D b", q - dhi * b, max = 0.0)
    row(M, "def", fl, rm, ">= d0 b", q - dlo * b, min = 0.0)
    row(M, "def", fl, rm, "<= d", q - d, max = 0.0)
    row(M, "def", fl, rm, ">= d - D(1-b)", q - d - dhi * b, min = -dhi)
    return q

def _nbits(span, step):
    nb = 0
    for _ in range(40):
        if (1 << nb) - 1 >= span / step - 1e-9:
            break
        nb += 1
    return nb

def _areas(M, fl):
    F = M["F"][fl]
    for r in M["rooms"][fl]:
        k = r["key"]
        v = M["V"][fl][k]
        mode = M["mode"]
        if v["fixed"]:
            M["A"][fl][k] = v["w"] * v["d"]
            continue
        dlo = v["dlo"]
        dhi = v["dhi"]
        if _isnum(v["w"]) or _isnum(v["d"]):
            M["A"][fl][k] = v["w"] * v["d"]
            continue
        if r["shared"] != None and r["shared"] in M["shared_area"]:
            M["A"][fl][k] = M["shared_area"][r["shared"]]
            continue
        anc = M["widths"].get(fl, {}).get(k)
        if anc != None and mode == "exact" and v["whi"] - v["wlo"] > 1e-9:
            # w = w_d + s_up sum 2^j u_j - s_dn sum 2^j l_j: the drawn width w_d (all bits 0) and both ends reachable
            wd = min(max(anc[1] - anc[0], v["wlo"]), v["whi"])
            up = v["whi"] - wd
            dn = wd - v["wlo"]
            nu = _nbits(up, M["step"]) if up > 1e-9 else 0
            nl = _nbits(dn, M["step"]) if dn > 1e-9 else 0
            su = up / ((1 << nu) - 1) if nu > 0 else 0.0
            sl = dn / ((1 << nl) - 1) if nl > 0 else 0.0
            ub = [binary(M, "%s:%s.wu%d" % (fl, k, j)) for j in range(nu)]
            lb = [binary(M, "%s:%s.wl%d" % (fl, k, j)) for j in range(nl)]
            cu = [su * (1 << j) for j in range(nu)]
            cl = [sl * (1 << j) for j in range(nl)]
            row(M, "def", fl, k, "width (about the drawn)", _sub(v["w"], _dot(cu + [-c for c in cl], ub + lb)), equals = wd)
            pu = [_prod(M, fl, "%s:%s.pu%d" % (fl, k, j), ub[j], v["d"], dlo, dhi) for j in range(nu)]
            pl = [_prod(M, fl, "%s:%s.pl%d" % (fl, k, j), lb[j], v["d"], dlo, dhi) for j in range(nl)]
            M["A"][fl][k] = wd * v["d"] + _dot(cu + [-c for c in cl], pu + pl)
        elif mode == "exact" or (mode == "proxy" and r["shared"] != None):
            # the lattice's points within this model's bounds: in the problem as posed, exactly lo + k * st for
            # k < 2^nb (both ends reachable); with bounds let go (a family left out, bounds="loose"), the same points
            # and more beyond them
            st = v["gst"]
            k0 = int(math.ceil((v["wlo"] - v["g0"]) / st - 1e-9))
            k1 = int(math.floor((v["whi"] - v["g0"]) / st + 1e-9))
            if k1 < k0:
                row(M, "size", fl, k, "a width on its grid within w %s..%s" % (_f(v["wlo"]), _f(v["whi"])), 0.0, min = 1.0)
                k1 = k0
            w0 = v["g0"] + k0 * st
            nb = _nbits((k1 - k0) * st, st) if k1 > k0 else 0
            bits = [binary(M, "%s:%s.wb%d" % (fl, k, j)) for j in range(nb)]
            row(M, "def", fl, k, "width", _sub(v["w"], _dot([st * (1 << j) for j in range(nb)], bits)), equals = w0)
            ps = [_prod(M, fl, "%s:%s.p%d" % (fl, k, j), bits[j], v["d"], dlo, dhi) for j in range(nb)]
            M["A"][fl][k] = w0 * v["d"] + _dot([st * (1 << j) for j in range(nb)], ps)
        elif mode == "grid":
            zs_ = [F["zones"][z] for z in _allowed(F, r)]
            cols = {}
            for c in (F["columns"] or []):
                cols[int(math.floor(c * 10000.0 + 0.5))] = c
            for z in zs_:
                cols[int(math.floor(z[0] * 10000.0 + 0.5))] = z[0]
                cols[int(math.floor(z[1] * 10000.0 + 0.5))] = z[1]
            faces = {}
            for z in F["zones"].values():
                faces[int(math.floor(z[0] * 10000.0 + 0.5))] = True
                faces[int(math.floor(z[1] * 10000.0 + 0.5))] = True
            # a column line on a fixed room's face (a lobby's) is that room's inside face: the wall between them -- the
            # room's gap to it (_gap), not half a partition either side of the line -- lies all on this room's side, so
            # a span may also start or end that far off it
            starts = {}
            ends = {}
            for n, o in F["fixed"].items():
                q = o["rect"]
                gq = _gap(M, fl, k, "@" + n)
                c0 = int(math.floor(q[0] * 10000.0 + 0.5))
                c1 = int(math.floor(q[1] * 10000.0 + 0.5))
                if c1 in cols and c1 not in faces:
                    starts.setdefault(c1, []).append(q[1] + gq)
                if c0 in cols and c0 not in faces:
                    ends.setdefault(c0, []).append(q[0] - gq)
            ck = sorted(cols.keys())
            spans = []
            named = {}
            for ia in range(len(ck)):
                for ib in range(ia + 1, len(ck)):
                    for x0 in [cols[ck[ia]] + (0.0 if ck[ia] in faces else P_PART / 2.0)] + starts.get(ck[ia], []):
                        for x1 in [cols[ck[ib]] - (0.0 if ck[ib] in faces else P_PART / 2.0)] + ends.get(ck[ib], []):
                            ww = x1 - x0
                            sn = "%s+%s" % (_f(x0), _f(ww))
                            if sn in named:
                                continue
                            if ww >= v["wlo"] - 1e-9 and ww <= v["whi"] + 1e-9 and x0 >= v["xlo"] - 1e-9 and x1 <= v["xhi"] + v["whi"] + 1e-9:
                                named[sn] = True
                                spans.append((x0, ww))
            if not spans:
                row(M, "size", fl, k, "a column span fits", 0.0, min = 1.0)
                M["A"][fl][k] = v["w"] * dlo
                continue
            zs = [binary(M, "%s:%s.span%s+%s" % (fl, k, _f(a), _f(b))) for a, b in spans]
            row(M, "def", fl, k, "one span", _sum(zs), equals = 1.0)
            row(M, "def", fl, k, "on columns (x)", _sub(v["x"], _dot([a for a, b in spans], zs)), equals = 0.0)
            row(M, "def", fl, k, "on columns (w)", _sub(v["w"], _dot([b for a, b in spans], zs)), equals = 0.0)
            qs = [_prod(M, fl, "%s:%s.qs%d" % (fl, k, j), zs[j], v["d"], dlo, dhi) for j in range(len(zs))]
            M["A"][fl][k] = _dot([b for a, b in spans], qs)
        elif mode == "proxy":
            ref = r["ref"] or ((v["wlo"] + v["whi"]) / 2.0, (dlo + dhi) / 2.0)
            w0 = ref[0]
            d0 = ref[1]
            M["A"][fl][k] = None
            M["Aproxy"][fl][k] = d0 * v["w"] + w0 * v["d"] - w0 * d0
            if r["annex_of"] != None:
                M["Aproxy"][fl][k] = dlo * v["w"] + v["wlo"] * v["d"] - v["wlo"] * dlo
                continue
            if len(M["annexes"][fl].get(k, [])) > 0:
                continue
            amin = max(r["area"][0], 70.0 if r["habitable"] else 0.0)
            if amin > 0 and not _dropped(M, "area", fl, k):
                _min_area_cuts(M, fl, r, amin)
            if r["area"][1] != None and not _dropped(M, "area", fl, k):
                wl = v["wlo"]
                wu = v["whi"]
                row(M, "area", fl, k, "area <= %s sq ft (program, conservative)" % _f(r["area"][1], ".0f"),
                    0.5 * (wu + wl) * v["d"] + 0.5 * (dlo + dhi) * v["w"], max = r["area"][1] + 0.5 * (wu * dlo + wl * dhi))
        else:
            fail("plan_solve: mode is exact, proxy, fast or grid, got %r" % mode)
        if r["shared"] != None:
            M["shared_area"][r["shared"]] = M["A"][fl][k]

def _min_area_cuts(M, fl, r, A, n = 8):
    """w d >= A by chords of d = A / w between breakpoints over w's range (conservative: above the curve)."""
    v = M["V"][fl][r["key"]]
    a = v["wlo"]
    b = v["whi"]
    pts = [a + (b - a) * k / n for k in range(n + 1)]
    for k in range(n):
        w1 = pts[k]
        w2 = pts[k + 1]
        if w2 - w1 < 1e-9:
            continue
        d1 = A / w1
        d2 = A / w2
        s = (d2 - d1) / (w2 - w1)
        row(M, "area", fl, r["key"], "area >= %s chord%d" % (_f(A, ".0f"), k), v["d"] - s * v["w"], min = d1 - s * w1)

def _annexes(M, fl, k):
    return M["annexes"][fl].get(k, [])

def _pieces(M, fl, k):
    out = [k]
    for a in _annexes(M, fl, k):
        out += _pieces(M, fl, a)
    return out

def _main_of(M, fl, k):
    by = M["by"][fl]
    for _ in range(10):
        r = by.get(k)
        if r == None or r["annex_of"] == None:
            return k
        k = r["annex_of"]
    return k

def _area_of(M, fl, k):
    A = M["A"][fl].get(k)
    return A if A != None else M["Aproxy"][fl].get(k)

def _total_area(M, fl, k):
    A = _area_of(M, fl, k)
    parts = [A]
    for a in _annexes(M, fl, k):
        parts.append(_total_area(M, fl, a))
    return _sum(parts)

# ----------------------------------------------------------------------------------------- per-room rules

def _rules(M, fl):
    for r in M["rooms"][fl]:
        if r["shared"] != None:
            first = [f for f in M["floors"] if r["key"] in M["by"][f]][0]
            if fl == first and (r["area"][0] > 0 or r["area"][1] != None):
                A = M["A"][fl][r["key"]]
                if r["area"][0] > 0:
                    row(M, "area", fl, r["key"], "area >= %s sq ft (program)" % _f(r["area"][0], ".0f"), A, min = r["area"][0])
                if r["area"][1] != None:
                    row(M, "area", fl, r["key"], "area <= %s sq ft (program)" % _f(r["area"][1], ".0f"), A, max = r["area"][1])
            continue
        if r["fixed"] != None or r["kept"]:
            continue
        k = r["key"]
        v = M["V"][fl][k]
        if r["annex_of"] != None:
            _annex(M, fl, r)
            continue
        A = M["A"][fl][k]
        if A != None and _annexes(M, fl, k):
            A = _total_area(M, fl, k)
        if A != None:
            r304 = r["habitable"] and not _dropped(M, "R304", fl, k)
            prog = r["area"][0] if not _dropped(M, "area", fl, k) else 0.0
            amin = max(prog, 70.0 if r304 else 0.0)
            if amin > 0:
                if r304 and amin <= 70.0:
                    row(M, "R304", fl, k, "area >= 70 sq ft (R304.1)", A, min = amin)
                else:
                    row(M, "area", fl, k, "area >= %s sq ft (program)" % _f(amin, ".0f"), A, min = amin)
            if r["area"][1] != None:
                row(M, "area", fl, k, "area <= %s sq ft (program)" % _f(r["area"][1], ".0f"), A, max = r["area"][1])
        # named dimension rows (the bounds already hold them; the rows make them reportable as binding rules)
        if M["bounds"] == "loose":
            for ax, nm in (("w", "width"), ("d", "depth")):
                lim = r["w"] if ax == "w" else r["d"]
                if lim[0] > 0.0:
                    row(M, "size", fl, k, "%s >= %s (program)" % (nm, ftin(lim[0])), v[ax], min = lim[0])
                row(M, "size", fl, k, "%s <= %s (program)" % (nm, ftin(lim[1])), v[ax], max = lim[1])
                if r["circulation"]:
                    hm = r["hall_min"] if r["hall_min"] != None else HALL_MIN
                    row(M, "R311", fl, k, "%s >= %s (%s)" % (nm, ftin(hm), "R311.6" if hm >= HALL_MIN else "R311.6 EXCEPTION as drawn"), v[ax], min = hm)
                if r["habitable"] and not (ax == "d" and _annexes(M, fl, k)):
                    row(M, "R304", fl, k, "%s >= 7'-0\" (R304.2)" % nm, v[ax], min = 7.0)
        for ax, nm in ([] if M["bounds"] == "loose" else [("w", "width"), ("d", "depth")]):
            cands = []
            if not _dropped(M, "size", fl, k):
                cands.append(((r["w"] if ax == "w" else r["d"])[0], "program", "size"))
            if r["circulation"]:
                hm = r["hall_min"] if r["hall_min"] != None else HALL_MIN
                cands.append((hm, "R311.6" if hm >= HALL_MIN else "R311.6 EXCEPTION as drawn", "R311"))
            if r["habitable"] and not (ax == "d" and _annexes(M, fl, k)):
                cands.append((7.0, "R304.2", "R304"))
            best = None
            for c in cands:
                if _dropped(M, c[2], fl, k):
                    continue
                if best == None or c[0] > best[0] + 1e-9 or (c[0] > best[0] - 1e-9 and best[2] == "size"):
                    best = c
            if best != None and best[0] > 0.0:
                row(M, best[2], fl, k, "%s >= %s (%s)" % (nm, ftin(best[0]), best[1]), v[ax], min = best[0])
        if r["aspect"]:
            row(M, "shape", fl, k, "aspect (w)", _sub(v["w"], r["aspect"] * v["d"]), max = 0.0)
            row(M, "shape", fl, k, "aspect (d)", _sub(v["d"], r["aspect"] * v["w"]), max = 0.0)
        if r["orient"]:
            a = r["orient"][0]
            b = r["orient"][1]
            o = binary(M, "%s:%s.turned" % (fl, k))
            row(M, "R307", fl, k, "fixtures (w)", _sub(v["w"], (b - a) * o), min = a)
            row(M, "R307", fl, k, "fixtures (d)", v["d"] + (b - a) * o, min = b)
        for c in r["holds"]:
            tag = "(%s,%s)" % (_f(c[0], ".1f"), _f(c[2], ".1f"))
            row(M, "contain", fl, k, "holds %s W" % tag, v["x"], max = c[0])
            row(M, "contain", fl, k, "holds %s E" % tag, v["x"] + v["w"], min = c[1])
            row(M, "contain", fl, k, "holds %s S" % tag, v["y"], max = c[2])
            row(M, "contain", fl, k, "holds %s N" % tag, v["y"] + v["d"], min = c[3])
        for t in r["touches"]:
            _on_line(M, fl, k, t["side"], t["at"], t["span"][0], t["span"][1], t["length"], None, "touch")

def _annex(M, fl, r):
    """An annex is flush against one wall of its main room, open, its whole edge on that wall. It either exists
    (both sides >= annex_min) or is nothing (w = d = 0, parked at the start of its wall)."""
    a = r["key"]
    m = r["annex_of"]
    xa0, xa1, ya0, ya1 = _ends(M, fl, a)
    xm0, xm1, ym0, ym1 = _ends(M, fl, m)
    Mb = max(M["Mu"][fl], M["Mv"][fl]) + 2.0
    va = M["V"][fl][a]
    pinned = va["fixed"]
    if not pinned:
        ex = binary(M, "%s:%s exists" % (fl, a), arr = True)
        row(M, "annex", fl, a, "nothing unless it exists (w)", _sub(va["w"], va["whi"] * ex), max = 0.0)
        row(M, "annex", fl, a, "nothing unless it exists (d)", _sub(va["d"], va["dhi"] * ex), max = 0.0)
        row(M, "annex", fl, a, ">= %s ft if it exists (w)" % _f(r["annex_min"], "g"), _sub(va["w"], r["annex_min"] * ex), min = 0.0)
        row(M, "annex", fl, a, ">= %s ft if it exists (d)" % _f(r["annex_min"], "g"), _sub(va["d"], r["annex_min"] * ex), min = 0.0)
        vm = M["V"][fl][m]
        along = "x" if (r["annex_side"] or "S") in ("S", "N") else "y"
        row(M, "def", fl, a, "parked (<=)", _sub(_sub(va[along], vm[along]), Mb * ex), max = 0.0)
        row(M, "def", fl, a, "parked (>=)", _sub(va[along], vm[along]) + Mb * ex, min = 0.0)
    bs = []
    sides = [r["annex_side"]] if (r["annex_side"] and not pinned) else _SIDES
    for side in sides:
        b = binary(M, "%s:%s on %s:%s" % (fl, a, m, side), arr = True)
        bs.append(b)
        if side == "W":
            eq = _sub(xm0, xa1)
            lo_hi = [(ym0, ya0), (ya1, ym1)]
        elif side == "E":
            eq = _sub(xa0, xm1)
            lo_hi = [(ym0, ya0), (ya1, ym1)]
        elif side == "S":
            eq = _sub(ym0, ya1)
            lo_hi = [(xm0, xa0), (xa1, xm1)]
        else:
            eq = _sub(ya0, ym1)
            lo_hi = [(xm0, xa0), (xa1, xm1)]
        row(M, "annex", fl, a, "flush %s of %s (<=)" % (side, m), eq + Mb * b, max = Mb)
        row(M, "annex", fl, a, "flush %s of %s (>=)" % (side, m), _sub(eq, Mb * b), min = -Mb)
        for k2 in range(2):
            lo_ = lo_hi[k2][0]
            hi_ = lo_hi[k2][1]
            row(M, "annex", fl, a, "within %s's %s wall %d" % (m, side, k2), _sub(lo_, hi_) + Mb * b, max = Mb)
    row(M, "annex", fl, a, "on one wall of %s" % m, _sum(bs), equals = 1.0)

def _on_line(M, fl, k, side, coord, lo, hi, L, b, fam):
    """Room k has its `side` edge on the line at `coord` and overlaps [lo, hi] by L (relaxed when binary b is 0)."""
    v = M["V"][fl][k]
    Mb = max(M["Mu"][fl], M["Mv"][fl])
    relax = 0.0 if b == None else Mb * _one_minus(b)
    c = _coord(M, coord)
    ce = 0.0 if _isnum(c) else c
    cc = c if _isnum(c) else 0.0
    nm = "%s-edge on %s %s [%s..%s]" % (side, "u" if side in ("W", "E") else "v", _f(cc) if _isnum(c) else "(moving)", _f(lo, ".1f"), _f(hi, ".1f"))
    if side == "N":
        row(M, fam, fl, k, nm, _sub(v["y"] + v["d"] + relax, ce), min = cc)
        a = v["x"]
        wv = v["w"]
        wlo = v["wlo"]
    elif side == "S":
        row(M, fam, fl, k, nm, _sub(_sub(v["y"], relax), ce), max = cc)
        a = v["x"]
        wv = v["w"]
        wlo = v["wlo"]
    elif side == "W":
        row(M, fam, fl, k, nm, _sub(_sub(v["x"], relax), ce), max = cc)
        a = v["y"]
        wv = v["d"]
        wlo = v["dlo"]
    else:
        row(M, fam, fl, k, nm, _sub(v["x"] + v["w"] + relax, ce), min = cc)
        a = v["y"]
        wv = v["d"]
        wlo = v["dlo"]
    row(M, fam, fl, k, nm + " overlap-", _sub(a, relax), max = hi - L)
    row(M, fam, fl, k, nm + " overlap+", a + wv + relax, min = lo + L)
    if wlo < L:
        row(M, fam, fl, k, nm + " own length", wv + relax, min = L)

# ---------------------------------------------------------------------------------------------- pairs

def _gap(M, fl, a, b):
    """The wall between room a (or an L-piece of one) and b: a room, "@name" (one of the floor's fixed rooms) or
    "#name" (an obstacle). An obstacle is drawn with its walls (a stair block, a lift shaft), so nothing comes
    between: 0. A fixed room is drawn to its inside faces, as a room is (a lobby), so it gets a room's gap: 0 where a
    -- or the room a is an L-piece of -- is open to it, a's own gap where it has one, the floor's partition otherwise
    (0 on an open-plan floor)."""
    by = M["by"][fl]
    F = M["F"][fl]
    ra = by.get(a)
    if b.startswith("#"):
        return 0.0
    if b.startswith("@"):
        if ra != None:
            main = by.get(_main_of(M, fl, a)) or ra
            if b[1:] in ra["open_to"] or b[1:] in main["open_to"]:
                return 0.0
            if ra["gap"] != None:
                return ra["gap"]
        return F["part"]
    rb = by.get(b)
    if ra != None and rb != None:
        if ra["annex_of"] == b or rb["annex_of"] == a or (ra["annex_of"] != None and ra["annex_of"] == rb["annex_of"]):
            return 0.0
    if ra != None and b in ra["open_to"]:
        return 0.0
    if rb != None and a in rb["open_to"]:
        return 0.0
    if ra != None and ra["gap"] != None:
        return ra["gap"]
    if rb != None and rb["gap"] != None:
        return rb["gap"]
    if ra != None and rb != None and ra["zones"] and rb["zones"] and len(ra["zones"]) == 1 and len(rb["zones"]) == 1:
        for za, zb, t in F["zone_walls"]:
            if (za == ra["zones"][0] and zb == rb["zones"][0]) or (zb == ra["zones"][0] and za == rb["zones"][0]):
                return t
    return F["part"]

def _zone_rects(M, fl, k):
    r = M["by"][fl][k]
    if r["fixed"] != None:
        return [r["fixed"]]
    F = M["F"][fl]
    return [F["zones"][z] for z in _allowed(F, r)]

def _nonoverlap(M, fl):
    F = M["F"][fl]
    rooms = M["rooms"][fl]
    keys = [r["key"] for r in rooms]
    by = M["by"][fl]
    others = [("#" + n, o["rect"]) for n, o in F["obstacles"].items()] + [("@" + n, o["rect"]) for n, o in F["fixed"].items()]
    for i in range(len(keys)):
        a = keys[i]
        ra = by[a]
        items = [(keys[j], None) for j in range(i + 1, len(keys))] + others
        for b, R in items:
            if R == None and ra["fixed"] != None and by[b]["fixed"] != None:
                continue
            if R != None and ra["fixed"] != None:
                continue
            # an obstacle's gap is 0, a fixed room's a room's (_gap): a room keeps its partition off the lobby
            g = _gap(M, fl, a, b)
            za = _zone_rects(M, fl, a)
            zb = [R] if R != None else _zone_rects(M, fl, b)
            # can they meet? zones that overlap; for a fixed room, a zone that comes within the room's wall of it (a
            # zone drawn to the lobby's face still keeps the partition off it)
            e = g if R != None else 0.0
            meet = False
            for x in za:
                for y in zb:
                    if _overlaps((x[0] - e, x[1] + e, x[2] - e, x[3] + e), y):
                        meet = True
            if not meet:
                continue
            _pair(M, fl, a, b, R, g)

def _pair(M, fl, a, b, R, g):
    xa0, xa1, ya0, ya1 = _ends(M, fl, a)
    xb0, xb1, yb0, yb1 = _ends(M, fl, b, R)
    A_ = _range(M, fl, a)
    B_ = _range(M, fl, b, R)
    poss = {"W": A_[2] + g <= B_[1] + 1e-9, "E": B_[2] + g <= A_[1] + 1e-9,
            "S": A_[6] + g <= B_[5] + 1e-9, "N": B_[6] + g <= A_[5] + 1e-9}
    Ms = {"W": max(A_[3] + g - B_[0], 0.0), "E": max(B_[3] + g - A_[0], 0.0),
          "S": max(A_[7] + g - B_[4], 0.0), "N": max(B_[7] + g - A_[4], 0.0)}
    exprs = {"W": _sub(xa1, xb0), "E": _sub(xb1, xa0), "S": _sub(ya1, yb0), "N": _sub(yb1, ya0)}
    sides = [s for s in _SIDES if poss[s]]
    pairname = "%s|%s" % (a, b)
    va = M["V"][fl][a]
    bothfixed = va["fixed"] and (R != None or M["V"][fl][b]["fixed"])
    if bothfixed:
        # both rectangles are given: which sides hold is a fact, not a choice
        held = [s for s in _SIDES if exprs[s] <= -g + _TOL_ROW]
        if not held:
            row(M, "pair", fl, pairname, "apart (overlap or closer than %s)" % ftin(g), 0.0, min = 1.0)
        M["rel"][fl][(a, b)] = {}
        return
    if not sides:
        row(M, "pair", fl, pairname, "cannot be kept apart", 0.0, min = 1.0)
        return
    bs = {}
    for s in sides:
        bn = binary(M, "%s:%s|%s:%s" % (fl, a, b, s), arr = R == None)
        bs[s] = bn
        row(M, "pair", fl, pairname, "%s of" % s, exprs[s] + Ms[s] * bn, max = Ms[s] - g)
    row(M, "pair", fl, pairname, "apart", _sum(list(bs.values())), min = 1.0)
    M["rel"][fl][(a, b)] = bs

# ---------------------------------------------------------------------------------------------- doors

def _doors(M, fl):
    """A door tree to the roots: each room (any piece of it) opens onto one parent (any piece of it), and parents
    are shallower, so every room reaches a root."""
    F = M["F"][fl]
    by = M["by"][fl]
    mains = [r for r in M["rooms"][fl] if r["annex_of"] == None and r["shared"] == None]
    n = len(mains) + 1
    depth = {}
    for r in mains:
        depth[r["key"]] = var(M, "%s:%s.depth" % (fl, r["key"]), 1.0, float(n))
    for root in F["roots"]:
        depth["@" + root] = 0.0
    for r in mains:
        if not r["opens_to"]:
            continue
        k = r["key"]
        as_ = []
        for c in r["opens_to"]:
            if c in by:
                targets = _pieces(M, fl, c)
            elif c in F["fixed"]:
                targets = ["@" + c]
            else:
                continue
            key_c = c if c in by else "@" + c
            via = []
            for ap in _pieces(M, fl, k):
                for t in targets:
                    a = binary(M, "%s:%s>%s" % (fl, ap, t), arr = True)
                    sides = _door_geom(M, fl, ap, t, a)
                    if not sides:
                        row(M, "door", fl, k, "no door %s>%s" % (ap, t), a, max = 0.0)
                    M["doors"][fl][(ap, t)] = {"a": a, "sides": sides}
                    via.append(a)
            if not via:
                continue
            apar = binary(M, "%s:%s=>%s" % (fl, k, key_c), arr = True)
            row(M, "door", fl, k, "=>%s via one door" % key_c, _sub(_sum(via), apar), equals = 0.0)
            as_.append(apar)
            M["parents"][fl][(k, key_c)] = apar
            if key_c in depth:
                row(M, "tree", fl, k, "deeper than %s" % key_c, _sub(_sub(depth[k], depth[key_c]), n * apar), min = 1 - n)
        row(M, "door", fl, k, "has one door", _sum(as_), equals = 1.0)
    M["depth"][fl] = depth

def _door_geom(M, fl, a, c, abin):
    """abin = 1 => a and c share a wall (gap in [gmin, gmax]) over >= DOOR."""
    F = M["F"][fl]
    by = M["by"][fl]
    R = F["fixed"][c[1:]]["rect"] if c.startswith("@") else None
    xa0, xa1, ya0, ya1 = _ends(M, fl, a)
    xc0, xc1, yc0, yc1 = _ends(M, fl, c, R)
    A_ = _range(M, fl, a)
    C_ = _range(M, fl, c, R)
    # the door passes the wall the pair rows keep between them, a fixed room's as a room's (_gap): a door onto the
    # lobby is in the partition between them, across it
    g = _gap(M, fl, a, c)
    gmin = g
    gmax = g
    dg = [x["door_gap"] for x in (by.get(a), by.get(c)) if x != None and x["door_gap"] != None]
    if dg and g > 0.0:
        gmin = max(dg)
        gmax = gmin
    L = DOOR
    Mu = M["Mu"][fl] + 1.0
    Mv = M["Mv"][fl] + 1.0
    sides = {}
    poss = {"W": C_[2] <= A_[1] + 1e-9 and A_[0] <= C_[3] + gmax + 1e-9,
            "E": A_[2] <= C_[1] + 1e-9 and C_[0] <= A_[3] + gmax + 1e-9,
            "S": C_[6] <= A_[5] + 1e-9 and A_[4] <= C_[7] + gmax + 1e-9,
            "N": A_[6] <= C_[5] + 1e-9 and C_[4] <= A_[7] + gmax + 1e-9}
    va = M["V"][fl][a]
    for side in _SIDES:
        if not poss[side]:
            continue
        if R != None and ((R[3] - R[2]) if side in ("W", "E") else (R[1] - R[0])) < L - 1e-9:
            continue
        if R == None and by[c]["fixed"] == None and M["V"][fl][c]["dhi" if side in ("W", "E") else "whi"] < L - 1e-9:
            continue
        s = binary(M, "%s:%s>%s:%s" % (fl, a, c, side), arr = True)
        sides[side] = s
        nm = "%s>%s on %s" % (a, c, side)
        if side == "W":
            gapx = _sub(xa0, xc1)
            ov = [(ya0 + L, yc1), (yc0 + L, ya1)]
            Mg = Mu
            Mo = Mv
        elif side == "E":
            gapx = _sub(xc0, xa1)
            ov = [(ya0 + L, yc1), (yc0 + L, ya1)]
            Mg = Mu
            Mo = Mv
        elif side == "S":
            gapx = _sub(ya0, yc1)
            ov = [(xa0 + L, xc1), (xc0 + L, xa1)]
            Mg = Mv
            Mo = Mu
        else:
            gapx = _sub(yc0, ya1)
            ov = [(xa0 + L, xc1), (xc0 + L, xa1)]
            Mg = Mv
            Mo = Mu
        rm = _main_of(M, fl, a)
        row(M, "door", fl, rm, nm + " gap<=", gapx + Mg * s, max = gmax + Mg)
        row(M, "door", fl, rm, nm + " gap>=", _sub(gapx, Mg * s), min = gmin - Mg)
        for kk in range(2):
            lhs = ov[kk][0]
            rhs = ov[kk][1]
            row(M, "door", fl, rm, nm + " overlap%d" % kk, _sub(lhs, rhs) + Mo * s, max = Mo)
        own_lo = va["dlo"] if side in ("W", "E") else va["wlo"]
        if own_lo < L:
            row(M, "door", fl, rm, nm + " own length", _sub(va["d"] if side in ("W", "E") else va["w"], L * s), min = 0.0)
        if R == None:
            vc = M["V"][fl][c]
            oc_lo = vc["dlo"] if side in ("W", "E") else vc["wlo"]
            if oc_lo < L:
                row(M, "door", fl, rm, nm + " other's length", _sub(vc["d"] if side in ("W", "E") else vc["w"], L * s), min = 0.0)
    if sides:
        row(M, "door", fl, _main_of(M, fl, a), "%s>%s one side" % (a, c), _sub(_sum(list(sides.values())), abin), equals = 0.0)
    return sides

# -------------------------------------------------------------------------------------- egress, holds, sym

def _egress(M, fl):
    F = M["F"][fl]
    for r in M["rooms"][fl]:
        if not r["sleeping"] or r["fixed"] != None or r["kept"]:
            continue
        if _dropped(M, "R310", fl, r["key"]):
            continue
        bs = []
        for k in range(len(F["egress"])):
            wl = F["egress"][k]
            b = binary(M, "%s:%s.egress%d" % (fl, r["key"], k), arr = True)
            bs.append((wl, b))
            _on_line(M, fl, r["key"], wl["side"], wl["at"], wl["span"][0], wl["span"][1], WINDOW, b, "R310")
        row(M, "R310", fl, r["key"], "escape window (R310)", _sum([b for _, b in bs]), min = 1.0)
        M["egress"][fl][r["key"]] = bs

def _must_hold(M, fl):
    F = M["F"][fl]
    Mb = max(M["Mu"][fl], M["Mv"][fl]) + 2.0
    for n in range(len(F["must_hold"])):
        entry = F["must_hold"][n]
        alts = entry if type(entry) == "list" else [entry]
        bs = []
        for alt in alts:
            c = alt["rect"]
            for k in alt["among"]:
                if k not in M["V"][fl]:
                    continue
                tag = "(%s,%s)" % (_f(c[0], ".1f"), _f(c[2], ".1f"))
                b = binary(M, "%s:hold%d@%s%s" % (fl, n, k, tag), arr = True)
                bs.append(b)
                v = M["V"][fl][k]
                rl = Mb * _one_minus(b)
                row(M, "contain", fl, k, "holds %s W" % tag, _sub(v["x"], rl), max = c[0])
                row(M, "contain", fl, k, "holds %s E" % tag, v["x"] + v["w"] + rl, min = c[1])
                row(M, "contain", fl, k, "holds %s S" % tag, _sub(v["y"], rl), max = c[2])
                row(M, "contain", fl, k, "holds %s N" % tag, v["y"] + v["d"] + rl, min = c[3])
        what = ", ".join(["%s,%s" % (_f(alt["rect"][0], ".1f"), _f(alt["rect"][2], ".1f")) for alt in alts])
        row(M, "contain", fl, "-", "must-hold %d (%s) inside a room" % (n, what), _sum(bs), min = 1.0)

def _sym(M, fl, a, b):
    va = M["V"][fl][a]
    vb = M["V"][fl][b]
    e = _sub(_sub(2 * va["y"] + va["d"], 2 * vb["y"]), vb["d"])
    row(M, "sym", fl, "%s/%s" % (a, b), "a's centre north of b's", e, min = 0.0)

# ------------------------------------------------------------------------------------------ cross-floor rules

def _plumbing(M):
    S = M["spec"]["stacks"]
    Mb = S["big"]
    c = S["reach"]
    reg = S["region"]
    if reg == None:
        reg = _bbox([z for fl in M["floors"] for z in M["F"][fl]["zones"].values()])
    st = []
    for s in range(S["count"]):
        us = var(M, "stack%d.u" % s, reg[0], reg[1])
        vs = var(M, "stack%d.v" % s, reg[2], reg[3])
        ys = binary(M, "stack%d used" % s)
        st.append((us, vs, ys))
        objective_term(M, -S["cost"] * ys)
    for s in range(S["count"] - 1):
        row(M, "sym", "-", "stack%d" % s, "stacks in order", _sub(st[s][2], st[s + 1][2]), min = 0.0)
    M["X"]["stacks"] = st
    for fl in M["floors"]:
        if fl in S["drain_below"]:
            continue
        for r in M["rooms"][fl]:
            if not r["wet"] or r["annex_of"] != None:
                continue
            v = M["V"][fl][r["key"]]
            gs = []
            for s in range(len(st)):
                us, vs, ys = st[s]
                g = binary(M, "%s:%s on stack%d" % (fl, r["key"], s))
                gs.append(g)
                k = r["key"]
                row(M, "plumb", fl, k, "on stack%d only if used" % s, _sub(g, ys), max = 0.0)
                row(M, "plumb", fl, k, "holds stack%d (W)" % s, _sub(us, v["x"]) + Mb * _one_minus(g), min = -c)
                row(M, "plumb", fl, k, "holds stack%d (E)" % s, _sub(_sub(_sub(us, v["x"]), v["w"]), Mb * _one_minus(g)), max = c)
                row(M, "plumb", fl, k, "holds stack%d (S)" % s, _sub(vs, v["y"]) + Mb * _one_minus(g), min = -c)
                row(M, "plumb", fl, k, "holds stack%d (N)" % s, _sub(_sub(_sub(vs, v["y"]), v["d"]), Mb * _one_minus(g)), max = c)
            row(M, "plumb", fl, r["key"], "drains to a stack", _sum(gs), equals = 1.0)
    for fl in M["floors"]:
        F = M["F"][fl]
        dry = []
        for r in M["rooms"][fl]:
            main = M["by"][fl][_main_of(M, fl, r["key"])]
            if r["shared"] != None or r["kept"]:
                continue
            if r["dry"] or (r["annex_of"] != None and main["dry"]):
                v = M["V"][fl][r["key"]]
                dry.append((r["key"], (v["x"], v["x"] + v["w"], v["y"], v["y"] + v["d"])))
        for n, o in F["obstacles"].items():
            if n in S["avoid"]:
                dry.append(("#" + n, o["rect"]))
        for k, (x0, x1, y0, y1) in dry:
            for s in range(len(st)):
                us, vs, ys = st[s]
                zs = []
                for side in _SIDES:
                    z = binary(M, "stack%d off %s:%s:%s" % (s, fl, k, side))
                    zs.append(z)
                    rl = Mb * _one_minus(z)
                    if side == "W":
                        row(M, "plumb", fl, k, "stack%d west of its middle" % s, _sub(_sub(us, x0), rl), max = c)
                    elif side == "E":
                        row(M, "plumb", fl, k, "stack%d east of its middle" % s, _sub(us, x1) + rl, min = -c)
                    elif side == "S":
                        row(M, "plumb", fl, k, "stack%d south of its middle" % s, _sub(_sub(vs, y0), rl), max = c)
                    else:
                        row(M, "plumb", fl, k, "stack%d north of its middle" % s, _sub(vs, y1) + rl, min = -c)
                row(M, "plumb", fl, k, "stack%d clear" % s, _sub(_sum(zs), ys), min = 0.0)

def _structure(M):
    S = M["spec"]["structure"]
    Mb = S["big"]
    K = S["lines"]
    span = S["span"]
    fls = S["floors"] if S["floors"] != None else M["all_floors"][:-1]
    B = {}
    beams = []
    for fl in fls:
        if fl not in M["floors"]:
            continue
        F = M["F"][fl]
        ext = S["extent"].get(fl)
        if ext == None:
            bb = _bbox(list(F["zones"].values()))
            ext = (bb[0], bb[1])
        u0 = ext[0]
        u1 = ext[1]
        bs = [var(M, "%s line%d" % (fl, k), u0 + S["margin"], u1 - S["margin"]) for k in range(K)]
        B[fl] = bs
        row(M, "struct", fl, "-", "span west of line 0 <= %s ft" % _f(span, "g"), _sub(bs[0], u0), max = span)
        for k in range(K - 1):
            row(M, "struct", fl, "-", "span between lines %d-%d <= %s ft" % (k, k + 1, _f(span, "g")), _sub(bs[k + 1], bs[k]), min = S["min_gap"], max = span)
        row(M, "struct", fl, "-", "span east of line %d <= %s ft" % (K - 1, _f(span, "g")), _sub(u1, bs[K - 1]), max = span)
        items = []
        for r in M["rooms"][fl]:
            v = M["V"][fl][r["key"]]
            items.append((r["key"], v, r["kind"], r["key"], False))  # a piece's own kind ("annex")
        for n, o in F["obstacles"].items():
            R = o["rect"]
            items.append(("#" + n, {"x": R[0], "w": R[1] - R[0], "d": R[3] - R[2]}, "obstacle", "#" + n, True))
        for key, v, kind, mainkey, is_obst in items:
            for k in range(K):
                b = bs[k]
                w_ = binary(M, "%s|%s line%d:W" % (key, fl, k))
                e_ = binary(M, "%s|%s line%d:E" % (key, fl, k))
                x_ = binary(M, "%s|%s line%d:beam" % (key, fl, k))
                row(M, "struct", fl, key, "and line%d" % k, _sum([w_, e_, x_]), equals = 1.0)
                x0 = v["x"]
                x1 = v["x"] + v["w"]
                dd = v["d"]
                row(M, "struct", fl, key, "west of line%d" % k, _sub(_sub(x1, b), Mb * _one_minus(w_)), max = 0.0 if is_obst else -P_PART / 2.0)
                row(M, "struct", fl, key, "east of line%d" % k, _sub(x0, b) + Mb * _one_minus(e_), min = 0.0 if is_obst else P_PART / 2.0)
                per_ft = S["clear_span_cost"] if (kind in S["clear_span"] or mainkey in S["clear_span"]) else S["beam_cost"]
                if is_obst:
                    objective_term(M, -per_ft * dd * x_)
                    beams.append((fl, key, k, x_, dd))
                else:
                    L = var(M, "beam %s %s line%d" % (key, fl, k), 0.0, 50.0)
                    row(M, "def", fl, key, "beam line%d length" % k, _sub(L, dd) + 50.0 * _one_minus(x_), min = 0.0)
                    objective_term(M, -per_ft * L)
                    beams.append((fl, key, k, x_, L))
    M["X"]["lines"] = B
    M["X"]["beams"] = beams
    transfers = []
    order = M["all_floors"]
    for i in range(1, len(order)):
        up = order[i]
        dn = order[i - 1]
        if up in B and dn in B:
            for k in range(K):
                a = binary(M, "%s line%d over %s line%d" % (up, k, dn, k))
                row(M, "struct", up, "-", "line%d over %s's (<=)" % (k, dn), _sub(_sub(B[up][k], B[dn][k]), Mb * _one_minus(a)), max = 0.0)
                row(M, "struct", up, "-", "line%d over %s's (>=)" % (k, dn), _sub(B[up][k], B[dn][k]) + Mb * _one_minus(a), min = 0.0)
                objective_term(M, -S["transfer_cost"] * _one_minus(a))
                transfers.append((up, k, dn, a))
    M["X"]["transfers"] = transfers

def _not_over(M):
    for no in M["spec"]["not_over"]:
        fa = no["floor"]
        fb = no["below"]
        if fa not in M["floors"] or fb not in M["floors"]:
            continue
        va = M["V"][fa][no["room"]]
        for kb in no["rooms"]:
            vb = M["V"][fb][kb]
            nm = "%s not over %s:%s" % (no["room"], fb, kb)
            bs = []
            for side in _SIDES:
                z = binary(M, "%s:%s (%s)" % (fa, nm, side))
                bs.append(z)
                if side == "W":
                    e = _sub(va["x"] + va["w"], vb["x"])
                elif side == "E":
                    e = _sub(vb["x"] + vb["w"], va["x"])
                elif side == "S":
                    e = _sub(va["y"] + va["d"], vb["y"])
                else:
                    e = _sub(vb["y"] + vb["d"], va["y"])
                row(M, "acoustic", fa, no["room"], "%s (%s)" % (nm, side), e + no["big"] * z, max = no["big"])
            row(M, "acoustic", fa, no["room"], nm, _sum(bs), min = 1.0)

def _owner_rules(M):
    for ru in M["spec"]["rules"]:
        fl = ru["floor"]
        if fl not in M["floors"]:
            continue
        k = ru["room"]
        if k not in M["V"][fl]:
            fail("plan_solve: rule on %r, which is not a room on %s" % (k, fl))
        v = M["V"][fl][k]
        fam = ru["family"]
        why = (" (%s)" % ru["why"]) if ru["why"] else ""
        w = ru["what"]
        meas = {"x0": v["x"], "x1": v["x"] + v["w"], "y0": v["y"], "y1": v["y"] + v["d"], "w": v["w"], "d": v["d"]}
        if w in meas or w == "area":
            e = _total_area(M, fl, k) if w == "area" else meas[w]
            bits = []
            if ru["min"] != None:
                bits.append(">= %s" % (_f(ru["min"], ".0f") + " sq ft" if w == "area" else ftin(ru["min"])))
            if ru["max"] != None:
                bits.append("<= %s" % (_f(ru["max"], ".0f") + " sq ft" if w == "area" else ftin(ru["max"])))
            if ru["equals"] != None:
                bits.append("= %s" % (_f(ru["equals"], ".0f") + " sq ft" if w == "area" else ftin(ru["equals"])))
            row(M, fam, fl, k, "%s %s%s" % (w, " ".join(bits), why), e, min = ru["min"], max = ru["max"], equals = ru["equals"])
        elif w == "holds":
            c = ru["rect"]
            row(M, fam, fl, k, "holds W%s" % why, v["x"], max = c[0])
            row(M, fam, fl, k, "holds E%s" % why, v["x"] + v["w"], min = c[1])
            row(M, fam, fl, k, "holds S%s" % why, v["y"], max = c[2])
            row(M, fam, fl, k, "holds N%s" % why, v["y"] + v["d"], min = c[3])
        elif w == "near":
            p = ru["point"]
            c = ru["reach"] or 0.0
            row(M, fam, fl, k, "near (W)%s" % why, v["x"], max = p[0] + c)
            row(M, fam, fl, k, "near (E)%s" % why, v["x"] + v["w"], min = p[0] - c)
            row(M, fam, fl, k, "near (S)%s" % why, v["y"], max = p[1] + c)
            row(M, fam, fl, k, "near (N)%s" % why, v["y"] + v["d"], min = p[1] - c)
        elif w == "clear":
            c = ru["rect"]
            Mb = max(M["Mu"][fl], M["Mv"][fl]) + 2.0
            bs = []
            exprs = {"W": _sub(v["x"] + v["w"], c[0]), "E": _sub(c[1], v["x"]), "S": _sub(v["y"] + v["d"], c[2]), "N": _sub(c[3], v["y"])}
            for side in _SIDES:
                b = binary(M, "%s:%s clear of %s (%s)%s" % (fl, k, _f(c[0], ".1f"), side, why))
                bs.append(b)
                row(M, fam, fl, k, "clear %s%s" % (side, why), exprs[side] + Mb * b, max = Mb)
            row(M, fam, fl, k, "clear of (%s, %s, %s, %s)%s" % (_f(c[0], ".1f"), _f(c[1], ".1f"), _f(c[2], ".1f"), _f(c[3], ".1f"), why), _sum(bs), min = 1.0)
        elif w == "touch":
            _on_line(M, fl, k, ru["side"], ru["at"], ru["span"][0], ru["span"][1], ru["length"], None, fam)
        else:
            fail("plan_solve.rule: what is x0, x1, y0, y1, w, d, area, holds, near, clear or touch; got %r" % w)

def _cantilever(M):
    cc = M["spec"]["cantilever"]
    if cc == None:
        return
    order = [f for f in M["all_floors"] if f in M["floors"]]
    for i in range(1, len(order)):
        up = M["F"][order[i]]
        dn = M["F"][order[i - 1]]
        c = cc.get(up["key"]) if type(cc) == "dict" else cc
        if c == None:
            continue
        grown = [(z[0] - c, z[1] + c, z[2] - c, z[3] + c) for z in dn["zones"].values()]
        for zn, z in up["zones"].items():
            if not _rect_in_union(z, grown):
                row(M, "envelope", up["key"], zn, "inside %s grown by %s" % (dn["key"], ftin(c)), 0.0, min = 1.0)

# ----------------------------------------------------------------------------------------------- the build

def build(spec, *, mode = "exact", step = 0.25, fix = None, pin = None, drop = None, objective = "score",
          anchor = None, within = None, tie = 1e-3, broken = "infeasible", name = None, floors = None,
          sym = True, hooks = True, extra = None, shift = None, widths = None, bounds = "tight", untag = None):
    """Build the problem's model on a new opt.problem and return it (a dict, M). `fix={binary name: 0|1}` builds
    those binaries as constants; `pin={floor: {room: rect}}` fixes rooms; `drop=[family, "family:floor",
    "family:floor:room"]` leaves rules out; `floors=` the floors to include (the others are not modelled);
    `objective` "score" (maximize the plan's value), "move" (minimize feet of wall moved from `anchor`, a
    layout {floor: {room: rect}}), or "none" (any feasible point); `within` with "move": the score at least
    this. `broken="skip"` leaves a constant rule that fails out (a check), instead of making the model
    infeasible. `extra(M)` adds rows after everything else (as hooks do). `untag=[family...]` writes those families'
    rules without tags (their names stay), so the builder's conflict() takes them as one group, tried last."""
    if mode not in ("exact", "proxy", "grid"):
        fail("plan_solve.build: mode is exact, proxy or grid (fast is solve()'s), got %r" % mode)
    nm = name or ("plan_solve_" + spec["name"])
    p = opt.problem(name = _feature_name(nm))
    dd = {}
    for x in list(spec.get("drop") or []) + list(drop or []):
        dd[x] = True
    allf = [F["key"] for F in spec["floors"]]
    fls = [f for f in allf if (floors == None or f in floors)]
    M = {
        "p": p, "name": nm, "spec": spec, "mode": mode, "step": float(step), "fix": dict(fix or {}),
        "pin": dict(pin or {}), "drop": dd, "broken_mode": broken, "objective": objective, "all_floors": allf,
        "floors": fls, "F": {}, "rooms": {}, "by": {}, "V": {}, "A": {}, "Aproxy": {}, "rel": {}, "doors": {},
        "parents": {}, "depth": {}, "zone_of": {}, "egress": {}, "annexes": {}, "Mu": {}, "Mv": {},
        "bins": {}, "arr": [], "obj": [], "names": {}, "broken": [], "fams": {}, "X": {}, "faces": {},
        "shared_v": {}, "shared_area": {}, "count": {"vars": 0, "binaries": 0, "rows": 0}, "notes": [],
        "shift": dict(shift or {}), "shifted": [], "widths": dict(widths or {}), "units": {}, "bounds": bounds,
        "untag": {f: True for f in (untag or [])}, "spec_drop": {x: True for x in (spec.get("drop") or [])},
    }
    for fc in spec["faces"]:
        M["faces"][fc["name"]] = var(M, "face:" + fc["name"], fc["min"], fc["max"])
    shared_list = list(spec["shared"]) + [c for c in spec["courts"] if c.get("rect") == None]
    for F in spec["floors"]:
        fl = F["key"]
        if fl not in fls:
            continue
        M["F"][fl] = F
        rooms = _floor_rooms(spec, fl, shared_list)
        M["rooms"][fl] = rooms
        M["by"][fl] = {r["key"]: r for r in rooms}
        an = {}
        for r in rooms:
            if r["annex_of"] != None:
                an.setdefault(r["annex_of"], []).append(r["key"])
        M["annexes"][fl] = an
        for key in ("V", "A", "Aproxy", "rel", "doors", "parents", "zone_of", "egress"):
            M[key][fl] = {}
        bb = _bbox(list(F["zones"].values()))
        M["Mu"][fl] = bb[1] - bb[0] + 2.0
        M["Mv"][fl] = bb[3] - bb[2] + 2.0
    for fl in fls:
        _vars(M, fl)
        _zones(M, fl)
        _areas(M, fl)
        _rules(M, fl)
        _nonoverlap(M, fl)
        _doors(M, fl)
        _egress(M, fl)
        _must_hold(M, fl)
    if sym:
        for fl, a, b in spec["sym"]:
            if fl in fls and a in M["V"][fl] and b in M["V"][fl] and not (M["V"][fl][a]["fixed"] and M["V"][fl][b]["fixed"]):
                _sym(M, fl, a, b)
    if spec["stacks"] != None:
        _plumbing(M)
    if spec["structure"] != None:
        _structure(M)
    _not_over(M)
    _owner_rules(M)
    _cantilever(M)
    if hooks:
        for h in spec["hooks"]:
            h(M)
    if extra != None:
        extra(M)
    _objective(M, objective, anchor, within, tie)
    return M

def _score_terms(M, fl):
    terms = []
    for r in M["rooms"][fl]:
        if r["fixed"] != None or r["annex_of"] != None or r["kept"] or r["shared"] != None:
            continue
        k = r["key"]
        A = _total_area(M, fl, k)
        terms.append(r["value"] * A)
        if r["knee"]:
            if _isnum(A):
                terms.append(-(r["value"] - r["knee"][1]) * max(0.0, A - r["knee"][0]))
            else:
                e = var(M, "%s:%s.past_knee" % (fl, k), 0.0, None)
                row(M, "value", fl, k, "past %s sq ft" % _f(r["knee"][0], ".0f"), _sub(e, A), min = -r["knee"][0])
                terms.append(-(r["value"] - r["knee"][1]) * e)
    return terms

def _tie_terms(M, fl):
    out = []
    for r in M["rooms"][fl]:
        if r["fixed"] != None or r["kept"] or r["shared"] != None:
            continue
        v = M["V"][fl][r["key"]]
        if v["fixed"]:
            continue
        out.append(r["tie"] * (v["x"] + v["y"]))
    return out

def _objective(M, objective, anchor, within, tie):
    p = M["p"]
    score = []
    ties = []
    for fl in M["floors"]:
        score += _score_terms(M, fl)
        ties += _tie_terms(M, fl)
    M["score_expr"] = _sum(score + M["obj"])
    if objective == "none":
        return
    if objective == "score":
        e = _sub(M["score_expr"], tie * _sum(ties)) if ties and tie else M["score_expr"]
        if _isnum(e):
            return
        p.maximize(e)
        return
    if objective == "move":
        if anchor == None:
            fail("plan_solve.build: objective=\"move\" needs anchor= (a layout)")
        terms = []
        for fl in M["floors"]:
            for k, R in anchor.get(fl, {}).items():
                if k not in M["V"][fl]:
                    continue
                v = M["V"][fl][k]
                if v["fixed"]:
                    continue
                faces = [(v["x"], R[0], "W"), (v["x"] + v["w"], R[1], "E"), (v["y"], R[2], "S"), (v["y"] + v["d"], R[3], "N")]
                for e, old, nm in faces:
                    t = var(M, "%s:%s.moved %s" % (fl, k, nm), 0.0, None)
                    row(M, "def", fl, k, "moved %s (+)" % nm, _sub(t, e), min = -old)
                    row(M, "def", fl, k, "moved %s (-)" % nm, t + e, min = old)
                    terms.append(t)
        if within != None:
            row(M, "def", "-", "-", "score at least", M["score_expr"], min = within)
        M["move_expr"] = _sum(terms)
        e = M["move_expr"]
        if not _isnum(M["score_expr"]):
            e = _sub(e, 1e-4 * M["score_expr"])
        if not _isnum(e):
            p.minimize(e)
        return
    fail("plan_solve.build: objective is score, move or none; got %r" % objective)

def run(M, *, time_limit = None, gap = None, max_nodes = None, fix = None, relax = None, objective = True, drop = None,
        start = None):
    """Solve M's problem as it stands (opt's own fix=, relax=, objective= and start= apply to this solve alone); the
    solution is kept as M["sol"] and returned."""
    kw = {}
    if time_limit != None:
        kw["time_limit"] = time_limit
    if gap != None:
        kw["gap"] = gap
    if max_nodes != None:
        kw["max_nodes"] = int(max_nodes)
    if fix:
        kw["fix"] = fix
    if relax:
        kw["relax"] = relax
    if drop:
        kw["drop"] = drop
    if not objective:
        kw["objective"] = False
    if start:
        kw["start"] = start
    sol = M["p"].solve(**kw)
    M["sol"] = sol
    return sol

def start_of(M, M0, sol):
    """A MIP start for M from another model's solution: the binaries the two share by name, at sol's values (a
    partial start; HiGHS completes the rest). For a model that relaxes M0 -- a rule moved out, a family left out --
    the point is feasible, so the search begins with sol's objective in hand."""
    if sol == None or not sol.feasible:
        return None
    out = {}
    for n, b in M["bins"].items():
        b0 = M0["bins"].get(n)
        if b0 != None:
            out[b] = 1 if _val(sol, b0) > 0.5 else 0
    return out

# --------------------------------------------------------------------------------------- reading an answer

def _has_point(sol):
    return sol.feasible

def layout_of(M, sol):
    """{floor: {room or piece: rect}} at the solution."""
    out = {}
    for fl in M["floors"]:
        lay = {}
        for r in M["rooms"][fl]:
            v = M["V"][fl][r["key"]]
            x = _val(sol, v["x"])
            y = _val(sol, v["y"])
            lay[r["key"]] = (x, x + _val(sol, v["w"]), y, y + _val(sol, v["d"]))
        out[fl] = lay
    return out

def arrangement_of(M, sol):
    """The arrangement binaries' values ({name: 0|1}): zones, sides, doors, escape walls, holds, L-pieces."""
    out = {}
    for n in M["arr"]:
        out[n] = 1 if _val(sol, M["bins"][n]) > 0.5 else 0
    return out

def _all_bins(M, sol):
    out = dict(M["fix"])
    for n, b in M["bins"].items():
        out[n] = 1 if _val(sol, b) > 0.5 else 0
    return out

def relations(M, fl, lay, tol = 1e-6):
    """The separations that hold in a layout: (a, b, side) with a fully on that side of b, its wall included."""
    out = []
    rooms = [r["key"] for r in M["rooms"][fl]]
    for i in range(len(rooms)):
        for j in range(i + 1, len(rooms)):
            a = rooms[i]
            b = rooms[j]
            if (a, b) not in M["rel"][fl]:
                continue
            A = lay[a]
            B = lay[b]
            g = _gap(M, fl, a, b)
            if A[1] + g <= B[0] + tol:
                out.append((a, b, "W"))
            if B[1] + g <= A[0] + tol:
                out.append((a, b, "E"))
            if A[3] + g <= B[2] + tol:
                out.append((a, b, "S"))
            if B[3] + g <= A[2] + tol:
                out.append((a, b, "N"))
    return out

def _family(n):
    i = n.find(":")
    return n[:i] if i > 0 else n

def _exact_score(spec, plan_rooms):
    """value x area per room (pieces summed), with the value curves; {(floor, key): area} -> (total, per floor)."""
    by = {}
    for r in spec["rooms"]:
        by[(r["floor"], r["key"])] = r
    tot = 0.0
    per = {}
    for fk, A in plan_rooms.items():
        r = by.get(fk)
        if r == None or r["kept"] or r["fixed"] != None:
            continue
        s = r["value"] * A
        if r["knee"] and A > r["knee"][0]:
            s -= (r["value"] - r["knee"][1]) * (A - r["knee"][0])
        tot += s
        per[fk[0]] = per.get(fk[0], 0.0) + s
    return tot, per

def layout_score(spec, lay):
    """A layout's score ({floor: {room or piece: rect}}, as drawn_layout reads a drawing): each room's main rect and
    its pieces' (spec rooms' pieces, or the annex "<key>_a") summed, at the problem's values."""
    areas = {}
    for r in spec["rooms"]:
        L = lay.get(r["floor"])
        if L == None or r["key"] not in L:
            continue
        keys = [r["key"]] + [pc["key"] for pc in (r.get("pieces") or [])]
        if not r.get("pieces") and r.get("annex") != None:
            keys.append(r["key"] + "_a")
        a = 0.0
        for k in keys:
            q = L.get(k)
            if q != None:
                a += _area(q)
        areas[(r["floor"], r["key"])] = a
    return _exact_score(spec, areas)[0]

def score(spec, plan):
    """A plan's score under a problem's values (value x area, pieces summed, value curves): rooms matched by
    floor and key; rooms the problem does not have count nothing."""
    areas = {}
    for r in plan["rooms"]:
        a = 0.0
        for q in r["rects"]:
            a += _area(q)
        areas[(r["floor"], r["key"])] = areas.get((r["floor"], r["key"]), 0.0) + a
    return _exact_score(spec, areas)[0]

def read(M, sol, *, extra = None, why = False, why_probes = 20, why_time = 30.0):
    """The answer as a plan value (@cadpole/plan.star's format), the solver's numbers in meta["plan_solve"]. Reading
    never searches for a conflict: for a MILP that is a deletion filter, many solves. `why=True` asks, for an
    infeasible solve, for the builder's family pass alone (`conflict(families_only=True)`, at most `why_probes`
    probes and `why_time` seconds) in meta["plan_solve"]["conflict"]; explain() is the full question."""
    spec = M["spec"]
    has = _has_point(sol)
    meta = {"status": sol.status, "ok": sol.ok, "feasible": has, "objective": sol.objective, "gap": sol.gap, "bound": sol.bound,
            "nodes": sol.nodes, "mode": M["mode"], "step": M["step"], "problem": M["name"],
            "stats": dict(M["count"]), "broken": list(M["broken"]), "families": dict(M["fams"]),
            "dropped": sorted(M["drop"].keys()), "fixed_binaries": len(M["fix"])}
    plan = {"name": spec["name"], "unit": "ft", "source": spec["source"] or ("plan_solve:" + spec["name"]),
            "site": spec["site"] or {}, "floors": [], "rooms": [], "doors": [], "windows": [], "stacks": [],
            "courts": [], "bearing": [], "meta": {"plan_solve": meta}}
    for F in spec["floors"]:
        if F["key"] not in M["floors"]:
            continue
        # the plate: the zones and the core's rects (a shaft with its walls may span the wall between two zones)
        core = [o["rect"] for o in F["obstacles"].values() if o["kind"] != "other"] + [o["rect"] for o in F["fixed"].values()]
        plan["floors"].append({"key": F["key"], "name": F["name"], "z": F["z"], "ceiling": F["ceiling"],
                               "plate": list(F["zones"].values()) + core})
    if not has:
        if why and sol.status == "infeasible":
            meta["conflict"] = sol.conflict(families_only = True, max_probes = why_probes, time_limit = why_time)
        return _plan_normalize(plan)
    lay = layout_of(M, sol)
    meta["layout"] = lay
    areas = {}
    for fl in M["floors"]:
        F = M["F"][fl]
        for r in M["rooms"][fl]:
            if r["annex_of"] != None:
                continue
            k = r["key"]
            if r["shared"] != None:
                if r["kind"] in ("court", "well"):
                    continue
            rects = []
            for pc in _pieces(M, fl, k):
                q = lay[fl][pc]
                if pc == k or _area(q) > 0.05:
                    rects.append(q)
            a = 0.0
            for q in rects:
                a += _area(q)
            areas[(fl, k)] = a
            plan["rooms"].append({"key": k, "name": r["name"], "floor": fl, "kind": r["kind"], "rects": rects,
                                  "unit": r["unit"], "habitable": r["habitable"], "sleeping": r["sleeping"],
                                  "kept": r["kept"], "ceiling": None, "core": r["core"], "furnish": r["furnish"]})
        for n, o in F["fixed"].items():
            plan["rooms"].append({"key": n, "name": o["name"], "floor": fl, "kind": o["kind"], "rects": [o["rect"]],
                                  "unit": "main", "habitable": False, "sleeping": False, "kept": False,
                                  "ceiling": None, "core": o["core"], "furnish": None})
        for n, o in F["obstacles"].items():
            if o["kind"] != "other":
                plan["rooms"].append({"key": n, "name": o["name"], "floor": fl, "kind": o["kind"], "rects": [o["rect"]],
                                      "unit": "main", "habitable": False, "sleeping": False, "kept": False,
                                      "ceiling": None, "core": o["core"], "furnish": None})
        for (ap, t), dr in M["doors"][fl].items():
            if _val(sol, dr["a"]) < 0.5:
                continue
            side = None
            for s, b in dr["sides"].items():
                if _val(sol, b) > 0.5:
                    side = s
            if side == None:
                continue
            A = lay[fl][ap]
            T = F["fixed"][t[1:]]["rect"] if t.startswith("@") else lay[fl][t]
            if side in ("W", "E"):
                lo = max(A[2], T[2])
                hi = min(A[3], T[3])
                at = (A[0] if side == "W" else A[1], (lo + hi) / 2.0)
            else:
                lo = max(A[0], T[0])
                hi = min(A[1], T[1])
                at = ((lo + hi) / 2.0, A[2] if side == "S" else A[3])
            plan["doors"].append({"key": "d_%s_%s" % (ap, t.replace("@", "")), "floor": fl, "a": _main_of(M, fl, ap),
                                  "b": t[1:] if t.startswith("@") else _main_of(M, fl, t), "wall": side, "at": at,
                                  "width": DOOR, "swing": "a", "hinge": "lo", "span": (lo, hi)})
        for k, bs in M["egress"][fl].items():
            for wl, b in bs:
                if _val(sol, b) < 0.5:
                    continue
                A = lay[fl][k]
                c = _val(sol, _coord(M, wl["at"]))
                if wl["side"] in ("W", "E"):
                    lo = max(A[2], wl["span"][0])
                    hi = min(A[3], wl["span"][1])
                    at = (c, (lo + hi) / 2.0)
                else:
                    lo = max(A[0], wl["span"][0])
                    hi = min(A[1], wl["span"][1])
                    at = ((lo + hi) / 2.0, c)
                plan["windows"].append({"key": "w_%s_%s" % (k, wl["side"].lower()), "floor": fl, "room": k,
                                        "wall": wl["side"], "at": at, "width": WINDOW, "sill": 2.0, "head": 7.0,
                                        "source": wl["source"], "egress": True})
                break
    for s in spec["courts"]:
        if s.get("rect") != None:
            have = [f for f in [s["base"]] + s["floors"] if f in M["floors"]]
            if have:  # its base, or the lowest of its floors this plan has
                plan["courts"].append({"key": s["key"], "kind": s["kind"], "rect": s["rect"], "base": have[0],
                                       "top": s["top"], "reaches": s["reaches"]})
            continue
        v = M["shared_v"].get(s["key"])
        if v == None:
            continue
        x = _val(sol, v["x"])
        y = _val(sol, v["y"])
        plan["courts"].append({"key": s["key"], "kind": s["kind"], "rect": (x, x + _val(sol, v["w"]), y, y + _val(sol, v["d"])),
                               "base": s["base"], "top": s["top"], "reaches": s["reaches"]})
    if "stacks" in M["X"]:
        n = 0
        for us, vs, ys in M["X"]["stacks"]:
            if _val(sol, ys) > 0.5:
                plan["stacks"].append({"key": "s%d" % n, "at": (_val(sol, us), _val(sol, vs)), "floors": list(M["floors"]), "kind": "dwv"})
                n += 1
    if "lines" in M["X"]:
        lines = {}
        for fl, bs in M["X"]["lines"].items():
            bb = _bbox(list(M["F"][fl]["zones"].values()))
            lines[fl] = [_val(sol, b) for b in bs]
            for u in lines[fl]:
                plan["bearing"].append({"floor": fl, "axis": "v", "at": u, "span": (bb[2], bb[3])})
        meta["lines"] = lines
        beams = []
        beam_ft = 0.0
        for fl, key, k, x_, L in M["X"]["beams"]:
            if _val(sol, x_) > 0.5:
                ln = _val(sol, L)
                beams.append({"floor": fl, "room": key, "line": k, "length": ln})
                beam_ft += ln
        meta["beams"] = beams
        meta["beam_ft"] = beam_ft
        meta["transfers"] = [{"floor": up, "line": k, "over": dn} for up, k, dn, a in M["X"]["transfers"] if _val(sol, a) < 0.5]
    if M["faces"]:
        meta["faces"] = {n: _val(sol, v) for n, v in M["faces"].items()}
    # what the hooks keep in M["X"] (an envelope's total, a stair strip's margin), at the solution
    ex = {}
    for k, v in M["X"].items():
        if k in ("stacks", "lines", "beams", "transfers"):
            continue
        items = v.items() if type(v) == "dict" else [(None, v)]
        for k2, v2 in items:
            if _isnum(v2) or type(v2) in ("OptVar", "OptExpr"):
                ex[k if k2 == None else k + "." + k2] = _val(sol, v2)
    if ex:
        meta["extra"] = ex
    tot, per = _exact_score(spec, areas)
    meta["score"] = tot
    meta["score_by_floor"] = per
    meta["areas"] = {"%s:%s" % fk: a for fk, a in areas.items()}
    b = [n for n in sol.binding() if _family(n) not in _QUIET]
    meta["binding"] = b
    fam = {}
    for n in b:
        f = _family(n)
        fam[f] = fam.get(f, 0) + 1
    meta["binding_by_family"] = fam
    if extra != None:
        meta.update(extra)
    return _plan_normalize(plan)

# --------------------------------------------------------------------------------------------- the entry points

def _sub_name(name, spec, tag):
    base = name or ("plan_solve_" + spec["name"])
    return base if tag == None else base + "_" + tag

def solve(spec, *, mode = "exact", step = 0.25, time_limit = None, gap = None, fix = None, pin = None, drop = None,
          floors = None, name = None, objective = "score", anchor = None, within = None, tie = 1e-3,
          broken = "infeasible", sym = True, widths = None, max_nodes = None, why = False):
    """Build and solve; the answer is a plan value with meta["plan_solve"] (status, score, gap, binding rows by
    family, broken constant rules, beams, stacks, faces). `mode="fast"` is the two-stage: the proxy model finds
    the arrangement, the exact model sizes it (two problems: name_proxy and name). An infeasible answer searches
    for no conflict unless `why=True` (read()'s: the builder's family pass, bounded); explain() asks why in full."""
    if mode == "fast":
        return _two_stage(spec, step = step, time_limit = time_limit, gap = gap, fix = fix, pin = pin, drop = drop,
                          floors = floors, name = name, tie = tie, sym = sym, why = why)
    M = build(spec, mode = mode, step = step, fix = fix, pin = pin, drop = drop, floors = floors,
              name = _sub_name(name, spec, None), objective = objective, anchor = anchor, within = within,
              tie = tie, broken = broken, sym = sym, widths = widths)
    sol = run(M, time_limit = time_limit, gap = gap, max_nodes = max_nodes)
    return read(M, sol, why = why)

def _two_stage(spec, *, step, time_limit, gap, fix, pin, drop, floors, name, tie, sym, why = False):
    M1 = build(spec, mode = "proxy", step = step, fix = fix, pin = pin, drop = drop, floors = floors,
               name = _sub_name(name, spec, "proxy"), tie = tie, sym = sym)
    s1 = run(M1, time_limit = time_limit, gap = gap)
    if not _has_point(s1):
        plan = read(M1, s1, why = why)
        plan["meta"]["plan_solve"]["stage"] = "proxy"
        return plan
    arr = arrangement_of(M1, s1)
    f2 = dict(fix or {})
    f2.update(arr)
    M2 = build(spec, mode = "exact", step = step, fix = f2, pin = pin, drop = drop, floors = floors,
               name = _sub_name(name, spec, None), tie = tie, sym = sym)
    s2 = run(M2, time_limit = time_limit, gap = gap)
    stage = "proxy arrangement, exact sizes"
    if not _has_point(s2):
        f3 = dict(fix or {})
        for n, v in arr.items():
            if ">" in n:
                f3[n] = v
        M2 = build(spec, mode = "exact", step = step, fix = f3, pin = pin, drop = drop, floors = floors,
                   name = _sub_name(name, spec, "doors"), tie = tie, sym = sym)
        s2 = run(M2, time_limit = time_limit, gap = gap)
        stage = "proxy door tree, exact sizes"
    plan = read(M2, s2, extra = {"stage": stage, "proxy_status": s1.status, "proxy_objective": s1.objective}, why = why)
    return plan

def _symmetric(F, axis, tol = 1e-6):
    """Whether a floor's zones, obstacles, fixed rooms and escape walls mirror onto themselves about its centre."""
    bb = _bbox(list(F["zones"].values()))
    def mir(r):
        if axis == "u":
            return (bb[0] + bb[1] - r[1], bb[0] + bb[1] - r[0], r[2], r[3])
        return (r[0], r[1], bb[2] + bb[3] - r[3], bb[2] + bb[3] - r[2])
    def same(a, b):
        return abs(a[0] - b[0]) < tol and abs(a[1] - b[1]) < tol and abs(a[2] - b[2]) < tol and abs(a[3] - b[3]) < tol
    groups = [list(F["zones"].values()), [o["rect"] for o in F["obstacles"].values()], [o["rect"] for o in F["fixed"].values()]]
    ew = []
    for wl in F["egress"]:
        if not _isnum(wl["at"]):
            return False
        if wl["side"] in ("W", "E"):
            ew.append((wl["at"], wl["at"], wl["span"][0], wl["span"][1]))
        else:
            ew.append((wl["span"][0], wl["span"][1], wl["at"], wl["at"]))
    groups.append(ew)
    for g in groups:
        for r in g:
            m = mir(r)
            if not any([same(m, q) for q in g]):
                return False
    return True

def _mirror_layout(M, lay, axis):
    out = {}
    for fl, L in lay.items():
        bb = _bbox(list(M["F"][fl]["zones"].values()))
        q = {}
        for k, r in L.items():
            if axis == "u":
                q[k] = (bb[0] + bb[1] - r[1], bb[0] + bb[1] - r[0], r[2], r[3])
            else:
                q[k] = (r[0], r[1], bb[2] + bb[3] - r[3], bb[2] + bb[3] - r[2])
        out[fl] = q
    return out

def _flip_terms(M, fl, lay, delta, name, keys):
    """An arrangement cut's binaries on one floor: for each separation that holds in `lay` among `keys`, a binary
    f and one row, f = 1 => the pair overlaps by >= delta along that axis (so it must be apart the other way)."""
    bs = []
    for a, b, side in relations(M, fl, lay):
        if keys != None and (a not in keys or b not in keys):
            continue
        if M["V"][fl][a]["fixed"] and M["V"][fl][b]["fixed"]:
            continue
        g = _gap(M, fl, a, b)
        xa0, xa1, ya0, ya1 = _ends(M, fl, a)
        xb0, xb1, yb0, yb1 = _ends(M, fl, b)
        f = binary(M, "%s:%s:%s|%s:%s" % (name, fl, a, b, side))
        Mb = (M["Mu"][fl] if side in ("W", "E") else M["Mv"][fl]) + 2 * delta
        if side == "W":
            e = _sub(xa1 + g, xb0)
        elif side == "E":
            e = _sub(xb1 + g, xa0)
        elif side == "S":
            e = _sub(ya1 + g, yb0)
        else:
            e = _sub(yb1 + g, ya0)
        row(M, "flip", fl, "%s|%s" % (a, b), "%s %s" % (side, name), _sub(e, (delta + Mb) * f), min = -Mb)
        bs.append(f)
    return bs

def flip_cut(M, lay, *, delta = 3.0, name = "flip", among = None, min_flips = 1):
    """Add an arrangement cut against the layout {floor: {room: rect}}: in the next solve some room among `among`
    ({floor: [keys]} or a list for every floor; None for all) moves `delta` feet past another. Returns the
    number of separations the cut can flip (0: nothing can move, so the cut is a dead end)."""
    bs = []
    for fl in M["floors"]:
        if fl not in lay:
            continue
        keys = among.get(fl) if type(among) == "dict" else among
        if type(among) == "dict" and keys == None:
            continue
        bs += _flip_terms(M, fl, lay[fl], delta, name, keys)
    row(M, "flip", "-", "-", "%s (>= %d flips)" % (name, min_flips), _sum(bs), min = float(min_flips))
    return len(bs)

def _max_move(a, b):
    m = 0.0
    for fl, L in a.items():
        for k, r in L.items():
            q = b.get(fl, {}).get(k)
            if q == None:
                continue
            for i in range(4):
                m = max(m, abs(r[i] - q[i]))
    return m

def options(spec, *, count = 3, differ = 3.0, among = None, mirrors = True, mode = "exact", step = 0.25,
            time_limit = None, gap = None, fix = None, pin = None, floors = None, name = None, tie = 1e-3):
    """Up to `count` plans that really differ: the best, then the best that moves some room among `among` at
    least `differ` feet past another (3 to 6 ft) against every earlier one -- rows added between solves on one
    problem. Mirror images count as one (a floor symmetric about its centre gets a cut against each earlier
    option's mirror too); symmetry rows (the problem's `sym`) are in from the start. Never a Hamming no-good on
    the arrangement binaries: two rooms can be apart on two sides at once, so a binary flips with nothing moving. Each plan's
    meta["plan_solve"]["option"] says its rank, its score against the best and the least it moved."""
    M = build(spec, mode = mode, step = step, fix = fix, pin = pin, floors = floors,
              name = _sub_name(name, spec, "options"), tie = tie)
    out = []
    lays = []
    axes = []
    if mirrors:
        for ax in ("u", "v"):
            if all([_symmetric(M["F"][fl], ax) for fl in M["floors"]]):
                axes.append(ax)
    best = None
    for k in range(count):
        if k > 0:
            n = 0
            for j in range(len(lays)):
                n += flip_cut(M, lays[j], delta = differ, name = "option%d-vs-%d" % (k + 1, j + 1), among = among)
                for ax in axes:
                    flip_cut(M, _mirror_layout(M, lays[j], ax), delta = differ, name = "option%d-vs-%d-mirror-%s" % (k + 1, j + 1, ax), among = among)
            if n == 0:
                break
        sol = run(M, time_limit = time_limit, gap = gap)
        if not _has_point(sol):
            break
        plan = read(M, sol)
        lay = plan["meta"]["plan_solve"]["layout"]
        sc = plan["meta"]["plan_solve"]["score"]
        if best == None:
            best = sc
        least = None
        least_m = None
        for L in lays:
            mm = _max_move(L, lay)
            least = mm if least == None else min(least, mm)
            for ax in axes:
                mm = _max_move(_mirror_layout(M, L, ax), lay)
                least_m = mm if least_m == None else min(least_m, mm)
        plan["meta"]["plan_solve"]["option"] = {"rank": k + 1, "score": sc, "vs_best": sc - best,
                                                "least_move_ft": least, "least_move_vs_mirrors_ft": least_m,
                                                "differ": differ, "mirrors": axes}
        out.append(plan)
        lays.append(lay)
    return out

def solve_floors(spec, *, order = None, mode = "exact", step = 0.25, time_limit = None, gap = None, pins = None,
                 name = None, tie = 1e-3):
    """Floor by floor, bottom up: each step solves one more floor with the floors below it fixed as found (the
    cross-floor rules -- stacks, lines over lines, not-over, the envelope -- tie it to them). A lower floor
    cannot make room for an upper one, but each step is one floor's MILP. `pins` {floor: {room: rect}} starts
    from given floors. Returns the last step's plan; meta["plan_solve"]["steps"] has each step's numbers."""
    order = order or [F["key"] for F in spec["floors"]]
    pins = dict(pins or {})
    steps = []
    plan = None
    for i in range(len(order)):
        fl = order[i]
        if fl in pins:
            continue
        fls = order[:i + 1]
        M = build(spec, mode = mode, step = step, pin = pins, floors = fls, name = _sub_name(name, spec, "floor_" + _feature_name(fl)), tie = tie)
        sol = run(M, time_limit = time_limit, gap = gap)
        plan = read(M, sol)
        ps = plan["meta"]["plan_solve"]
        steps.append({"floor": fl, "status": ps["status"], "objective": ps["objective"], "gap": ps["gap"], "score": ps.get("score")})
        if not ps["feasible"]:
            break
        pins[fl] = ps["layout"][fl]
    if plan != None:
        plan["meta"]["plan_solve"]["steps"] = steps
    return plan

# ------------------------------------------------------------------------------------ reading a drawn plan

_LETTERS = "abcdefghijklmnopqrstuvwxyz"

def _held_sides(A, B, g, tol = 1e-6):
    """The sides A is fully on of B, its wall g included."""
    out = []
    if A[1] + g <= B[0] + tol:
        out.append("W")
    if B[1] + g <= A[0] + tol:
        out.append("E")
    if A[3] + g <= B[2] + tol:
        out.append("S")
    if B[3] + g <= A[2] + tol:
        out.append("N")
    return out

def _inside(q, z, tol = 1e-6):
    return q[0] >= z[0] - tol and q[1] <= z[1] + tol and q[2] >= z[2] - tol and q[3] <= z[3] + tol

def _flush_side(q, m, tol = 0.05):
    """The side of rect m that rect q sits against, flush and overlapping it; None if it does not."""
    ou = min(q[1], m[1]) - max(q[0], m[0])
    ov = min(q[3], m[3]) - max(q[2], m[2])
    if abs(q[3] - m[2]) <= tol and ou > tol:
        return "S"
    if abs(q[2] - m[3]) <= tol and ou > tol:
        return "N"
    if abs(q[1] - m[0]) <= tol and ov > tol:
        return "W"
    if abs(q[0] - m[1]) <= tol and ov > tol:
        return "E"
    return None

def _parked(m, side):
    """Where an L-piece that is nothing sits: a point at the start of its main room's wall (the parked rows)."""
    if side == "N":
        return (m[0], m[0], m[3], m[3])
    if side == "E":
        return (m[1], m[1], m[2], m[2])
    return (m[0], m[0], m[2], m[2])

def _cut(q, f, tol = 1e-6):
    """q less f when f crosses q along one side, whole in the other axis (what is left is one rectangle); else None."""
    if f[2] <= q[2] + tol and f[3] >= q[3] - tol:
        if f[0] <= q[0] + tol and f[1] > q[0] and f[1] < q[1]:
            return (f[1], q[1], q[2], q[3])
        if f[1] >= q[1] - tol and f[0] < q[1] and f[0] > q[0]:
            return (q[0], f[0], q[2], q[3])
    if f[0] <= q[0] + tol and f[1] >= q[1] - tol:
        if f[2] <= q[2] + tol and f[3] > q[2] and f[3] < q[3]:
            return (q[0], q[1], f[3], q[3])
        if f[3] >= q[3] - tol and f[2] < q[3] and f[2] > q[2]:
            return (q[0], q[1], q[2], f[2])
    return None

def _join(a, b, tol = 1e-6):
    """The union of two rects when it is a rectangle (same span, touching across it); else None."""
    if abs(a[0] - b[0]) < tol and abs(a[1] - b[1]) < tol:
        if abs(a[3] - b[2]) < tol:
            return (a[0], a[1], a[2], b[3])
        if abs(b[3] - a[2]) < tol:
            return (a[0], a[1], b[2], a[3])
    if abs(a[2] - b[2]) < tol and abs(a[3] - b[3]) < tol:
        if abs(a[1] - b[0]) < tol:
            return (a[0], b[1], a[2], a[3])
        if abs(b[1] - a[0]) < tol:
            return (b[0], a[1], a[2], a[3])
    return None

def _clean_rects(F, key, rects, notes, tol = 0.05, absorbed = None):
    """A drawn room's rects read against its floor: a rect that is the floor's own fixed room or obstacle is left to
    it, an obstacle crossing a rect along one side is cut off it, a rect outside every zone is dropped, and rects that
    join into one rectangle are joined -- each noted. The main rect stays first. The fixed rooms the drawing drew
    this room over (a rect inside one, or one crossing a rect) go into `absorbed`: the room is open to them."""
    own = [(n, o["rect"]) for n, o in F["fixed"].items()] + [(n, o["rect"]) for n, o in F["obstacles"].items()]
    zones = [(z[0] - tol, z[1] + tol, z[2] - tol, z[3] + tol) for z in F["zones"].values()]
    out = []
    for i in range(len(rects)):
        q = tuple(rects[i])
        hit = None
        for n, f in own:
            if _inside(q, f, tol):
                hit = n
                break
        if hit != None:
            notes.append("%s on %s: its rect %d is the floor's %s, left to it" % (key, F["key"], i + 1, hit))
            if absorbed != None and hit in F["fixed"]:
                absorbed.append(hit)
            continue
        for n, f in own:
            if _overlaps(q, f):
                q2 = _cut(q, f)
                if q2 != None:
                    notes.append("%s on %s: the %s cut off its rect %d" % (key, F["key"], n, i + 1))
                    q = q2
                    if absorbed != None and n in F["fixed"]:
                        absorbed.append(n)
        if i > 0 and not _rect_in_union(q, zones):
            notes.append("%s on %s: its rect %d lies outside the floor's zones, dropped" % (key, F["key"], i + 1))
            continue
        out.append(q)
    for _ in range(len(out)):
        done = True
        for i in range(len(out)):
            for j in range(i + 1, len(out)):
                u = _join(out[i], out[j])
                if u != None:
                    out = out[:i] + [u] + out[i + 1:j] + out[j + 1:]
                    notes.append("%s on %s: two of its rects joined into one" % (key, F["key"]))
                    done = False
                    break
            if not done:
                break
        if done:
            break
    return out

def drawn_layout(spec, plan, *, floors = None):
    """A drawn plan read against a problem: (spec2, layout, notes). spec2 is the problem with each room's pieces
    matched to the drawing -- a room drawn with k rects gets k-1 pieces, each flush on the rect it touches, and an
    L-piece the problem allows but the drawing lacks stays nothing; layout is {floor: {room or piece: rect}};
    notes name what the drawing has that the problem does not, and the reverse. Rooms are matched by floor and key.
    Each room's rects are read against its floor first: what is the floor's own fixed room or obstacle (a core's
    lobby drawn into a hall) is left to it, an obstacle crossing a rect along one side is cut off it, a piece
    outside the floor's zones is dropped, and pieces that join into one rectangle are joined. A room drawn over a
    fixed room (a rect inside it, or one it crosses) is open to it in spec2: no wall between them, where any other
    room keeps its partition off a fixed room."""
    drawn = {}
    for r in plan.get("rooms", []):
        drawn[(r["floor"], r["key"])] = r
    rooms = []
    lay = {}
    notes = []
    keys = {}
    for r in spec["rooms"]:
        fl = r["floor"]
        keys[(fl, r["key"])] = True
        if floors != None and fl not in floors:
            rooms.append(r)
            continue
        d = drawn.get((fl, r["key"]))
        if d == None:
            notes.append("%s on %s is not in the drawing: the solver places it" % (r["key"], fl))
            rooms.append(r)
            continue
        if fl not in lay:
            lay[fl] = {}
        Fl = [F for F in spec["floors"] if F["key"] == fl][0]
        absorbed = []
        rects = _clean_rects(Fl, r["key"], [tuple(x) for x in d["rects"]], notes, absorbed = absorbed) if r["fixed"] == None else [tuple(x) for x in d["rects"]]
        q = dict(r)
        q["absorbed"] = absorbed  # the floor's fixed rooms the drawing drew into this room: it is open to them
        ab = []
        for f in absorbed:
            if f not in r["open_to"] and f not in ab:
                ab.append(f)
        if ab:
            # no partition between them (a fixed room keeps a room's gap otherwise), and its L-pieces with it
            q["open_to"] = list(r["open_to"]) + ab
            notes.append("%s on %s is drawn over the floor's %s: open to %s, no wall between" % (
                r["key"], fl, ", ".join(ab), "it" if len(ab) == 1 else "them"))
        if r["fixed"] != None:
            lay[fl][r["key"]] = r["fixed"]
            rooms.append(q)
            continue
        lay[fl][r["key"]] = rects[0]
        if len(rects) > 1:
            pcs = []
            placed = [(r["key"], rects[0])]
            for i in range(1, len(rects)):
                rr = rects[i]
                of = None
                side = None
                for pk, pr in placed:
                    sd = _flush_side(rr, pr)
                    if sd != None:
                        of = pk
                        side = sd
                        break
                if of == None:
                    of = r["key"]
                    notes.append("%s on %s: its rect %d touches none of its others" % (r["key"], fl, i + 1))
                pk = "%s_%s" % (r["key"], _LETTERS[i - 1])
                ext = max(rr[1] - rr[0], rr[3] - rr[2], 40.0)
                pcs.append({"key": pk, "of": of, "side": side, "w": (0.0, ext), "d": (0.0, ext), "min": 0.0})
                placed.append((pk, rr))
                lay[fl][pk] = rr
            q["pieces"] = pcs
            q["annex"] = None
        elif r.get("annex") != None:
            side = r["annex"]["side"]
            lay[fl][r["key"] + "_a"] = _parked(rects[0], side)
        rooms.append(q)
    fls = [F["key"] for F in spec["floors"] if floors == None or F["key"] in floors]
    for r in plan.get("rooms", []):
        if r["floor"] in fls and (r["floor"], r["key"]) not in keys and r["kind"] not in ("stair", "lift", "lobby"):
            notes.append("%s on %s is drawn but not in the problem: ignored" % (r["key"], r["floor"]))
    out = dict(spec)
    out["rooms"] = rooms
    return out, lay, notes

def _door_tree_fix(fl, F, rooms, by, doors, drawn_kinds, fix, notes):
    """The drawn doors are a graph; the model's door tree takes, for each room, its neighbour on its shortest way to
    the floor's roots through them (breadth first, deterministic). A drawn door to the stair (the core) counts as one
    to the floor's first root (a lift's door is its landing, which a must-hold holds), and a room the drawing drew over a fixed room (the lobby inside a hall) as open to
    it, and a drawn door within a wall of that part (its `at`) opens onto the fixed room. A room whose way runs
    through a door the problem does not offer it (not in opens_to) is left to the solver."""
    if not F["roots"]:
        return
    def node(x):
        if x in by and by[x]["annex_of"] == None:
            return x
        if x in F["fixed"]:
            return "@" + x
        if drawn_kinds.get((fl, x)) in ("stair", "lobby"):
            return "@" + F["roots"][0]  # a door to the drawn stair is one to the core (a lift's door is its landing, a must-hold)
        return None
    adj = {}
    def link(a, b):
        if a == None or b == None or a == b:
            return
        adj.setdefault(a, {})[b] = True
        adj.setdefault(b, {})[a] = True
    absorbed_of = {}
    for r in rooms:
        if r.get("absorbed"):
            absorbed_of[r["key"]] = r["absorbed"]
    def end(x, at):
        # a door on the part of a room the drawing drew over a fixed room (the lobby inside a hall) opens onto it
        if at != None and x in absorbed_of:
            for f in absorbed_of[x]:
                q = F["fixed"][f]["rect"]
                du = max(q[0] - at[0], 0.0, at[0] - q[1])
                dv = max(q[2] - at[1], 0.0, at[1] - q[3])
                if du <= 0.6 and dv <= 0.6:
                    return "@" + f
        return node(x)
    for d in doors:
        if d.get("floor") != fl:
            continue
        at = d.get("at")
        link(end(d["a"], at), end(d["b"], at))
    for r in rooms:
        for f in r.get("absorbed") or []:
            link(r["key"], "@" + f)
    parent = {}
    seen = {}
    frontier = []
    for rt in F["roots"]:
        seen["@" + rt] = True
        frontier.append("@" + rt)
    for _ in range(len(rooms) + len(F["fixed"]) + 2):
        nxt = []
        for x in frontier:
            for y in sorted(adj.get(x, {}).keys()):
                if y in seen:
                    continue
                seen[y] = True
                parent[y] = x
                nxt.append(y)
        if not nxt:
            break
        frontier = nxt
    for r in rooms:
        k = r["key"]
        if r["annex_of"] != None or not r["opens_to"]:
            continue
        p = parent.get(k)
        if p == None:
            notes.append("%s on %s: no drawn way to the stair through doors; its door is left to the solver" % (k, fl))
            continue
        target = p[1:] if p.startswith("@") else p
        if target not in r["opens_to"]:
            notes.append("%s on %s: its drawn way to the stair runs through %s, not among its opens_to; left to the solver" % (k, fl, target))
            continue
        for c in r["opens_to"]:
            kc = c if c in by else "@" + c
            fix["%s:%s=>%s" % (fl, k, kc)] = 1 if c == target else 0

def arrangement_fix(spec, lay, *, doors = None, windows = None, floors = None, tol = 0.005, kinds = None):
    """The arrangement of a layout as binary values, ({name: 0 or 1}, notes): the zone each room is in, the sides
    each pair is apart on (a side that does not hold is 0; the only one that does is 1; two that hold stay free), an
    L-piece's existence and wall, each room's door parent (from a plan's `doors`: each room's first step toward the
    stair through them; `kinds` {(floor, key): kind} names the drawing's stair and lift rooms) and escape wall (from
    its egress `windows`). Build with fix= this and the solver sizes the rooms in the same arrangement. Pure: no problem made.
    `tol` is the drawing's own (1/16 in): a pair drawn with its wall to within it is apart on that side."""
    fix = {}
    notes = []
    shared_list = list(spec["shared"]) + [c for c in spec["courts"] if c.get("rect") == None]
    drawn_kinds = dict(kinds or {})
    egress_of = {}
    for w in (windows or []):
        if w.get("egress"):
            egress_of[(w.get("floor"), w["room"])] = w
    for F in spec["floors"]:
        fl = F["key"]
        if floors != None and fl not in floors:
            continue
        L = lay.get(fl)
        if L == None:
            continue
        rooms = _floor_rooms(spec, fl, shared_list)
        by = {r["key"]: r for r in rooms}
        an = {}
        for r in rooms:
            if r["annex_of"] != None:
                an.setdefault(r["annex_of"], []).append(r["key"])
        P = {"by": {fl: by}, "F": {fl: F}, "annexes": {fl: an}}
        present = {}
        for r in rooms:
            q = L.get(r["key"])
            if q != None and (r["annex_of"] == None or _area(q) > 0.05):
                present[r["key"]] = q
        for r in rooms:
            k = r["key"]
            if k not in present or r["fixed"] != None:
                continue
            zn = _allowed(F, r)
            if len(zn) > 1:
                inside = [z for z in zn if _inside(present[k], F["zones"][z])]
                if inside:
                    for z in zn:
                        fix["%s:%s@%s" % (fl, k, z)] = 1 if z == inside[0] else 0
                else:
                    notes.append("%s on %s lies in none of its zones" % (k, fl))
        others = [("#" + n, o["rect"]) for n, o in F["obstacles"].items()] + [("@" + n, o["rect"]) for n, o in F["fixed"].items()]
        for i in range(len(rooms)):
            a = rooms[i]["key"]
            if a not in present:
                continue
            items = [(rooms[j]["key"], None) for j in range(i + 1, len(rooms))] + others
            for b, R in items:
                if R == None and b not in present:
                    continue
                g = _gap(P, fl, a, b)  # an obstacle's 0, a fixed room's a room's
                held = _held_sides(present[a], R if R != None else present[b], g, tol)
                if not held:
                    if R == None or g > 0.0 or _overlaps(present[a], R):
                        notes.append("%s and %s on %s are closer than their wall (%s) or overlap" % (a, b.replace("#", "").replace("@", ""), fl, ftin(g)))
                    continue
                for s in _SIDES:
                    nm = "%s:%s|%s:%s" % (fl, a, b, s)
                    if s not in held:
                        fix[nm] = 0
                    elif len(held) == 1:
                        fix[nm] = 1
        for r in rooms:
            if r["annex_of"] == None:
                continue
            k = r["key"]
            q = L.get(k)
            if q == None:
                continue
            exists = _area(q) > 0.05
            fix["%s:%s exists" % (fl, k)] = 1 if exists else 0
            side = r["annex_side"]
            if exists and side == None:
                side = _flush_side(q, L.get(r["annex_of"]) or q)
            if side != None:
                for s in _SIDES:
                    fix["%s:%s on %s:%s" % (fl, k, r["annex_of"], s)] = 1 if s == side else 0
        if doors != None:
            _door_tree_fix(fl, F, rooms, by, doors, drawn_kinds, fix, notes)
        for r in rooms:
            k = r["key"]
            w = egress_of.get((fl, k)) or egress_of.get((None, k))
            if w == None or not r["sleeping"]:
                continue
            hit = None
            for i in range(len(F["egress"])):
                wl = F["egress"][i]
                if wl["side"] != w["wall"] or not _isnum(wl["at"]):
                    continue
                c = w["at"][0] if wl["side"] in ("W", "E") else w["at"][1]
                t = w["at"][1] if wl["side"] in ("W", "E") else w["at"][0]
                if abs(c - wl["at"]) <= 0.6 and t >= wl["span"][0] - tol and t <= wl["span"][1] + tol:
                    hit = i
                    break
            if hit == None:
                notes.append("%s on %s: its escape window is on none of the floor's escape walls" % (k, fl))
                continue
            for i in range(len(F["egress"])):
                fix["%s:%s.egress%d" % (fl, k, i)] = 1 if i == hit else 0
    return fix, notes
