"""@cadpole/plan.star -- a floor plan as plain data, and the queries every @cadpole/plan* module shares.

    load("@cadpole/plan.star", "plan", "site", "floor", "room", "door", "window", "fixture", "stack", "court",
         "wall", "bearing", "normalize", "check_record", "validate", "validate_rows",
         "get_room", "get_floor", "rooms_on", "area", "width", "depth", "dims", "bbox",
         "shared_walls", "neighbours", "door_graph", "door_path", "exterior_walls", "room_local",
         "door_between", "window_on", "walk_length", "clear_width", "rooms_crossed", "plan_diff", "ftin", "sqft", "toy_house")

A plan is dicts, lists, tuples, numbers and strings and nothing else, so any module can build one or read one
without loading this file. This module gives:

* **constructors** that fill the defaults and refuse a bad record with a message naming it: plan, site, floor,
  room, door, window, fixture, stack, court, wall, bearing; `normalize(p)` fills the same defaults in a plan built
  by hand, and `check_record(what, rec)` is the refusal as a list of messages (the constructors fail on its first);
* **validate(p)**: every problem in a plan, however it was built, as sentences; `validate_rows(p)` the same as check
  rows (key, name, ok, detail, rule, severity, floor, room, at | rect) for the red pen;
* **queries**: get_room, get_floor, rooms_on, area (sq ft), width, depth, dims, clear_width, bbox, shared_walls,
  neighbours, door_graph, door_path (kinds and rooms to avoid, units to stay in), walk_length, exterior_walls (what
  each piece faces, and its facade), rooms_crossed, plan_diff and room_local, plus doors_of, windows_of, fixtures_of,
  stacks_on, courts_on, floors_by_z, kind_group, is_outside, is_habitable, is_sleeping;
* **placing**: door_between and window_on make a door or window record on a wall the plan already has;
* **formatting**: ftin (2'-6", or 2'-11 15/16" to the sixteenth; down=True never rounds a clear width up), sqft
  (1,234 sq ft), size_text (11'-6" x 13'-6");
* **toy_house()**: a synthetic two-floor 25 x 60 ft house for tests and examples (no real site in it).

It records nothing when loaded or called: no feature, no parameter, no export. Its purity probe is `ftin`.

## Frame and units

* Every length is in **feet** (a float) and every area in **square feet**. `@cadpole/building.star`'s ft() turns a
  length into Cadpole's millimetres, only where a module makes geometry.
* **u runs east, v north.** A rect is `(u0, u1, v0, v1)` at the inside faces, u0 < u1 and v0 < v1. A room's first
  rect is its main one; an L-shaped room has a second that touches it.
* A wall is named by the side of the room it bounds: "N", "S", "E" or "W". Along a wall, positions run west to east
  on an N or S wall and south to north on an E or W wall; "lo" is the lower-coordinate end.

## Tolerances

* `WALL_GAP` = 1.0 ft: two rooms share a wall when their facing inside faces are 0 to 1'-0" apart. Partitions run
  4 to 7 1/4 in (0.33 to 0.6 ft); the rest of the foot takes 8 to 12-in concrete, party and fire walls. Every query
  that looks across a wall takes `gap=`.
* `TOL` = 0.05 ft (5/8 in): a face may run this far into its neighbour, and a door or window this far past its
  wall's end, and still count. validate() calls two rooms overlapping when they run into each other more than this
  both ways.
* `STACK_REACH` = 0.6 ft: a stack belongs to a room when it is within 7 1/4 in of it (in its wall).
* `COURT_REACH` = 2.5 ft: an exterior wall faces a court, well or slot whose edge is within 2'-6" of the room's
  inside face (the wall between them up to about 2 ft thick).
* Exterior walls: against the floor's `plate` when it has one (the side is exterior where the point WALL_GAP beyond
  it is outside the plate), else derived (no other room within WALL_GAP beyond it). Outdoor rooms (deck, court,
  well) never cover a wall. Pieces shorter than WALL_GAP, which are where partitions end, are dropped.
"""

# ---------------------------------------------------------------------------------------------------------- vocabulary

WALL_GAP = 1.0
TOL = 0.05
STACK_REACH = 0.6
COURT_REACH = 2.5
DOOR_WIDTH = 2.67  # 2'-8" clear

KIND_GROUPS = {
    "living": ["living", "dining", "family", "den", "study", "office", "play", "media", "guest", "bed"],
    "wet": ["kitchen", "bath", "powder", "wc", "laundry", "pantry"],
    "circulation": ["hall", "entry", "foyer", "mud", "stair", "lift", "landing", "lobby"],
    "storage": ["closet", "storage", "mech", "garage", "workshop"],
    "outdoor": ["deck", "court", "well"],
    "other": ["other"],
}
GROUPS = ["living", "wet", "circulation", "storage", "outdoor", "other"]
KINDS = [k for g in GROUPS for k in KIND_GROUPS[g]]
PLUMBED = ["kitchen", "bath", "powder", "wc", "laundry"]  # the wet kinds that sit on a stack
SIDES = ["N", "S", "E", "W"]
SWINGS = ["a", "b", "slide", "pocket", "open"]
HINGES = ["lo", "hi"]
SOURCES = ["street", "rear", "side", "deck", "roof"]
SOURCE_PREFIXES = ["court:", "well:", "slot:"]
COURT_KINDS = ["court", "well", "slot"]
WALL_KINDS = ["exterior", "partition", "bearing", "concrete", "party", "fire"]

_GROUP_OF = {k: g for g in GROUPS for k in KIND_GROUPS[g]}
_OPP = {"N": "S", "S": "N", "E": "W", "W": "E"}
_RECORDS = ["plan", "site", "floor", "room", "door", "window", "fixture", "furniture", "stack", "court", "wall", "bearing"]

def kind_group(kind):
    """The group a room kind is in: "living", "wet", "circulation", "storage", "outdoor" or "other"; None if unknown."""
    return _GROUP_OF.get(kind)

def default_habitable(kind):
    """True for the living kinds and the kitchen."""
    return _GROUP_OF.get(kind) == "living" or kind == "kitchen"

def default_sleeping(kind):
    """True for a bedroom and a guest room."""
    return kind == "bed" or kind == "guest"

def is_outside(b):
    """Whether a door's far side is outside: "outside", or "outside:street", "outside:yard", "outside:deck", ..."""
    return type(b) == "string" and (b == "outside" or b.startswith("outside:"))

def is_source(s):
    """Whether s is a window source: street, rear, side, deck, roof, or court:/well:/slot: and a court's key."""
    if type(s) != "string":
        return False
    if s in SOURCES:
        return True
    for p in SOURCE_PREFIXES:
        if s.startswith(p) and len(s) > len(p):
            return True
    return False

# ---------------------------------------------------------------------------------------------------------- small helpers

def _num(x):
    t = type(x)
    return t == "int" or t == "float"

def _seq(x):
    t = type(x)
    return t == "list" or t == "tuple"

def _is_point(x):
    return _seq(x) and len(x) == 2 and _num(x[0]) and _num(x[1])

def _g(x):
    """A plan coordinate as text: 14.4, 14, 0.33."""
    s = format(float(x), ".2f")
    if "." in s:
        s = s.rstrip("0").rstrip(".")
    if s == "-0":
        s = "0"
    return s

def _rs(r):
    return "(%s, %s, %s, %s)" % (_g(r[0]), _g(r[1]), _g(r[2]), _g(r[3]))

def _q(x):
    if type(x) == "string":
        return '"%s"' % x
    return str(x)

def _who(what, rec):
    """How a message names a record: room "bed2", door "d_bed2", wall on L1 from (1, 2) to (3, 2)."""
    if type(rec) != "dict":
        return what
    k = rec.get("key")
    if type(k) == "string" and k:
        return '%s "%s"' % (what, k)
    if what == "wall" and _is_point(rec.get("a")) and _is_point(rec.get("b")):
        a, b = rec["a"], rec["b"]
        return "wall on %s from (%s, %s) to (%s, %s)" % (rec.get("floor"), _g(a[0]), _g(a[1]), _g(b[0]), _g(b[1]))
    if what == "bearing":
        return "bearing line on %s at %s %s" % (rec.get("floor"), "u" if rec.get("axis") == "v" else "v", str(rec.get("at")))
    if what == "furniture":
        return 'furniture "%s" in room "%s"' % (rec.get("kind"), rec.get("room"))
    if what == "plan":
        return 'plan "%s"' % rec.get("name")
    return what

def rect_problem(r, what = "rect"):
    """None when r is a rect (four numbers, u0 < u1 and v0 < v1), else why not, naming `what`."""
    if not _seq(r):
        return "%s: a rect is a tuple (u0, u1, v0, v1) in feet, not a %s" % (what, type(r))
    if len(r) != 4:
        return "%s: a rect is 4 numbers (u0, u1, v0, v1), not %d" % (what, len(r))
    for x in r:
        if not _num(x):
            return "%s: rect %s holds %s, which is not a number" % (what, str(r), _q(x))
    if r[0] >= r[1]:
        return "%s: rect %s is in the wrong order: u0 %s is not less than u1 %s (a rect is (u0, u1, v0, v1))" % (
            what,
            _rs(r),
            _g(r[0]),
            _g(r[1]),
        )
    if r[2] >= r[3]:
        return "%s: rect %s is in the wrong order: v0 %s is not less than v1 %s (a rect is (u0, u1, v0, v1))" % (
            what,
            _rs(r),
            _g(r[2]),
            _g(r[3]),
        )
    return None

def _rect(r):
    return (float(r[0]), float(r[1]), float(r[2]), float(r[3]))

def _pt(p):
    return (float(p[0]), float(p[1]))

def _rect_list(rects, rect):
    """rects= as a list of rects, or one rect; rect= as one rect."""
    if rect != None:
        return [rect]
    if rects == None:
        return []
    if _seq(rects) and len(rects) == 4 and _num(rects[0]):
        return [rects]
    if _seq(rects):
        return list(rects)
    return [rects]

def _refuse(what, rec):
    ps = check_record(what, rec)
    if ps:
        more = "" if len(ps) == 1 else " (and %d more: %s)" % (len(ps) - 1, "; ".join(ps[1:]))
        fail("plan.%s: %s%s" % (what, ps[0], more))

def _norm_rects(rs):
    out = []
    for r in rs:
        out.append(_rect(r) if rect_problem(r) == None else r)
    return out

# ---------------------------------------------------------------------------------------------------------- record checks

def _need_str(out, who, rec, field):
    v = rec.get(field)
    if type(v) != "string" or v == "":
        out.append("%s: %s is a non-empty string, not %s" % (who, field, _q(v)))

def _opt_str(out, who, rec, field):
    v = rec.get(field)
    if v != None and type(v) != "string":
        out.append("%s: %s is a string or None, not %s" % (who, field, _q(v)))

def _opt_bool(out, who, rec, field):
    v = rec.get(field)
    if v != None and type(v) != "bool":
        out.append("%s: %s is True or False, not %s" % (who, field, _q(v)))

def _pos(out, who, rec, field, required = True):
    v = rec.get(field)
    if v == None and not required:
        return
    if not _num(v) or v <= 0:
        out.append("%s: %s is a length in feet, more than 0, not %s" % (who, field, _q(v)))

def _check_plan(p):
    who = _who("plan", p)
    out = []
    if type(p.get("name")) != "string":
        out.append("%s: name is a string, not %s" % (who, _q(p.get("name"))))
    if p.get("unit", "ft") != "ft":
        out.append('%s: unit is "ft" (every length in feet), not %s' % (who, _q(p.get("unit"))))
    for f in ["floors", "rooms", "doors", "windows", "fixtures", "furniture", "stacks", "courts", "walls", "bearing"]:
        v = p.get(f, [])
        if not _seq(v):
            out.append("%s: %s is a list, not %s" % (who, f, type(v)))
    if type(p.get("site", {})) != "dict":
        out.append("%s: site is a dict, not %s" % (who, type(p.get("site"))))
    if type(p.get("meta", {})) != "dict":
        out.append("%s: meta is a dict, not %s" % (who, type(p.get("meta"))))
    return out

