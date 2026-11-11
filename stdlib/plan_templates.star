"""@cadpole/plan_templates.star -- the parti library for 25-ft row-house lots, as data.

    load("@cadpole/plan_templates.star", "SITE", "PROGRAM", "library", "side_hall", "cross_stair", "court",
         "open_core", "mirror", "split_column", "with_light_well", "without_court", "optional_court",
         "short_columns", "floors_only", "single_floor", "door_rules", "knob", "program_with")

A *parti* is the organizing idea of a plan: a hall along one party wall, a stair across the middle, a court cut into
the side, an open floor round a core. Here each one is a template -- a few dozen lines of plain data -- and
`@cadpole/plan_parti.star`'s `fit()` turns a template, a site and a program into one mixed-integer program over every
floor at once. The solver only *fits* the template: the floor depths, the stair's place, each room's span and depth,
the doors and the plumbing stacks. Everything here is data; nothing is solved on load, and nothing is recorded.

## A template

    {"key": "side_hall", "name": "Side hall", "idea": "...", "precedent": "...",
     "lines": [3.7, 7.6, 18.2],          # partition centre lines across the inside width (u, feet from the west wall's
                                         # inside face): the column grid. Columns are numbered 0.. from the west.
     "floors": ["G", "M", "U"],          # bottom up: ground, main, upper (and "T", a top floor)
     "stair": {"type": "side", "col": 0},          # a straight run along v in one column, or
                                                   # {"type": "cross", "land_w": 0, "land_e": 3}: across the house
     "wells": [{"pre": "well", "col": 3, "floors": ["M", "U"], "rear": True, "side": "E", "lit_depth": 12.0}],
     "bay": {"host": {"M": "living", "U": "bed2"}, "u0": 9.9, "u1": 21.9, "proj": 3.0},
     "bearing": [2],                     # line indices no room may cross, on any floor
     "stacks": [3, 3, 3],                # one plumbing stack candidate per entry, on that line index
     "orders": [("G", "garage", "den")], # (floor, a, b): a lies wholly in front of (street-side of) b
     "patterns": {"two_sided": ("weight", 30.0)},  # Alexander's patterns, weighted or ("hard", n)
     "rooms": [...],                     # placements, below
     "doors": {"wish": [], "must": [], "ban": []}, # the door graph beyond each room's access door (fit() reads it)
     "short": [],                        # columns that may end short of the floor's rear wall: (floor, col) or col
     "fixed": {}, "fix_depth": {},       # kept rooms pinned: key -> {"span": (a, b), "y": v, "d": depth}
    }

A *room placement* names the room (a program key such as "living", or a template-owned room such as a hall), its
floor, the column spans it may take (`(a, b)`: columns a through b), the rooms it may open off (`parents`, in any
order; "ROOT" is the street, "stair.top" the arrival landing), and its facade pins (`front`, `rear`). Program rooms
take their sizes from the program; template-owned rooms (halls, closets, stores, the stair, courts) carry their own.
Any size field a placement sets overrides the program's: the open core's kitchen sets `lit=False, borrow=16.0`.

## The program

`PROGRAM` is the benchmark household: a one-car garage, an entry, a den and full bath on the ground floor; living,
dining, kitchen, powder room and pantry on the main floor; a primary suite, two bedrooms, a bath and a laundry above.
Each entry gives a room's name, its plan kind (the plan format's vocabulary), its kernel role (`hab`, `sleep`, `wet`,
`circ`, `serv`, `garage`), its area range and target in sq ft, its value per sq ft (a quarter of it past the target),
its depth range, its least width and its fixture depths. A program may add rooms a template has a place for --
`bed4` -- and leave out rooms it does not want.

## Parameters

Every template function takes `mirror=` (the plan flipped onto the other party wall), `floors=` (3, or 4 where the
template has a four-floor layout), `split=` (a list of `(col, u)`: split column `col` at `u`), `court=` (True,
False, or "optional" where the template has one) and `light_well=` (a neighbour's well to match:
`{"side": "W", "y": 18.0, "d": 12.0}`), plus its own knobs, listed in `t["knobs"]`.

The four partis take any
number of floors, wells on either party wall, kept rooms, and extra and forbidden doors. The geometry is the 25 x 100-ft
lot: a 24-ft inside width between 6-in party walls, 5 ft of front setback and a 35-ft rear yard, so at most 59 ft of
inside depth; partitions 0.4 ft thick.
"""

# ------------------------------------------------------------------ the benchmark site and program

SITE = {
    "lot": (0.0, 25.0, 0.0, 100.0),       # (u0, u1, v0, v1), feet; the street on the south
    "street": "S",
    "north_deg": 0.0,
    "setbacks": {"front": 5.0, "rear": 35.0, "east": 0.0, "west": 0.0},
    "wall": 0.5,                           # party and exterior walls
    "partition": 0.4,
    "far_cap": 3600.0,                     # gross sq ft over all floors, garage excluded
    "floor_to_floor": 10.0,
    "ceiling": 9.0,
}

LOW = 0.1  # service rooms: area past the target is worth little (less than it costs to build)

_HAB = {"wmin": 7.0, "d": (7.0, 30.0), "lit": True, "aspect": 2.0}

def _with(base, **kw):
    out = dict(base)
    out.update(kw)
    return out

# The program: what the household asks for. Same sizes in every parti, so the kernel score compares partis.
PROGRAM = {
    "vestibule": {"name": "Vestibule", "kind": "entry", "role": "circ", "area": (20.0, 80.0), "target": 36.0,
                  "value": 0.3, "mind": (4.0, 5.0), "d": (4.0, 10.0), "knee": LOW},
    "garage": {"name": "Garage", "kind": "garage", "role": "garage", "area": (200.0, 260.0), "wmin": 10.0,
               "d": (20.0, 22.0)},
    "den": _with(_HAB, name = "Den / flex", kind = "den", role = "hab", area = (100.0, 320.0), target = 180.0,
                 value = 0.7, garden = True),
    "bathG": {"name": "Bath", "kind": "bath", "role": "wet", "area": (38.0, 75.0), "target": 55.0, "value": 0.8,
              "wet": True, "mind": (5.0, 7.5), "d": (5.0, 14.0), "knee": LOW},
    "living": _with(_HAB, name = "Living", kind = "living", role = "hab", area = (200.0, 450.0), target = 300.0,
                    value = 1.0, day = True),
    "dining": _with(_HAB, name = "Dining", kind = "dining", role = "hab", area = (120.0, 300.0), target = 170.0,
                    value = 0.9, day = True),
    "kitchen": _with(_HAB, name = "Kitchen", kind = "kitchen", role = "hab", area = (130.0, 300.0), target = 200.0,
                     value = 1.0, wet = True, day = True, aspect = 2.5),
    "powder": {"name": "Powder", "kind": "powder", "role": "wet", "area": (20.0, 40.0), "target": 24.0, "value": 0.5,
               "wet": True, "mind": (4.5, 5.0), "d": (4.5, 10.0), "knee": LOW},
    "pantry": {"name": "Pantry", "kind": "pantry", "role": "serv", "area": (14.0, 60.0), "target": 28.0,
               "value": 0.5, "mind": (3.5, 4.0), "d": (3.5, 17.0), "knee": LOW},
    "primary": _with(_HAB, name = "Primary bedroom", kind = "bed", role = "sleep", area = (150.0, 320.0),
                     target = 220.0, value = 1.0, d = (7.0, 24.0)),
    "pcloset": {"name": "Primary closet", "kind": "closet", "role": "serv", "area": (24.0, 80.0), "target": 45.0,
                "value": 0.6, "mind": (4.5, 5.0), "d": (4.5, 16.0), "knee": LOW},
    "pbath": {"name": "Primary bath", "kind": "bath", "role": "wet", "area": (38.0, 75.0), "target": 55.0,
              "value": 0.8, "wet": True, "mind": (5.0, 7.5), "d": (5.0, 14.0), "knee": LOW},
    "bed2": _with(_HAB, name = "Bedroom 2", kind = "bed", role = "sleep", area = (90.0, 220.0), target = 130.0,
                  value = 1.0, d = (7.0, 24.0)),
    "bed3": _with(_HAB, name = "Bedroom 3", kind = "bed", role = "sleep", area = (90.0, 220.0), target = 130.0,
                  value = 1.0, d = (7.0, 24.0)),
    "bath2": {"name": "Bath 2", "kind": "bath", "role": "wet", "area": (38.0, 75.0), "target": 55.0, "value": 0.8,
              "wet": True, "mind": (5.0, 7.5), "d": (5.0, 14.0), "knee": LOW},
    "laundry": {"name": "Laundry", "kind": "laundry", "role": "wet", "area": (20.0, 60.0), "target": 32.0,
                "value": 0.5, "wet": True, "mind": (3.5, 5.5), "d": (3.5, 12.0), "knee": LOW},
}

