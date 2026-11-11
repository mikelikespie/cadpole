# @cadpole/plan_audit.star -- the red pen: an architect's and a code reader's checks on a fixed plan, returned as
# a model's check rows (key, name, ok, detail) plus the marks that draw them.
#
#     load("@cadpole/plan_audit.star", "audit", "redpen_set", "row_line")
#     A = audit(plan)                      # {"rows": [...], "marks": [...], "count": {...}}
#     for r in A["rows"]:
#         print(row_line(r))               # "PASS key: name -- detail", or FAIL / WARN / NOTE
#     redpen_set(plan, A["rows"])          # plan_draw's sheets with the marks, written as <plan>_redpen.pdf
#
# A plan is plain data (the @cadpole/plan.star format): lengths in feet, u east and v north,
# rects (u0, u1, v0, v1) to inside faces. A row is {"key", "name", "ok", "detail", "rule", "severity", "floor", "room",
# "at" | "rect"}: `severity` is the rule's weight -- "fail" (code, or a plan that does not work), "warn" (a house rule,
# an architect's mark) or "note" (a measure, an unknown, a question for the building department) -- and `ok` whether it
# held. A row that could not be decided (no net clear opening given, a question for the building department) is
# ok=False with severity "note".
#
# The families, each a function of its own: code_rows (R304, R305, R310.1, R311.6, R311.7, CPC 402.5 and R307.1,
# showers, R302.5.1, R302.1), door_rows (the door graph: garage to house, pantry, baths, powder rooms, bedrooms, reach,
# dead-end halls, the kitchen's doors, walks), stack_rows, filler_rows, furnish_rows (@cadpole/plan_furnish.star's room stage),
# isovist_rows (reported only); light_rows lives in @cadpole/plan_light.star; the plan's own drafting problems are
# plan.star's validate_rows. Code readings here are unverified: the building department has the last word on CPC
# 402.5's 24 in, R310.1's yard or court, and R302.1(2) with sprinklers.
#
# Pure: loading records nothing. Running makes features in two families only, and both take name=: the light family
# (view_cone needs bodies; on a hidden layer) and the furniture re-pack (plan_furnish's opt problems); redpen_set()
# records an export. Everything is in feet.

load("@cadpole/plan.star", "exterior_walls", "normalize", "room_local", "validate_rows")
load("@cadpole/plan_draw.star", "plan_set")
load("@cadpole/plan_furnish.star", "furnish")
load("@cadpole/plan_furniture.star", "SETS")
load("@cadpole/plan_light.star", "light_rows")

# ---------------------------------------------------------------- vocabulary (the plan format's room kinds)
LIVING = ("living", "dining", "family", "den", "study", "office", "play", "media", "guest", "bed")
WET = ("kitchen", "bath", "powder", "wc", "laundry")
BATHS = ("bath", "powder", "wc")
CIRCULATION = ("hall", "entry", "foyer", "mud", "stair", "lift", "landing", "lobby")
STORAGE = ("closet", "storage", "mech", "pantry")
OUTDOOR = ("deck", "court", "well")
DAY = ("living", "dining", "kitchen", "family", "den", "study", "office", "play", "media")
PASSAGES = ("hall", "landing", "lobby")  # what R311.6 reads as a hallway
ENTRIES = ("entry", "foyer", "lobby")

# ---------------------------------------------------------------- the thresholds; audit(rules=) overrides any of them
RULES = {
    # code (each an unverified reading; the section is in every row)
    "hab_area": 70.0,  # R304.1, sq ft
    "hab_dim": 7.0,  # R304.2, ft (kitchens exempt)
    "ceil_hab": 7.0,  # R305.1: habitable space and halls
    "ceil_bath": 6.0 + 8.0 / 12.0,  # R305.1: baths, toilet rooms, laundry
    "hall_min": 3.0,  # R311.6
    "stair_min": 3.0,  # R311.7.1
    "landing_min": 3.0,  # R311.7.6, in the direction of travel
    "sill_max": 44.0 / 12.0,  # R310.2.2
    "egress_area": 5.7,  # R310.2.1, net clear sq ft
    "egress_area_grade": 5.0,  # R310.2.1, a grade-floor opening
    "egress_h": 24.0 / 12.0,
    "egress_w": 20.0 / 12.0,
    "basement_below": 6.0,  # a floor more than this below grade is a basement (R310.1 wants it an opening)
    "yards_reach": True,  # street, rear, side and deck glass taken to open onto a yard that reaches a public way
    "wc_front": 24.0 / 12.0,  # CPC 402.5 (California)
    "wc_front_irc": 21.0 / 12.0,  # IRC R307.1
    "wc_side": 15.0 / 12.0,  # both: centre to a side wall or obstruction (a 30-in space)
    "wc_depth": 28.0 / 12.0,  # an elongated bowl and tank, when no fixture is drawn
    "shower_min": 30.0 / 12.0,  # CPC 408.6, IRC R307.2
    "shower_area": 1024.0 / 144.0,
    "fsd_none": 3.0,  # Table R302.1(1): no openings closer
    "fsd_limited": 5.0,  # Table R302.1(1): 25 % between 3 and 5 ft
    "open_share": 0.25,
    "ext_wall": 0.5,  # exterior wall thickness when the plan gives no walls
    # doors
    "pantry_walk": 25.0,  # ft; a pantry further than this from the kitchen is a mark
    "kitchen_doors": 3,
    "dead_end": 15.0,  # ft of hall past its last door
    "hall_len": 12.0,  # a hall at least this long that serves one room is a mark
    "bed_walk": 50.0,  # ft from the stair to a bedroom before it is a mark
    # stacks
    "stack_tol": 0.6,  # a stack within this of a room's edge is in its wall
    # fillers
    "sliver": 3.2,  # long side over short side for a service room to be leftover depth
    "sliver_area": 25.0,
    "filler_area": 100.0,  # a storage room bigger than this is a filler
    "storage_share": 0.15,  # of a floor's net area
    "hall_share": 0.20,
    "aspect": 2.0,  # a habitable room this long for its width is a mark
    "glazing": 0.08,  # R303.1
    # what to leave out: row keys or key prefixes ("code:R305", "doors:walks")
    "skip": [],
}

_EPS = 1e-6
_BIG = 1e9