def _check_site(s):
    who = "site"
    out = []
    if s.get("lot") != None:
        m = rect_problem(s["lot"], "site lot")
        if m:
            out.append(m)
    lp = s.get("lot_poly")
    if lp != None:
        if not _seq(lp) or len(lp) < 3 or not all([_is_point(x) for x in lp]):
            out.append("site: lot_poly is a list of 3 or more (u, v) points")
    if s.get("street", "S") not in SIDES:
        out.append('site: street is the side the public way is on, "N", "S", "E" or "W", not %s' % _q(s.get("street")))
    if not _num(s.get("north_deg", 0.0)):
        out.append("site: north_deg is a number of degrees, not %s" % _q(s.get("north_deg")))
    sb = s.get("setbacks")
    if sb != None:
        if type(sb) != "dict":
            out.append("site: setbacks is a dict of feet (front, rear, east, west), not %s" % type(sb))
        else:
            for k, v in sb.items():
                if not _num(v):
                    out.append("site: setback %s is a number of feet, not %s" % (k, _q(v)))
    nb = s.get("neighbors")
    if nb != None:
        if not _seq(nb):
            out.append("site: neighbors is a list of {rect, height}")
        else:
            for i, n in enumerate(nb):
                if type(n) != "dict":
                    out.append("site: neighbor %d is a dict {rect, height}" % i)
                    continue
                m = rect_problem(n.get("rect"), "site neighbor %d" % i)
                if m:
                    out.append(m)
                if n.get("height") != None and not _num(n.get("height")):
                    out.append("site: neighbor %d's height is a number of feet" % i)
    return out

def _check_floor(f):
    who = _who("floor", f)
    out = []
    _need_str(out, who, f, "key")
    if f.get("name") != None and type(f.get("name")) != "string":
        out.append("%s: name is a string, not %s" % (who, _q(f.get("name"))))
    if not _num(f.get("z", 0.0)):
        out.append("%s: z is the floor's elevation in feet, not %s" % (who, _q(f.get("z"))))
    _pos(out, who, f, "ceiling", required = False)
    pl = f.get("plate")
    if pl != None:
        if not _seq(pl):
            out.append("%s: plate is a list of rects (its built footprint, inside faces), not %s" % (who, type(pl)))
        else:
            for i, r in enumerate(_rect_list(pl, None)):
                m = rect_problem(r, "%s plate rect %d" % (who, i))
                if m:
                    out.append(m)
    return out

def _check_room(r):
    who = _who("room", r)
    out = []
    _need_str(out, who, r, "key")
    _need_str(out, who, r, "floor")
    k = r.get("kind")
    if k not in _GROUP_OF:
        out.append("%s: unknown kind %s; the kinds are %s" % (who, _q(k), ", ".join(KINDS)))
    rs = r.get("rects")
    if not _seq(rs) or len(rs) == 0:
        out.append("%s: rects is a list of one or more rects (u0, u1, v0, v1), not %s" % (who, _q(rs)))
    else:
        for i, x in enumerate(rs):
            m = rect_problem(x, "%s rect %d" % (who, i))
            if m:
                out.append(m)
    if r.get("unit", "main") != None and type(r.get("unit", "main")) != "string":
        out.append('%s: unit is the dwelling unit, a string ("main", "unit2", ...), not %s' % (who, _q(r.get("unit"))))
    _opt_bool(out, who, r, "habitable")
    _opt_bool(out, who, r, "sleeping")
    _opt_bool(out, who, r, "kept")
    _pos(out, who, r, "ceiling", required = False)
    _opt_str(out, who, r, "core")
    _opt_str(out, who, r, "furnish")
    _opt_str(out, who, r, "name")
    return out

def _check_door(d):
    who = _who("door", d)
    out = []
    _need_str(out, who, d, "key")
    _need_str(out, who, d, "floor")
    _need_str(out, who, d, "a")
    _need_str(out, who, d, "b")
    if d.get("a") != None and d.get("a") == d.get("b"):
        out.append("%s: a and b are the same room, %s" % (who, _q(d.get("a"))))
    if d.get("wall") not in SIDES:
        out.append('%s: wall is the wall of room a it is in, "N", "S", "E" or "W", not %s' % (who, _q(d.get("wall"))))
    at = d.get("at")
    if at == None:
        if d.get("span") == None:
            out.append("%s: needs at=(u, v), the opening's centre, or span=(lo, hi) where a solver may put it" % who)
    elif not _is_point(at):
        out.append("%s: at is the opening's centre (u, v) in feet, not %s" % (who, _q(at)))
    _pos(out, who, d, "width", required = False)
    if d.get("swing", "a") not in SWINGS:
        out.append('%s: swing is "a" (into room a), "b", "slide", "pocket" or "open", not %s' % (who, _q(d.get("swing"))))
    if d.get("hinge", "lo") not in HINGES:
        out.append('%s: hinge is "lo" or "hi" (the end along the wall), not %s' % (who, _q(d.get("hinge"))))
    sp = d.get("span")
    if sp != None and not (_is_point(sp) and sp[0] < sp[1]):
        out.append("%s: span is (lo, hi) along the wall, lo < hi, not %s" % (who, _q(sp)))
    return out

def _check_window(w):
    who = _who("window", w)
    out = []
    _need_str(out, who, w, "key")
    _need_str(out, who, w, "floor")
    _need_str(out, who, w, "room")
    src = w.get("source", "side")
    if not is_source(src):
        out.append('%s: source is "street", "rear", "side", "deck", "roof", or "court:", "well:" or "slot:" and a court\'s key, not %s' % (who, _q(src)))
    if src != "roof" and w.get("wall") not in SIDES:
        out.append('%s: wall is the room\'s wall it is in, "N", "S", "E" or "W", not %s' % (who, _q(w.get("wall"))))
    if not _is_point(w.get("at")):
        out.append("%s: at is the opening's centre (u, v) in feet, not %s" % (who, _q(w.get("at"))))
    _pos(out, who, w, "width", required = False)
    sill, head = w.get("sill", 2.0), w.get("head", 7.0)
    if not _num(sill) or not _num(head):
        out.append("%s: sill and head are heights in feet above the room's floor" % who)
    elif head <= sill:
        out.append("%s: head %s is not above sill %s" % (who, _g(head), _g(sill)))
    _opt_bool(out, who, w, "egress")
    cl = w.get("clear")
    if cl != None and not (_is_point(cl) and cl[0] > 0 and cl[1] > 0):
        out.append("%s: clear is the net clear opening (w, h) in feet, not %s" % (who, _q(cl)))
    return out

def _check_fixture(f):
    who = _who("fixture", f)
    out = []
    _need_str(out, who, f, "key")
    _need_str(out, who, f, "room")
    _need_str(out, who, f, "kind")
    m = rect_problem(f.get("rect"), who)
    if m:
        out.append(m)
    if f.get("front", "N") not in SIDES:
        out.append('%s: front is the side it is used from, "N", "S", "E" or "W", not %s' % (who, _q(f.get("front"))))
    return out

def _check_furniture(f):
    who = _who("furniture", f)
    out = []
    _need_str(out, who, f, "room")
    _need_str(out, who, f, "kind")
    m = rect_problem(f.get("rect"), who)
    if m:
        out.append(m)
    if f.get("front") != None and f.get("front") not in SIDES:
        out.append('%s: front is "N", "S", "E" or "W", not %s' % (who, _q(f.get("front"))))
    return out

def _check_stack(s):
    who = _who("stack", s)
    out = []
    _need_str(out, who, s, "key")
    if not _is_point(s.get("at")):
        out.append("%s: at is a point (u, v) in feet, not %s" % (who, _q(s.get("at"))))
    fl = s.get("floors")
    if not _seq(fl) or len(fl) == 0 or not all([type(x) == "string" for x in fl]):
        out.append("%s: floors is a list of the floor keys it passes, not %s" % (who, _q(fl)))
    if type(s.get("kind", "dwv")) != "string":
        out.append('%s: kind is a string ("dwv"), not %s' % (who, _q(s.get("kind"))))
    return out

def _check_court(c):
    who = _who("court", c)
    out = []
    _need_str(out, who, c, "key")
    if c.get("kind") not in COURT_KINDS:
        out.append('%s: kind is "court", "well" or "slot", not %s' % (who, _q(c.get("kind"))))
    m = rect_problem(c.get("rect"), who)
    if m:
        out.append(m)
    _need_str(out, who, c, "base")
    if not _num(c.get("top")):
        out.append("%s: top is the elevation of the open sky above it, in feet, not %s" % (who, _q(c.get("top"))))
    if c.get("reaches") not in [None, "street", "rear"]:
        out.append('%s: reaches is "street", "rear" or None, not %s' % (who, _q(c.get("reaches"))))
    return out

def _check_wall(w):
    who = _who("wall", w)
    out = []
    _need_str(out, who, w, "floor")
    if not _is_point(w.get("a")) or not _is_point(w.get("b")):
        out.append("%s: a and b are its end points (u, v)" % who)
    elif w["a"][0] == w["b"][0] and w["a"][1] == w["b"][1]:
        out.append("%s: a and b are the same point" % who)
    _pos(out, who, w, "t", required = False)
    if w.get("kind", "partition") not in WALL_KINDS:
        out.append("%s: kind is one of %s, not %s" % (who, ", ".join(WALL_KINDS), _q(w.get("kind"))))
    return out

def _check_bearing(b):
    who = _who("bearing", b)
    out = []
    _need_str(out, who, b, "floor")
    if b.get("axis") not in ["u", "v"]:
        out.append('%s: axis is "u" (a line running east-west, at a v) or "v" (running north-south, at a u), not %s' % (who, _q(b.get("axis"))))
    if not _num(b.get("at")):
        out.append("%s: at is the line's coordinate in feet, not %s" % (who, _q(b.get("at"))))
    sp = b.get("span")
    if sp != None and not (_is_point(sp) and sp[0] < sp[1]):
        out.append("%s: span is (lo, hi) along the line, lo < hi, not %s" % (who, _q(sp)))
    return out

_CHECKS = {
    "plan": _check_plan,
    "site": _check_site,
    "floor": _check_floor,
    "room": _check_room,
    "door": _check_door,
    "window": _check_window,
    "fixture": _check_fixture,
    "furniture": _check_furniture,
    "stack": _check_stack,
    "court": _check_court,
    "wall": _check_wall,
    "bearing": _check_bearing,
}

def check_record(what, rec):
    """The problems with one record taken alone, as sentences naming it ([] when it is well formed).

    `what` is "plan", "site", "floor", "room", "door", "window", "fixture", "furniture", "stack", "court", "wall" or
    "bearing". References to other records (a door's rooms, a stack's floors) are validate()'s, which sees the plan.
    The constructors fail with the first of these."""
    f = _CHECKS.get(what)
    if f == None:
        fail("plan.check_record: what is one of %s, not %s" % (", ".join(_RECORDS), _q(what)))
    if type(rec) != "dict":
        return ["%s: a record is a dict, not a %s" % (what, type(rec))]
    return f(rec)

# ---------------------------------------------------------------------------------------------------------- constructors

def site(lot = None, *, lot_poly = None, street = "S", north_deg = 0.0, setbacks = None, neighbors = None):
    """The site: the lot (a rect, or lot_poly=[(u, v), ...]), the side the street is on, true north in degrees
    clockwise from +v, and optional setbacks {"front", "rear", "east", "west"} and neighbours [{"rect", "height"}]."""
    rec = {"street": street, "north_deg": north_deg}
    if lot != None:
        rec["lot"] = lot
    if lot_poly != None:
        rec["lot_poly"] = lot_poly
    if setbacks != None:
        rec["setbacks"] = setbacks
    if neighbors != None:
        rec["neighbors"] = neighbors
    _refuse("site", rec)
    if lot != None:
        rec["lot"] = _rect(lot)
    if lot_poly != None:
        rec["lot_poly"] = [_pt(x) for x in lot_poly]
    if setbacks != None:
        rec["setbacks"] = {k: float(v) for k, v in setbacks.items()}
    if neighbors != None:
        rec["neighbors"] = [dict(n, rect = _rect(n["rect"])) for n in neighbors]
    rec["north_deg"] = float(north_deg)
    return rec

def floor(key, *, name = None, z = 0.0, ceiling = 8.0, plate = None):
    """A floor: its key ("L1"), name, elevation z and ceiling height in feet, and optionally its plate, the built
    footprint at the inside faces of its exterior walls, as a list of rects (or one rect). The plate decides which
    room walls are exterior; without one they are derived from the rooms."""
    rec = {"key": key, "name": name if name != None else key, "z": z, "ceiling": ceiling}
    if plate != None:
        rec["plate"] = _rect_list(plate, None)
    _refuse("floor", rec)
    rec["z"] = float(z)
    rec["ceiling"] = float(ceiling)
    if plate != None:
        rec["plate"] = [_rect(r) for r in rec["plate"]]
    return rec