# The rooms the partis are compared on (program rooms differ in character).
PROGRAM_KEYS = ["den", "bathG", "living", "dining", "kitchen", "powder", "pantry", "bed2", "bed3", "primary", "bath2",
                "laundry", "pbath", "pcloset", "garage", "vestibule"]

# Rooms a program may add, with their sizes (a template says where they may go).
EXTRA_ROOMS = {
    "bed4": _with(_HAB, name = "Bedroom 4", kind = "bed", role = "sleep", area = (90.0, 220.0), target = 130.0,
                  value = 1.0, d = (7.0, 24.0)),
    "study": _with(_HAB, name = "Study", kind = "study", role = "hab", area = (70.0, 160.0), target = 100.0,
                   value = 0.6, optional = True),
    "fam": _with(_HAB, name = "Family room", kind = "family", role = "hab", area = (120.0, 260.0), target = 180.0,
                 value = 0.8, day = True),
}

# ------------------------------------------------------------------ room makers

def _place(key, floor, spans, kw):
    out = {"key": key, "floor": floor, "spans": [tuple(s) for s in spans], "program": True}
    out.update(kw)
    if "spans" in kw:
        out["spans"] = [tuple(s) for s in kw["spans"]]
    return out

def living(f, spans, **kw):
    return _place("living", f, spans, kw)

def dining(f, spans, **kw):
    return _place("dining", f, spans, kw)

def kitchen(f, spans, **kw):
    return _place("kitchen", f, spans, kw)

def den(spans, **kw):
    return _place("den", kw.pop("floor", "G"), spans, kw)

def bedroom(key, spans, **kw):
    return _place(key, kw.pop("floor", "U"), spans, kw)

def primary(spans, **kw):
    return _place("primary", kw.pop("floor", "U"), spans, kw)

def bath(key, f, spans, **kw):
    return _place(key, f, spans, kw)

def powder(spans, **kw):
    return _place("powder", kw.pop("floor", "M"), spans, kw)

def laundry(spans, **kw):
    return _place("laundry", kw.pop("floor", "U"), spans, kw)

def pantry(spans, **kw):
    return _place("pantry", kw.pop("floor", "M"), spans, kw)

def pcloset(spans, **kw):
    return _place("pcloset", kw.pop("floor", "U"), spans, kw)

def vestibule(spans, **kw):
    return _place("vestibule", "G", spans, _with({"front": True, "parents": ["ROOT"]}, **kw))

def garage(spans, amax = 260.0):
    out = _place("garage", "G", spans, {"front": True, "parents": ["ROOT"]})
    if amax != 260.0:
        out["area"] = (200.0, amax)
    return out

def extra_room(key, f, spans, **kw):
    """A placement for a room the program may add (EXTRA_ROOMS: bed4, study, fam): used only if the program asks."""
    return _place(key, f, spans, _with({"if_program": True}, **kw))

def closet(key, f, spans, **kw):
    base = {"key": key, "floor": f, "role": "serv", "kind": "closet", "name": "Closet", "label": "Cl.",
            "spans": [tuple(s) for s in spans], "area": (6.0, 70.0), "target": 30.0, "value": 0.3, "d": (2.0, 20.0),
            "knee": LOW, "optional": True}
    base.update(kw)
    return base

def storage(key, f, spans, name = "Storage", kind = "storage", **kw):
    base = {"key": key, "floor": f, "role": "serv", "kind": kind, "name": name, "spans": [tuple(s) for s in spans],
            "area": (15.0, 260.0), "target": 80.0, "value": 0.3, "d": (3.0, 30.0), "knee": LOW, "aspect": 3.5}
    base.update(kw)
    return base

def hall(key, f, spans, name = "Hall", **kw):
    base = {"key": key, "floor": f, "role": "circ", "kind": "hall", "name": name, "spans": [tuple(s) for s in spans],
            "value": 0.05, "wmin": 3.0, "d": (3.0, 999.0), "hall": False}
    base.update(kw)
    return base

def stair(f, span):
    return {"key": "stair" + f, "floor": f, "role": "stair", "kind": "stair", "name": "Stair", "spans": [tuple(span)],
            "d": (3.5, 18.5), "core": "stair"}

def well(f, span, rear = False, d = (6.0, 16.0), name = "Light court", pre = "well", kind = None):
    return {"key": pre + f, "floor": f, "role": "void", "kind": kind or ("court" if rear else "well"), "name": name,
            "spans": [tuple(span)], "d": d, "rear": rear, "well": pre}

# ------------------------------------------------------------------ helpers on template data

def _template(key, name, idea, precedent, lines, stair_, rooms, **kw):
    t = {"key": key, "name": name, "idea": idea, "precedent": precedent, "lines": list(lines), "stair": stair_,
         "floors": ["G", "M", "U"], "rooms": rooms, "orders": [], "bearing": [], "stacks": [], "wells": [],
         "bay": None, "patterns": {}, "joists": "", "skylight": False, "fixed": {}, "fix_depth": {},
         "doors": {"wish": [], "must": [], "ban": []}, "short": [], "knobs": {}, "notes": [], "variant": []}
    t.update(kw)
    return t

def copy_template(t):
    """A deep enough copy of a template to edit: its rooms, lists and dicts are new."""
    out = dict(t)
    out["rooms"] = [dict(r) for r in t["rooms"]]
    for k in ("lines", "floors", "orders", "bearing", "stacks", "short", "notes", "variant"):
        out[k] = list(t.get(k, []))
    out["wells"] = [dict(w) for w in t.get("wells", [])]
    out["stair"] = None if t.get("stair") == None else dict(t["stair"])
    out["bay"] = None if t.get("bay") == None else dict(t["bay"], host = dict(t["bay"]["host"]))
    out["patterns"] = dict(t.get("patterns", {}))
    out["fixed"] = dict(t.get("fixed", {}))
    out["fix_depth"] = dict(t.get("fix_depth", {}))
    d = t.get("doors", {})
    out["doors"] = {"wish": list(d.get("wish", [])), "must": list(d.get("must", [])), "ban": list(d.get("ban", []))}
    out["knobs"] = dict(t.get("knobs", {}))
    return out

def room_of(t, key):
    for r in t["rooms"]:
        if r["key"] == key:
            return r
    return None

def set_room(t, key, **kw):
    """Change fields of room `key` in template t (in place), returning t."""
    for i, r in enumerate(t["rooms"]):
        if r["key"] == key:
            r2 = dict(r)
            r2.update(kw)
            if "spans" in kw:
                r2["spans"] = [tuple(s) for s in kw["spans"]]
            t["rooms"][i] = r2
    return t

def tidy(t, strict = True):
    """Drop door parents that no longer exist on the room's floor (after a room was removed). A room left with no
    parent is an error, or with `strict=False` a room listed in t["orphans"] (the precheck reports it)."""
    keys = {}
    for r in t["rooms"]:
        keys[(r["floor"], r["key"])] = True
    orphans = []
    for i, r in enumerate(t["rooms"]):
        ps = []
        for x in r.get("parents", []):
            if x in ("ROOT", "stair.top") or (r["floor"], x) in keys:
                if x not in ps:
                    ps.append(x)
        if r.get("parents") and not ps:
            if strict:
                fail("plan_templates: %s: every door parent was removed (%s)" % (r["key"], r["parents"]))
            orphans.append(r["key"])
        if ps != list(r.get("parents", [])):
            t["rooms"][i] = dict(r, parents = ps)
    t["orphans"] = orphans
    floors_of = {}
    for r in t["rooms"]:
        floors_of[r["key"]] = r["floor"]
    t["orders"] = [o for o in t["orders"] if floors_of.get(o[1]) == o[0] and floors_of.get(o[2]) == o[0]]
    return t