# ---------------------------------------------------------------- formatting (imperial, as every detail is written)
def ftin(x, frac=1):
    """Feet as feet-inches to the nearest 1/frac inch: ftin(7.4167) is 7'-5", ftin(2.9948, 16) is 2'-11 15/16"."""
    n = int(math.round(abs(x) * 12.0 * frac))
    whole, part = n // frac, n % frac
    s = "%d'-%d" % (whole // 12, whole % 12)
    if part:
        num, den = part, frac
        for _ in range(8):
            if num % 2 or den % 2:
                break
            num, den = num // 2, den // 2
        s = s + " %d/%d" % (num, den)
    return ("-" if x < 0 and n > 0 else "") + s + "\""


def inches(x, frac=16):
    """Feet as inches to the nearest 1/frac: inches(0.0052) is 1/16 in, inches(2.9948) is 35 15/16 in."""
    n = int(math.round(abs(x) * 12.0 * frac))
    whole, part = n // frac, n % frac
    s = "%d" % whole if whole or not part else ""
    if part:
        num, den = part, frac
        for _ in range(8):
            if num % 2 or den % 2:
                break
            num, den = num // 2, den // 2
        s = (s + " " if s else "") + "%d/%d" % (num, den)
    return ("-" if x < 0 and n > 0 else "") + s + " in"


def sqft(a):
    return format(a, ",.0f") + " sq ft"


def pct(x):
    return format(100.0 * x, ".0f") + "%"


def _lower(s):
    return s.lower() if s else s


# ---------------------------------------------------------------- geometry (feet; rects are (u0, u1, v0, v1))
def _w(q):
    return q[1] - q[0]


def _d(q):
    return q[3] - q[2]


def _ra(q):
    return (q[1] - q[0]) * (q[3] - q[2])


def _mid(q):
    return (0.5 * (q[0] + q[1]), 0.5 * (q[2] + q[3]))


def _least(q):
    return min(q[1] - q[0], q[3] - q[2])


def _longest(q):
    return max(q[1] - q[0], q[3] - q[2])


def _in(q, p, tol=0.0):
    """p in rect q grown by tol (tol < 0 shrinks it)."""
    return p[0] >= q[0] - tol and p[0] <= q[1] + tol and p[1] >= q[2] - tol and p[1] <= q[3] + tol


def _depth_in(q, p):
    """How far inside q the point p is: the distance to the nearest edge (negative outside)."""
    return min(p[0] - q[0], q[1] - p[0], p[1] - q[2], q[3] - p[1])


def _dist_rect(q, p):
    """Distance from p to rect q (0 inside)."""
    du = max(q[0] - p[0], 0.0, p[0] - q[1])
    dv = max(q[2] - p[1], 0.0, p[1] - q[3])
    return math.hypot(du, dv)


def _man(a, b):
    return abs(a[0] - b[0]) + abs(a[1] - b[1])


def _overlap(a0, a1, b0, b1):
    return min(a1, b1) - max(a0, b0)


def _facing(a, b, gmax):
    """The side of rect a that faces rect b across a gap of at most gmax (a wall), with the run they share:
    (side, gap, lo, hi), or None."""
    ou = _overlap(a[0], a[1], b[0], b[1])
    ov = _overlap(a[2], a[3], b[2], b[3])
    if ou > 0.05:
        g = b[2] - a[3]
        if g >= -0.05 and g <= gmax:
            return ("N", g, max(a[0], b[0]), min(a[1], b[1]))
        g = a[2] - b[3]
        if g >= -0.05 and g <= gmax:
            return ("S", g, max(a[0], b[0]), min(a[1], b[1]))
    if ov > 0.05:
        g = b[0] - a[1]
        if g >= -0.05 and g <= gmax:
            return ("E", g, max(a[2], b[2]), min(a[3], b[3]))
        g = a[0] - b[1]
        if g >= -0.05 and g <= gmax:
            return ("W", g, max(a[2], b[2]), min(a[3], b[3]))
    return None


_NORMAL = {"N": (0.0, 1.0), "S": (0.0, -1.0), "E": (1.0, 0.0), "W": (-1.0, 0.0)}
_SIDE_WORD = {"N": "north", "S": "south", "E": "east", "W": "west"}


# ---------------------------------------------------------------- the plan, read
def _rects(r):
    return [tuple(q) for q in r.get("rects", [])]


def _main(r):
    return tuple(r["rects"][0])


def _area(r):
    return sum_([_ra(q) for q in _rects(r)])


def sum_(xs):
    t = 0.0
    for x in xs:
        t += x
    return t


def _name(r):
    return r.get("name") or r["key"]


def _dims(q):
    return "%s x %s" % (ftin(_w(q)), ftin(_d(q)))


def _habitable(r):
    h = r.get("habitable")
    if h == None:
        return r["kind"] in LIVING or r["kind"] == "kitchen"
    return h


def _sleeping(r):
    s = r.get("sleeping")
    if s == None:
        return r["kind"] in ("bed", "guest")
    return s


def _kept(r):
    return r.get("kept", False) == True


def _unit(r):
    return r.get("unit") or "main"


def floors_of(plan):
    """The plan's floors from the lowest up, with a stand-in for any floor a room names that the plan does not list."""
    fl = [f for f in plan.get("floors", [])]
    have = {f["key"]: True for f in fl}
    for r in plan.get("rooms", []):
        if r["floor"] not in have:
            fl.append({"key": r["floor"], "z": 0.0})
            have[r["floor"]] = True
    return sorted(fl, key = lambda f: f.get("z", 0.0))


def _floor(plan, key):
    for f in floors_of(plan):
        if f["key"] == key:
            return f
    return {"key": key, "z": 0.0}


def storey(plan, key):
    """Floor-to-floor height: to the next floor up, else the ceiling and a foot of structure, else 10 ft."""
    fl = floors_of(plan)
    for i, f in enumerate(fl):
        if f["key"] == key:
            if i + 1 < len(fl) and fl[i + 1].get("z") != None and f.get("z") != None and fl[i + 1]["z"] > f["z"]:
                return fl[i + 1]["z"] - f["z"]
            if f.get("ceiling"):
                return f["ceiling"] + 1.0
    return 10.0


def ceiling(plan, r):
    c = r.get("ceiling")
    if c == None:
        c = _floor(plan, r["floor"]).get("ceiling")
    return c


def rooms_on(plan, fk):
    return [r for r in plan.get("rooms", []) if r["floor"] == fk]


def _room_map(plan):
    return {r["key"]: r for r in plan.get("rooms", [])}


def _node(b):
    """A door's far side as a graph node: every "outside:..." is one node, "outside"."""
    if b == None or b == "outside" or b.startswith("outside:"):
        return "outside"
    return b


def _court(plan, key):
    for c in plan.get("courts", []):
        if c["key"] == key:
            return c
    return None


def _grade(plan):
    return plan.get("site", {}).get("grade", 0.0)


# ---------------------------------------------------------------- rows
def _row(family, what, floor, room, ok, name, detail, rule, severity, at = None, rect = None):
    key = "%s:%s:%s:%s" % (family, what, floor or "-", room or "-")
    r = {"key": key, "name": name, "ok": ok, "detail": detail, "rule": rule, "severity": severity, "floor": floor, "room": room}
    if at != None:
        r["at"] = (at[0], at[1])
    if rect != None:
        r["rect"] = tuple(rect)
    return r


def _rules(rules):
    R = dict(RULES)
    if rules:
        for k, v in rules.items():
            R[k] = v
    return R


def _skipped(R, key):
    for s in R.get("skip", []):
        if key == s or key.startswith(s):
            return True
    return False


# ================================================================ code
def _widest(rects):
    """The widest rectangle a room's pieces make: pieces that meet along an edge combine over the run they share (a few
    rounds), and the one whose least side is longest wins. The main rect for a plain room."""
    pool = [tuple(q) for q in rects]
    for _ in range(3):
        add = []
        for i in range(len(pool)):
            for j in range(len(pool)):
                if i == j:
                    continue
                a, b = pool[i], pool[j]
                if abs(a[3] - b[2]) <= 0.05 and _overlap(a[0], a[1], b[0], b[1]) > 0.5:
                    c = (max(a[0], b[0]), min(a[1], b[1]), a[2], b[3])
                elif abs(a[1] - b[0]) <= 0.05 and _overlap(a[2], a[3], b[2], b[3]) > 0.5:
                    c = (a[0], b[1], max(a[2], b[2]), min(a[3], b[3]))
                else:
                    continue
                if c not in pool and c not in add:
                    add.append(c)
        if not add:
            break
        pool = pool + add
    best = pool[0]
    for q in pool:
        if _least(q) > _least(best) + 1e-9:
            best = q
    return best


def _r304(plan, R):
    out = []
    for r in plan.get("rooms", []):
        if not _habitable(r) or r["kind"] in OUTDOOR:
            continue
        q = _widest(_rects(r))
        a = _area(r)
        least = _least(q)
        kit = r["kind"] == "kitchen"  # R304.1 and R304.2's exception: kitchens
        ok_a = a >= R["hab_area"] - _EPS or kit
        ok_d = least >= R["hab_dim"] - _EPS or kit
        near = abs(a - R["hab_area"]) < 1.0
        area_s = (format(a, ".1f") + " sq ft") if near else sqft(a)
        if not ok_d:
            head = "%s %s wide (R304.2 wants %s)" % (_name(r), ftin(least), ftin(R["hab_dim"]))
        elif not ok_a:
            head = "%s %s (R304.1 wants %s)" % (_name(r), area_s, sqft(R["hab_area"]))
        else:
            head = "%s %s, %s" % (_name(r), _dims(q), area_s)
        tail = []
        if not ok_d or not ok_a:
            tail.append("%s, %s" % (_dims(q), area_s))
        if len(_rects(r)) > 1:
            tail.append("7 ft read on the widest rectangle its pieces make, %s" % _dims(q))
        if kit:
            tail.append("kitchens are exempt from R304's 70 sq ft and 7 ft")
        out.append(_row("code", "R304", r["floor"], r["key"], ok_a and ok_d, "R304: a habitable room is 70 sq ft and 7 ft across",
                        "; ".join([head] + tail), "R304", "fail", rect = q))
    return out


def _r305(plan, R):
    out = []
    for f in floors_of(plan):
        bad, n, lo = [], 0, None
        for r in rooms_on(plan, f["key"]):
            k = r["kind"]
            if _habitable(r) or k in PASSAGES or k in ("entry", "foyer"):
                need = R["ceil_hab"]
            elif k in ("bath", "powder", "wc", "laundry"):
                need = R["ceil_bath"]
            else:
                continue
            c = ceiling(plan, r)
            if c == None:
                continue
            n += 1
            lo = c if lo == None else min(lo, c)
            if c < need - _EPS:
                bad.append(r)
                out.append(_row("code", "R305", f["key"], r["key"], False, "R305: ceilings 7 ft (baths and laundries 6'-8\")",
                                "%s ceiling %s (R305.1 wants %s)" % (_name(r), ftin(c), ftin(need)), "R305", "fail", rect = _main(r)))
        if n and not bad:
            out.append(_row("code", "R305", f["key"], None, True, "R305: ceilings 7 ft (baths and laundries 6'-8\")",
                            "%s: %d rooms checked, the lowest ceiling %s" % (f["key"], n, ftin(lo)), "R305", "fail"))
    return out


def _r311_6(plan, R, G):
    out = []
    for r in plan.get("rooms", []):
        k = r["kind"]
        if k not in PASSAGES:
            continue
        if k != "hall":
            # a landing or lobby is a hallway when it is the way to a room
            served = [n for (n, d) in G["adj"].get(r["key"], []) if n != "outside" and G["rooms"][n]["kind"] not in ("stair", "lift")]
            if not served:
                continue
        # the main rect is the passage; a room's other pieces are nooks and landings beside it (a lift's door, a stair's
        # top step), named but not read as its clear width
        at = _main(r)
        least = _least(at)
        ok = least >= R["hall_min"] - 1e-4
        short = R["hall_min"] - least
        head = "%s %s clear" % (_name(r), ftin(least, 16))
        if not ok:
            head = head + ", %s short of R311.6's %s" % (inches(short), ftin(R["hall_min"]))
        nooks = [q for q in _rects(r)[1:] if _least(q) < R["hall_min"] - 1e-4]
        tail = ["its main rect read; narrower pieces beside it: %s" % ", ".join([_dims(q) for q in nooks])] if nooks else []
        out.append(_row("code", "R311.6", r["floor"], r["key"], ok, "R311.6: a hallway 36 in wide", "; ".join([head] + tail), "R311.6", "fail", rect = at))
    return out


def stair_width(r):
    """A stair room's flight width: room["flight_width"] (plan.star's adapters carry it), room["stair_width"], or
    room["stair"]["width"], else the main rect's least side (a straight run in its own rect)."""
    w = r.get("flight_width")
    if w == None:
        w = r.get("stair_width")
    if w == None and r.get("stair"):
        w = r["stair"].get("width")
    if w == None:
        w = _least(_main(r))
    return w


def _r311_7(plan, R, G):
    out = []
    seen = {}
    stairs = [r for r in plan.get("rooms", []) if r["kind"] == "stair"]
    for s in stairs:
        c = s.get("core") or s["key"]
        w = stair_width(s)
        if c in seen:
            seen[c] = (min(seen[c][0], w), seen[c][1] + [s])
        else:
            seen[c] = (w, [s])
    for c in sorted(seen.keys()):
        w, ss = seen[c]
        ok = w >= R["stair_min"] - 1e-4
        nm = _name(ss[0])
        head = "%s %s wide" % (nm, ftin(w, 16)) + ("" if ok else " (R311.7.1 wants %s)" % ftin(R["stair_min"]))
        out.append(_row("code", "R311.7-width", ss[0]["floor"], ss[0]["key"], ok, "R311.7: stairs 36 in wide",
                        head + ("; on %d floors" % len(ss) if len(ss) > 1 else ""), "R311.7.1", "fail", rect = _main(ss[0])))
    # landings: a landing or lobby that meets a stair's end; its depth across the edge they share is the direction of travel
    # (the stair's ends are its rect's short sides, where its flights arrive)
    for r in plan.get("rooms", []):
        if r["kind"] not in ("landing", "lobby"):
            continue
        for s in stairs:
            if s["floor"] != r["floor"]:
                continue
            # a flight arrives at an end of its run, so a landing is read where it meets the stair rect's short sides
            # (a U-stair's too: its flights run along its long side); along a long side a room is a hallway (R311.6)
            hits, ends = [], []
            for q in _rects(r):
                for sq in _rects(s):
                    f = _facing(q, sq, 1.0)
                    if f == None or f[3] - f[2] < 1.5:
                        continue
                    hits.append((q, f))
                    long_u = _w(sq) >= _d(sq)
                    if (long_u and f[0] in ("E", "W")) or (not long_u and f[0] in ("N", "S")):
                        ends.append((q, f))
            if not hits:
                continue
            hit = (ends or hits)[0]
            q, f = hit
            depth = _d(q) if f[0] in ("N", "S") else _w(q)
            along = f[3] - f[2]
            sw = stair_width(s)
            ok_d = depth >= R["landing_min"] - 1e-4
            ok_w = along >= min(sw, R["stair_min"]) - 1e-4
            head = "%s %s deep in the direction of travel" % (_name(r), ftin(depth, 16))
            if not ok_d:
                head = head + " (R311.7.6 wants %s)" % ftin(R["landing_min"])
            tail = ["%s along the stair's end" % ftin(along, 16) + ("" if ok_w else ", narrower than the %s stair" % ftin(sw, 16))]
            out.append(_row("code", "R311.7-landing", r["floor"], r["key"], ok_d and ok_w,
                            "R311.7: a landing as wide as the stair and 36 in in the direction of travel",
                            "; ".join([head] + tail), "R311.7.6", "fail", rect = q))
    return out


def _opening_reach(plan, R, src):
    """What an escape opening opens onto: ("ok" | "ask" | "fail", words)."""
    s = src or "side"
    if s == "street":
        return ("ok", "onto the street")
    if s in ("rear", "side", "deck"):
        if R["yards_reach"]:
            return ("ok", "onto the %s yard (taken to reach a public way)" % s if s != "deck" else "onto a deck (taken to reach a public way)")
        return ("ask", "onto the %s yard; does it reach a public way?" % s)
    if s == "roof":
        return ("fail", "a roof opening")
    parts = s.split(":", 1)
    kind = parts[0]
    ck = parts[1] if len(parts) > 1 else ""
    c = _court(plan, ck)
    reaches = c.get("reaches") if c else None
    if kind == "well":
        if reaches == None:
            return ("fail", "onto the light well %s, which does not open to a public way (R310.1 wants a yard or court that does)" % ck)
        return ("ask", "ask the building department: the light well %s reaches the %s; does R310.1 take it as a court open to a public way?" % (ck, reaches))
    if reaches == None:
        return ("fail", "onto the %s %s, which does not reach a public way" % (kind, ck))
    return ("ask", "ask the building department: is the %s %s a yard or court that opens to a public way? (it reaches the %s)" % (kind, ck, reaches))


def _escape(plan, R, r, w):
    """One window as an escape opening: (status, words). status: ok, ask, unknown, fail."""
    f = _floor(plan, r["floor"])
    reach, rw = _opening_reach(plan, R, w.get("source"))
    words = [rw]
    status = reach
    sill = w.get("sill", 0.0)
    if sill > R["sill_max"] + 1e-4:
        words.append("its sill %s over the floor (R310.2.2 wants 44 in at most)" % ftin(sill))
        status = "fail"
    z = f.get("z", 0.0)
    at_grade = abs(z + sill - _grade(plan)) <= R["sill_max"]
    need = R["egress_area_grade"] if at_grade else R["egress_area"]
    clear = w.get("clear")
    if clear:
        cw, ch = clear[0], clear[1]
        a = cw * ch
        bad = []
        if a < need - 1e-4:
            bad.append("%s net clear (wants %s)" % (format(a, ".1f") + " sq ft", format(need, ".1f") + " sq ft"))
        if ch < R["egress_h"] - 1e-4:
            bad.append("%s high (wants 24 in)" % inches(ch, 1))
        if cw < R["egress_w"] - 1e-4:
            bad.append("%s wide (wants 20 in)" % inches(cw, 1))
        if bad:
            words.append("; ".join(bad))
            status = "fail"
        else:
            words.append("%s x %s clear, %s sq ft" % (inches(cw, 1), inches(ch, 1), format(a, ".1f")))
    else:
        rough_h = w.get("head", 0.0) - sill
        rough = w["width"] * rough_h
        if rough < need - 1e-4 or w["width"] < R["egress_w"] or rough_h < R["egress_h"]:
            words.append("even the rough opening, %s x %s, is under R310.2.1's %s sq ft, 24 x 20 in" % (ftin(w["width"]), ftin(rough_h), format(need, ".1f")))
            status = "fail"
        elif status == "ok":
            status = "unknown"
            words.append("net clear opening not given (R310.2.1: %s sq ft, 24 in high, 20 in wide)" % format(need, ".1f"))
        else:
            words.append("net clear opening not given")
    return (status, words)


_RANK = {"ok": 0, "ask": 1, "unknown": 2, "fail": 3}


def _r310(plan, R, G):
    out = []
    wins = {}
    for w in plan.get("windows", []):
        wins.setdefault(w["room"], []).append(w)
    rooms = G["rooms"]

    def best_of(r):
        ws = wins.get(r["key"], [])
        marked = [w for w in ws if w.get("egress")]
        cands = marked if marked else ws
        best = None
        for w in cands:
            st, words = _escape(plan, R, r, w)
            if best == None or _RANK[st] < _RANK[best[0]]:
                best = (st, words, w)
        for (n, d) in G["adj"].get(r["key"], []):
            if n == "outside" and d != None:
                b = d.get("b", "") if d.get("a") == r["key"] else d.get("a", "")
                where = b.split(":", 1)[1] if ":" in b else "yard"
                st = "ok" if (where == "street" or R["yards_reach"]) else "ask"
                if best == None or _RANK[st] < _RANK[best[0]]:
                    best = (st, ["a door to the %s" % where], None)
        return best

    for r in plan.get("rooms", []):
        if not _sleeping(r):
            continue
        b = best_of(r)
        if b == None:
            out.append(_row("code", "R310.1", r["floor"], r["key"], False, "R310.1: every sleeping room an escape opening onto a yard or court open to a public way",
                            "%s has no window or outside door to escape by" % _name(r), "R310.1", "fail", rect = _main(r)))
            continue
        st, words, w = b
        wk = (" (%s)" % w["key"]) if w != None and w.get("key") else ""
        sev = "fail" if st in ("ok", "fail") else "note"
        head = "%s: escape%s %s" % (_name(r), wk, words[0])
        out.append(_row("code", "R310.1", r["floor"], r["key"], st == "ok", "R310.1: every sleeping room an escape opening onto a yard or court open to a public way",
                        "; ".join([head] + words[1:]) + ("; unknown until the net clear opening is drawn" if st == "unknown" else ""),
                        "R310.1", sev, at = w["at"] if w != None else None, rect = _main(r) if w == None else None))
    # basements: a floor well below grade wants one escape opening of its own
    for f in floors_of(plan):
        if f.get("z", 0.0) >= _grade(plan) - R["basement_below"]:
            continue
        rs = [r for r in rooms_on(plan, f["key"]) if r["kind"] not in OUTDOOR]
        if not rs:
            continue
        # a basement is a story: a crawl (no habitable room, under 7 ft) is not one
        if not [r for r in rs if _habitable(r)] and (f.get("ceiling") or 0.0) < R["ceil_hab"] - _EPS:
            continue
        best = None
        for r in rs:
            b = best_of(r)
            if b != None and (best == None or _RANK[b[0]] < _RANK[best[0]]):
                best = (b[0], b[1], b[2], r)
        ok = best != None and best[0] == "ok"
        if best == None:
            d = "%s is a basement (its floor %s below grade) with no window or door out" % (f["key"], ftin(_grade(plan) - f.get("z", 0.0)))
            sev = "fail"
        else:
            d = "%s, a basement: %s %s" % (f["key"], _name(best[3]), "; ".join(best[1]))
            sev = "fail" if best[0] in ("ok", "fail") else "note"
        out.append(_row("code", "R310.1-basement", f["key"], None, ok, "R310.1: a basement has an escape opening", d, "R310.1", sev))
    return out


def _fixtures_in(plan, rk):
    return [x for x in plan.get("fixtures", []) if x.get("room") == rk]


def _exit_dist(rects, p, n, maxd = 60.0):
    """How far from p along the axis direction n before leaving the union of rects (a room's floor): exact, rect to
    rect. 0 when p is outside them all."""
    t = 0.0
    for _ in range(len(rects) + 2):
        q = (p[0] + n[0] * t, p[1] + n[1] * t)
        best = t
        for r in rects:
            if not _in(r, q, 1e-6):
                continue
            if n[0] > 0:
                e = r[1] - p[0]
            elif n[0] < 0:
                e = p[0] - r[0]
            elif n[1] > 0:
                e = r[3] - p[1]
            else:
                e = p[1] - r[2]
            best = max(best, e)
        if best <= t + 1e-9:
            break
        t = best
    return min(t, maxd)


def _in_any(rects, p):
    for q in rects:
        if _in(q, p, 1e-9):
            return True
    return False


def _clear_ahead(room, others, fx, front, half):
    """The clear depth in front of a fixture's front edge, over a band `half` either side of its centre line (None:
    the fixture's own width): to the room's walls and to the other fixtures."""
    q = fx["rect"]
    n = _NORMAL[front]
    c = _mid(q)
    rects = _rects(room)
    if front in ("N", "S"):
        e = q[3] if front == "N" else q[2]
        band = (c[0] - half, c[0] + half) if half != None else (q[0], q[1])
        lat = [(band[0] + 1e-3, e), (c[0], e), (band[1] - 1e-3, e)]
    else:
        e = q[1] if front == "E" else q[0]
        band = (c[1] - half, c[1] + half) if half != None else (q[2], q[3])
        lat = [(e, band[0] + 1e-3), (e, c[1]), (e, band[1] - 1e-3)]
    best = _BIG
    for i, p in enumerate(lat):
        # the band's edges count where they are in the room; past its wall the side clearance is what fails
        if i != 1 and not _in_any(rects, p):
            continue
        best = min(best, _exit_dist(rects, p, n))
    for o in others:
        oq = o["rect"]
        if front in ("N", "S"):
            if _overlap(band[0], band[1], oq[0], oq[1]) <= 1e-6:
                continue
            g = oq[2] - e if front == "N" else e - oq[3]
        else:
            if _overlap(band[0], band[1], oq[2], oq[3]) <= 1e-6:
                continue
            g = oq[0] - e if front == "E" else e - oq[1]
        if g >= -1e-6:
            best = min(best, g)
    return best


def _side_clear(room, others, fx, front):
    """From the fixture's centre line to the nearest wall or fixture on each side, over its depth."""
    q = fx["rect"]
    c = _mid(q)
    rects = _rects(room)
    if front in ("N", "S"):
        dirs = (("W", (-1.0, 0.0)), ("E", (1.0, 0.0)))
        span = (q[2], q[3])
    else:
        dirs = (("S", (0.0, -1.0)), ("N", (0.0, 1.0)))
        span = (q[0], q[1])
    res = []
    for (sd, n) in dirs:
        best = _BIG
        for t in (0.1, 0.5, 0.9):
            if front in ("N", "S"):
                p = (c[0], span[0] + t * (span[1] - span[0]))
            else:
                p = (span[0] + t * (span[1] - span[0]), c[1])
            best = min(best, _exit_dist(rects, p, n))
        for o in others:
            oq = o["rect"]
            if front in ("N", "S"):
                if _overlap(span[0], span[1], oq[2], oq[3]) <= 1e-6:
                    continue
                g = (c[0] - oq[1]) if sd == "W" else (oq[0] - c[0])
            else:
                if _overlap(span[0], span[1], oq[0], oq[1]) <= 1e-6:
                    continue
                g = (c[1] - oq[3]) if sd == "S" else (oq[2] - c[1])
            if g >= -1e-6:
                best = min(best, g)
        res.append(best)
    return res


def _fixture_rows(plan, R):
    out = []
    rooms = _room_map(plan)
    drawn = {}
    for x in plan.get("fixtures", []):
        drawn[x.get("room")] = True
    for x in plan.get("fixtures", []):
        r = rooms.get(x.get("room"))
        if r == None:
            continue
        k = x.get("kind", "")
        others = [o for o in _fixtures_in(plan, r["key"]) if o.get("key") != x.get("key") and o["rect"] != x["rect"]]
        nm = "%s %s" % (_name(r), {"wc": "WC", "toilet": "WC", "lav": "lavatory", "vanity": "lavatory", "shower": "shower"}.get(k, k))
        if k in ("wc", "toilet", "lav", "vanity"):
            front = x.get("front")
            if front not in _NORMAL:
                continue
            ahead = _clear_ahead(r, others, x, front, R["wc_side"] if k in ("wc", "toilet") else None)
            ok = ahead >= R["wc_front"] - 1e-4
            irc = ahead >= R["wc_front_irc"] - 1e-4
            head = "%s: %s clear in front" % (nm, inches(ahead, 4))
            words = ["CPC 402.5 wants 24 in (it governs in California; unverified), the IRC's R307.1 21 in%s" % (" (that passes)" if (irc and not ok) else "")]
            if k in ("wc", "toilet"):
                sides = _side_clear(r, others, x, front)
                ok_s = min(sides) >= R["wc_side"] - 1e-4
                words.append("%s and %s from its centre to the sides (15 in each wanted: a 30-in space)" % (inches(sides[0], 4), inches(sides[1], 4)))
                ok = ok and ok_s
                if not ok_s:
                    head = head + ", %s to a side" % inches(min(sides), 4)
            out.append(_row("code", "CPC402.5-" + (x.get("key") or k), r["floor"], r["key"], ok,
                            "CPC 402.5: the WC in a 30-in space with 24 in clear in front (the IRC's R307.1: 21 in)" if k in ("wc", "toilet") else
                            "CPC 402.5: 24 in clear in front of the lavatory (the IRC's R307.1: 21 in)",
                            "; ".join([head] + words), "CPC 402.5", "fail", rect = x["rect"]))
        elif k == "shower":
            q = x["rect"]
            least = _least(q)
            a = _ra(q)
            ok = least >= R["shower_min"] - 1e-4 and a >= R["shower_area"] - 1e-4
            out.append(_row("code", "shower-" + (x.get("key") or "shower"), r["floor"], r["key"], ok,
                            "CPC 408.6 and R307.2: a shower 30 in across and 1,024 sq in",
                            "%s %s x %s, %s sq in%s" % (nm, inches(_w(q), 1), inches(_d(q), 1), format(a * 144.0, ",.0f"),
                                                        "" if ok else " (wants 30 in across and 1,024 sq in)"), "CPC 408.6", "fail", rect = q))
    # rooms with no fixtures drawn: a WC compartment by its size; a bath or powder room cannot be measured
    for r in plan.get("rooms", []):
        if r["kind"] not in BATHS or r["key"] in drawn:
            continue
        q = _main(r)
        if r["kind"] == "wc":
            need_d = R["wc_depth"] + R["wc_front"]
            ok = _least(q) >= 2.0 * R["wc_side"] - 1e-4 and _longest(q) >= need_d - 1e-4
            out.append(_row("code", "CPC402.5-room", r["floor"], r["key"], ok, "CPC 402.5: the WC in a 30-in space with 24 in clear in front (the IRC's R307.1: 21 in)",
                            "%s %s: %s a 30-in WC space %s deep (a %s bowl and 24 in in front; estimated, no fixture drawn)" % (
                                _name(r), _dims(q), "holds" if ok else "does not hold", ftin(need_d), ftin(R["wc_depth"])), "CPC 402.5", "fail", rect = q))
        else:
            out.append(_row("code", "CPC402.5-room", r["floor"], r["key"], False, "CPC 402.5: the WC in a 30-in space with 24 in clear in front (the IRC's R307.1: 21 in)",
                            "%s: not measured, no fixtures drawn (%s)" % (_name(r), _dims(q)), "CPC 402.5", "note", rect = q))
    return out


def _r302_5(plan, R, G):
    out = []
    rooms = G["rooms"]
    for d in plan.get("doors", []):
        a, b = d["a"], _node(d["b"])
        if b == "outside" or a not in rooms or b not in rooms:
            continue
        ra, rb = rooms[a], rooms[b]
        if ra["kind"] == "garage" and rb["kind"] != "garage":
            g, o = ra, rb
        elif rb["kind"] == "garage" and ra["kind"] != "garage":
            g, o = rb, ra
        else:
            continue
        if _sleeping(o):
            out.append(_row("code", "R302.5.1", d.get("floor") or g["floor"], o["key"], False, "R302.5.1: no opening from a garage into a sleeping room",
                            "%s opens from the %s into %s (R302.5.1 forbids it)" % (d.get("key") or "a door", _lower(_name(g)), _lower(_name(o))), "R302.5.1", "fail", at = d.get("at")))
        else:
            out.append(_row("code", "R302.5.1", d.get("floor") or g["floor"], o["key"], True, "R302.5.1: the door from the garage into the house",
                            "the door from the %s into %s: solid wood or steel 1 3/8 in thick, or 20-minute rated, self-closing" % (_lower(_name(g)), _lower(_name(o))),
                            "R302.5.1", "note", at = d.get("at")))
    return out


# ---------------------------------------------------------------- R302.1: openings against the fire separation distance
def _lot_poly(plan):
    s = plan.get("site", {})
    if s.get("lot_poly"):
        return [tuple(p) for p in s["lot_poly"]]
    if s.get("lot"):
        q = s["lot"]
        return [(q[0], q[2]), (q[1], q[2]), (q[1], q[3]), (q[0], q[3])]
    return None


def _ray_to_poly(poly, p, n):
    """Distance from p along unit n to the lot boundary (the nearest crossing ahead), or None."""
    best = None
    m = len(poly)
    for i in range(m):
        a, b = poly[i], poly[(i + 1) % m]
        ex, ey = b[0] - a[0], b[1] - a[1]
        den = n[0] * ey - n[1] * ex
        if abs(den) < 1e-12:
            continue
        wx, wy = a[0] - p[0], a[1] - p[1]
        t = (wx * ey - wy * ex) / den
        s = (wx * n[1] - wy * n[0]) / den
        if t >= -1e-6 and s >= -1e-6 and s <= 1.0 + 1e-6:
            if best == None or t < best:
                best = t
    return best


def _wall_t(plan, R, fk, p, side):
    """The exterior wall's thickness at p: from the plan's walls when it draws them, else rules ext_wall."""
    for w in plan.get("walls", []):
        if w.get("floor") != fk:
            continue
        a, b = w["a"], w["b"]
        if side in ("E", "W") and abs(a[0] - b[0]) < 1e-6:
            if abs(a[0] - p[0]) <= w.get("t", 0.5) + 0.05 and min(a[1], b[1]) - 0.05 <= p[1] and p[1] <= max(a[1], b[1]) + 0.05:
                return w.get("t", R["ext_wall"])
        if side in ("N", "S") and abs(a[1] - b[1]) < 1e-6:
            if abs(a[1] - p[1]) <= w.get("t", 0.5) + 0.05 and min(a[0], b[0]) - 0.05 <= p[0] and p[0] <= max(a[0], b[0]) + 0.05:
                return w.get("t", R["ext_wall"])
    return R["ext_wall"]


def fire_distance(plan, fk, p, side, rules = None):
    """The fire separation distance from a point on a room's wall line (its inside face) facing `side`: through the wall,
    then to the lot line. None if the plan has no lot. (The street side is measured to the lot line here too; R302.1
    measures it to the street's centre line, so callers skip it.)"""
    R = _rules(rules)
    poly = _lot_poly(plan)
    if poly == None:
        return None
    n = _NORMAL[side]
    t = _wall_t(plan, R, fk, p, side)
    q = (p[0] + n[0] * t, p[1] + n[1] * t)
    d = _ray_to_poly(poly, q, n)
    if d == None:
        return 0.0
    return d


def _merge_runs(raw, gap = 1.0):
    """Exterior wall runs on one side and one line (within 0.6 ft) joined across gaps up to `gap`: the wall is continuous
    where a partition meets it, though no room's face is there."""
    out = []
    for side in ("N", "S", "E", "W"):
        rs = sorted([x for x in raw if x[1] == side], key = lambda x: (x[2], x[3]))
        groups = []
        for x in rs:
            placed = False
            for g in groups:
                if abs(g[0][2] - x[2]) <= 0.6:
                    g.append(x)
                    placed = True
                    break
            if not placed:
                groups.append([x])
        for g in groups:
            g = sorted(g, key = lambda x: x[3])
            cur = None
            for x in g:
                if cur != None and x[3] - cur[4] <= gap:
                    cur = (cur[0], cur[1], cur[2], cur[3], max(cur[4], x[4]))
                else:
                    if cur != None:
                        out.append(cur)
                    cur = x
            if cur != None:
                out.append(cur)
    return out


def _r302_1(plan, R):
    out = []
    street = plan.get("site", {}).get("street", "S")
    if _lot_poly(plan) == None:
        return out
    wins = plan.get("windows", [])
    for f in floors_of(plan):
        fk = f["key"]
        h = storey(plan, fk)
        raw = []
        for r in rooms_on(plan, fk):
            if r["kind"] in OUTDOOR:
                continue
            for e in exterior_walls(plan, r["key"]):
                # every exterior wall but the street's, by its direction (plan.star's `facade`; `faces` says what it
                # looks onto): one on a court or slot is as far from the lot line as it is
                if e.get("facade", e["faces"]) != "street":
                    raw.append((r, e["wall"], e["line"], e["span"][0], e["span"][1]))
        runs = _merge_runs(raw)
        for side in ("W", "E", "N", "S"):
            if side == street:
                continue
            bands = {"none": [0.0, 0.0, [], _BIG, None], "limited": [0.0, 0.0, [], _BIG, None]}  # wall sq ft, openings sq ft, names, least fsd, a place
            for (r, sd, line, lo, hi) in runs:
                if sd != side:
                    continue
                pm = ((0.5 * (lo + hi), line) if side in ("N", "S") else (line, 0.5 * (lo + hi)))
                fsd = fire_distance(plan, fk, pm, side, R)
                if fsd == None or fsd >= R["fsd_limited"] - 1e-6:
                    continue
                b = "none" if fsd < R["fsd_none"] - 1e-6 else "limited"
                bands[b][0] += (hi - lo + 2.0 * R["ext_wall"]) * h  # the wall's outer face, corner to corner
                bands[b][3] = min(bands[b][3], fsd)
            for w in wins:
                if w.get("floor") != fk or w.get("wall") != side:
                    continue
                if (w.get("source") or "side") == "roof":
                    continue
                fsd = fire_distance(plan, fk, w["at"], side, R)
                if fsd == None or fsd >= R["fsd_limited"] - 1e-6:
                    continue
                b = "none" if fsd < R["fsd_none"] - 1e-6 else "limited"
                bands[b][1] += w["width"] * max(w.get("head", 0.0) - w.get("sill", 0.0), 0.0)
                bands[b][2].append(w.get("key") or w["room"])
                bands[b][3] = min(bands[b][3], fsd)
                if bands[b][4] == None:
                    bands[b][4] = w["at"]
            for d in plan.get("doors", []):
                if d.get("floor") != fk or d.get("wall") != side or _node(d["b"]) != "outside":
                    continue
                fsd = fire_distance(plan, fk, d["at"], side, R)
                if fsd == None or fsd >= R["fsd_limited"] - 1e-6:
                    continue
                b = "none" if fsd < R["fsd_none"] - 1e-6 else "limited"
                bands[b][1] += d["width"] * (6.0 + 8.0 / 12.0)
                bands[b][2].append(d.get("key") or d["a"])
                bands[b][3] = min(bands[b][3], fsd)
                if bands[b][4] == None:
                    bands[b][4] = d["at"]
            wall = "%s's %s wall" % (fk, _SIDE_WORD[side])
            wn, on, names, fn, place = bands["none"]
            if wn > 0 or on > 0:
                ok = on <= 1e-6
                out.append(_row("code", "R302.1-" + side + "-3ft", fk, None, ok, "R302.1: no openings within 3 ft of a lot line",
                                ("%s %s from the lot line: %s" % (wall, ftin(fn), ("openings " + ", ".join(names)) if names else "no openings")) +
                                "; Table R302.1(1) and (2): none under 3 ft; the wall 1-hour rated from both sides",
                                "R302.1", "fail", at = place if not ok else None))
            wl, ol, names, fl, place = bands["limited"]
            if wl > 0 or ol > 0:
                share = ol / wl if wl > 0 else 1.0
                ok = share <= R["open_share"] + 1e-6
                out.append(_row("code", "R302.1-" + side + "-5ft", fk, None, ok, "R302.1: openings 3 to 5 ft from a lot line at most 25 % of the wall",
                                "%s %s from the lot line: openings %s of its wall; %s of openings (%s) in %s of wall, Table R302.1(1) (no sprinklers) allows 25%%" % (
                                    wall, ftin(fl), pct(share), sqft(ol), ", ".join(names) if names else "none", sqft(wl)) +
                                "; the wall 1-hour rated; sprinklered (R313), Table R302.1(2) would allow unlimited openings at 3 ft or more and no rating: ask the building department",
                                "R302.1", "fail", at = place if not ok else None))
    return out


def code_rows(plan, rules = None):
    """R304, R305, R310.1, R311.6, R311.7, CPC 402.5 / R307.1, showers, R302.5.1 and R302.1 as rows. A kept room (an
    existing one) gets its dimensional rows (R304, R305, R311.6, R311.7, fixtures) as notes: existing, not new work.
    Escape openings and garage doors keep their weight."""
    plan = normalize(plan)
    R = _rules(rules)
    G = door_graph(plan)
    out = []
    dims = []
    dims.extend(_r304(plan, R))
    dims.extend(_r305(plan, R))
    dims.extend(_r311_6(plan, R, G))
    dims.extend(_r311_7(plan, R, G))
    dims.extend(_fixture_rows(plan, R))
    kept = {r["key"]: True for r in plan.get("rooms", []) if _kept(r)}
    for x in dims:
        if x.get("room") in kept and not x["ok"] and x["severity"] != "note":
            x = dict(x, severity = "note", detail = x["detail"] + "; an existing room, kept")
        out.append(x)
    out.extend(_r310(plan, R, G))
    out.extend(_r302_5(plan, R, G))
    out.extend(_r302_1(plan, R))
    return [r for r in out if not _skipped(R, r["key"])]


# ================================================================ doors, as a graph
def door_graph(plan):
    """Rooms joined by their doors (every "outside:..." is one node, "outside"), and each stair or lift joined to the
    same core's stair or lift on the next floor: {"rooms": {key: room}, "adj": {node: [(node, door or None)]}}."""
    rooms = _room_map(plan)
    adj = {k: [] for k in rooms}
    adj["outside"] = []
    for d in plan.get("doors", []):
        a, b = d["a"], _node(d.get("b"))
        if a not in rooms or (b != "outside" and b not in rooms):
            continue
        adj[a].append((b, d))
        adj[b].append((a, d))
    cores = {}
    for r in plan.get("rooms", []):
        c = r.get("core")
        if c and r["kind"] in ("stair", "lift"):
            cores.setdefault(c, []).append(r)
    for c, rs in cores.items():
        rs = sorted(rs, key = lambda r: _floor(plan, r["floor"]).get("z", 0.0))
        for i in range(len(rs) - 1):
            adj[rs[i]["key"]].append((rs[i + 1]["key"], None))
            adj[rs[i + 1]["key"]].append((rs[i]["key"], None))
    return {"rooms": rooms, "adj": adj}


def reach(G, starts, banned = None):
    """The nodes reached from `starts` without entering a banned node (a dict or list of keys)."""
    ban = {}
    for b in (banned or []):
        ban[b] = True
    seen = {}
    q = []
    for s in starts:
        if s not in ban and s not in seen:
            seen[s] = True
            q.append(s)
    for _ in range(len(G["adj"]) + 2):
        if not q:
            break
        x = q.pop()
        for (y, d) in G["adj"].get(x, []):
            if y in ban or y in seen:
                continue
            seen[y] = True
            q.append(y)
    return seen


def entries(plan, G, unit = "main"):
    """A dwelling unit's front doors: its rooms with a door to the street; else to any outside; garages last."""
    rooms = G["rooms"]
    best = []
    for want in ("street", "any", "garage"):
        for r in plan.get("rooms", []):
            if _unit(r) != unit:
                continue
            if (want == "garage") != (r["kind"] == "garage"):
                continue
            for (n, d) in G["adj"].get(r["key"], []):
                if n != "outside" or d == None:
                    continue
                far = d.get("b") if d.get("a") == r["key"] else d.get("a")
                if want == "street" and far != "outside:street":
                    continue
                if r["key"] not in best:
                    best.append(r["key"])
        if best:
            return best
    # no outside door at all: an entry, foyer or lobby
    return [r["key"] for r in plan.get("rooms", []) if _unit(r) == unit and r["kind"] in ENTRIES]


def _house_inside(plan, G, unit = "main"):
    """What a unit's front door reaches without passing a garage or going outside."""
    gar = [k for k, r in G["rooms"].items() if r["kind"] == "garage"]
    starts = [e for e in entries(plan, G, unit) if G["rooms"][e]["kind"] != "garage"]
    return reach(G, starts, gar + ["outside"])


def _door_pts(plan, fk):
    """The doors on a floor as (point, a, b) with the far side as a node."""
    out = []
    for d in plan.get("doors", []):
        if d.get("floor") != fk or d.get("at") == None:
            continue
        out.append((tuple(d["at"]), d["a"], _node(d.get("b")), d))
    return out


def walks(plan, fk, start_room, start_pt, targets, banned = None):
    """Walking distances on one floor, through door points: Manhattan from point
    to point inside each room, from start_pt (in start_room; None starts on start_room's door points) to each target
    room's centre. Never through "outside" or a banned room. {target: ft or None}."""
    rooms = _room_map(plan)
    ban = {"outside": True}
    for b in (banned or []):
        ban[b] = True
    D = _door_pts(plan, fk)
    n = len(D)
    dist = [None] * n
    for i in range(n):
        p, a, b, _ = D[i]
        if a == start_room or b == start_room:
            dist[i] = 0.0 if start_pt == None else _man(start_pt, p)
    done = [False] * n
    for _ in range(n):
        bi, bd = -1, None
        for i in range(n):
            if not done[i] and dist[i] != None and (bd == None or dist[i] < bd):
                bi, bd = i, dist[i]
        if bi < 0:
            break
        done[bi] = True
        p, a, b, _ = D[bi]
        for x in (a, b):
            if x in ban and x != start_room:
                continue
            for j in range(n):
                if done[j]:
                    continue
                pj, aj, bj, _ = D[j]
                if aj != x and bj != x:
                    continue
                nd = bd + _man(p, pj)
                if dist[j] == None or nd < dist[j]:
                    dist[j] = nd
    out = {}
    for t in targets:
        r = rooms.get(t)
        if r == None:
            out[t] = None
            continue
        c = _mid(_main(r))
        if t == start_room and start_pt != None:
            out[t] = _man(start_pt, c)
            continue
        best = None
        for i in range(n):
            p, a, b, _ = D[i]
            if dist[i] == None or (a != t and b != t):
                continue
            v = dist[i] + _man(p, c)
            if best == None or v < best:
                best = v
        out[t] = best
    return out


def _nbrs(G, k):
    return [n for (n, d) in G["adj"].get(k, []) if d != None]


def _doors_of(G, k):
    return [(n, d) for (n, d) in G["adj"].get(k, []) if d != None]


def _suite(G, b):
    """A bath's own bedrooms: those opening onto it, or onto a closet that opens onto it."""
    rooms = G["rooms"]
    own = {}
    for n in _nbrs(G, b["key"]):
        if n == "outside":
            continue
        if _sleeping(rooms[n]):
            own[n] = True
        elif rooms[n]["kind"] in ("closet", "storage"):
            for m in _nbrs(G, n):
                if m != "outside" and _sleeping(rooms[m]):
                    own[m] = True
    return own


def _bath_cost(G, b, k, own = None):
    """What passing room k costs on the way to bath b: a garage, a closet, the outside or a bedroom not b's own, 1."""
    if k == "outside":
        return 1
    r = G["rooms"][k]
    if r["kind"] == "garage" or r["kind"] in STORAGE:
        return 1
    if _sleeping(r):
        o = own if own != None else _suite(G, b)
        return 0 if k in o else 1
    return 0


def _cheapest_way(plan, G, b, starts):
    """The way from a unit's entries to bath b passing the fewest garages, closets, outside legs and other bedrooms:
    the list of nodes, entry first, or None."""
    own = _suite(G, b)
    nodes = list(G["adj"].keys())
    INF = 1 << 30
    cost = {k: INF for k in nodes}
    prev = {}
    for s in starts:
        cost[s] = _bath_cost(G, b, s, own)
        prev[s] = None
    done = {}
    for _ in range(len(nodes)):
        best, bc = None, INF
        for k in nodes:
            if k not in done and cost[k] < bc:
                best, bc = k, cost[k]
        if best == None:
            break
        done[best] = True
        if best == b["key"]:
            break
        for (y, d) in G["adj"].get(best, []):
            c = bc + (0 if y == b["key"] else _bath_cost(G, b, y, own))
            if c < cost.get(y, INF):
                cost[y] = c
                prev[y] = best
    if cost.get(b["key"], INF) >= INF:
        return None
    path = [b["key"]]
    cur = b["key"]
    for _ in range(len(nodes) + 1):
        p = prev.get(cur)
        if p == None:
            break
        path.append(p)
        cur = p
    return list(reversed(path))


def door_rows(plan, rules = None):
    """The door graph's rows: a door from the garage into the house; R302.5.1's ban is in code_rows; the pantry off
    the kitchen; no bath reached only through a garage, a closet or another bedroom; the powder room off circulation;
    bedrooms off day rooms (a warning); every room reachable from its unit's entry; dead-end halls; the kitchen's door
    count; walks from the kitchen to the pantry and from the stair to each bedroom."""
    plan = normalize(plan)
    R = _rules(rules)
    G = door_graph(plan)
    rooms = G["rooms"]
    out = []
    units = {}
    for r in plan.get("rooms", []):
        units[_unit(r)] = True
    inside = {}
    full = {}
    for u in units:
        inside[u] = _house_inside(plan, G, u)
        full[u] = reach(G, entries(plan, G, u) + ["outside"])

    # every room reachable from its unit's entry (passing outside allowed); habitable rooms not only through the garage
    for r in plan.get("rooms", []):
        if r["kind"] in OUTDOOR:
            continue
        u = _unit(r)
        if r["key"] not in full[u]:
            out.append(_row("doors", "reach", r["floor"], r["key"], False, "Every room reachable from the entry",
                            "%s: no way to it from the entry" % _name(r) + ("; an existing room, kept: its doors may not be in the plan" if _kept(r) else ""),
                            "doors: reach", "note" if _kept(r) else "fail", rect = _main(r)))
        elif _habitable(r) and r["key"] not in inside[u] and not _kept(r):
            out.append(_row("doors", "via-garage", r["floor"], r["key"], False, "No room reached only through the garage",
                            "%s: reached only through the garage or from outside" % _name(r), "doors: reach", "fail", rect = _main(r)))

    # the garage's door into the house
    for g in [r for r in plan.get("rooms", []) if r["kind"] == "garage"]:
        u = _unit(g)
        into = [n for n in _nbrs(G, g["key"]) if n != "outside" and n in inside[u]]
        if into:
            out.append(_row("doors", "garage-door", g["floor"], g["key"], True, "A door from the garage into the house",
                            "%s opens into %s" % (_name(g), ", ".join([_lower(_name(rooms[n])) for n in into])), "doors: garage", "fail", rect = _main(g)))
        else:
            out.append(_row("doors", "garage-door", g["floor"], g["key"], False, "A door from the garage into the house",
                            "No door from the garage into the house; %s opens only to %s" % (_lower(_name(g)), ", ".join(
                                sorted([("outside" if n == "outside" else _lower(_name(rooms[n]))) for n in {x: 1 for x in _nbrs(G, g["key"])}.keys()])) or "nothing"),
                            "doors: garage", "fail", rect = _main(g)))

    # baths: never reached only through a garage, a closet or another bedroom
    for b in [r for r in plan.get("rooms", []) if r["kind"] in BATHS]:
        if _kept(b):
            continue
        u = _unit(b)
        path = _cheapest_way(plan, G, b, entries(plan, G, u))
        if path == None:
            continue  # unreachable: the reach rows say so
        blockers = [k for k in path if k != b["key"] and _bath_cost(G, b, k) > 0]
        if not blockers:
            continue
        nb = _nbrs(G, b["key"])
        rank = lambda k: 0 if (k != "outside" and rooms[k]["kind"] == "garage") else (1 if (k != "outside" and rooms[k]["kind"] in STORAGE) else (2 if k != "outside" else 3))
        first = sorted(blockers, key = rank)[0]
        if len(nb) == 1 and nb[0] != "outside" and rooms[nb[0]]["kind"] == "garage":
            head = "%s: its only door is from the %s" % (_name(b), _lower(_name(rooms[nb[0]])))
        elif first == "outside":
            head = "%s reached only from outside" % _name(b)
        else:
            v = rooms[first]
            head = "%s reached only through the %s (%s)" % (_name(b), _lower(_name(v)), _dims(_main(v)))
        out.append(_row("doors", "bath-reach", b["floor"], b["key"], False, "No bath reached only through a garage, a closet or another bedroom",
                        head + "; the way in: " + " > ".join([("outside" if k == "outside" else _lower(_name(rooms[k]))) for k in path]),
                        "doors: baths", "fail", rect = _main(b)))

    # powder rooms (and hall baths) off circulation, not off living, dining or kitchen
    for b in [r for r in plan.get("rooms", []) if r["kind"] in BATHS]:
        if _kept(b):
            continue
        bad = [n for n in _nbrs(G, b["key"]) if n != "outside" and rooms[n]["kind"] in ("living", "dining", "kitchen", "family")]
        if b["kind"] != "powder" and not bad:
            continue
        ok = not bad
        out.append(_row("doors", "powder", b["floor"], b["key"], ok, "The powder room off circulation, not off living, dining or kitchen",
                        ("%s opens off %s" % (_name(b), ", ".join([_lower(_name(rooms[n])) for n in _nbrs(G, b["key"]) if n != "outside"]) or "nothing")) if ok else
                        "%s opens into the %s" % (_name(b), " and the ".join([_lower(_name(rooms[n])) for n in bad])), "doors: powder", "warn", rect = _main(b)))

    # bedrooms off day rooms or other bedrooms: a warning only
    for r in plan.get("rooms", []):
        if not _sleeping(r) or _kept(r):
            continue
        bad = [n for n in _nbrs(G, r["key"]) if n != "outside" and (rooms[n]["kind"] in DAY or (_sleeping(rooms[n]) and n != r["key"]))]
        if bad:
            out.append(_row("doors", "bed-off-day", r["floor"], r["key"], False, "Bedrooms open off circulation, not off day rooms",
                            "%s opens off the %s" % (_name(r), " and the ".join([_lower(_name(rooms[n])) for n in bad])), "doors: bedrooms", "warn", rect = _main(r)))

    # the pantry off the kitchen, and the walk between them
    for p in [r for r in plan.get("rooms", []) if r["kind"] == "pantry"]:
        if _kept(p):
            continue
        ks = [r for r in rooms_on(plan, p["floor"]) if r["kind"] == "kitchen" and _unit(r) == _unit(p)]
        if not ks:
            out.append(_row("doors", "pantry", p["floor"], p["key"], True, "The pantry off the kitchen",
                            "%s: no kitchen on %s (a bar or a store)" % (_name(p), p["floor"]), "doors: pantry", "note", rect = _main(p)))
            continue
        best = None
        for k in ks:
            w = walks(plan, p["floor"], k["key"], _mid(_main(k)), [p["key"]])[p["key"]]
            adjacent = k["key"] in _nbrs(G, p["key"])
            if best == None or (w != None and (best[1] == None or w < best[1])):
                best = (k, w, adjacent)
        k, w, adjacent = best
        ok = adjacent or (w != None and w <= R["pantry_walk"])
        if adjacent:
            d = "%s opens off the %s (a %s walk, centre to centre)" % (_name(p), _lower(_name(k)), ftin(w) if w != None else "short")
        elif w == None:
            d = "%s: no way to it from the %s on this floor" % (_name(p), _lower(_name(k)))
        else:
            d = "%s: a %s walk from the %s" % (_name(p), ftin(w), _lower(_name(k))) + ("" if ok else "; its door is not on the kitchen")
        out.append(_row("doors", "pantry", p["floor"], p["key"], ok, "The pantry off the kitchen", d, "doors: pantry", "warn", rect = _main(p)))

    # the kitchen's doors
    for k in [r for r in plan.get("rooms", []) if r["kind"] == "kitchen"]:
        if _kept(k):
            continue
        ds = [(x, d) for (x, d) in _doors_of(G, k["key"]) if d.get("swing") != "open"]  # open plan is no door
        n = len(ds)
        ok = n <= R["kitchen_doors"]
        opens = [x for (x, d) in _doors_of(G, k["key"]) if d.get("swing") == "open"]
        out.append(_row("doors", "kitchen-doors", k["floor"], k["key"], ok, "The kitchen's doors: at most 3 (each one breaks a counter run)",
                        "%s: %d door%s (%s)" % (_name(k), n, "" if n == 1 else "s", ", ".join([("outside" if x == "outside" else _lower(_name(rooms[x]))) for (x, d) in ds])) +
                        ("; open to the %s" % ", ".join([_lower(_name(rooms[x])) for x in opens]) if opens else ""),
                        "doors: kitchen", "warn", rect = _main(k)))

    # halls: the rooms they serve, and how far they run past their last door
    for h in [r for r in plan.get("rooms", []) if r["kind"] == "hall"]:
        if _kept(h):
            continue
        q = _main(h)
        along_u = _w(q) >= _d(q)
        L = _longest(q)
        served, ends = [], []
        for (n, d) in _doors_of(G, h["key"]):
            kind = "outside" if n == "outside" else rooms[n]["kind"]
            if kind in ("closet", "storage", "mech", "pantry"):
                continue
            at = d.get("at")
            if at != None:
                ends.append(at[0] if along_u else at[1])
            if kind not in ("stair", "lift") and n not in served:
                served.append(n)
        lo, hi = (q[0], q[1]) if along_u else (q[2], q[3])
        dead = 0.0
        if ends:
            dead = max(min(ends) - lo, hi - max(ends))
        else:
            dead = L
        only = len(served) <= 1 and L >= R["hall_len"]
        ok = not only and dead < R["dead_end"]
        words = []
        if only:
            words.append("%s hall serves only %s" % (ftin(L), ("the " + _lower(_name(rooms[served[0]]))) if served and served[0] != "outside" else ("the outside door" if served else "closets")))
        if dead >= R["dead_end"]:
            words.append("%s past its last door" % ftin(dead))
        if not words:
            words.append("%s %s, serving %s" % (_name(h), ftin(L), ", ".join([("outside" if n == "outside" else _lower(_name(rooms[n]))) for n in served]) or "nothing"))
        out.append(_row("doors", "hall", h["floor"], h["key"], ok, "Halls that lead somewhere: no dead ends, no hall for one room",
                        "; ".join(words), "doors: halls", "warn", rect = q))

    # walks from the stair to each bedroom, floor by floor (a note; a mark past rules bed_walk)
    for f in floors_of(plan):
        st = [r for r in rooms_on(plan, f["key"]) if r["kind"] == "stair"]
        beds = [r for r in rooms_on(plan, f["key"]) if _sleeping(r)]
        if not st or not beds:
            continue
        best = {}
        for s in st:
            w = walks(plan, f["key"], s["key"], None, [b["key"] for b in beds])
            for k, v in w.items():
                if v != None and (best.get(k) == None or v < best[k]):
                    best[k] = v
        far = [b for b in beds if best.get(b["key"]) != None and best[b["key"]] > R["bed_walk"]]
        words = ["%s %s" % (_name(b), ftin(best[b["key"]]) if best.get(b["key"]) != None else "no way") for b in beds]
        out.append(_row("doors", "walks", f["key"], None, not far, "Walks from the stair to the bedrooms, through the doors",
                        "%s, from the stair: %s" % (f["key"], ", ".join(words)), "doors: walks", "note" if not far else "warn"))
    return [r for r in out if not _skipped(R, r["key"])]


# ================================================================ stacks
def stack_rows(plan, rules = None):
    """Plumbing stacks: every wet room above the lowest floor on a stack in its walls; no stack standing in a day room,
    a hall's clear width, a garage's clear space or a stair; every stack continuous over its floors and in a wall on
    each of them."""
    plan = normalize(plan)
    R = _rules(rules)
    tol = R["stack_tol"]
    out = []
    fl = floors_of(plan)
    order = {f["key"]: i for i, f in enumerate(fl)}
    lowest = fl[0]["key"] if fl else None
    stacks = plan.get("stacks", [])
    for r in plan.get("rooms", []):
        if r["kind"] not in WET or _kept(r):
            continue
        on, implied = [], []
        for s in stacks:
            if r["floor"] not in s.get("floors", []):
                continue
            dmin = min([_dist_rect(q, s["at"]) for q in _rects(r)])
            if dmin <= tol + 1e-9:
                (implied if s.get("implied") else on).append(s.get("key") or "a stack")
        if implied and not on:
            out.append(_row("stacks", "wet", r["floor"], r["key"], True, "Every wet room on a stack in its walls",
                            "%s on %s, an implied drain (the plan draws no stack there)" % (_name(r), ", ".join(implied)), "stacks", "note", rect = _main(r)))
            continue
        if r["floor"] == lowest and not on:
            out.append(_row("stacks", "wet", r["floor"], r["key"], True, "Every wet room on a stack in its walls",
                            "%s: no stack, on the lowest floor (it can drain under the slab)" % _name(r), "stacks", "note", rect = _main(r)))
            continue
        out.append(_row("stacks", "wet", r["floor"], r["key"], len(on) > 0, "Every wet room on a stack in its walls",
                        ("%s on %s" % (_name(r), ", ".join(on))) if on else "%s: no stack within %s of its walls" % (_name(r), ftin(tol)),
                        "stacks", "fail", rect = _main(r)))
    for s in stacks:
        sk = s.get("key") or "stack"
        p = s["at"]
        fs = [f for f in s.get("floors", []) if f in order]
        idx = sorted([order[f] for f in fs])
        # a floor between its ends that it does not list is a gap only where that floor is built under the stack (a
        # floor of another part of the house, beside it, is not)
        gaps = [fl[i]["key"] for i in range(idx[0], idx[-1] + 1) if i not in idx and
                any([_dist_rect(q, p) <= tol for x in rooms_on(plan, fl[i]["key"]) for q in _rects(x)])] if idx else []
        problems = []
        for fk in fs:
            near, standing = False, []
            for r in rooms_on(plan, fk):
                if r["kind"] in OUTDOOR:
                    continue
                for q in _rects(r):
                    dep = _depth_in(q, p)
                    if dep > tol:
                        standing.append(r)
                    elif dep > -tol - 1e-9:
                        near = True
                        if r["kind"] == "hall" and dep > 0.05:
                            # in the hall's wall but proud of it: what is left of the clear width
                            left = _least(q) - dep - 0.25
                            if left < R["hall_min"] - 1e-4:
                                problems.append(("fail", fk, r, "%s stands %s into the %s: %s clear past it" % (sk, inches(dep, 1), _lower(_name(r)), ftin(left, 16))))
            for r in standing:
                k = r["kind"]
                if k in DAY or k in ("garage", "stair", "lift") or k == "hall":
                    where = {"garage": "the garage's clear space", "hall": "the hall's clear width", "stair": "the stair", "lift": "the lift"}.get(k, "the " + _lower(_name(r)))
                    problems.append(("fail", fk, r, "%s stands in %s on %s, not in a wall" % (sk, where, fk)))
                elif _sleeping(r):
                    problems.append(("warn", fk, r, "%s stands in %s on %s, not in a wall" % (sk, _lower(_name(r)), fk)))
            if not near and not standing:
                problems.append(("fail", fk, None, "%s has no wall to run in on %s (no room within %s)" % (sk, fk, ftin(tol))))
        if gaps:
            problems.append(("fail", fs[0], None, "%s skips %s: a stack runs every floor between its ends" % (sk, ", ".join(gaps))))
        if not problems:
            out.append(_row("stacks", "stack-" + sk, fs[0] if fs else None, None, True, "Stacks in walls on every floor, continuous",
                            "%s in a wall on %s" % (sk, ", ".join(fs)), "stacks", "fail", at = p))
        for (sev, fk, r, words) in problems:
            out.append(_row("stacks", "stack-" + sk, fk, r["key"] if r else None, False, "Stacks in walls on every floor, continuous",
                            words, "stacks", sev, at = p))
    return [r for r in out if not _skipped(R, r["key"])]


# ================================================================ fillers
def _program_unasked(plan, program):
    """The rooms a program did not ask for. program: a list of room keys or kinds (strings), of dicts with "key" or
    "kind" (and "count"), or a dict of key or kind to a count or a spec."""
    rooms = plan.get("rooms", [])
    keys = {r["key"]: True for r in rooms}
    want_keys, want_kinds = {}, {}
    items = []
    if type(program) == "dict":
        for k, v in program.items():
            items.append((k, v))
    else:
        for it in program:
            items.append((it, None))
    for (k, v) in items:
        if type(k) == "dict":
            if k.get("key"):
                want_keys[k["key"]] = True
            elif k.get("kind"):
                want_kinds[k["kind"]] = want_kinds.get(k["kind"], 0) + int(k.get("count", 1))
            continue
        if k in keys:
            want_keys[k] = True
        else:
            n = v if type(v) == "int" else ((v or {}).get("count", 1) if type(v) == "dict" else 1)
            want_kinds[k] = want_kinds.get(k, 0) + n
    out = []
    for r in rooms:
        if r["key"] in want_keys:
            continue
        if want_kinds.get(r["kind"], 0) > 0:
            want_kinds[r["kind"]] -= 1
            continue
        if r["kind"] in CIRCULATION or r["kind"] in OUTDOOR or _kept(r):
            continue
        out.append(r)
    return out


def filler_rows(plan, program = None, rules = None):
    """Filler rooms: rooms the program did not ask for (when given); service rooms that are leftover depth (slivers)
    or oversized; storage, closet and hall shares per floor; habitable rooms with no window (R303.1's 8 %); habitable
    rooms at 2:1 or longer."""
    plan = normalize(plan)
    R = _rules(rules)
    out = []
    if program != None:
        for r in _program_unasked(plan, program):
            out.append(_row("fillers", "unasked", r["floor"], r["key"], False, "Rooms the program asked for, and no others",
                            "%s %s, %s: the program did not ask for it" % (_name(r), _dims(_main(r)), sqft(_area(r))), "fillers", "warn", rect = _main(r)))
    for r in plan.get("rooms", []):
        if _kept(r):
            continue
        k = r["kind"]
        q = _main(r)
        a = _area(r)
        words = []
        if k in STORAGE or k in ("mud", "laundry") or k in BATHS:
            if _least(q) > 0 and _longest(q) / _least(q) > R["sliver"] and a > R["sliver_area"]:
                words.append("leftover depth")
        if (k in ("closet", "storage", "mech", "mud")) and a > R["filler_area"]:
            words.append("a %s filler" % sqft(a))
        if words:
            out.append(_row("fillers", "leftover", r["floor"], r["key"], False, "Service rooms sized for a use, not leftover depth",
                            "%s %s: %s" % (_dims(q), _lower(_name(r)), ", ".join(words)), "fillers", "warn", rect = q))
    for f in floors_of(plan):
        rs = rooms_on(plan, f["key"])
        net = sum_([_area(r) for r in rs if r["kind"] not in ("garage", "stair", "lift") and r["kind"] not in OUTDOOR])
        if net <= 0 or not [r for r in rs if _habitable(r)]:
            continue  # a floor of storage and services only (a crawl, a cellar) has no share to keep
        st = sum_([_area(r) for r in rs if r["kind"] in ("closet", "storage", "pantry", "mech")])
        hl = sum_([_area(r) for r in rs if r["kind"] in ("hall", "entry", "foyer", "mud", "landing", "lobby")])
        ok = st <= R["storage_share"] * net + 1e-6 and hl <= R["hall_share"] * net + 1e-6
        out.append(_row("fillers", "shares", f["key"], None, ok, "Storage and halls a modest share of each floor",
                        "%s: storage, closets and pantry %s (%s), halls %s (%s) of %s net (garage, stair and courts out; %s and %s wanted at most)" % (
                            f["key"], sqft(st), pct(st / net), sqft(hl), pct(hl / net), sqft(net), pct(R["storage_share"]), pct(R["hall_share"])),
                        "fillers", "warn" if not ok else "note"))
    wins = {}
    for w in plan.get("windows", []):
        wins.setdefault(w.get("room"), []).append(w)
    for r in plan.get("rooms", []):
        if not _habitable(r) or r["kind"] in OUTDOOR:
            continue
        ws = wins.get(r["key"], [])
        a = _area(r)
        glass = sum_([w.get("width", 0.0) * max(w.get("head", 0.0) - w.get("sill", 0.0), 0.0) for w in ws])
        if not ws:
            out.append(_row("fillers", "window", r["floor"], r["key"], False, "Habitable rooms with a window (R303.1)",
                            "%s has no window; R303.1 wants glass of 8%% of the floor, or artificial light and ventilation" % _name(r),
                            "R303.1", "warn" if r["kind"] == "kitchen" else "fail", rect = _main(r)))
        else:
            ok = glass >= R["glazing"] * a - 1e-6
            out.append(_row("fillers", "window", r["floor"], r["key"], ok, "Habitable rooms with a window (R303.1)",
                            "%s: glass %s of its floor; %s of glass%s" % (_name(r), pct(glass / a if a > 0 else 0.0), sqft(glass),
                                                                       "" if ok else "; R303.1 wants 8%, or artificial light and ventilation"),
                            "R303.1", "warn" if not ok else "note", rect = _main(r)))
    for r in plan.get("rooms", []):
        if not _habitable(r) or r["kind"] == "kitchen" or _kept(r):
            continue
        q = _widest(_rects(r))
        if _least(q) <= 0:
            continue
        ratio = _longest(q) / _least(q)
        if ratio >= R["aspect"] - 0.02:
            out.append(_row("fillers", "shape", r["floor"], r["key"], False, "Habitable rooms under 2:1",
                            "%s %s: %s:1" % (_name(r), _dims(q), format(ratio, ".1f")), "fillers", "warn", rect = q))
    return [r for r in out if not _skipped(R, r["key"])]


# ================================================================ furniture
def _safe(x):
    out = ""
    for c in str(x).elems():
        out += c if (c.isalnum() or c == "_") else "_"
    return out


def furnish_rows(plan, rules = None, rooms = None, name = None):
    """Each furnished room re-packed at its final size with every door and every fixture reachable: @cadpole/plan_furnish.
    star's room stage on plan.star's room_local(). The rooms: `rooms` (keys), else rules
    "furnish_rooms", else every room that names a furniture set and every sleeping room (its kind's set). Each solve is
    a feature named <name>_<room> (name defaults to plan_audit_<plan>); audit() passes its own."""
    plan = normalize(plan)
    R = _rules(rules)
    nm = _safe(name or ("plan_audit_" + str(plan.get("name") or "plan")))
    keys = rooms if rooms != None else R.get("furnish_rooms")
    if keys == None:
        keys = [r["key"] for r in plan.get("rooms", []) if not _kept(r) and (r.get("furnish") or _sleeping(r))]
    byk = _room_map(plan)
    out = []
    for k in keys:
        r = byk.get(k)
        if r == None:
            continue
        res = furnish(room_local(plan, k), name = "%s_%s" % (nm, _safe(k)))
        st = res.get("set")
        if st == None:
            continue
        label = SETS[st]["label"] if st in SETS else st
        q = _main(r)
        if res["ok"]:
            # plan_furnish packs a room of several rects in its bounding box: say when a piece needs the other rects
            spill = []
            for f in res.get("furniture", []):
                fr = f.get("rect")
                if fr != None and (fr[0] < q[0] - 0.05 or fr[1] > q[1] + 0.05 or fr[2] < q[2] - 0.05 or fr[3] > q[3] + 0.05):
                    nm_ = f.get("item") or f.get("kind") or "piece"
                    if nm_ not in spill:
                        spill.append(nm_)
            out.append(_row("furnish", "repack", r["floor"], k, True, "Furniture re-packed at the drawn size, with every door",
                            "%s %s holds the %s set (%s), every use point reachable" % (_name(r), _dims(q), st, label) +
                            ("; its %s stand%s partly in the room's other rects (a bay, an alcove), outside its main %s" % (
                                " and ".join(spill), "s" if len(spill) == 1 else "", _dims(q)) if spill else ""),
                            "furnish (plan_furnish)", "fail", rect = q))
            continue
        need = res.get("need_depth")
        if res.get("relaxed") != None:
            head = "%s %s: the %s set fits only off the stack wall" % (_name(r), _dims(q), st)
            sev = "warn"
        elif "does not fit" in res["reason"] and need == None:
            head = "%s, %s wide: the %s set does not fit at any depth" % (_name(r), ftin(_w(q)), st)
            sev = "fail"
        elif "does not fit" in res["reason"]:
            head = "%s, %s deep: the %s set needs %s at %s wide" % (_name(r), ftin(_d(q)), st, ftin(need), ftin(_w(q)))
            sev = "fail"
        else:
            head = "%s %s: the %s set does not pack" % (_name(r), _dims(q), st)
            sev = "fail" if res.get("status") in ("optimal", "infeasible") else "note"
        out.append(_row("furnish", "repack", r["floor"], k, False, "Furniture re-packed at the drawn size, with every door",
                        "%s; %s (%s); plan_furnish: %s" % (head, label, _dims(q), res["reason"]), "furnish (plan_furnish)", sev, rect = q))
    return [x for x in out if not _skipped(R, x["key"])]


# ================================================================ isovists (reported only; kept out of every loop)
def _slab(q, p, d):
    """The interval of t >= 0 where p + t d is inside rect q, or None."""
    t0, t1 = 0.0, _BIG
    for (pa, da, lo, hi) in ((p[0], d[0], q[0], q[1]), (p[1], d[1], q[2], q[3])):
        if abs(da) < 1e-12:
            if pa < lo - 1e-9 or pa > hi + 1e-9:
                return None
            continue
        a, b = (lo - pa) / da, (hi - pa) / da
        if a > b:
            a, b = b, a
        t0, t1 = max(t0, a), min(t1, b)
        if t0 > t1:
            return None
    return (t0, t1)


def isovist(plan, floor, at, rays = 180, doors = "all", reach = 80.0):
    """What an eye at `at` on `floor` sees of the floor, with its doors open (doors="open": cased openings only):
    {"area": sq ft, "far": ft, "share": of the floor's indoor area}. The free space is the floor's indoor rooms and a
    1-ft bridge across each doorway; each ray runs while those overlap from the eye (exact, interval by interval)."""
    plan = normalize(plan)
    free = []
    tot = 0.0
    for r in rooms_on(plan, floor):
        if r["kind"] in OUTDOOR:
            continue
        for q in _rects(r):
            free.append(q)
            tot += _ra(q)
    for d in plan.get("doors", []):
        if d.get("floor") != floor or d.get("at") == None or _node(d.get("b")) == "outside":
            continue
        if doors == "open" and d.get("swing") != "open":
            continue
        a, h = d["at"], 0.5 * d.get("width", 2.67)
        free.append((a[0] - h, a[0] + h, a[1] - 1.0, a[1] + 1.0) if d.get("wall") in ("N", "S") else (a[0] - 1.0, a[0] + 1.0, a[1] - h, a[1] + h))
    area, far = 0.0, 0.0
    dth = 360.0 / rays
    for i in range(rays):
        th = (i + 0.5) * dth
        dv = (math.cos(th), math.sin(th))
        ivs = sorted(_keep_iv([_slab(q, at, dv) for q in free]), key = lambda x: x[0])
        t = 0.0
        if not ivs or ivs[0][0] > 1e-9:
            continue
        for (a, b) in ivs:
            if a > t + 1e-9:
                break
            t = max(t, b)
        t = min(t, reach)
        area += 0.5 * t * t * math.radians(dth)
        far = max(far, t)
    return {"area": area, "far": far, "share": area / tot if tot > 0 else 0.0}


def _keep_iv(xs):
    return [x for x in xs if x != None]


def isovist_rows(plan, rules = None):
    """Isovist openness from each unit's entry and from the stair on every floor, as notes: the one measure kept out of
    every loop, to catch a solver gaming the others. Doors open; 180 rays."""
    plan = normalize(plan)
    R = _rules(rules)
    G = door_graph(plan)
    rooms = G["rooms"]
    out = []
    pts = []
    for u in {_unit(r): 1 for r in plan.get("rooms", [])}:
        for e in entries(plan, G, u):
            r = rooms[e]
            if r["kind"] == "garage":
                continue
            for (n, d) in G["adj"].get(e, []):
                if n == "outside" and d != None and d.get("at") != None and d.get("wall") in _NORMAL:
                    nn = _NORMAL[d["wall"]]
                    pts.append(("entry", r["floor"], e, (d["at"][0] - 2.0 * nn[0], d["at"][1] - 2.0 * nn[1]), "the %s's door" % _lower(_name(r))))
                    break
    for s in [r for r in plan.get("rooms", []) if r["kind"] == "stair"]:
        for (n, d) in G["adj"].get(s["key"], []):
            if d == None or n == "outside" or d.get("at") == None or d.get("wall") not in _NORMAL:
                continue
            nn = _NORMAL[d["wall"]]
            sign = 1.0 if d["a"] == s["key"] else -1.0  # a's wall: step out of the stair into the room
            p = (d["at"][0] + sign * 1.5 * nn[0], d["at"][1] + sign * 1.5 * nn[1])
            pts.append(("stair", s["floor"], s["key"], p, "the stair's opening into the %s" % _lower(_name(rooms[n]))))
            break
    for (what, fk, rk, p, words) in pts:
        v = isovist(plan, fk, p)
        out.append(_row("isovist", what, fk, rk, True, "Isovist openness from the entry and the stair (reported only)",
                        "%s: from %s, %s of floor in view (%s of %s), out to %s; doors open; kept out of every loop" % (
                            fk, words, sqft(v["area"]), pct(v["share"]), fk, ftin(v["far"])), "isovist", "note", at = p))
    return [x for x in out if not _skipped(R, x["key"])]


# ================================================================ the audit
FAMILIES = ("plan", "code", "doors", "stacks", "fillers", "light", "furnish", "isovist")


def audit(plan, program = None, rules = None, furnish = True, light = True, validate = True, name = None, isovists = True):
    """The red pen: every family's rows and the marks to draw them.

    {"rows": [...], "marks": redpen(rows, plan), "count": {"fail": n, "warn": n, "note": n, "pass": n}}

    program: what the plan was asked to hold (filler_rows says what it accepts); rules: overrides of RULES and of
    plan_light's LIGHT (rules["skip"] drops rows by key prefix: "code:R305", "light"); furnish: the furniture re-pack;
    light: the light family, which measures sky with view_cone and so makes boxes (name= prefixes them; a document
    that audits twice gives each call its own name); validate: the plan's own drafting problems (plan.star's
    validate_rows); isovists: openness from the entry and the stair, reported only. Pure when light and furnish are
    False (the re-pack's solves are opt problems, named after name=)."""
    plan = normalize(plan)
    R = _rules(rules)
    rows = []
    if validate:
        rows.extend([r for r in validate_rows(plan) if not _skipped(R, r["key"])])
    rows.extend(code_rows(plan, R))
    rows.extend(door_rows(plan, R))
    rows.extend(stack_rows(plan, R))
    rows.extend(filler_rows(plan, program, R))
    if light:
        rows.extend([r for r in light_rows(plan, rules = rules, measure = True, name = name) if not _skipped(R, r["key"])])
    if isovists:
        rows.extend(isovist_rows(plan, R))
    if furnish:
        rows.extend(furnish_rows(plan, R, name = name))
    # one complaint, one mark: a room the furniture re-pack or R304's width marks keeps no 2:1 shape mark; a room with no
    # window keeps no "dark" mark
    covered = {}
    for x in rows:
        if not x["ok"] and x["severity"] == "fail" and x.get("room") and (x["key"].startswith("furnish:") or x["key"].startswith("code:R304:")):
            covered[("fillers:shape:", x["room"])] = "the furniture re-pack's mark" if x["key"].startswith("furnish:") else "R304's mark"
        if not x["ok"] and x["key"].startswith("fillers:window:") and x["severity"] == "fail":
            covered[("light:dark:", x["room"])] = "its no-window mark"
    out = []
    for x in rows:
        hit = None
        for (pre, rk), why in covered.items():
            if x["key"].startswith(pre) and x.get("room") == rk and not x["ok"]:
                hit = why
        out.append(dict(x, severity = "note", detail = x["detail"] + "; " + hit + " covers it") if hit != None else x)
    rows = out
    count = {"fail": 0, "warn": 0, "note": 0, "pass": 0}
    for r in rows:
        if r["ok"]:
            count["pass"] += 1
        else:
            count[r["severity"]] = count.get(r["severity"], 0) + 1
    return {"rows": rows, "marks": redpen(rows, plan), "count": count}


def failures(rows, severities = ("fail", "warn")):
    """The rows that did not hold, of these severities."""
    return [r for r in rows if not r["ok"] and r["severity"] in severities]


def questions(rows):
    """The rows that are questions for the building department or could not be decided (ok False, severity note)."""
    return [r for r in rows if not r["ok"] and r["severity"] == "note"]


def row_line(r, prefix = ""):
    """One row as a model's check list prints it: "<prefix>PASS key: name -- detail" (FAIL, WARN or NOTE when it
    did not hold)."""
    return "%s%s %s: %s -- %s" % (prefix, "PASS" if r["ok"] else r["severity"].upper(), r["key"], r["name"], r["detail"])


def _split(d):
    i = d.find("; ")
    return (d, "") if i < 0 else (d[:i], d[i + 2:])


def _mark_at(r, plan):
    if r.get("at") != None:
        return r["at"]
    q = r.get("rect")
    if q == None and plan != None and r.get("room"):
        for x in plan.get("rooms", []):
            if x["key"] == r["room"]:
                q = x["rects"][0]
    if q == None:
        return None
    return (q[0] + min(1.3, 0.5 * (q[1] - q[0])), q[3] - min(1.6, 0.5 * (q[3] - q[2])))


def redpen(rows, plan = None, kinds = ("fail", "warn", "ask")):
    """The marks for plan_draw's red pen: the rows that did not hold and have a place (at, rect, or a room of
    `plan`), as display copies ordered floor by floor in plan order, so plan_draw numbers them the way a marked-up
    sheet reads. Each copy is a check row -- name the short finding (the detail's first clause), detail the rest, at
    the mark's point -- with n (its number), text (the short finding), kind ("fail", "warn" or "ask") and of (the
    row's key). An "ask" is a note that is a question for the building department (its detail says "ask the building department"); its
    copy is a "warn" so that plan_draw, which leaves notes out, draws it. Pass the marks as plan_draw's checks:
    plan_set(p, checks=redpen(rows, p)). A row with no place (a floor's shares) is numbered and listed with no ring
    (plan_draw lists a floor's rows, and the plan's own on the first sheet)."""
    order = {}
    if plan != None:
        for i, f in enumerate(floors_of(plan)):
            order[f["key"]] = i
    cand = []
    for i, r in enumerate(rows):
        if r["ok"]:
            continue
        kind = r["severity"]
        if kind == "note":
            if "ask the building department" not in r["detail"]:
                continue
            kind = "ask"
        if kind not in kinds:
            continue
        at = _mark_at(r, plan)
        fl = r.get("floor")
        cand.append((order.get(fl, 99) if fl != None else -1, i, r, at, kind))
    cand = sorted(cand, key = lambda c: (c[0], c[1]))
    marks = []
    seen = {}
    for (o, i, r, at, kind) in cand:
        head, rest = _split(r["detail"])
        m = {"key": "redpen:" + r["key"], "name": head, "ok": False, "detail": rest or r.get("name") or "", "rule": r.get("rule"),
             "severity": "fail" if kind == "fail" else "warn", "floor": r.get("floor"), "room": r.get("room"),
             "n": len(marks) + 1, "text": head, "kind": kind, "of": r["key"]}
        if at != None:
            # two marks on one spot step down the room
            k = (r.get("floor"), int(at[0] * 4), int(at[1] * 4))
            n = seen.get(k, 0)
            seen[k] = n + 1
            m["at"] = (at[0], at[1] - 1.6 * n)
        marks.append(m)
    return marks


def redpen_set(plan, rows, name = None, project = None, stamp = None, per_sheet = None, scale = None):
    """The red pen on plan_draw's sheets: plan_set(plan, checks=redpen(rows, plan)), exported as <name>.pdf (cadpole
    export --run-exports writes it). Returns the sheet set."""
    nm = name or ("%s_redpen" % (plan.get("name") or "plan"))
    S = plan_set(plan, name = nm, checks = redpen(rows, plan), project = project,
                 stamp = stamp or ["RED PEN", "CODE READINGS UNVERIFIED"], per_sheet = per_sheet, scale = scale)
    export(S, nm + ".pdf")
    return S