def room(
        key,
        name = None,
        *,
        floor,
        kind,
        rects = None,
        rect = None,
        unit = "main",
        habitable = None,
        sleeping = None,
        kept = False,
        ceiling = None,
        core = None,
        furnish = None,
        **extra):
    """A room: key, name, floor key, kind (KINDS), and its rects -- `rects=[main, l_piece]` or `rect=` for one.

    habitable and sleeping default by kind (the living kinds and the kitchen are habitable; bed and guest sleep);
    unit is the dwelling unit ("main"); kept marks an existing room exempt from design rules; ceiling overrides the
    floor's; rooms of one stair or lift on every floor share a core key (so do a short flight and the room it lands in
    on the next floor up), which door_graph links floor to floor; furnish names the furniture set to hold.
    Any other keyword is kept on the record. Refuses an unknown kind or a bad rect, naming the room."""
    rec = dict(extra)
    rec.update({
        "key": key,
        "name": name if name != None else key,
        "floor": floor,
        "kind": kind,
        "rects": _rect_list(rects, rect),
        "unit": unit,
        "habitable": habitable if habitable != None else default_habitable(kind),
        "sleeping": sleeping if sleeping != None else default_sleeping(kind),
        "kept": kept,
        "ceiling": ceiling,
        "core": core,
        "furnish": furnish,
    })
    _refuse("room", rec)
    rec["rects"] = [_rect(r) for r in rec["rects"]]
    if ceiling != None:
        rec["ceiling"] = float(ceiling)
    return rec

def door(
        key,
        *,
        a,
        b,
        wall,
        at = None,
        floor = None,
        width = DOOR_WIDTH,
        swing = "a",
        hinge = "lo",
        span = None,
        **extra):
    """A door from room a to room b ("outside", "outside:street", ... for an outside door) in room a's `wall`, its
    opening's centre `at` on the wall line, `width` its clear width (2'-8" by default). swing: "a" (the leaf swings
    into a), "b", "slide", "pocket" or "open" (cased, no leaf); hinge "lo" or "hi", the end along the wall the leaf
    hangs from. span=(lo, hi) is where a solver may put it before it is placed. floor defaults to room a's when the
    door goes into plan()."""
    rec = dict(extra)
    rec.update({
        "key": key,
        "floor": floor,
        "a": a,
        "b": b,
        "wall": wall,
        "at": at,
        "width": width,
        "swing": swing,
        "hinge": hinge,
        "span": span,
    })
    probs = [m for m in check_record("door", rec) if floor != None or " floor is " not in m]
    if probs:
        fail("plan.door: " + "; ".join(probs))
    if at != None:
        rec["at"] = _pt(at)
    if span != None:
        rec["span"] = _pt(span)
    rec["width"] = float(width)
    return rec

def window(
        key,
        *,
        room,
        wall,
        at,
        floor = None,
        width = 3.0,
        sill = 2.0,
        head = 7.0,
        source = "side",
        egress = False,
        clear = None,
        **extra):
    """A window in `room`'s `wall`, its centre `at` on the wall line, `width` wide, sill and head above the room's
    floor. source is what it looks onto: "street", "rear", "side", "court:<key>", "well:<key>", "slot:<key>", "deck"
    or "roof" (a skylight, wall=None). egress marks the room's emergency escape opening (R310); clear its net clear
    opening (w, h) when known. floor defaults to the room's when the window goes into plan()."""
    rec = dict(extra)
    rec.update({
        "key": key,
        "floor": floor,
        "room": room,
        "wall": wall,
        "at": at,
        "width": width,
        "sill": sill,
        "head": head,
        "source": source,
        "egress": egress,
        "clear": clear,
    })
    probs = [m for m in check_record("window", rec) if floor != None or " floor is " not in m]
    if probs:
        fail("plan.window: " + "; ".join(probs))
    rec["at"] = _pt(at)
    rec["width"] = float(width)
    rec["sill"] = float(sill)
    rec["head"] = float(head)
    if clear != None:
        rec["clear"] = _pt(clear)
    return rec

def fixture(key, *, room, kind, rect, front = "N", **extra):
    """A plumbing fixture or built-in (kind is free text: "wc", "lav", "tub", "shower", "sink", "range", ...) in
    `room`, its footprint `rect`, used from its `front` side."""
    rec = dict(extra)
    rec.update({"key": key, "room": room, "kind": kind, "rect": rect, "front": front})
    _refuse("fixture", rec)
    rec["rect"] = _rect(rect)
    return rec

def stack(key, *, at, floors, kind = "dwv", **extra):
    """A plumbing stack (or another vertical run: kind "dwv" by default) at (u, v) through the listed floors. Any other
    keyword is kept on the record (an adapter marks a drain it infers: implied=True)."""
    rec = dict(extra)
    rec.update({"key": key, "at": at, "floors": list(floors) if _seq(floors) else floors, "kind": kind})
    _refuse("stack", rec)
    rec["at"] = _pt(at)
    return rec

def court(key, *, kind, rect, base, top, reaches = None, **extra):
    """A court, light well or slot ("court", "well", "slot"): its open rect, the floor it starts at (`base`), the
    elevation of the open sky above it (`top`), and `reaches` "street" or "rear" if it opens to a yard that reaches the
    public way (R310.1), else None."""
    rec = dict(extra)
    rec.update({"key": key, "kind": kind, "rect": rect, "base": base, "top": top, "reaches": reaches})
    _refuse("court", rec)
    rec["rect"] = _rect(rect)
    rec["top"] = float(top)
    return rec

def wall(*, floor, a, b, t = 0.5, kind = "partition"):
    """An explicit wall on `floor` from point a to point b (its centreline), t thick: "exterior", "partition",
    "bearing", "concrete", "party" or "fire". A plan without walls derives them from its rooms."""
    rec = {"floor": floor, "a": a, "b": b, "t": t, "kind": kind}
    _refuse("wall", rec)
    rec["a"] = _pt(a)
    rec["b"] = _pt(b)
    rec["t"] = float(t)
    return rec

def bearing(*, floor, axis, at, span = None):
    """A bearing line on `floor`: axis "v" runs north-south at u = at; axis "u" runs east-west at v = at; span is
    its extent (lo, hi) along itself."""
    rec = {"floor": floor, "axis": axis, "at": at, "span": span}
    _refuse("bearing", rec)
    rec["at"] = float(at)
    if span != None:
        rec["span"] = _pt(span)
    return rec

def _fill_room(r):
    out = dict(r)
    k = out.get("kind")
    if out.get("name") == None:
        out["name"] = out.get("key")
    if "unit" not in out or out["unit"] == None:
        out["unit"] = "main"
    if out.get("habitable") == None and k in _GROUP_OF:
        out["habitable"] = default_habitable(k)
    if out.get("sleeping") == None and k in _GROUP_OF:
        out["sleeping"] = default_sleeping(k)
    if "kept" not in out:
        out["kept"] = False
    for f in ["ceiling", "core", "furnish"]:
        if f not in out:
            out[f] = None
    if _seq(out.get("rects")):
        out["rects"] = _norm_rects(_rect_list(out["rects"], None))
    return out

def normalize(p):
    """A copy of a plan built by hand with every default filled the way the constructors fill them: unit "ft", empty
    lists, rooms' unit/habitable/sleeping/kept, doors' and windows' floors from their rooms, widths, swings and the
    rest. Nothing is checked or refused here: validate() reports."""
    rooms = [_fill_room(r) if type(r) == "dict" else r for r in p.get("rooms", [])]
    floor_of = {r.get("key"): r.get("floor") for r in rooms if type(r) == "dict"}
    doors = []
    for d in p.get("doors", []):
        if type(d) != "dict":
            doors.append(d)
            continue
        x = dict(d)
        if x.get("floor") == None:
            x["floor"] = floor_of.get(x.get("a"))
        x.setdefault("width", DOOR_WIDTH)
        x.setdefault("swing", "a")
        x.setdefault("hinge", "lo")
        x.setdefault("span", None)
        x.setdefault("at", None)
        doors.append(x)
    windows = []
    for w in p.get("windows", []):
        if type(w) != "dict":
            windows.append(w)
            continue
        x = dict(w)
        if x.get("floor") == None:
            x["floor"] = floor_of.get(x.get("room"))
        x.setdefault("width", 3.0)
        x.setdefault("sill", 2.0)
        x.setdefault("head", 7.0)
        x.setdefault("source", "side")
        x.setdefault("egress", False)
        x.setdefault("clear", None)
        windows.append(x)
    floors = []
    for f in p.get("floors", []):
        if type(f) != "dict":
            floors.append(f)
            continue
        x = dict(f)
        x.setdefault("name", x.get("key"))
        x.setdefault("z", 0.0)
        x.setdefault("ceiling", 8.0)
        if x.get("plate") != None and _seq(x["plate"]):
            x["plate"] = _norm_rects(_rect_list(x["plate"], None))
        floors.append(x)
    st = dict(p.get("site") or {})
    st.setdefault("street", "S")
    st.setdefault("north_deg", 0.0)
    out = dict(p)
    out.update({
        "name": p.get("name", "plan"),
        "unit": p.get("unit", "ft"),
        "source": p.get("source", ""),
        "site": st,
        "floors": floors,
        "rooms": rooms,
        "doors": doors,
        "windows": windows,
        "fixtures": list(p.get("fixtures", [])),
        "furniture": list(p.get("furniture", [])),
        "stacks": list(p.get("stacks", [])),
        "courts": list(p.get("courts", [])),
        "walls": list(p.get("walls", [])),
        "bearing": list(p.get("bearing", [])),
        "meta": dict(p.get("meta") or {}),
    })
    return out

def plan(
        name,
        *,
        floors = None,
        rooms = None,
        doors = None,
        windows = None,
        fixtures = None,
        furniture = None,
        stacks = None,
        courts = None,
        walls = None,
        bearing = None,
        site = None,
        source = "",
        meta = None):
    """A plan value from its parts (each a list of records), with the defaults filled (normalize). It checks
    nothing across records: call validate() on it."""
    return normalize({
        "name": name,
        "unit": "ft",
        "source": source,
        "site": site or {},
        "floors": floors or [],
        "rooms": rooms or [],
        "doors": doors or [],
        "windows": windows or [],
        "fixtures": fixtures or [],
        "furniture": furniture or [],
        "stacks": stacks or [],
        "courts": courts or [],
        "walls": walls or [],
        "bearing": bearing or [],
        "meta": meta or {},
    })

# ---------------------------------------------------------------------------------------------------------- geometry kernels

def _side(r, s):
    """Side s of rect r as (line, lo, hi): the face's coordinate across the wall, and its span along it."""
    if s == "N":
        return (r[3], r[0], r[1])
    if s == "S":
        return (r[2], r[0], r[1])
    if s == "E":
        return (r[1], r[2], r[3])
    return (r[0], r[2], r[3])

def _ns(s):
    return s == "N" or s == "S"

def _out(s):
    return 1.0 if (s == "N" or s == "E") else -1.0

def _along(r, s):
    """Rect r's extent along side s's direction."""
    if _ns(s):
        return (r[0], r[1])
    return (r[2], r[3])

def _facing_gap(ra, rb, s):
    """How far rb's near face is beyond ra's side s (negative when rb runs into ra across it)."""
    if s == "N":
        return rb[2] - ra[3]
    if s == "S":
        return ra[2] - rb[3]
    if s == "E":
        return rb[0] - ra[1]
    return ra[0] - rb[1]

def _far(ra, rb, s):
    """How far rb reaches beyond ra's side s."""
    if s == "N":
        return rb[3] - ra[3]
    if s == "S":
        return ra[2] - rb[2]
    if s == "E":
        return rb[1] - ra[1]
    return ra[0] - rb[0]

def _subtract(segs, lo, hi):
    out = []
    for a, b in segs:
        if hi <= a or lo >= b:
            out.append((a, b))
        else:
            if lo > a:
                out.append((a, lo))
            if hi < b:
                out.append((hi, b))
    return out

def _inter(a, b):
    u0, u1 = max(a[0], b[0]), min(a[1], b[1])
    v0, v1 = max(a[2], b[2]), min(a[3], b[3])
    if u1 <= u0 or v1 <= v0:
        return None
    return (u0, u1, v0, v1)

def _union_area(rects):
    if not rects:
        return 0.0
    us = sorted({x: 1 for r in rects for x in [r[0], r[1]]}.keys())
    total = 0.0
    for k in range(len(us) - 1):
        a, b = us[k], us[k + 1]
        iv = sorted([(r[2], r[3]) for r in rects if r[0] <= a and r[1] >= b])
        cov = 0.0
        c0, c1 = None, None
        for lo, hi in iv:
            if c1 == None or lo > c1:
                if c1 != None:
                    cov += c1 - c0
                c0, c1 = lo, hi
            elif hi > c1:
                c1 = hi
        if c1 != None:
            cov += c1 - c0
        total += cov * (b - a)
    return total