def mirror(t):
    """The template flipped east-west: the same parti on the other party wall. Mirror images are one plan."""
    q = copy_template(t)
    W = 24.0
    C = len(t["lines"]) + 1
    q["lines"] = [W - x for x in reversed(t["lines"])]

    def mc(c):
        return C - 1 - c

    def ms(sp):
        return (mc(sp[1]), mc(sp[0]))

    q["rooms"] = [dict(r, spans = [ms(s) for s in r["spans"]]) for r in t["rooms"]]
    q["bearing"] = [C - k for k in t["bearing"]]
    q["stacks"] = [C - k for k in t["stacks"]]
    st = None if t.get("stair") == None else dict(t["stair"])
    if st == None:
        pass
    elif st["type"] == "side":
        st["col"] = mc(st["col"])
    else:
        st["land_w"], st["land_e"] = mc(st["land_w"]), mc(st["land_e"])
    q["stair"] = st
    q["wells"] = [dict(w, col = mc(w["col"]), side = ("W" if w.get("side", "E") == "E" else "E")) for w in t["wells"]]
    if t.get("bay"):
        q["bay"] = dict(t["bay"], u0 = W - t["bay"]["u1"], u1 = W - t["bay"]["u0"])
    q["fixed"] = {k: dict(v, span = ms(v["span"])) for k, v in t.get("fixed", {}).items()}
    q["short"] = [(s[0], mc(s[1])) if type(s) == "tuple" else mc(s) for s in t.get("short", [])]
    q["key"] = t["key"] + "_m"
    q["name"] = t["name"] + " (mirrored)"
    q["variant"] = list(t.get("variant", [])) + ["mirror"]
    return q

def split_column(t, col, at):
    """Split column `col` into two at u = `at` (a new partition line): every span keeps its extent, so a room that
    took the old column may take either half or both. Lines, bearing, stacks, wells, the stair and pins renumber."""
    lines = list(t["lines"])
    C = len(lines) + 1
    edges = [0.0] + lines + [24.0]
    if not (edges[col] < at and at < edges[col + 1]):
        fail("plan_templates.split_column: u = %s is not inside column %d" % (at, col))
    q = copy_template(t)
    q["lines"] = sorted(lines + [at])

    def lo(c):
        return c if c <= col else c + 1

    def hi(c):
        return c if c < col else c + 1

    new_rooms = []
    for r in t["rooms"]:
        sp = []
        for s in r["spans"]:
            if r.get("role") in ("stair", "void") and s[0] == col and s[1] == col:
                # a stair or a court keeps one column: the half that holds it (an east well the east half, else
                # the west half)
                east = r.get("role") == "void" and [w for w in t["wells"] if w["pre"] == r.get("well") and w.get("side", "E") == "E"]
                n = (hi(s[0]), hi(s[0])) if east else (lo(s[0]), lo(s[0]))
            else:
                n = (lo(s[0]), hi(s[1]))
            if n not in sp:
                sp.append(n)
            if r.get("role") not in ("stair", "void", "garage") and s[0] == col and s[1] == col:
                for extra in ((col, col), (col + 1, col + 1)):
                    if extra not in sp:
                        sp.append(extra)
        new_rooms.append(dict(r, spans = sp))
    q["rooms"] = new_rooms
    q["bearing"] = [k if k <= col else k + 1 for k in t["bearing"]]
    q["stacks"] = [k if k <= col else k + 1 for k in t["stacks"]]
    st = None if t.get("stair") == None else dict(t["stair"])
    if st == None:
        pass
    elif st["type"] == "side":
        st["col"] = lo(st["col"])
    else:
        st["land_w"], st["land_e"] = lo(st["land_w"]), hi(st["land_e"])
    q["stair"] = st
    q["wells"] = [dict(w, col = (hi(w["col"]) if w.get("side", "E") == "E" else lo(w["col"]))) for w in t["wells"]]
    q["fixed"] = {k: dict(v, span = (lo(v["span"][0]), hi(v["span"][1]))) for k, v in t.get("fixed", {}).items()}
    q["key"] = t["key"] + "_split%d" % col
    q["variant"] = list(t.get("variant", [])) + ["split %d at %s" % (col, at)]
    return q

def with_light_well(t, side = "W", y = 18.0, d = 12.0, floors = ("M", "U"), pre = "lw"):
    """A neighbour's light well to match: a fixed void `d` ft long from v = `y`, on the party wall `side`, on
    `floors`. It lights the rooms beside it (the 2H row's court source), never an escape window."""
    q = copy_template(t)
    C = len(t["lines"]) + 1
    col = 0 if side == "W" else C - 1
    for f in floors:
        q["rooms"].append(dict(well(f, (col, col), d = (d, d), name = "Light well (matches the neighbour's)", pre = pre),
                               site = True))
        q["fixed"][pre + f] = {"span": (col, col), "y": y, "d": d}
    q["wells"] = q["wells"] + [{"pre": pre, "col": col, "floors": list(floors), "rear": False, "lit_depth": 12.0,
                                "side": side}]
    q["key"] = t["key"] + "__lw" + side
    q["name"] = t["name"] + ", a %s light well to match" % ("west" if side == "W" else "east")
    q["variant"] = list(t.get("variant", [])) + ["light well %s" % side]
    return q

def without_court(t):
    """The template with its own court or rear arm filled in: the void rooms go, and their columns are tiled by the
    rooms that may take them."""
    q = copy_template(t)
    pres = [w["pre"] for w in t["wells"]]
    q["rooms"] = [r for r in t["rooms"] if not (r.get("role") == "void" and r.get("well") in pres)]
    q["wells"] = []
    q["key"] = t["key"] + "__nocourt"
    q["name"] = t["name"] + ", no court"
    q["variant"] = list(t.get("variant", [])) + ["no court"]
    return tidy(q)

def optional_court(t):
    """The template's court made the solver's choice: cut it or fill it in."""
    q = copy_template(t)
    pres = [w["pre"] for w in t["wells"]]
    for i, r in enumerate(q["rooms"]):
        if r.get("role") == "void" and r.get("well") in pres:
            q["rooms"][i] = dict(r, optional = True)
    q["key"] = t["key"] + "__courtopt"
    q["variant"] = list(t.get("variant", [])) + ["court optional"]
    return q

def floors_only(t, floors):
    """Template t cut down to some of its floors (consecutive, bottom up): their rooms, wells, orders and pins. The
    rooms that opened off the arrival landing of the lowest floor kept open off its stair room instead; with one
    floor the stair coupling goes too (the stair room stays as a room that holds its column). For drafting a
    template a floor or two at a time, and for tests: a floor solves in well under a second."""
    fs = list(floors)
    order = list(t["floors"])
    idx = [order.index(f) for f in fs]
    if idx != list(range(idx[0], idx[0] + len(idx))):
        fail("plan_templates.floors_only: %s are not consecutive floors of %s" % (fs, order))
    first = fs[0]
    q = copy_template(t)
    rooms = []
    for r in t["rooms"]:
        if r["floor"] not in fs:
            continue
        r2 = dict(r)
        if len(fs) == 1 and r2.get("role") == "stair":
            r2["d"] = (3.5, 18.5)
        if r["floor"] == first:
            r2["parents"] = [("stair" + first) if x == "stair.top" else x for x in r.get("parents", [])]
        rooms.append(r2)
    q["rooms"] = rooms
    q["floors"] = fs
    if len(fs) == 1:
        q["stair"] = None
    q["orders"] = [o for o in t["orders"] if o[0] in fs]
    q["wells"] = [dict(w, floors = [x for x in w["floors"] if x in fs]) for w in t["wells"]]
    q["wells"] = [w for w in q["wells"] if w["floors"]]
    if t.get("bay"):
        host = {k: v for k, v in t["bay"]["host"].items() if k in fs}
        q["bay"] = dict(t["bay"], host = host) if host else None
    keys = [r["key"] for r in rooms]
    q["fixed"] = {k: v for k, v in t.get("fixed", {}).items() if k in keys}
    q["fix_depth"] = {k: v for k, v in t.get("fix_depth", {}).items() if k in fs}
    q["short"] = [x for x in t.get("short", []) if type(x) != "tuple" or x[0] in fs]
    q["partial"] = True  # the program's rooms on other floors are not this template's to place
    q["key"] = "%s__%s" % (t["key"], "".join(fs))
    q["name"] = "%s, floor%s %s alone" % (t["name"], "s" if len(fs) > 1 else "", " and ".join(fs))
    q["variant"] = list(t.get("variant", [])) + ["floors %s alone" % "".join(fs)]
    return q