def _in_rect(pt, r, tol):
    return pt[0] >= r[0] - tol and pt[0] <= r[1] + tol and pt[1] >= r[2] - tol and pt[1] <= r[3] + tol

def _dist_rect(pt, r):
    du = max(r[0] - pt[0], 0.0, pt[0] - r[1])
    dv = max(r[2] - pt[1], 0.0, pt[1] - r[3])
    return math.sqrt(du * du + dv * dv)

# ---------------------------------------------------------------------------------------------------------- lookups

def _index(p):
    rooms = {}
    on = {}
    for r in p.get("rooms", []):
        if type(r) != "dict" or type(r.get("key")) != "string":
            continue
        if not _seq(r.get("rects")) or any([rect_problem(x) != None for x in r["rects"]]) or len(r["rects"]) == 0:
            continue
        if r["key"] in rooms:
            continue  # a key used twice: the first record is the room (validate says so)
        rooms[r["key"]] = r
        on.setdefault(r.get("floor"), []).append(r)
    floors = {f["key"]: f for f in p.get("floors", []) if type(f) == "dict" and type(f.get("key")) == "string"}
    courts_on = {}
    for c in p.get("courts", []):
        if type(c) != "dict" or rect_problem(c.get("rect")) != None:
            continue
        base = floors.get(c.get("base"))
        for f in floors.values():
            if base != None and _num(f.get("z")) and _num(base.get("z")) and f["z"] >= base["z"] - TOL:
                if not _num(c.get("top")) or f["z"] < c["top"]:
                    courts_on.setdefault(f["key"], []).append(c)
    st = (p.get("site") or {}).get("street", "S")
    return {"rooms": rooms, "on": on, "floors": floors, "courts_on": courts_on, "street": st if st in SIDES else "S"}

def get_room(p, key):
    """The room record with this key; fails naming the plan's rooms when there is none."""
    for r in p.get("rooms", []):
        if r.get("key") == key:
            return r
    fail('plan.get_room: plan "%s" has no room %s (its rooms: %s)' % (p.get("name"), _q(key), ", ".join([str(r.get("key")) for r in p.get("rooms", [])])))

def get_floor(p, key):
    """The floor record with this key; fails naming the plan's floors when there is none."""
    for f in p.get("floors", []):
        if f.get("key") == key:
            return f
    fail('plan.get_floor: plan "%s" has no floor %s (its floors: %s)' % (p.get("name"), _q(key), ", ".join([str(f.get("key")) for f in p.get("floors", [])])))

def get_door(p, key):
    """The door record with this key; fails when there is none."""
    for d in p.get("doors", []):
        if d.get("key") == key:
            return d
    fail('plan.get_door: plan "%s" has no door %s' % (p.get("name"), _q(key)))

def floors_by_z(p):
    """The plan's floors, lowest first."""
    return sorted(p.get("floors", []), key = lambda f: f.get("z", 0.0))

def rooms_on(p, floor, *, kinds = None, unit = None):
    """The rooms on a floor, in plan order; kinds= keeps those kinds (a list, or a group name such as "wet"), unit=
    one dwelling unit."""
    ks = None
    if kinds != None:
        ks = KIND_GROUPS[kinds] if type(kinds) == "string" and kinds in KIND_GROUPS else ([kinds] if type(kinds) == "string" else kinds)
    out = []
    for r in p.get("rooms", []):
        if r.get("floor") != floor:
            continue
        if ks != None and r.get("kind") not in ks:
            continue
        if unit != None and r.get("unit", "main") != unit:
            continue
        out.append(r)
    return out

def doors_of(p, key):
    """The doors into or out of a room (either side)."""
    return [d for d in p.get("doors", []) if d.get("a") == key or d.get("b") == key]

def windows_of(p, key):
    """The windows of a room."""
    return [w for w in p.get("windows", []) if w.get("room") == key]

def fixtures_of(p, key):
    """The fixtures in a room."""
    return [f for f in p.get("fixtures", []) if f.get("room") == key]

def stacks_on(p, floor):
    """The stacks that pass a floor."""
    return [s for s in p.get("stacks", []) if floor in s.get("floors", [])]

def courts_on(p, floor):
    """The courts, wells and slots open at a floor: those whose base floor is at or below it and whose top is above."""
    return _index(p)["courts_on"].get(floor, [])

def is_habitable(r):
    """A room's habitable flag, or its kind's default."""
    h = r.get("habitable")
    return h if h != None else default_habitable(r.get("kind"))

def is_sleeping(r):
    """A room's sleeping flag, or its kind's default."""
    s = r.get("sleeping")
    return s if s != None else default_sleeping(r.get("kind"))

# ---------------------------------------------------------------------------------------------------------- measures

def _rects_of(x, floor = None):
    if type(x) == "dict":
        if "rooms" in x:
            out = []
            for r in x["rooms"]:
                if floor == None or r.get("floor") == floor:
                    out.extend(r.get("rects", []))
            return out
        if "rects" in x:
            return list(x["rects"])
        if "rect" in x:
            return [x["rect"]]
        fail("plan: a room, a plan, a rect or a list of rects, not a dict with keys %s" % ", ".join(x.keys()))
    if _seq(x) and len(x) == 4 and _num(x[0]):
        return [x]
    if _seq(x):
        return list(x)
    fail("plan: a room, a plan, a rect or a list of rects, not %s" % type(x))

def area(x, *, floor = None, kinds = None, unit = None):
    """Square feet: of a rect, a list of rects (their union), a room (its rects' union: an L counts once), or a plan
    (the sum of its rooms' areas; floor=, kinds= (a list or a group name) and unit= filter them)."""
    if type(x) == "dict" and "rooms" in x:
        ks = None
        if kinds != None:
            ks = KIND_GROUPS[kinds] if type(kinds) == "string" and kinds in KIND_GROUPS else ([kinds] if type(kinds) == "string" else kinds)
        total = 0.0
        for r in x["rooms"]:
            if floor != None and r.get("floor") != floor:
                continue
            if ks != None and r.get("kind") not in ks:
                continue
            if unit != None and r.get("unit", "main") != unit:
                continue
            total += _union_area(r["rects"])
        return total
    return _union_area(_rects_of(x))

def _main(x):
    if type(x) == "dict":
        return x["rects"][0]
    return x

def width(x):
    """The main rect's extent east-west (u), in feet: of a room, or of a rect."""
    m = _main(x)
    return m[1] - m[0]

def depth(x):
    """The main rect's extent north-south (v), in feet: of a room, or of a rect."""
    m = _main(x)
    return m[3] - m[2]

def dims(x):
    """(width, depth) of the main rect, in feet."""
    return (width(x), depth(x))

def bbox(x, *, floor = None):
    """(u0, u1, v0, v1) round a rect, a list of rects, a room, or a plan's rooms (on `floor` when given); None when
    there is nothing."""
    rs = _rects_of(x, floor)
    if not rs:
        return None
    return (min([r[0] for r in rs]), max([r[1] for r in rs]), min([r[2] for r in rs]), max([r[3] for r in rs]))

def centre(x):
    """The centre (u, v) of a rect, or of a room's main rect."""
    m = _main(x)
    return ((m[0] + m[1]) / 2.0, (m[2] + m[3]) / 2.0)

# ---------------------------------------------------------------------------------------------------------- walls

def _shared(ra, rb, gap, min_len):
    out = []
    for i, x in enumerate(ra["rects"]):
        for j, y in enumerate(rb["rects"]):
            for s in SIDES:
                g = _facing_gap(x, y, s)
                if g < -TOL or g > gap or _far(x, y, s) <= TOL:
                    continue
                line, lo, hi = _side(x, s)
                ya, yb = _along(y, s)
                slo, shi = max(lo, ya), min(hi, yb)
                if shi - slo <= max(min_len, TOL):
                    continue
                out.append({
                    "a": ra["key"],
                    "b": rb["key"],
                    "wall": s,
                    "line": line,
                    "far": line + _out(s) * g,
                    "gap": g,
                    "span": (slo, shi),
                    "length": shi - slo,
                    "a_rect": i,
                    "b_rect": j,
                })
    return sorted(_merge_runs(out, ["wall", "line", "far"]), key = lambda w: -w["length"])

def _merge_runs(segs, same):
    """Pieces of one wall line that touch end to end (an L's two rects along one face) as one piece: segments agreeing
    on the `same` fields (lines to TOL) whose spans meet within TOL merge; the first keeps its other fields."""
    out = []
    for x in sorted(segs, key = lambda w: (w["wall"], w["line"], w["span"][0])):
        hit = None
        for k in range(len(out)):
            y = out[k]
            ok = True
            for f in same:
                a, b = x[f], y[f]
                if _num(a) and _num(b):
                    if abs(a - b) > TOL:
                        ok = False
                elif a != b:
                    ok = False
            if ok and x["span"][0] <= y["span"][1] + TOL and x["span"][1] >= y["span"][0] - TOL:
                hit = k
        if hit == None:
            out.append(dict(x))
        else:
            y = out[hit]
            lo, hi = min(y["span"][0], x["span"][0]), max(y["span"][1], x["span"][1])
            out[hit] = dict(y, span = (lo, hi), length = hi - lo)
    return out

def shared_walls(p, a, b, *, gap = WALL_GAP, min_len = 0.0):
    """The walls rooms a and b share on their floor (keys, or room dicts), longest first. Each is a dict:
    wall (the side of a), line (a's face), far (b's face), gap (the wall's thickness between the two inside faces,
    from -TOL to `gap`), span (lo, hi) along the wall, length, and a_rect / b_rect (which rects meet). A corner
    that only touches is not a wall. [] when they share none, or are on different floors."""
    ra = a if type(a) == "dict" else get_room(p, a)
    rb = b if type(b) == "dict" else get_room(p, b)
    if ra.get("floor") != rb.get("floor"):
        return []
    return _shared(ra, rb, gap, min_len)

def neighbours(p, key, *, gap = WALL_GAP, min_len = 0.5):
    """The keys of the rooms on the same floor that share at least min_len of wall with this one, in plan order."""
    r = get_room(p, key)
    out = []
    for o in rooms_on(p, r["floor"]):
        if o["key"] != key and _shared(r, o, gap, min_len):
            out.append(o["key"])
    return out

neighbors = neighbours

def _cover(segs, rc, oc, s, gap):
    g = _facing_gap(rc, oc, s)
    if g > gap or _far(rc, oc, s) <= TOL:
        return segs
    a, b = _along(oc, s)
    return _subtract(segs, a, b)

def _plate_cover(segs, line, s, plate, probe):
    at = line + _out(s) * probe
    for pr in plate:
        if _ns(s):
            if pr[2] < at and at < pr[3]:
                segs = _subtract(segs, pr[0], pr[1])
        elif pr[0] < at and at < pr[1]:
            segs = _subtract(segs, pr[2], pr[3])
    return segs

def _label(parts, lo, hi, what):
    out = []
    for a, b, w in parts:
        if w != None or hi <= a or lo >= b:
            out.append((a, b, w))
            continue
        if lo > a:
            out.append((a, lo, None))
        out.append((max(a, lo), min(b, hi), what))
        if hi < b:
            out.append((hi, b, None))
    return out

def _absorb(parts, min_len):
    """A facade remnant shorter than min_len beside a court or deck piece (the end of the wall round it) takes that
    piece's label; then neighbours with one label merge."""
    out = []
    for i in range(len(parts)):
        a, b, w = parts[i]
        if w == None and b - a < min_len:
            if i > 0 and parts[i - 1][2] != None:
                w = parts[i - 1][2]
            elif i + 1 < len(parts) and parts[i + 1][2] != None:
                w = parts[i + 1][2]
        out.append((a, b, w))
    merged = []
    for a, b, w in out:
        if merged and merged[-1][2] == w and a - merged[-1][1] <= TOL:
            merged[-1] = (merged[-1][0], b, w)
        else:
            merged.append((a, b, w))
    return merged

def _faces(idx, r, rc, s, a, b, decks, gap, min_len):
    parts = [(a, b, None)]
    for c in idx["courts_on"].get(r.get("floor"), []):
        g = _facing_gap(rc, c["rect"], s)
        if g < -TOL or g > COURT_REACH or _far(rc, c["rect"], s) <= TOL:
            continue
        ca, cb = _along(c["rect"], s)
        parts = _label(parts, ca, cb, "%s:%s" % (c.get("kind"), c.get("key")))
    for o in decks:
        for oc in o["rects"]:
            g = _facing_gap(rc, oc, s)
            if g < -TOL or g > gap or _far(rc, oc, s) <= TOL:
                continue
            ca, cb = _along(oc, s)
            parts = _label(parts, ca, cb, "deck:" + o["key"])
    st = idx["street"]
    facade = "street" if s == st else ("rear" if s == _OPP[st] else "side")
    return [(x, y, w if w != None else facade) for x, y, w in _absorb(parts, min_len) if y - x > TOL]

def _exterior(idx, r, gap, min_len, mode):
    fl = idx["floors"].get(r.get("floor"))
    plate = fl.get("plate") if fl != None else None
    use_plate = mode == "plate" or (mode == None and plate)
    if mode == "plate" and not plate:
        fail('plan.exterior_walls: floor %s has no plate; mode="derived" or None derives the walls from the rooms' % _q(r.get("floor")))
    same = [o for o in idx["on"].get(r.get("floor"), []) if o["key"] != r["key"]]
    others = [o for o in same if _GROUP_OF.get(o.get("kind")) != "outdoor"]
    decks = [o for o in same if _GROUP_OF.get(o.get("kind")) == "outdoor"]
    out = []
    for i, rc in enumerate(r["rects"]):
        for s in SIDES:
            line, lo, hi = _side(rc, s)
            segs = [(lo, hi)]
            for j, oc in enumerate(r["rects"]):
                if j != i:
                    segs = _cover(segs, rc, oc, s, TOL)
            if use_plate:
                segs = _plate_cover(segs, line, s, plate, gap)
            else:
                for o in others:
                    for oc in o["rects"]:
                        segs = _cover(segs, rc, oc, s, gap)
            for a, b in segs:
                if b - a < min_len:
                    continue
                st = idx["street"]
                facade = "street" if s == st else ("rear" if s == _OPP[st] else "side")
                for x, y, what in _faces(idx, r, rc, s, a, b, decks, gap, min_len):
                    out.append({"room": r["key"], "rect": i, "wall": s, "line": line, "span": (x, y), "length": y - x, "faces": what, "facade": facade})
    merged = _merge_runs(out, ["wall", "line", "faces"])
    order = {s: k for k, s in enumerate(SIDES)}
    return sorted(merged, key = lambda w: (w["rect"], order[w["wall"]], w["span"][0]))

def exterior_walls(p, key, *, gap = WALL_GAP, min_len = None, mode = None):
    """A room's exterior walls: dicts of room, rect (which of its rects), wall (side), line (its inside face's
    coordinate), span (lo, hi) along it, length, and faces -- what is beyond: "street", "rear" or "side" (by the
    side the site's street is on), "court:<key>", "well:<key>" or "slot:<key>" (a court within COURT_REACH), or
    "deck:<key>" (an outdoor room within `gap`); and facade -- "street", "rear" or "side" by direction alone, whatever
    court or deck is beyond (a fire-separation check wants it: a slot is still a side yard).

    mode: "plate" against the floor's plate (the side is exterior where the point `gap` beyond it is outside the
    plate), "derived" (no other indoor room within `gap` beyond it), or None, the plate when the floor has one.
    Pieces shorter than min_len (default `gap`) are where partitions end, and are dropped."""
    idx = _index(p)
    r = idx["rooms"].get(key)
    if r == None:
        r = get_room(p, key)
    return _exterior(idx, r, gap, gap if min_len == None else min_len, mode)

# ---------------------------------------------------------------------------------------------------------- doors as a graph

def _core_links(p, idx):
    """Rooms of one core on consecutive floors (by z), as pairs."""
    zs = {f["key"]: f.get("z", 0.0) for f in p.get("floors", []) if type(f) == "dict"}
    by_core = {}
    for r in idx["rooms"].values():
        c = r.get("core")
        if c != None:
            by_core.setdefault(c, []).append(r)
    out = []
    for c in sorted(by_core.keys()):
        rs = sorted(by_core[c], key = lambda r: zs.get(r.get("floor"), 0.0))
        for k in range(len(rs) - 1):
            out.append((rs[k]["key"], rs[k + 1]["key"], c))
    return out

def door_graph(p, *, floor = None, cores = True):
    """The plan's rooms as a graph: {room key: [edge, ...]} plus the node "outside", where an edge is
    {"to": key, "door": door key or None, "via": "door" | "core:<key>"}. Every outside door ("outside:street", ...)
    meets the one node "outside". cores=True links the rooms of one stair or lift core on consecutive floors (only
    when floor= is None, the whole plan)."""
    idx = _index(p)
    g = {}
    for r in p.get("rooms", []):
        if type(r) == "dict" and (floor == None or r.get("floor") == floor):
            g[r.get("key")] = []
    g["outside"] = []
    for d in p.get("doors", []):
        if floor != None and d.get("floor") != floor:
            continue
        a = d.get("a")
        b = "outside" if is_outside(d.get("b")) else d.get("b")
        if a not in g or b not in g:
            continue
        g[a].append({"to": b, "door": d.get("key"), "via": "door"})
        g[b].append({"to": a, "door": d.get("key"), "via": "door"})
    if cores and floor == None:
        for x, y, c in _core_links(p, idx):
            if x in g and y in g:
                g[x].append({"to": y, "door": None, "via": "core:" + c})
                g[y].append({"to": x, "door": None, "via": "core:" + c})
    return g

def door_path(p, a, b, *, avoid_kinds = None, avoid = None, units = None, through_outside = False, cores = True, edges = False):
    """The way from room a to room b through the fewest doors (and core links between floors), as the list of room
    keys from a to b; b may be "outside". None when there is none.

    avoid_kinds (a list, or a group name such as "storage") and avoid (keys) are rooms the way may not pass through;
    a and b themselves may be of them. units (a list of dwelling units, e.g. ["main"]) keeps the way inside those units'
    rooms. The way never passes outside unless through_outside=True. edges=True returns the edges taken instead
    ({"from", "to", "door", "via"})."""
    g = door_graph(p, cores = cores)
    if a not in g:
        fail("plan.door_path: no room %s" % _q(a))
    if b not in g:
        fail("plan.door_path: no room %s" % _q(b))
    ak = []
    if avoid_kinds != None:
        ak = KIND_GROUPS[avoid_kinds] if type(avoid_kinds) == "string" and avoid_kinds in KIND_GROUPS else ([avoid_kinds] if type(avoid_kinds) == "string" else avoid_kinds)
    av = {k: 1 for k in (avoid or [])}
    kind = {r.get("key"): r.get("kind") for r in p.get("rooms", []) if type(r) == "dict"}
    unit_of = {r.get("key"): r.get("unit", "main") for r in p.get("rooms", []) if type(r) == "dict"}
    prev = {a: None}
    frontier = [a]
    found = a == b
    for _ in range(len(g) + 1):
        if found or not frontier:
            break
        nxt = []
        for x in frontier:
            if x != a and (x in av or kind.get(x) in ak or (x == "outside" and not through_outside) or
                           (units != None and x != "outside" and unit_of.get(x) not in units)):
                continue
            for e in g[x]:
                y = e["to"]
                if y in prev:
                    continue
                prev[y] = (x, e)
                if y == b:
                    found = True
                    break
                nxt.append(y)
            if found:
                break
        frontier = nxt
    if not found:
        return None
    path = [b]
    taken = []
    cur = b
    for _ in range(len(prev) + 1):
        if prev[cur] == None:
            break
        x, e = prev[cur]
        taken.append({"from": x, "to": cur, "door": e["door"], "via": e["via"]})
        path.append(x)
        cur = x
    if edges:
        return list(reversed(taken))
    return list(reversed(path))

# ---------------------------------------------------------------------------------------------------------- more queries

def clear_width(x):
    """The narrowest clear dimension of a room (any of its rects) or a rect, in feet: a hall's or landing's width."""
    rs = _rects_of(x)
    return min([min(r[1] - r[0], r[3] - r[2]) for r in rs])

def rooms_crossed(p, floor, axis, at, *, span = None):
    """The rooms on `floor` a line crosses: axis "v" runs north-south at u = at, "u" east-west at v = at (a bearing line's
    convention); span limits it along itself. [(room key, rect index, (lo, hi))] for each rect it passes through, the
    (lo, hi) being where along the line it is inside that rect (a beam's length there)."""
    out = []
    for r in rooms_on(p, floor):
        for i, x in enumerate(r["rects"]):
            if axis == "v":
                if x[0] < at and at < x[1]:
                    lo, hi = x[2], x[3]
                else:
                    continue
            else:
                if x[2] < at and at < x[3]:
                    lo, hi = x[0], x[1]
                else:
                    continue
            if span != None:
                lo, hi = max(lo, span[0]), min(hi, span[1])
                if hi <= lo:
                    continue
            out.append((r["key"], i, (lo, hi)))
    return out

def walk_length(p, a, b, *, avoid_kinds = None, avoid = None, through_outside = False):
    """An estimate of the walk from room a to room b in feet: along door_path's fewest-doors route, the straight runs from
    a's centre to its first door, door to door, and the last door to b's centre (room centres for core links, which
    count a flight as its plan run). None when there is no way. A walk through furniture or round corners is longer:
    this is the lower bound a plan check can use."""
    steps = door_path(p, a, b, avoid_kinds = avoid_kinds, avoid = avoid, through_outside = through_outside, edges = True)
    if steps == None:
        return None
    if not steps:
        return 0.0
    doors = {d.get("key"): d for d in p.get("doors", [])}
    rooms = {r.get("key"): r for r in p.get("rooms", [])}

    def where(k):
        r = rooms.get(k)
        return centre(r) if r != None else None

    pts = [where(a)]
    for e in steps:
        d = doors.get(e["door"]) if e["door"] != None else None
        if d != None and d.get("at") != None:
            pts.append(d["at"])
        else:
            q = where(e["to"])
            if q != None:
                pts.append(q)
    if b != "outside":
        pts.append(where(b))
    total = 0.0
    for k in range(len(pts) - 1):
        if pts[k] == None or pts[k + 1] == None:
            continue
        du, dv = pts[k + 1][0] - pts[k][0], pts[k + 1][1] - pts[k][1]
        total += math.sqrt(du * du + dv * dv)
    return total

def plan_diff(a, b, *, tol = TOL):
    """How plan b's rooms differ from plan a's, by key: {"added": [keys only in b], "removed": [keys only in a],
    "changed": [{"key", "floor" (b's), "moved": (du, dv) of the main rect's centre, "area": b's sq ft less a's,
    "floor_changed": bool, "kind_changed": bool}]} for rooms in both whose rects differ by more than tol (or whose floor or
    kind changed), largest move first; "same": the count of rooms that did not change."""
    ra = {r.get("key"): r for r in a.get("rooms", []) if type(r) == "dict"}
    rb = {r.get("key"): r for r in b.get("rooms", []) if type(r) == "dict"}
    changed = []
    same = 0
    for k, x in rb.items():
        if k not in ra:
            continue
        y = ra[k]
        moved_rects = len(x["rects"]) != len(y["rects"]) or any([abs(p - q) > tol for i in range(min(len(x["rects"]), len(y["rects"]))) for p, q in zip(x["rects"][i], y["rects"][i])])
        if not moved_rects and x.get("floor") == y.get("floor") and x.get("kind") == y.get("kind"):
            same += 1
            continue
        ca, cb = centre(y), centre(x)
        changed.append({
            "key": k,
            "floor": x.get("floor"),
            "moved": (cb[0] - ca[0], cb[1] - ca[1]),
            "area": area(x) - area(y),
            "floor_changed": x.get("floor") != y.get("floor"),
            "kind_changed": x.get("kind") != y.get("kind"),
        })
    return {
        "added": [k for k in rb.keys() if k not in ra],
        "removed": [k for k in ra.keys() if k not in rb],
        "changed": sorted(changed, key = lambda c: -math.sqrt(c["moved"][0] * c["moved"][0] + c["moved"][1] * c["moved"][1])),
        "same": same,
    }

# ---------------------------------------------------------------------------------------------------------- a room in its own frame