def single_floor(t, f):
    """One floor of template t on its own (floors_only with one floor)."""
    return floors_only(t, [f])

def short_columns(t, cols = None, floors = None):
    """Template t with columns that may end short of the floor's rear wall, instead of filling up with stores:
    `cols` (all by default) on `floors` (all by default). The plate steps where a column ends."""
    q = copy_template(t)
    C = len(t["lines"]) + 1
    cs = list(range(C)) if cols == None else list(cols)
    fs = list(t["floors"]) if floors == None else list(floors)
    for f in fs:
        for c in cs:
            if (f, c) not in q["short"]:
                q["short"].append((f, c))
    q["key"] = t["key"] + "__short"
    q["variant"] = list(t.get("variant", [])) + ["short columns"]
    return q

# The door rules an architect would add: rooms whose parents are restricted, and extra doors.
DOOR_FIXES = {
    "side_hall": {"parents": {"pantry": ["kitchen"], "powder": ["hallM"], "bathG": ["hallG"], "mud": ["garage"]},
                  "must": [("garage", ["hallG", "vestibule"])]},
    "cross_stair": {"parents": {"pantry": ["kitchen"], "powder": ["hallM", "hallM2"]},
                    "must": [("garage", ["hallG1", "hallG2", "hallG3", "vestibule"])]},
    "court": {"parents": {"pantry": ["kitchen"], "powder": [], "storeG": ["hallG", "hallG2", "den"]},
              "must": [("garage", ["hallG", "hallG2", "vestibule"])]},
    "open_core": {"parents": {"pantry": ["kitchen"], "powder": [], "storeG": ["hallG", "den"],
                              "workG": ["hallG", "den", "storeG"]},
                  "must": [("garage", ["hallG", "vestibule", "storeG2"])]},
}

def door_rules(t, which = "all", base = None):
    """An architect's door rules on template t. `which`: "all", or one rule:
    "pantry" (the pantry opens off the kitchen), "garage" (a door from the garage into the house), "bath" (the
    ground-floor bath and stores open off a hall, not the garage), "powder" (the powder room opens off a hall, not a
    day room). `base` names the parti whose DOOR_FIXES apply (default: t's key up to "__")."""
    q = copy_template(t)
    b = base or t["key"].split("__")[0]
    halls = {}
    for r in t["rooms"]:
        if r.get("role") == "circ" and r.get("program") != True:
            halls.setdefault(r["floor"], []).append(r["key"])
    if which == "all":
        fx = DOOR_FIXES[b]
        for i, r in enumerate(q["rooms"]):
            if r["key"] in fx["parents"]:
                ps = fx["parents"][r["key"]]
                if not ps:  # no hall on that floor: the powder opens off a stair hall or passage where it can
                    ps = [p for p in r.get("parents", []) if p not in ("living", "dining", "kitchen")] or r["parents"]
                q["rooms"][i] = dict(r, parents = list(ps))
        q["doors"]["must"] = q["doors"]["must"] + list(fx["must"])
        q["key"] = t["key"] + "__fix"
        q["name"] = t["name"] + ", with an architect's door rules"
    else:
        for i, r in enumerate(q["rooms"]):
            ps = list(r.get("parents", []))
            if which == "pantry" and r["key"] == "pantry":
                q["rooms"][i] = dict(r, parents = ["kitchen"])
            elif which == "bath" and r["key"] in ("bathG", "storeG", "mud", "workG", "storeG2") and "garage" in ps:
                q["rooms"][i] = dict(r, parents = [x for x in ps if x != "garage"] or ps)
            elif which == "powder" and r["key"] == "powder":
                hs = [x for x in ps if x in halls.get("M", [])]
                q["rooms"][i] = dict(r, parents = hs or ps, area = (20.0, 50.0))
        if which == "garage":
            q["doors"]["must"] = q["doors"]["must"] + [("garage", halls.get("G", []) + ["vestibule"])]
        q["key"] = "%s__only_%s" % (t["key"], which)
        q["name"] = "%s, %s rule" % (t["name"], which)
    q["variant"] = list(t.get("variant", [])) + ["door rules: " + which]
    return q

def door_rule_is_vacuous(t, which):
    """True when one of door_rules' single rules changes nothing in t's doors (the powder rule where the main floor
    has no hall the powder room may open off)."""
    if which != "powder":
        return False
    halls = [r["key"] for r in t["rooms"] if r.get("role") == "circ" and r["floor"] == "M" and r.get("program") != True]
    r = room_of(t, "powder")
    return r == None or not [x for x in r.get("parents", []) if x in halls]

def knob(t, name, value = True):
    """Turn one of template t's own knobs (t["knobs"]): returns the edited copy."""
    ks = t.get("knobs", {})
    if name not in ks:
        fail("plan_templates.knob: %s has no knob %r (has: %s)" % (t["key"], name, ", ".join(sorted(ks.keys()))))
    return ks[name](t, value)

def _finish(t, mirror_ = False, split = None, court_ = True, light_well = None):
    if court_ == False:
        t = without_court(t)
    elif court_ == "optional":
        t = optional_court(t)
    for c, at in (split or []):
        t = split_column(t, c, at)
    if light_well:
        t = with_light_well(t, side = light_well.get("side", "W"), y = light_well.get("y", 18.0),
                            d = light_well.get("d", 12.0), floors = light_well.get("floors", ("M", "U")))
    if mirror_:
        t = mirror(t)
    return t

# ================================================================== 1. side hall (Victorian), with a rear-arm court

def _side_hall_master(t, value = True):
    """The primary bedroom at the street behind the bay (the Victorian master); bedroom 2 at the garden."""
    q = copy_template(t)
    if not value:
        return q
    set_room(q, "bed2", front = False, rear = True, spans = [(2, 2), (2, 3)])
    set_room(q, "primary", front = True, rear = False, spans = [(2, 3)])
    set_room(q, "pbath", spans = [(2, 2), (3, 3)])
    set_room(q, "pcloset", spans = [(2, 2), (3, 3)])
    q["orders"] = [o for o in q["orders"] if o != ("U", "bed2", "primary")] + [("U", "primary", "bed2")]
    q["bay"] = dict(q["bay"], host = {"M": "living", "U": "primary"})
    q["key"] = t["key"] + "_front_master"
    q["name"] = "Side hall, primary at the street"
    q["variant"] = list(t.get("variant", [])) + ["primary at the street"]
    return q