def _opening_on(r, at, prefer, gap):
    """Where point `at` sits on room r's walls: (rect index, side, distance along from the side's start), the side
    whose face is at `at` or up to `gap` short of it; `prefer` wins a tie. None when it is on none of them."""
    best = None
    for i, rc in enumerate(r["rects"]):
        for s in SIDES:
            line, lo, hi = _side(rc, s)
            al = at[0] if _ns(s) else at[1]
            ac = at[1] if _ns(s) else at[0]
            if al < lo - TOL or al > hi + TOL:
                continue
            d = (ac - line) * _out(s)
            if d < -TOL or d > gap + TOL:
                continue
            score = abs(d) + (0.0 if s == prefer else 100.0)
            if best == None or score < best[0]:
                best = (score, i, s, al - lo)
    if best == None:
        return None
    return (best[1], best[2], best[3])

def room_local(p, key, *, gap = WALL_GAP):
    """A room in its own frame, for the stage solves: the origin is the main rect's south-west
    inside corner, so every coordinate is the plan's less `origin`.

        {"key", "kind", "furnish", "origin": (u0, v0), "w", "d", "ceiling", "rects": [local rects, main first],
         "doors":   [{"key", "wall", "offset", "width", "swing": "in" | "out" | "slide" | "pocket" | "open",
                      "hinge": "lo" | "hi", "to"}],
         "windows": [{"key", "wall", "offset", "width", "sill", "head", "egress", "source"}],
         "fixtures": [{"key", "kind", "rect": local, "front"}],
         "stacks":  [(u, v) local, for the stacks within STACK_REACH of the room on its floor]}

    `wall` is this room's side the opening is in (for room b of a door, the side facing room a); `offset` is the
    opening's CENTRE along that wall from its start (west to east on N and S walls, south to north on E and W), so
    the opening spans offset - width/2 to offset + width/2. Each door and window also carries
    `lo` and `hi` (those two jambs), `rect` (which of the room's rects the wall is on; the offset is from that
    rect's side) and `point` (the opening's centre, local). `to` is the room on the other side, or the door's
    "outside..." string. The record also carries `name`, `floor` and `furniture` (the room's furniture records, local)."""
    r = get_room(p, key)
    m = r["rects"][0]
    ou, ov = m[0], m[2]
    fl = None
    for f in p.get("floors", []):
        if f.get("key") == r.get("floor"):
            fl = f
    ceiling = r.get("ceiling")
    if ceiling == None and fl != None:
        ceiling = fl.get("ceiling")
    rects = [(rc[0] - ou, rc[1] - ou, rc[2] - ov, rc[3] - ov) for rc in r["rects"]]
    doors = []
    for d in p.get("doors", []):
        if d.get("floor", r.get("floor")) != r.get("floor") or d.get("at") == None:
            continue
        if d.get("a") == key:
            mine, other, prefer = True, d.get("b"), d.get("wall")
        elif d.get("b") == key:
            mine, other, prefer = False, d.get("a"), _OPP.get(d.get("wall"))
        else:
            continue
        loc = _opening_on(r, d["at"], prefer, gap)
        if loc == None:
            continue
        i, s, off = loc
        sw = d.get("swing", "a")
        if sw == "a":
            sw = "in" if mine else "out"
        elif sw == "b":
            sw = "out" if mine else "in"
        w = d.get("width", DOOR_WIDTH)
        doors.append({
            "key": d.get("key"),
            "wall": s,
            "offset": off,
            "width": w,
            "swing": sw,
            "hinge": d.get("hinge", "lo"),
            "to": other,
            "lo": off - w / 2.0,
            "hi": off + w / 2.0,
            "rect": i,
            "point": (d["at"][0] - ou, d["at"][1] - ov),
        })
    windows = []
    for wn in p.get("windows", []):
        if wn.get("room") != key or not _is_point(wn.get("at")):
            continue
        if wn.get("source") == "roof":
            continue
        loc = _opening_on(r, wn["at"], wn.get("wall"), gap)
        if loc == None:
            continue
        i, s, off = loc
        w = wn.get("width", 3.0)
        windows.append({
            "key": wn.get("key"),
            "wall": s,
            "offset": off,
            "width": w,
            "sill": wn.get("sill", 2.0),
            "head": wn.get("head", 7.0),
            "egress": wn.get("egress", False),
            "source": wn.get("source", "side"),
            "lo": off - w / 2.0,
            "hi": off + w / 2.0,
            "rect": i,
            "point": (wn["at"][0] - ou, wn["at"][1] - ov),
        })
    fixtures = []
    for f in p.get("fixtures", []):
        if f.get("room") != key:
            continue
        fr = f["rect"]
        fixtures.append({"key": f.get("key"), "kind": f.get("kind"), "rect": (fr[0] - ou, fr[1] - ou, fr[2] - ov, fr[3] - ov), "front": f.get("front", "N")})
    furniture = []
    for f in p.get("furniture", []):
        if f.get("room") != key or rect_problem(f.get("rect")) != None:
            continue
        fr = f["rect"]
        furniture.append({"kind": f.get("kind"), "rect": (fr[0] - ou, fr[1] - ou, fr[2] - ov, fr[3] - ov), "front": f.get("front")})
    stacks = []
    for s in p.get("stacks", []):
        if r.get("floor") not in s.get("floors", []):
            continue
        if any([_dist_rect(s["at"], rc) <= STACK_REACH + 1e-9 for rc in r["rects"]]):
            stacks.append((s["at"][0] - ou, s["at"][1] - ov))
    return {
        "key": key,
        "name": r.get("name", key),
        "floor": r.get("floor"),
        "kind": r.get("kind"),
        "furnish": r.get("furnish"),
        "origin": (ou, ov),
        "w": m[1] - m[0],
        "d": m[3] - m[2],
        "ceiling": ceiling,
        "rects": rects,
        "doors": doors,
        "windows": windows,
        "fixtures": fixtures,
        "stacks": stacks,
        "furniture": furniture,
    }

# ---------------------------------------------------------------------------------------------------------- placing openings

def _place_on(segs, along, width, who):
    if along == None:
        seg = segs[0]
        for s in segs:
            if s["length"] > seg["length"]:
                seg = s
        return seg, (seg["span"][0] + seg["span"][1]) / 2.0
    for s in segs:
        if along - width / 2.0 >= s["span"][0] - TOL and along + width / 2.0 <= s["span"][1] + TOL:
            return s, along
    fail("plan.%s: a %s opening centred at %s does not fit on any of these walls: %s" % (
        who,
        ftin(width),
        _g(along),
        ", ".join(["%s %s..%s" % (s["wall"], _g(s["span"][0]), _g(s["span"][1])) for s in segs]),
    ))

def door_between(p, key, a, b, *, wall = None, along = None, width = DOOR_WIDTH, swing = "a", hinge = "lo", gap = WALL_GAP):
    """A door record from room a to room b, on the longest wall they share (on a's side `wall` when given), centred
    on it or at `along` (u on an N or S wall, v on an E or W wall). For b "outside..." it goes on a's exterior wall
    `wall` (required). `at` lands on room a's face. p may be a plan still being built: any dict with "rooms"."""
    ra = get_room(p, a)
    if is_outside(b):
        if wall == None:
            fail("plan.door_between: a door from %s to %s needs wall=, the side of %s it is in" % (_q(a), b, _q(a)))
        segs = [s for s in exterior_walls(p, a, gap = gap) if s["wall"] == wall]
    else:
        segs = [s for s in shared_walls(p, a, b, gap = gap) if wall == None or s["wall"] == wall]
    if not segs:
        fail("plan.door_between: rooms %s and %s share no wall%s" % (_q(a), _q(b), "" if wall == None else " on %s's %s side" % (_q(a), wall)))
    seg, pos = _place_on(segs, along, width, "door_between")
    at = (pos, seg["line"]) if _ns(seg["wall"]) else (seg["line"], pos)
    return door(key, floor = ra.get("floor"), a = a, b = b, wall = seg["wall"], at = at, width = width, swing = swing, hinge = hinge)

def window_on(p, key, room_key, wall, *, along = None, width = 3.0, sill = 2.0, head = 7.0, source = None, egress = False, clear = None, gap = WALL_GAP):
    """A window record in room_key's exterior wall `wall`, centred on its longest exterior piece or at `along`;
    source defaults to what that piece faces ("deck:<key>" becomes "deck")."""
    r = get_room(p, room_key)
    segs = [s for s in exterior_walls(p, room_key, gap = gap) if s["wall"] == wall]
    if not segs:
        fail("plan.window_on: room %s has no exterior wall on its %s side" % (_q(room_key), wall))
    seg, pos = _place_on(segs, along, width, "window_on")
    src = source
    if src == None:
        src = "deck" if seg["faces"].startswith("deck:") else seg["faces"]
    at = (pos, seg["line"]) if _ns(wall) else (seg["line"], pos)
    return window(key, floor = r.get("floor"), room = room_key, wall = wall, at = at, width = width, sill = sill, head = head, source = src, egress = egress, clear = clear)

# ---------------------------------------------------------------------------------------------------------- validate

def _add(out, key, name, msg, floor = None, room = None, at = None, rect = None, severity = "fail"):
    out.append({"key": key, "name": name, "msg": msg, "floor": floor, "room": room, "at": at, "rect": rect, "severity": severity})

def _dups(out, recs, what):
    seen = {}
    for x in recs:
        if type(x) != "dict":
            continue
        k = x.get("key")
        if type(k) == "string":
            if k in seen:
                _add(out, "plan:dup:%s:%s" % (what, k), "%s key used twice" % what, '%s key "%s" is used by two %ss' % (what, k, what))
            seen[k] = 1

def _opening_fits(segs, along, width):
    for s in segs:
        if along - width / 2.0 >= s["span"][0] - TOL and along + width / 2.0 <= s["span"][1] + TOL:
            return True
    return False

def _spans_text(segs):
    if not segs:
        return "none"
    return ", ".join(["%s..%s" % (_g(s["span"][0]), _g(s["span"][1])) for s in segs])