def side_hall(mirror = False, floors = 3, split = None, court = True, light_well = None, primary_at_street = False):
    """The side hall: a hall along the west party wall runs the full depth of every floor, the stair rises in it, and
    rooms 16 ft wide stand in a file east of it, formal at the street, kitchen at the garden. A rear-arm court lights
    the back rooms' sides. Grid: c0 stair 3'-6" | c1 hall 3'-6" | c2 10'-2" | c3 5'-7"; line 2 bears."""
    RB = [(2, 3)]  # the room band: 16'-3" wide, east of the bearing hall wall
    rooms = [
        # ground
        vestibule([(0, 1)]),
        garage([(2, 2)]),
        stair("G", (0, 0)),
        hall("hallG", "G", [(1, 1)], parents = ["vestibule"], hall = True, garden = True),
        closet("clG1", "G", [(0, 0)], parents = ["vestibule", "hallG"]),
        closet("clG2", "G", [(0, 0)], parents = ["hallG"]),
        storage("mud", "G", [(3, 3)], name = "Mudroom", kind = "mud", parents = ["garage"], optional = True),
        bath("bathG", "G", [(3, 3), (2, 2), (2, 3)], parents = ["hallG", "garage"]),
        storage("storeG", "G", [(2, 3), (2, 2), (3, 3)], parents = ["hallG", "garage"]),
        den(RB, rear = True, parents = ["hallG"]),
        # main
        stair("M", (0, 0)),
        hall("hallM", "M", [(1, 1)], parents = ["stair.top"], hall = True),
        living("M", RB, front = True, parents = ["hallM"]),
        dining("M", [(2, 3), (2, 2)], parents = ["hallM", "living"]),
        kitchen("M", [(2, 2), (2, 3)], rear = True, parents = ["hallM", "dining"]),
        powder([(3, 3), (2, 2)], parents = ["hallM", "dining", "living"]),
        pantry([(3, 3), (0, 0), (2, 2)], parents = ["hallM", "kitchen", "dining"]),
        closet("clM1", "M", [(0, 0)], parents = ["hallM"]),
        closet("clM2", "M", [(0, 0), (3, 3)], parents = ["hallM", "dining", "kitchen"]),
        well("M", (3, 3), rear = True, d = (6.0, 30.0), name = "Court (rear arm)"),
        # upper
        stair("U", (0, 0)),
        hall("hallU", "U", [(1, 1)], parents = ["stair.top"], hall = True),
        bedroom("bed2", RB, front = True, parents = ["hallU"]),
        bedroom("bed3", [(0, 1), (2, 2), (2, 3)], parents = ["hallU"]),
        extra_room("bed4", "U", [(2, 2), (2, 3), (0, 1)], parents = ["hallU"]),
        primary([(2, 2), (2, 3)], rear = True, parents = ["hallU"]),
        bath("bath2", "U", [(3, 3), (2, 2)], parents = ["hallU", "bed3"]),
        laundry([(3, 3), (2, 2)], parents = ["hallU"]),
        pcloset([(2, 2), (3, 3)], parents = ["primary"]),
        bath("pbath", "U", [(3, 3), (2, 2)], parents = ["primary", "pcloset"]),
        closet("clU1", "U", [(0, 0)], parents = ["hallU"]),
        closet("clU2", "U", [(0, 0), (3, 3)], parents = ["hallU", "bed2", "primary"]),
        well("U", (3, 3), rear = True, d = (6.0, 30.0), name = "Court (rear arm)"),
    ]
    t = _template(
        "side_hall", "Side hall",
        "A hall along the west party wall runs the full depth of every floor; the stair rises in it; rooms 16 ft wide " +
        "stand in a file east of it, formal at the street, kitchen at the garden. A rear-arm court lights the back " +
        "rooms' sides.",
        "Queen Anne side-hall plan; SF Italianate/Stick row houses (their rear arm)",
        [3.7, 7.6, 18.2], {"type": "side", "col": 0}, rooms,
        orders = [("G", "garage", "den"), ("M", "living", "dining"), ("M", "dining", "kitchen"), ("U", "bed2", "primary")],
        bearing = [2], stacks = [3, 3, 3],
        wells = [{"pre": "well", "col": 3, "floors": ["M", "U"], "rear": True, "lit_depth": 12.0, "side": "E"}],
        bay = {"host": {"M": "living", "U": "bed2"}, "u0": 9.9, "u1": 21.9, "proj": 3.0},
        patterns = {"two_sided": ("weight", 30.0), "hall_light": ("hard",), "gradient": ("hard",)},
        joists = "2x12 @ 16 in (DF #2) from the hall wall to the east party wall: 16'-5\" clear",
        knobs = {"primary_at_street": _side_hall_master},
    )
    if floors == 4:
        t = _side_hall_4f(t)
    elif floors != 3:
        fail("plan_templates.side_hall: floors = 3 or 4, got %r" % floors)
    if primary_at_street:
        t = _side_hall_master(t)
    return _finish(t, mirror, split, court, light_well)

def _side_hall_4f(t):
    """Four floors: the primary suite and a family room move up to a top floor; a study on the upper floor."""
    RB = [(2, 3)]
    keep = [r for r in t["rooms"] if r["floor"] in ("G", "M")]
    U = [
        stair("U", (0, 0)),
        hall("hallU", "U", [(1, 1)], parents = ["stair.top"], hall = True),
        bedroom("bed2", RB, front = True, parents = ["hallU"]),
        bedroom("bed3", [(0, 1), (2, 2), (2, 3)], parents = ["hallU"]),
        extra_room("study", "U", [(2, 2), (2, 3), (0, 1)], parents = ["hallU"], optional = True),
        bath("bath2", "U", [(3, 3), (2, 2)], parents = ["hallU", "bed3"]),
        laundry([(3, 3), (2, 2)], parents = ["hallU"]),
        closet("clU1", "U", [(0, 0)], parents = ["hallU"]),
        closet("clU2", "U", [(0, 0), (3, 3)], parents = ["hallU", "bed2"]),
        well("U", (3, 3), rear = True, d = (6.0, 30.0), name = "Court (rear arm)"),
    ]
    T = [
        stair("T", (0, 0)),
        hall("hallT", "T", [(1, 1)], parents = ["stair.top"], hall = True),
        extra_room("fam", "T", RB, front = True, parents = ["hallT"]),
        primary([(2, 2), (2, 3)], rear = True, parents = ["hallT"], floor = "T"),
        pcloset([(2, 2), (3, 3)], parents = ["primary"], floor = "T"),
        bath("pbath", "T", [(3, 3), (2, 2)], parents = ["primary", "pcloset"]),
        closet("clT1", "T", [(0, 0)], parents = ["hallT"]),
        closet("clT2", "T", [(0, 0), (3, 3)], parents = ["hallT", "fam", "primary"]),
        well("T", (3, 3), rear = True, d = (6.0, 30.0), name = "Court (rear arm)"),
    ]
    q = copy_template(t)
    q["rooms"] = keep + U + T
    q["floors"] = ["G", "M", "U", "T"]
    q["orders"] = [("G", "garage", "den"), ("M", "living", "dining"), ("M", "dining", "kitchen"), ("T", "fam", "primary")]
    q["wells"] = [dict(q["wells"][0], floors = ["M", "U", "T"])]
    q["bay"] = dict(q["bay"], host = {"M": "living", "U": "bed2", "T": "fam"})
    q["key"] = t["key"] + "__4f"
    q["name"] = t["name"] + ", four floors"
    q["variant"] = list(t.get("variant", [])) + ["four floors"]
    q["program_add"] = ["fam", "study"]
    return q

# ================================================================== 2. cross stair: rooms front and back

def cross_stair(mirror = False, floors = 3, split = None, court = True, light_well = None):
    """The cross stair (Edwardian flats; SF builder houses of 1925-50): a straight stair runs across the house in the
    middle with a stair hall beside it; two blocks of rooms, one at the street and one at the garden, paired across the
    full 24-ft width; baths and stores fill the dark middle. Grid: c0 3'-0" | c1 8'-2" | c2 3'-11" | c3 3'-0" | c4
    4'-4"; the flight spans c1..c2."""
    rooms = [
        # ground: the entry on the east, a passage beside the garage to the stair hall across the middle
        vestibule([(3, 4)]),
        garage([(0, 1)]),
        stair("G", (0, 2)),
        hall("hallG1", "G", [(4, 4)], parents = ["vestibule"], name = "Entry hall"),
        hall("hallG2", "G", [(0, 4)], parents = ["hallG1"], name = "Stair hall"),
        hall("hallG3", "G", [(4, 4), (3, 4)], parents = ["hallG2"], name = "Hall", optional = True, garden = True),
        den([(0, 4), (0, 1)], rear = True, parents = ["hallG2", "hallG3"]),
        bath("bathG", "G", [(2, 3), (2, 4), (0, 1)], parents = ["hallG1", "hallG2", "hallG3", "den"]),
        storage("storeG", "G", [(0, 1), (0, 4), (2, 4), (2, 3)], parents = ["hallG1", "hallG2", "hallG3", "den"]),
        closet("clG1", "G", [(3, 3), (2, 3)], parents = ["hallG1"]),
        closet("clG2", "G", [(2, 2), (2, 3), (3, 3)], parents = ["vestibule", "hallG1"]),
        # main
        stair("M", (0, 3)),
        hall("hallM", "M", [(0, 4), (0, 3), (1, 4), (1, 3)], parents = ["stair.top"], name = "Stair hall", hall = False),
        hall("hallM2", "M", [(4, 4)], parents = ["stair.top", "hallM"], optional = True),
        living("M", [(0, 4), (0, 1)], front = True, parents = ["hallM"]),
        kitchen("M", [(2, 4), (0, 1)], rear = True, parents = ["stair.top", "hallM2", "hallM"]),
        dining("M", [(0, 1), (2, 4)], rear = True, parents = ["hallM", "stair.top", "hallM2", "kitchen"]),
        powder([(2, 2), (3, 4), (2, 3)], parents = ["hallM", "hallM2"]),
        pantry([(2, 2), (4, 4), (3, 4), (0, 0)], parents = ["kitchen", "hallM", "hallM2"]),
        closet("clM1", "M", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)], parents = ["hallM", "hallM2", "stair.top"]),
        closet("clM2", "M", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)],
               parents = ["hallM", "hallM2", "living", "dining", "kitchen"]),
        # upper
        stair("U", (1, 3)),
        hall("hallU", "U", [(0, 4), (0, 3), (1, 4), (0, 2)], parents = ["stair.top"], name = "Stair hall"),
        hall("hallU2", "U", [(4, 4)], parents = ["stair.top", "hallU"], optional = True),
        hall("hallUW", "U", [(0, 0)], parents = ["hallU"], optional = True),
        bedroom("bed2", [(0, 1), (0, 2)], front = True, parents = ["hallU"]),
        bedroom("bed3", [(2, 4), (3, 4)], front = True, parents = ["hallU"]),
        extra_room("bed4", "U", [(0, 1), (0, 2), (2, 4), (3, 4), (0, 4)], parents = ["hallU", "hallU2", "hallUW"]),
        primary([(0, 4), (0, 1), (2, 4), (0, 2)], rear = True, parents = ["hallU", "hallU2", "hallUW", "stair.top"]),
        bath("bath2", "U", [(2, 3), (0, 1), (2, 4), (3, 4)], parents = ["hallU", "hallU2", "hallUW"]),
        laundry([(2, 2), (2, 3), (0, 1), (3, 4)], parents = ["hallU", "hallU2", "hallUW"]),
        pcloset([(0, 0), (2, 3), (3, 4), (0, 1), (2, 4)], parents = ["primary"]),
        bath("pbath", "U", [(2, 4), (2, 3), (0, 1), (3, 4)], parents = ["primary", "pcloset"]),
        closet("clU1", "U", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)], parents = ["hallU", "hallU2", "hallUW"]),
        closet("clU2", "U", [(0, 0), (2, 2), (3, 3), (4, 4)], parents = ["hallU", "hallU2", "hallUW", "bed2", "bed3"]),
    ]
    t = _template(
        "cross_stair", "Cross stair, rooms front and back",
        "A straight stair runs across the house in the middle with a stair hall beside it; the house is two blocks " +
        "of rooms, one at the street and one at the garden, paired across the full 24-ft width; baths and stores " +
        "fill the dark middle.",
        "Edwardian flats; SF builder houses of 1925-50 (living in front, kitchen and dining behind)",
        [3.2, 11.8, 16.1, 19.5], {"type": "cross", "land_w": 0, "land_e": 3}, rooms,
        orders = [("G", "garage", "den"), ("M", "living", "hallM"), ("M", "hallM", "kitchen"), ("M", "hallM", "dining"),
                  ("U", "bed2", "primary"), ("U", "bed3", "primary"), ("U", "bed2", "hallU"), ("U", "hallU", "primary")],
        bearing = [], stacks = [2, 2, 2],
        bay = {"host": {"M": "living", "U": "bed2"}, "u0": 1.0, "u1": 11.0, "proj": 3.0},
        patterns = {"two_sided": ("weight", 30.0), "entrance_turn": ("weight", 10.0), "gradient": ("hard",)},
        joists = "14-in I-joists, party wall to party wall (24'-0\" clear); stair opening framed with headers",
        skylight = True,
    )
    if floors == 4:
        t = _cross_stair_4f(t)
    elif floors != 3:
        fail("plan_templates.cross_stair: floors = 3 or 4, got %r" % floors)
    return _finish(t, mirror, split, court, light_well)

def _cross_stair_4f(t):
    keep = [r for r in t["rooms"] if r["floor"] in ("G", "M")]
    U = [
        stair("U", (0, 3)),
        hall("hallU", "U", [(0, 4), (0, 3), (1, 4), (1, 3)], parents = ["stair.top"], name = "Stair hall"),
        hall("hallU2", "U", [(4, 4)], parents = ["stair.top", "hallU"], optional = True),
        bedroom("bed2", [(0, 1), (0, 2)], front = True, parents = ["hallU"]),
        bedroom("bed3", [(2, 4), (3, 4)], front = True, parents = ["hallU"]),
        extra_room("study", "U", [(0, 4), (0, 1), (2, 4), (0, 2)], rear = True, parents = ["hallU", "hallU2", "stair.top"],
                   optional = True),
        bath("bath2", "U", [(2, 3), (0, 1), (2, 4), (3, 4)], parents = ["hallU", "hallU2"]),
        laundry([(2, 2), (2, 3), (0, 1), (3, 4)], parents = ["hallU", "hallU2"]),
        closet("clU1", "U", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)], parents = ["hallU", "hallU2"]),
        closet("clU2", "U", [(0, 0), (2, 2), (3, 3), (4, 4)], parents = ["hallU", "hallU2", "bed2", "bed3"]),
    ]
    T = [
        stair("T", (1, 3)),
        hall("hallT", "T", [(0, 4), (0, 3), (1, 4), (0, 2)], parents = ["stair.top"], name = "Stair hall"),
        hall("hallT2", "T", [(4, 4)], parents = ["stair.top", "hallT"], optional = True),
        hall("hallTW", "T", [(0, 0)], parents = ["hallT"], optional = True),
        extra_room("fam", "T", [(0, 1), (0, 2), (0, 4)], front = True, parents = ["hallT"]),
        primary([(0, 4), (0, 1), (2, 4), (0, 2)], rear = True, parents = ["hallT", "hallT2", "hallTW", "stair.top"],
                floor = "T"),
        pcloset([(0, 0), (2, 3), (3, 4), (0, 1), (2, 4)], parents = ["primary"], floor = "T"),
        bath("pbath", "T", [(2, 4), (2, 3), (0, 1), (3, 4)], parents = ["primary", "pcloset"]),
        closet("clT1", "T", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)], parents = ["hallT", "hallT2", "hallTW"]),
        closet("clT2", "T", [(0, 0), (2, 2), (3, 3), (4, 4), (3, 4)], parents = ["hallT", "hallT2", "hallTW", "fam"]),
    ]
    q = copy_template(t)
    q["rooms"] = keep + U + T
    q["floors"] = ["G", "M", "U", "T"]
    q["orders"] = [o for o in t["orders"] if o[0] in ("G", "M")] + [("U", "bed2", "hallU"), ("U", "bed3", "hallU"),
                                                                   ("T", "fam", "hallT"), ("T", "hallT", "primary")]
    q["bay"] = dict(q["bay"], host = {"M": "living", "U": "bed2", "T": "fam"})
    q["key"] = t["key"] + "__4f"
    q["name"] = t["name"] + ", four floors"
    q["variant"] = list(t.get("variant", [])) + ["four floors"]
    q["program_add"] = ["fam", "study"]
    return q

# ================================================================== 3. court (the Patio Plan): rooms wrap a middle court