def _problems(p):
    out = []
    if type(p) != "dict":
        _add(out, "plan:type", "not a plan", "a plan is a dict, not a %s" % type(p))
        return out
    for m in check_record("plan", p):
        _add(out, "plan:record", "plan record", m)
    st = p.get("site") or {}
    if type(st) == "dict":
        for m in check_record("site", st):
            _add(out, "plan:site", "site record", m)
    lists = {}
    for f in ["floors", "rooms", "doors", "windows", "fixtures", "furniture", "stacks", "courts", "walls", "bearing"]:
        v = p.get(f, [])
        lists[f] = v if _seq(v) else []

    # each record alone, then keys
    rec_ok = {}
    for what, f in [("floor", "floors"), ("room", "rooms"), ("door", "doors"), ("window", "windows"), ("fixture", "fixtures"),
                    ("furniture", "furniture"), ("stack", "stacks"), ("court", "courts"), ("wall", "walls"), ("bearing", "bearing")]:
        for i, x in enumerate(lists[f]):
            ms = check_record(what, x)
            rec_ok["%s:%d" % (f, i)] = not ms
            for m in ms:
                k = x.get("key") if type(x) == "dict" else None
                fl = x.get("floor") if type(x) == "dict" else None
                rm = x.get("key") if what == "room" and type(x) == "dict" else (x.get("room") if type(x) == "dict" else None)
                _add(out, "plan:%s:%s" % (what, k if k != None else i), "%s record" % what, m, floor = fl, room = rm)
        if what != "wall" and what != "bearing" and what != "furniture":
            _dups(out, lists[f], what)

    idx = _index(p)
    floors = idx["floors"]
    rooms = idx["rooms"]

    # references
    for i, r in enumerate(lists["rooms"]):
        if type(r) == "dict" and type(r.get("floor")) == "string" and r["floor"] not in floors:
            _add(out, "plan:ref:room:%s" % r.get("key"), "room on no floor", 'room "%s" is on floor "%s", which the plan does not have' % (r.get("key"), r["floor"]), room = r.get("key"))
    for i, d in enumerate(lists["doors"]):
        if not rec_ok["doors:%d" % i]:
            continue
        who = 'door "%s"' % d["key"]
        if d["floor"] not in floors:
            _add(out, "plan:ref:door:%s" % d["key"], "door on no floor", '%s is on floor "%s", which the plan does not have' % (who, d["floor"]), floor = d["floor"])
        for side in ["a", "b"]:
            k = d[side]
            if side == "b" and is_outside(k):
                continue
            if k not in rooms:
                _add(out, "plan:ref:door:%s:%s" % (d["key"], side), "door to no room", '%s: its room %s "%s" is not in the plan' % (who, side, k), floor = d["floor"], at = d.get("at"))
            elif rooms[k].get("floor") != d["floor"]:
                _add(out, "plan:ref:door:%s:%s" % (d["key"], side), "door off its floor", '%s is on %s but its room %s "%s" is on %s' % (who, d["floor"], side, k, rooms[k].get("floor")), floor = d["floor"], at = d.get("at"))
    for i, w in enumerate(lists["windows"]):
        if not rec_ok["windows:%d" % i]:
            continue
        who = 'window "%s"' % w["key"]
        if w["room"] not in rooms:
            _add(out, "plan:ref:window:%s" % w["key"], "window in no room", '%s: its room "%s" is not in the plan' % (who, w["room"]), floor = w["floor"], at = w["at"])
        elif rooms[w["room"]].get("floor") != w["floor"]:
            _add(out, "plan:ref:window:%s" % w["key"], "window off its floor", '%s is on %s but its room "%s" is on %s' % (who, w["floor"], w["room"], rooms[w["room"]].get("floor")), floor = w["floor"], at = w["at"])
        src = w.get("source", "side")
        for pre in SOURCE_PREFIXES:
            if src.startswith(pre):
                ck = src[len(pre):]
                hit = [c for c in lists["courts"] if type(c) == "dict" and c.get("key") == ck]
                if not hit:
                    _add(out, "plan:ref:window:%s:source" % w["key"], "window onto no court", '%s looks onto %s, but the plan has no court "%s"' % (who, src, ck), floor = w["floor"], room = w["room"], at = w["at"])
                elif hit[0].get("kind") + ":" != pre:
                    _add(out, "plan:ref:window:%s:source" % w["key"], "window source kind", '%s looks onto %s, but "%s" is a %s' % (who, src, ck, hit[0].get("kind")), floor = w["floor"], room = w["room"], at = w["at"])
    for i, f in enumerate(lists["fixtures"]):
        if rec_ok["fixtures:%d" % i] and f["room"] not in rooms:
            _add(out, "plan:ref:fixture:%s" % f["key"], "fixture in no room", 'fixture "%s": its room "%s" is not in the plan' % (f["key"], f["room"]), rect = f["rect"])
    for i, f in enumerate(lists["furniture"]):
        if rec_ok["furniture:%d" % i] and f["room"] not in rooms:
            _add(out, "plan:ref:furniture:%d" % i, "furniture in no room", 'furniture "%s": its room "%s" is not in the plan' % (f["kind"], f["room"]), rect = f["rect"])
    for i, s in enumerate(lists["stacks"]):
        if not rec_ok["stacks:%d" % i]:
            continue
        missing = [x for x in s["floors"] if x not in floors]
        if missing:
            _add(out, "plan:ref:stack:%s" % s["key"], "stack through no floor", 'stack "%s" passes floor%s %s, which the plan does not have' % (s["key"], "s" if len(missing) > 1 else "", ", ".join(['"%s"' % x for x in missing])), at = s["at"])
    for i, c in enumerate(lists["courts"]):
        if rec_ok["courts:%d" % i] and c["base"] not in floors:
            _add(out, "plan:ref:court:%s" % c["key"], "court on no floor", 'court "%s" starts at floor "%s", which the plan does not have' % (c["key"], c["base"]), rect = c["rect"])
    for what, f in [("wall", "walls"), ("bearing", "bearing")]:
        for i, x in enumerate(lists[f]):
            if rec_ok["%s:%d" % (f, i)] and x["floor"] not in floors:
                _add(out, "plan:ref:%s:%d" % (what, i), "%s on no floor" % what, '%s is on floor "%s", which the plan does not have' % (_who(what, x), x["floor"]))

    # geometry: overlaps, plates, L pieces, cores
    for fk in sorted(idx["on"].keys(), key = lambda k: str(k)):
        rs = idx["on"][fk]
        for i in range(len(rs)):
            for j in range(i + 1, len(rs)):
                for x in rs[i]["rects"]:
                    for y in rs[j]["rects"]:
                        o = _inter(x, y)
                        if o == None or o[1] - o[0] <= TOL or o[3] - o[2] <= TOL:
                            continue
                        a_, b_ = rs[i]["key"], rs[j]["key"]
                        _add(out, "plan:overlap:%s:%s" % (a_, b_), "rooms overlap",
                             'rooms "%s" and "%s" overlap on %s by %s x %s (%s sq ft) at u %s..%s, v %s..%s' % (
                                 a_, b_, fk, ftin(o[1] - o[0]), ftin(o[3] - o[2]), format(_union_area([o]), ".1f"), _g(o[0]), _g(o[1]), _g(o[2]), _g(o[3])),
                             floor = fk, room = a_, rect = o)
        fl = floors.get(fk)
        plate = fl.get("plate") if fl != None else None
        if plate and all([rect_problem(x) == None for x in plate]):
            for r in rs:
                if _GROUP_OF.get(r.get("kind")) == "outdoor":
                    continue
                inside = _union_area([q for q in [_inter(x, y) for x in r["rects"] for y in plate] if q != None])
                outside = _union_area(r["rects"]) - inside
                if outside > 0.25:
                    _add(out, "plan:plate:%s" % r["key"], "room off its plate", 'room "%s" runs %s sq ft outside floor %s\'s plate' % (r["key"], format(outside, ".1f"), fk), floor = fk, room = r["key"], rect = bbox(r))
        for r in rs:
            if len(r["rects"]) < 2:
                continue
            reached = {0: 1}
            for _ in range(len(r["rects"])):
                for a_ in range(len(r["rects"])):
                    if a_ not in reached:
                        continue
                    for b_ in range(len(r["rects"])):
                        if b_ in reached:
                            continue
                        x, y = r["rects"][a_], r["rects"][b_]
                        touch = False
                        for s in SIDES:
                            g = _facing_gap(x, y, s)
                            ya, yb = _along(y, s)
                            xa, xb = _along(x, s)
                            if g <= WALL_GAP and min(xb, yb) - max(xa, ya) > TOL and _far(x, y, s) > TOL:
                                touch = True
                        if touch or _inter(x, y) != None:
                            reached[b_] = 1
            if len(reached) < len(r["rects"]):
                _add(out, "plan:pieces:%s" % r["key"], "room in pieces", 'room "%s": its rects do not all reach its main rect (an L-shaped room\'s second rect shares a side with the first, or faces it across one wall)' % r["key"], floor = fk, room = r["key"], rect = bbox(r))
    cores = {}
    for r in rooms.values():
        if r.get("core") != None:
            cores.setdefault(r["core"], []).append(r)
    for c in sorted(cores.keys()):
        seen = {}
        for r in cores[c]:
            if r.get("floor") in seen:
                _add(out, "plan:core:%s" % c, "core twice on a floor", 'core "%s" has two rooms on floor %s: "%s" and "%s"' % (c, r.get("floor"), seen[r.get("floor")], r["key"]), floor = r.get("floor"), room = r["key"])
            seen[r.get("floor")] = r["key"]
        ks = {r.get("kind"): 1 for r in cores[c]}
        if "stair" in ks and "lift" in ks:
            _add(out, "plan:core:%s:kind" % c, "core of mixed kinds", 'core "%s" is a stair on one floor and a lift on another' % c)

    # doors on walls
    ext_cache = {}
    def ext(k):
        if k not in ext_cache:
            ext_cache[k] = _exterior(idx, rooms[k], WALL_GAP, WALL_GAP, None)
        return ext_cache[k]
    for i, d in enumerate(lists["doors"]):
        if not rec_ok["doors:%d" % i] or d.get("at") == None:
            continue
        a_ = rooms.get(d["a"])
        if a_ == None or a_.get("floor") != d["floor"]:
            continue
        side = d["wall"]
        w = d.get("width", DOOR_WIDTH)
        at = d["at"]
        al = at[0] if _ns(side) else at[1]
        ac = at[1] if _ns(side) else at[0]
        who = 'door "%s"' % d["key"]
        if is_outside(d["b"]):
            segs = [s for s in ext(d["a"]) if s["wall"] == side]
            ok = False
            for s in segs:
                dd = (ac - s["line"]) * _out(side)
                if dd >= -TOL and dd <= WALL_GAP + TOL and al - w / 2.0 >= s["span"][0] - TOL and al + w / 2.0 <= s["span"][1] + TOL:
                    ok = True
            if not ok:
                _add(out, "plan:door:%s" % d["key"], "door off the outside wall",
                     '%s to %s: its %s opening at %s %s is not on an exterior wall of "%s"\'s %s side (exterior there: %s)' % (
                         who, d["b"], ftin(w), "u" if _ns(side) else "v", _g(al), d["a"], side, _spans_text(segs)),
                     floor = d["floor"], room = d["a"], at = at)
            continue
        b_ = rooms.get(d["b"])
        if b_ == None or b_.get("floor") != d["floor"]:
            continue
        segs = [s for s in _shared(a_, b_, WALL_GAP, 0.0) if s["wall"] == side]
        if not segs:
            _add(out, "plan:door:%s" % d["key"], "door off a shared wall",
                 '%s: rooms "%s" and "%s" share no wall on "%s"\'s %s side' % (who, d["a"], d["b"], d["a"], side),
                 floor = d["floor"], room = d["a"], at = at)
            continue
        ok = False
        for s in segs:
            lo_ac, hi_ac = min(s["line"], s["far"]), max(s["line"], s["far"])
            if ac >= lo_ac - TOL and ac <= hi_ac + TOL and al - w / 2.0 >= s["span"][0] - TOL and al + w / 2.0 <= s["span"][1] + TOL:
                ok = True
        if not ok:
            _add(out, "plan:door:%s" % d["key"], "door off a shared wall",
                 '%s: its %s opening centred at (%s, %s) is not on the wall "%s" shares with "%s" (%s side at %s %s, along %s)' % (
                     who, ftin(w), _g(at[0]), _g(at[1]), d["a"], d["b"], side, "v" if _ns(side) else "u", _g(segs[0]["line"]), _spans_text(segs)),
                 floor = d["floor"], room = d["a"], at = at)

    # windows on exterior walls
    for i, w in enumerate(lists["windows"]):
        if not rec_ok["windows:%d" % i] or w.get("source", "side") == "roof":
            continue
        r = rooms.get(w["room"])
        if r == None or r.get("floor") != w["floor"]:
            continue
        side = w["wall"]
        at = w["at"]
        al = at[0] if _ns(side) else at[1]
        ac = at[1] if _ns(side) else at[0]
        wd = w.get("width", 3.0)
        segs = [s for s in ext(w["room"]) if s["wall"] == side]
        hit = None
        for s in segs:
            dd = (ac - s["line"]) * _out(side)
            if dd >= -TOL and dd <= WALL_GAP + TOL and al - wd / 2.0 >= s["span"][0] - TOL and al + wd / 2.0 <= s["span"][1] + TOL:
                hit = s
        # a window may span two pieces of one wall that face different things (a court, then the street)
        if hit == None and segs:
            lo_, hi_ = al - wd / 2.0, al + wd / 2.0
            cover = [(lo_, hi_)]
            for s in segs:
                dd = (ac - s["line"]) * _out(side)
                if dd >= -TOL and dd <= WALL_GAP + TOL:
                    cover = _subtract(cover, s["span"][0] - TOL, s["span"][1] + TOL)
            if not cover:
                hit = segs[0]
        if hit == None:
            _add(out, "plan:window:%s" % w["key"], "window off an exterior wall",
                 'window "%s": its %s opening at %s %s is not on an exterior wall of "%s"\'s %s side (exterior there: %s)' % (
                     w["key"], ftin(wd), "u" if _ns(side) else "v", _g(al), w["room"], side, _spans_text(segs)),
                 floor = w["floor"], room = w["room"], at = at)
            continue
        src = w.get("source", "side")
        court_keys = {c.get("key"): 1 for c in lists["courts"] if type(c) == "dict"}
        for pre in SOURCE_PREFIXES:
            if src.startswith(pre) and src[len(pre):] in court_keys:
                faces = [s["faces"] for s in segs if s["span"][0] - TOL <= al and al <= s["span"][1] + TOL]
                if src not in faces:
                    _add(out, "plan:window:%s:faces" % w["key"], "window source",
                         'window "%s" says it looks onto %s, but its wall faces %s there' % (w["key"], src, " and ".join(faces) if faces else "nothing the plan knows"),
                         floor = w["floor"], room = w["room"], at = at, severity = "warn")

    # fixtures inside their rooms
    for i, f in enumerate(lists["fixtures"]):
        if not rec_ok["fixtures:%d" % i] or f["room"] not in rooms:
            continue
        r = rooms[f["room"]]
        inside = _union_area([q for q in [_inter(f["rect"], x) for x in r["rects"]] if q != None])
        if _union_area([f["rect"]]) - inside > 0.1:
            _add(out, "plan:fixture:%s" % f["key"], "fixture outside its room", 'fixture "%s" (%s) runs outside room "%s"' % (f["key"], f["kind"], f["room"]), floor = r.get("floor"), room = f["room"], rect = f["rect"])
    return out