def court(mirror = False, floors = 3, split = None, court = True, light_well = None):
    """The middle court (the Rousseau brothers' 1932 Patio Plan): a light court about 7'-8" x 8-20 ft is cut into the
    east party wall in the middle of the house, from the main floor up; the stair lands in a dining room on the court,
    the living room and kitchen take the full width at the two ends and see the court too; upstairs a bedroom sits on
    it. Grid: c0 stair and entry 4'-0" | c1 3'-6" | c2 7'-7" | c3 7'-8" court on the east party wall."""
    rooms = [
        # ground
        vestibule([(0, 0), (0, 1)]),
        garage([(2, 3)], amax = 330.0),  # the grid leaves the garage the east 15'-8" (the stair's foot must stay reachable)
        stair("G", (0, 0)),
        hall("hallG", "G", [(0, 0), (1, 1), (0, 1), (1, 2), (1, 3), (3, 3)], parents = ["vestibule"], hall = False,
             garden = True),
        hall("hallG2", "G", [(0, 3), (1, 3), (0, 1), (2, 3), (1, 2)], parents = ["vestibule", "hallG"], optional = True,
             garden = True),
        den([(0, 3), (1, 3), (2, 3)], rear = True, parents = ["hallG", "hallG2"]),
        bath("bathG", "G", [(2, 2), (1, 2), (2, 3)], parents = ["hallG", "hallG2", "den"]),
        storage("storeG", "G", [(2, 3), (3, 3), (2, 2), (1, 3), (0, 0), (1, 2)], parents = ["hallG", "hallG2", "den", "garage"]),
        closet("clG1", "G", [(0, 0), (3, 3), (2, 2)], parents = ["hallG", "hallG2", "vestibule"]),
        closet("clG2", "G", [(0, 0), (3, 3), (2, 2)], parents = ["hallG", "hallG2", "den"]),
        # main: the stair lands in the dining room, which sits on the court; living in front, kitchen behind
        stair("M", (0, 0)),
        dining("M", [(1, 2)], parents = ["stair.top"], through = True),
        living("M", [(0, 3)], front = True, through = True, parents = ["dining"]),
        kitchen("M", [(1, 3), (2, 3), (0, 3)], rear = True, parents = ["dining"]),
        powder([(2, 2), (1, 2), (0, 1)], parents = ["dining", "living"]),
        pantry([(0, 1), (0, 0), (2, 2), (1, 1), (1, 2)], parents = ["kitchen", "dining"]),
        closet("clM1", "M", [(0, 0), (2, 2), (1, 1)], parents = ["dining", "living", "kitchen"]),
        closet("clM2", "M", [(0, 0), (1, 1)], parents = ["dining", "kitchen", "living"]),
        well("M", (3, 3), d = (8.0, 20.0), name = "Court"),
        # upper
        stair("U", (0, 0)),
        hall("hallU", "U", [(1, 1), (1, 2)], parents = ["stair.top"]),
        bedroom("bed2", [(0, 3), (1, 3), (2, 3)], front = True, parents = ["hallU"]),
        bedroom("bed3", [(2, 2)], parents = ["hallU"]),
        extra_room("bed4", "U", [(0, 1), (0, 3), (1, 3), (2, 3), (2, 2)], parents = ["hallU"]),
        primary([(0, 3), (1, 3), (2, 3)], rear = True, parents = ["hallU"]),
        bath("bath2", "U", [(0, 1), (2, 2), (1, 2)], parents = ["hallU"]),
        laundry([(0, 0), (0, 1), (2, 2)], parents = ["hallU"]),
        pcloset([(0, 1), (2, 2), (1, 2), (0, 0)], parents = ["primary"]),
        bath("pbath", "U", [(0, 1), (2, 2), (1, 2)], parents = ["primary", "pcloset"]),
        closet("clU1", "U", [(0, 0), (0, 1), (1, 1)], parents = ["hallU", "bed2", "primary"]),
        closet("clU2", "U", [(0, 0), (0, 1), (1, 1)], parents = ["hallU", "primary", "bed2"]),
        well("U", (3, 3), d = (8.0, 20.0), name = "Court"),
    ]
    t = _template(
        "court", "Middle court (the Patio Plan)",
        "A light court about 7'-8\" x 8-20 ft is cut into the east party wall in the middle of the house, from the " +
        "main floor up; the stair lands in a dining room on the court, the living room and kitchen take the full " +
        "width at the two ends and see the court too; upstairs a bedroom sits on it.",
        "Rousseau brothers' 1932 Patio Plan; SF light wells shared with the neighbour (Residential Design Guidelines)",
        [4.2, 8.1, 16.1], {"type": "side", "col": 0}, rooms,
        orders = [("G", "garage", "den"), ("M", "living", "dining"), ("M", "dining", "kitchen"), ("U", "bed2", "bed3"),
                  ("U", "bed3", "primary")],
        bearing = [], stacks = [2, 2, 1, 3],
        wells = [{"pre": "well", "col": 3, "floors": ["M", "U"], "rear": False, "lit_depth": 12.0, "side": "E"}],
        patterns = {"two_sided": ("weight", 30.0), "entrance_turn": ("weight", 10.0), "gradient": ("hard",)},
        joists = "14-in I-joists party wall to party wall; a beam along the court's west face",
        skylight = True,
    )
    if floors == 4:
        t = _court_4f(t)
    elif floors != 3:
        fail("plan_templates.court: floors = 3 or 4, got %r" % floors)
    return _finish(t, mirror, split, court, light_well)

def _court_4f(t):
    keep = [r for r in t["rooms"] if r["floor"] in ("G", "M")]
    U = [
        stair("U", (0, 0)),
        hall("hallU", "U", [(1, 1), (1, 2)], parents = ["stair.top"]),
        bedroom("bed2", [(0, 3), (1, 3), (2, 3)], front = True, parents = ["hallU"]),
        bedroom("bed3", [(2, 2)], parents = ["hallU"]),
        extra_room("study", "U", [(0, 3), (1, 3), (2, 3)], rear = True, parents = ["hallU"], optional = True),
        bath("bath2", "U", [(0, 1), (2, 2), (1, 2)], parents = ["hallU"]),
        laundry([(0, 0), (0, 1), (2, 2)], parents = ["hallU"]),
        closet("clU1", "U", [(0, 0), (0, 1), (1, 1)], parents = ["hallU", "bed2", "study"]),
        closet("clU2", "U", [(0, 0), (0, 1), (1, 1)], parents = ["hallU", "study", "bed2"]),
        well("U", (3, 3), d = (8.0, 20.0), name = "Court"),
    ]
    T = [
        stair("T", (0, 0)),
        hall("hallT", "T", [(1, 1), (1, 2)], parents = ["stair.top"]),
        extra_room("fam", "T", [(0, 3), (1, 3), (2, 3)], front = True, parents = ["hallT"]),
        primary([(0, 3), (1, 3), (2, 3)], rear = True, parents = ["hallT"], floor = "T"),
        pcloset([(0, 1), (2, 2), (1, 2), (0, 0)], parents = ["primary"], floor = "T"),
        bath("pbath", "T", [(0, 1), (2, 2), (1, 2)], parents = ["primary", "pcloset"]),
        closet("clT1", "T", [(0, 0), (0, 1), (1, 1), (2, 2)], parents = ["hallT", "fam", "primary"]),
        closet("clT2", "T", [(0, 0), (0, 1), (1, 1), (2, 2)], parents = ["hallT", "primary", "fam"]),
        well("T", (3, 3), d = (8.0, 20.0), name = "Court"),
    ]
    q = copy_template(t)
    q["rooms"] = keep + U + T
    q["floors"] = ["G", "M", "U", "T"]
    q["orders"] = [o for o in t["orders"] if o[0] in ("G", "M")] + [("U", "bed2", "bed3"), ("U", "bed3", "study"),
                                                                   ("T", "fam", "primary")]
    q["wells"] = [dict(q["wells"][0], floors = ["M", "U", "T"])]
    q["key"] = t["key"] + "__4f"
    q["name"] = t["name"] + ", four floors"
    q["variant"] = list(t.get("variant", [])) + ["four floors"]
    q["program_add"] = ["fam", "study"]
    return q

# ================================================================== 4. open core: an open main floor round a core