def validate(p):
    """Every problem in a plan, as sentences that name the records; [] when it is clean. It finds:
    records that are malformed (a rect in the wrong order, an unknown kind, a bad swing, ...); keys used twice;
    dangling keys (a door's rooms, a window's room or court, a fixture's room, a room's floor, a stack's floors, a
    court's base floor, a wall's floor); rooms on one floor overlapping more than TOL both ways; a room outside its
    floor's plate; an L-shaped room whose rects do not touch; a core twice on a floor or of mixed kinds; a door not
    on a wall its two rooms share (within WALL_GAP), or for an outside door not on an exterior wall of room a; a
    window not on an exterior wall of its room (skylights aside), or not facing the court its source names; a
    fixture outside its room. A door and a window must lie within their wall, jambs included, to TOL."""
    return [x["msg"] for x in _problems(p)]

def validate_rows(p):
    """validate() as check rows: {"key", "name", "ok": False, "detail", "rule": "plan", "severity", "floor",
    "room", "at" | "rect"}, ready for plan_draw's red pen."""
    rows = []
    for x in _problems(p):
        row = {"key": x["key"], "name": x["name"], "ok": False, "detail": x["msg"], "rule": "plan", "severity": x["severity"], "floor": x["floor"], "room": x["room"]}
        if x["at"] != None:
            row["at"] = x["at"]
        elif x["rect"] != None:
            row["rect"] = x["rect"]
        rows.append(row)
    return rows

# ---------------------------------------------------------------------------------------------------------- formatting

def _gcd(a, b):
    for _ in range(64):
        if b == 0:
            break
        a, b = b, a % b
    return a

def ftin(x, frac = None, down = False):
    """Feet as feet-inches: ftin(2.5) is 2'-6"; to the inch by default, or to 1/frac in (frac 2, 4, 8, 16, 32 or
    64): ftin(2.9948, 16) is 2'-11 15/16". Negative lengths take a minus sign: -0'-6". down=True rounds toward zero
    (to within 1/64 in) instead of to the nearest: a clear width of 2'-11 15/16" writes 2'-11", never 3'-0"."""
    den = 1
    if frac != None:
        den = int(frac)
        if den not in [1, 2, 4, 8, 16, 32, 64]:
            fail("plan.ftin: frac is 2, 4, 8, 16, 32 or 64 (the fraction of an inch to round to), not %s" % _q(frac))
    if down:
        n = int(math.floor(abs(x) * 12.0 * den + den / 64.0))
    else:
        n = int(math.round(abs(x) * 12.0 * den))
    whole = n // den
    rem = n % den
    s = "%d'-%d" % (whole // 12, whole % 12)
    if rem:
        g = _gcd(rem, den)
        s += " %d/%d" % (rem // g, den // g)
    s += '"'
    if x < 0 and n > 0:
        s = "-" + s
    return s

def sqft(x, digits = 0):
    """Square feet as text: sqft(1234.4) is "1,234 sq ft"; digits= decimals."""
    return format(x, ",." + str(int(digits)) + "f") + " sq ft"

def size_text(x, frac = None):
    """A room's (or rect's) main rect as W x D in feet-inches: 11'-6" x 13'-6"."""
    return "%s x %s" % (ftin(width(x), frac), ftin(depth(x), frac))

# ---------------------------------------------------------------------------------------------------------- a synthetic house

def toy_house():
    """A synthetic two-floor house, 25 x 60 ft outside on a 25 x 100-ft lot with the street to the south, for tests
    and examples: 6-in exterior walls, 4.8-in partitions; a stair core on both floors, an L-shaped kitchen, a powder
    room and a bath with fixtures, two stacks, a roof deck, windows with sources and egress, and outside doors to the
    street and the yard. Nothing in it is a real site."""
    L1, L2 = "L1", "L2"
    rooms = [
        room("living", "Living", floor = L1, kind = "living", rect = (0.5, 14.0, 10.5, 28.0)),
        room("entry", "Entry", floor = L1, kind = "entry", rect = (14.4, 24.5, 10.5, 18.0)),
        room("hall1", "Hall", floor = L1, kind = "hall", rect = (14.4, 19.6, 18.4, 40.0)),
        room("stair1", "Stair", floor = L1, kind = "stair", rect = (20.0, 24.5, 18.4, 30.0), core = "st"),
        room("powder", "Powder", floor = L1, kind = "powder", rect = (20.0, 24.5, 30.4, 36.0)),
        room("closet1", "Coats", floor = L1, kind = "closet", rect = (20.0, 24.5, 36.4, 40.0)),
        room("kitchen", "Kitchen", floor = L1, kind = "kitchen", rects = [(0.5, 14.0, 28.4, 50.0), (14.0, 24.5, 40.4, 50.0)]),
        room("family", "Family", floor = L1, kind = "family", rect = (0.5, 24.5, 50.4, 69.5)),
        room("bed1", "Bedroom 1", floor = L2, kind = "bed", rect = (0.5, 12.0, 10.5, 24.0), furnish = "queen"),
        room("bath", "Bath", floor = L2, kind = "bath", rect = (12.4, 24.5, 10.5, 18.0)),
        room("hall2", "Hall", floor = L2, kind = "hall", rect = (12.4, 19.6, 18.4, 40.0)),
        room("stair2", "Stair", floor = L2, kind = "stair", rect = (20.0, 24.5, 18.4, 30.0), core = "st"),
        room("laundry", "Laundry", floor = L2, kind = "laundry", rect = (20.0, 24.5, 30.4, 40.0)),
        room("bed2", "Bedroom 2", floor = L2, kind = "bed", rect = (0.5, 12.0, 24.4, 40.0), furnish = "twin"),
        room("primary", "Primary", floor = L2, kind = "bed", rect = (0.5, 24.5, 40.4, 54.0), furnish = "king"),
        room("deck", "Roof deck", floor = L2, kind = "deck", rect = (0.5, 24.5, 54.4, 69.5)),
    ]
    doors = [
        door("d_front", floor = L1, a = "entry", b = "outside:street", wall = "S", at = (19.5, 10.5), width = 3.0, swing = "a", hinge = "lo"),
        door("d_entry_living", floor = L1, a = "entry", b = "living", wall = "W", at = (14.4, 14.25), width = 4.0, swing = "open"),
        door("d_entry_hall", floor = L1, a = "entry", b = "hall1", wall = "N", at = (17.0, 18.0), width = 3.0, swing = "open"),
        door("d_stair1", floor = L1, a = "hall1", b = "stair1", wall = "E", at = (19.6, 24.0), width = 3.0, swing = "open"),
        door("d_powder", floor = L1, a = "powder", b = "hall1", wall = "W", at = (20.0, 33.2), width = 2.33, swing = "a", hinge = "lo"),
        door("d_closet1", floor = L1, a = "closet1", b = "hall1", wall = "W", at = (20.0, 38.2), width = 2.0, swing = "b", hinge = "hi"),
        door("d_hall_kitchen", floor = L1, a = "hall1", b = "kitchen", wall = "N", at = (17.0, 40.0), width = 3.0, swing = "open"),
        door("d_living_kitchen", floor = L1, a = "living", b = "kitchen", wall = "N", at = (7.25, 28.0), width = 6.0, swing = "open"),
        door("d_kitchen_family", floor = L1, a = "kitchen", b = "family", wall = "N", at = (7.25, 50.0), width = 8.0, swing = "open"),
        door("d_rear", floor = L1, a = "family", b = "outside:yard", wall = "N", at = (12.5, 69.5), width = 6.0, swing = "slide"),
        door("d_bed1", floor = L2, a = "bed1", b = "hall2", wall = "E", at = (12.0, 21.2), width = 2.67, swing = "a", hinge = "hi"),
        door("d_bath", floor = L2, a = "bath", b = "hall2", wall = "N", at = (16.0, 18.0), width = 2.5, swing = "a", hinge = "lo"),
        door("d_stair2", floor = L2, a = "hall2", b = "stair2", wall = "E", at = (19.6, 24.0), width = 3.0, swing = "open"),
        door("d_laundry", floor = L2, a = "laundry", b = "hall2", wall = "W", at = (20.0, 35.2), width = 2.5, swing = "pocket"),
        door("d_bed2", floor = L2, a = "bed2", b = "hall2", wall = "E", at = (12.0, 32.0), width = 2.67, swing = "a", hinge = "lo"),
        door("d_primary", floor = L2, a = "primary", b = "hall2", wall = "S", at = (16.0, 40.4), width = 2.67, swing = "a", hinge = "lo"),
        door("d_deck", floor = L2, a = "primary", b = "deck", wall = "N", at = (12.5, 54.0), width = 6.0, swing = "slide"),
    ]
    windows = [
        window("w_living", floor = L1, room = "living", wall = "S", at = (7.25, 10.5), width = 6.0, sill = 1.5, head = 7.5, source = "street"),
        window("w_kitchen", floor = L1, room = "kitchen", wall = "W", at = (0.5, 39.0), width = 4.0, sill = 3.5, head = 7.0, source = "side"),
        window("w_powder", floor = L1, room = "powder", wall = "E", at = (24.5, 33.2), width = 2.0, sill = 5.0, head = 7.0, source = "side"),
        window("w_family", floor = L1, room = "family", wall = "N", at = (4.0, 69.5), width = 4.0, sill = 2.5, head = 7.0, source = "rear"),
        window("w_bed1", floor = L2, room = "bed1", wall = "S", at = (6.25, 10.5), width = 4.0, sill = 2.0, head = 7.0, source = "street", egress = True, clear = (3.5, 3.0)),
        window("w_bath", floor = L2, room = "bath", wall = "S", at = (18.0, 10.5), width = 2.5, sill = 4.0, head = 7.0, source = "street"),
        window("w_bed2", floor = L2, room = "bed2", wall = "W", at = (0.5, 32.0), width = 3.0, sill = 2.0, head = 7.0, source = "side", egress = True),
        window("w_primary", floor = L2, room = "primary", wall = "W", at = (0.5, 47.0), width = 4.0, sill = 2.0, head = 7.0, source = "side", egress = True),
        window("w_primary_n", floor = L2, room = "primary", wall = "N", at = (20.0, 54.0), width = 4.0, sill = 2.0, head = 7.0, source = "deck"),
        window("w_hall2_sky", floor = L2, room = "hall2", wall = None, at = (16.0, 29.0), width = 2.0, sill = 7.5, head = 8.5, source = "roof"),
    ]
    fixtures = [
        fixture("f_tub", room = "bath", kind = "tub", rect = (12.4, 17.4, 10.5, 13.0), front = "N"),
        fixture("f_wc", room = "bath", kind = "wc", rect = (22.0, 24.5, 13.0, 14.5), front = "W"),
        fixture("f_lav", room = "bath", kind = "lav", rect = (20.0, 22.5, 16.3, 18.0), front = "S"),
        fixture("f_pwc", room = "powder", kind = "wc", rect = (22.0, 24.5, 33.5, 35.0), front = "W"),
        fixture("f_plav", room = "powder", kind = "lav", rect = (20.0, 21.5, 30.4, 32.0), front = "E"),
        fixture("f_sink", room = "kitchen", kind = "sink", rect = (0.5, 2.5, 41.0, 44.0), front = "E"),
    ]
    stacks = [
        stack("s_bath", at = (24.7, 16.0), floors = [L1, L2]),
        stack("s_wet", at = (24.7, 33.2), floors = [L1, L2]),
        stack("s_sink", at = (0.2, 42.0), floors = [L1]),
    ]
    return plan(
        "toy_house",
        source = "plan.toy_house",
        site = site((0.0, 25.0, 0.0, 100.0), street = "S", setbacks = {"front": 10.0, "rear": 30.0, "east": 0.0, "west": 0.0}),
        floors = [
            floor(L1, name = "First floor", z = 0.0, ceiling = 9.0, plate = [(0.5, 24.5, 10.5, 69.5)]),
            floor(L2, name = "Second floor", z = 10.0, ceiling = 8.5, plate = [(0.5, 24.5, 10.5, 54.0)]),
        ],
        rooms = rooms,
        doors = doors,
        windows = windows,
        fixtures = fixtures,
        stacks = stacks,
        bearing = [bearing(floor = L1, axis = "v", at = 14.2, span = (10.5, 50.0))],
        meta = {"toy": True},
    )