def open_core(mirror = False, floors = 3, split = None, court = True, light_well = None):
    """The open core: the stair, the powder room and pantry (and upstairs the baths and laundry) stack in one core in
    the middle; the main floor is one open room that loops round it -- dining at the street, living and deck at the
    garden, the kitchen at the heart beside the core. Grid: c0 passage 3'-6" | c1 stair 3'-6" | c2 wet core 5'-2" |
    c3 open 10'-7"."""
    rooms = [
        # ground
        vestibule([(0, 1)]),
        garage([(3, 3), (2, 3)]),
        stair("G", (1, 1)),
        hall("hallG", "G", [(0, 0)], parents = ["vestibule"], hall = True, garden = True),
        den([(1, 3), (0, 3)], rear = True, parents = ["hallG"]),
        bath("bathG", "G", [(2, 2), (2, 3)], parents = ["den", "hallG"]),
        storage("storeG", "G", [(1, 3), (1, 2), (2, 3), (3, 3), (2, 2)], parents = ["hallG", "den", "garage"]),
        storage("storeG2", "G", [(2, 2)], parents = ["vestibule", "garage"], optional = True, name = "Store"),
        closet("clG1", "G", [(1, 1), (2, 2)], parents = ["hallG", "den", "vestibule"]),
        closet("clG2", "G", [(1, 1), (2, 2)], parents = ["hallG", "den", "storeG"]),
        storage("workG", "G", [(2, 3), (3, 3), (2, 2), (1, 3), (1, 2), (1, 1)], name = "Workshop", kind = "workshop",
                optional = True, parents = ["hallG", "den", "garage", "storeG"]),
        # main: the dining at the street, the living and the deck at the garden, the kitchen at the heart
        stair("M", (1, 1)),
        living("M", [(0, 3)], rear = True, through = True, parents = ["stair.top"], open_to = ["passage", "kitchen"]),
        hall("passage", "M", [(0, 0)], name = "Passage", parents = ["living"], through = True,
             open_to = ["dining", "living"]),
        dining("M", [(0, 3)], front = True, through = True, parents = ["passage", "living"],
               open_to = ["passage", "kitchen"]),
        kitchen("M", [(3, 3)], parents = ["living", "dining"], lit = False, borrow = 16.0, open_to = ["dining", "living"]),
        powder([(2, 2)], parents = ["dining", "living", "kitchen"]),
        pantry([(2, 2)], parents = ["kitchen", "dining", "living"]),
        closet("clM1", "M", [(2, 2)], parents = ["dining", "living", "kitchen"]),
        # upper
        stair("U", (1, 1)),
        hall("hallU", "U", [(0, 0)], parents = ["stair.top"]),
        hall("hallU2", "U", [(0, 3), (0, 2), (2, 3)], parents = ["hallU"], optional = True, name = "Hall"),
        bedroom("bed2", [(0, 1), (0, 2), (0, 3)], front = True, parents = ["hallU", "hallU2"]),
        bedroom("bed3", [(2, 3), (3, 3)], front = True, parents = ["hallU2"], optional = False),
        extra_room("bed4", "U", [(0, 1), (0, 2), (2, 3), (3, 3), (0, 3)], parents = ["hallU", "hallU2"]),
        primary([(0, 3), (2, 3)], rear = True, parents = ["hallU", "hallU2"]),
        bath("bath2", "U", [(2, 2), (2, 3), (3, 3)], parents = ["hallU2", "hallU"]),
        laundry([(2, 2), (3, 3)], parents = ["hallU2", "hallU"]),
        pcloset([(2, 2), (3, 3), (2, 3)], parents = ["primary"]),
        bath("pbath", "U", [(2, 2), (3, 3), (2, 3)], parents = ["primary", "pcloset"]),
        closet("clU1", "U", [(2, 2), (3, 3)], parents = ["hallU2", "primary", "bed3"]),
        closet("clU2", "U", [(2, 2), (3, 3)], parents = ["hallU2", "primary", "bed3", "bed2"]),
    ]
    t = _template(
        "open_core", "Open floor around a core",
        "The stair, the powder room and pantry (and upstairs the baths and laundry) stack in one core in the middle; " +
        "the main floor is one open room that loops round it -- dining at the street, living and deck at the garden, " +
        "the kitchen at the heart beside the core.",
        "Modern SF remodels that open the floor front to back; Alexander's farmhouse kitchen (139) and flow through " +
        "rooms (131)",
        [3.7, 7.6, 13.2], {"type": "side", "col": 1}, rooms,
        orders = [("G", "garage", "den"), ("G", "vestibule", "hallG"), ("M", "dining", "kitchen"),
                  ("M", "kitchen", "living"), ("M", "dining", "passage"), ("M", "passage", "living"),
                  ("U", "bed2", "primary"), ("U", "bed3", "primary")],
        bearing = [], stacks = [2, 3, 3],
        patterns = {"two_sided": ("weight", 30.0), "entrance_turn": ("weight", 10.0)},
        joists = "14-in I-joists party wall to party wall (24'-0\" clear); the core's walls carry the stair's headers",
        skylight = True,
    )
    if floors == 4:
        t = _open_core_4f(t)
    elif floors != 3:
        fail("plan_templates.open_core: floors = 3 or 4, got %r" % floors)
    return _finish(t, mirror, split, court, light_well)

def _open_core_4f(t):
    keep = [r for r in t["rooms"] if r["floor"] in ("G", "M")]
    U = [
        stair("U", (1, 1)),
        hall("hallU", "U", [(0, 0)], parents = ["stair.top"]),
        hall("hallU2", "U", [(0, 3), (0, 2), (2, 3)], parents = ["hallU"], optional = True, name = "Hall"),
        bedroom("bed2", [(0, 1), (0, 2), (0, 3)], front = True, parents = ["hallU", "hallU2"]),
        bedroom("bed3", [(2, 3), (3, 3)], front = True, parents = ["hallU2"]),
        extra_room("study", "U", [(0, 3), (2, 3)], rear = True, parents = ["hallU", "hallU2"], optional = True),
        bath("bath2", "U", [(2, 2), (2, 3), (3, 3)], parents = ["hallU2", "hallU"]),
        laundry([(2, 2), (3, 3)], parents = ["hallU2", "hallU"]),
        closet("clU1", "U", [(2, 2), (3, 3)], parents = ["hallU2", "study", "bed3"]),
        closet("clU2", "U", [(2, 2), (3, 3)], parents = ["hallU2", "study", "bed3", "bed2"]),
    ]
    T = [
        stair("T", (1, 1)),
        hall("hallT", "T", [(0, 0)], parents = ["stair.top"]),
        hall("hallT2", "T", [(0, 3), (0, 2), (2, 3)], parents = ["hallT"], optional = True, name = "Hall"),
        extra_room("fam", "T", [(0, 1), (0, 2), (0, 3)], front = True, parents = ["hallT", "hallT2"]),
        primary([(0, 3), (2, 3)], rear = True, parents = ["hallT", "hallT2"], floor = "T"),
        pcloset([(2, 2), (3, 3), (2, 3)], parents = ["primary"], floor = "T"),
        bath("pbath", "T", [(2, 2), (3, 3), (2, 3)], parents = ["primary", "pcloset"]),
        closet("clT1", "T", [(2, 2), (3, 3)], parents = ["hallT2", "primary", "fam"]),
        closet("clT2", "T", [(2, 2), (3, 3)], parents = ["hallT2", "primary", "fam"]),
    ]
    q = copy_template(t)
    q["rooms"] = keep + U + T
    q["floors"] = ["G", "M", "U", "T"]
    q["orders"] = [o for o in t["orders"] if o[0] in ("G", "M")] + [("U", "bed2", "study"), ("U", "bed3", "study"),
                                                                   ("T", "fam", "primary")]
    q["key"] = t["key"] + "__4f"
    q["name"] = t["name"] + ", four floors"
    q["variant"] = list(t.get("variant", [])) + ["four floors"]
    q["program_add"] = ["fam", "study"]
    return q

# ------------------------------------------------------------------ the library

LIBRARY = {"side_hall": side_hall, "cross_stair": cross_stair, "court": court, "open_core": open_core}

def library(keys = None, **kw):
    """The templates named by `keys` (all four by default), each made with the same parameters `kw`."""
    ks = keys or ["side_hall", "cross_stair", "court", "open_core"]
    out = []
    for k in ks:
        if k not in LIBRARY:
            fail("plan_templates.library: no template %r (have: %s)" % (k, ", ".join(sorted(LIBRARY.keys()))))
        out.append(LIBRARY[k](**kw))
    return out

def program_with(*extra, base = None):
    """The benchmark program plus rooms from EXTRA_ROOMS (by key) or dicts {key: entry}."""
    out = dict(base or PROGRAM)
    for x in extra:
        if type(x) == "string":
            out[x] = EXTRA_ROOMS[x]
        else:
            out.update(x)
    return out
