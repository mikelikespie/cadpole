"""@cadpole/plan_furnish.star -- the room stage: one room's furniture or fixtures packed by a small MILP, re-walked
at the room's final size, and the fit tables and `fit_row` a plan MILP reads instead of furniture variables.

    load("@cadpole/plan_furnish.star", "furnish", "least_depth", "least_depth_record", "holds", "fit_table", "fit_row",
         "fit_lookup", "DOOR_CLASSES", "walk_check", "furnish_probe")

    r = furnish(room_local(plan, "bed2"), "full")      # @cadpole/plan.star's room_local(); the set from @cadpole/plan_furniture.star
    r["ok"], r["reason"]                                # True, "" -- or False and why not
    r["furniture"], r["fixtures"]                       # plan records, in plan coordinates (feet)

## furnish(local, set=None, ...)

`local` is `room_local()`'s shape (feet, the main rect's south-west inside corner at the origin, each
door's and window's `offset` the opening's centre along its wall). `set` names a set of `@cadpole/plan_furniture.star`
(`local["furnish"]`, else the room kind's default). The MILP places every item of the set against a wall (one binary
per wall and shape, a continuous offset along it) or free-standing (rotated 0 or 90 degrees), and keeps:

* every solid out of every other item's solid and use space (clearances may overlap each other);
* every solid out of each door's swing: the opening (the leaf plus 2 in of jambs), as deep as the leaf plus 18 in to
  step past it; at a door that swings away, slides or sits in a pocket, the 18-in step; at a cased opening, a 36-in
  landing (`door_step=` changes the 18 in; `strict=True` keeps the use spaces out of the leaf's own swing too, as the
  IRC figure's "to any wall, fixture or door" reads; the fit tables keep a 36-in landing at every opening
  that does not swing in);
* every piece taller than a window's sill out of a 24-in zone in front of that window ("only below-sill items under
  windows");
* every wet fixture on a wall that holds a stack, when the room has one (`wet="stack"`), or the fewest wet walls
  (`wet="fewest"`), or neither (`wet="any"`);
* every piece inside the room, on a wall the room has (not on an open side). A room of several rects (an L) is packed in
  its bounding box with the box's parts outside it kept clear of pieces and use spaces and of the walk, and doors in
  its inner walls kept clear and walked from.

Its objective: the walk from the door to each use point (an L1 stand-in, 0.02 a inch), less the
bonus of optional pieces placed, plus 100 a wet wall when it counts them.

**The re-walk.** Then every use point is walked from a door on a 2-in grid over the room's floor, the solids grown by
9 in (half of a person 18 in wide), diagonal steps never cutting a corner. A point that cannot be
reached cuts that placement (the walls and shapes chosen) and the MILP solves again, up to 6 times. So `ok` means every use point is reachable at the room's
final size. On that grid a diagonal step needs both of its sides free, so the reachable cells are exactly those
reachable by straight steps: the walk floods free runs row by row.

It returns `{"ok", "reason", "set", "furniture", "fixtures", "clears", "wet_walls", "cuts", "status", "layout", "W",
"D", "origin"}`. `furniture` and `fixtures` are plan records (`{"key", "room", "kind", "item", "piece", "rect", "front",
"h"}`, rect `(u0, u1, v0, v1)` in plan feet, `front` the side its use space faces, `h` its height in feet); `clears`
the use spaces, for drawing. When it is not ok, `reason` says why: the set does not fit (with `need_depth`, the least
depth that would hold it at this width), the wet fixtures do not all fit on the stack wall (with `relaxed`, the layout on
the fewest wet walls), a use point stays unreachable, or the catalog has no such set (never an error: an audit runs on).

## Fit tables and fit_row

`least_depth(set, width, door_class)` solves the room stage with the depth a variable and
returns the least clear depth, in feet, that holds the set at that clear width with that door class (None if none up
to 40 ft does). `fit_table(set, door_class, widths)` is one table: `{"set", "door", "rows": [(width, depth or None)]}`.
`@cadpole/plan_fit_tables.star` holds the generated ones.

`fit_row(p, w, d, table, name=)` adds to a plan MILP `p` the one rule "the room is at least as deep as the least depth
at its width". See its docstring for the exact encoding of the step function and why it is exact.

## Units

The room stage models in INCHES: its catalog and every source are in inches.
Everything that crosses this module's API is in feet. `fit_row` writes its rows in feet, into the caller's plan MILP.

Probe for the purity test: `furnish_probe()`.
"""

load("@cadpole/plan_furniture.star", "PIECES", "SETS", "SET_FOR_KIND", "TALL", "set_items")

IN = 12.0  # inches a foot
CELL = 2.0  # in: the re-walk's grid
BODY = 9.0  # in: half a person's width; solids grow by this on the grid
WALK_CUTS = 6  # placements cut before the re-walk gives up
OCT = 0.41421356237309515  # sqrt(2) - 1: the work triangle's octagonal norm
WALLS = ("S", "N", "W", "E")
WET_WALL = 100.0  # the objective's cost of one more wet wall (in of depth-equivalent)
_OPP = {"S": "N", "N": "S", "W": "E", "E": "W"}
_BIG = 1e18

# door openings (in): a leaf plus 2 in of jambs
OP_BED, OP_BATH, OP_FRONT, OP_HOUSE = 34.0, 32.0, 38.0, 34.0
CORE = 45.0  # the side-hall door: its near edge 45 in from the room's west end

def furnish_probe():
    """The purity probe: a pure function (an empty room's re-walk)."""
    return walk_check({"W": 48.0, "D": 48.0, "rects": [], "doors": [("door0", 0.0, 0.0, 30.0, 36.0)], "door_specs": [("S", 0.0, 30.0, True)]}, None, {})

def _fail(msg):
    fail("plan_furnish: " + msg)

def _num(x):
    return type(x) == "int" or type(x) == "float"

def _sub(a, b):
    """a - b where a may be a plain number (Starlark has no reflected subtraction): written as -b + a, which is the same
    rows and the same constant as the Python shim's a - b."""
    if _num(a):
        if _num(b):
            return a - b
        return -b + a
    return a - b

# ---------------------------------------------------------------------------------------------- the spec

def _item(name, shapes, walls = "any", h = 0.0, optional = False, bonus = 0.0, wet = False, fixture = False, piece = None):
    return {"name": name, "shapes": shapes, "walls": walls, "h": float(h), "optional": optional, "bonus": float(bonus),
            "wet": wet, "fixture": fixture, "piece": piece if piece != None else name}

def _door(wall, off, op, swing, leaf = None, step = 18.0, name = None, landing = 36.0):
    """One door as the room stage sees it: `off` the opening's low end along its wall (in), a number, ("hi", k) (k from
    the high end) or ("free", a, b) (anywhere a from the low end and b from the high end); `op` the opening; `swing`
    True when its leaf swings into the room. The keep-out is op x (leaf + step), or op x `landing` (36 in: the
    landing at an opening that does not swing in)."""
    return {"wall": wall, "off": off, "op": float(op), "swing": swing, "leaf": float(leaf if leaf != None else (op - 2.0 if swing else 0.0)),
            "step": float(step), "name": name, "landing": float(landing)}

def _spec(set_name, W, doors, window = None, open_sides = (), keep = (), windows = (), variant = None, wet = "any", stack_walls = (), strict = False, name = None, without = (),
          keep_clear = (), starts = (), block = ()):
    """The internal spec of one room (inches): the set's items with their shapes, the doors, the free window zone
    (wall, length) a fit table assumes or the room's fixed `windows` [(wall, lo, hi, sill)], the open sides, fixed
    keep-outs [(name, x0, y0, x1, y1)]."""
    if set_name not in SETS:
        _fail("no set named %r" % set_name)
    st = SETS[set_name]
    items = []
    for it in set_items(set_name):
        if it["name"] in without:
            continue
        shapes = it["shapes"]
        if it["piece"] == "car":
            # the car's clear floor is the garage's width
            sh = shapes[0]
            shapes = [{"w": float(W), "d": sh["d"], "rects": [sh["rects"][0], (0.0, 0.0, 120.0, 240.0, "clear", "10 x 20 clear")], "points": sh["points"], "corner": "lo"}]
        walls = it["walls"]
        if walls == "any":
            walls = WALLS
        items.append(_item(it["name"], shapes, walls, it["h"], it["optional"], it["bonus"], it["wet"], it["fixture"], it["piece"]))
    return {"name": name if name != None else st["label"], "set": set_name, "items": items, "doors": list(doors), "window": window,
            "windows": list(windows), "open_sides": tuple(open_sides), "min_d": st["min_d"], "keep": list(keep), "extra": list(st["extra"]),
            "wet": wet, "stack_walls": tuple(stack_walls), "strict": strict, "keep_clear": list(keep_clear), "starts": list(starts),
            "block": list(block)}

# ---------------------------------------------------------------------------------------------- the model

def _nm(m, family, what):
    return "%s:%s:%s:%s" % (family, m["floor"], m["room"], what)

def _fname(*parts):
    """A feature name from parts: letters, digits, '_', '.', '-' and '+' kept, anything else '_' (a feature name may not
    hold ':', '#' or '/')."""
    out = []
    for c in "_".join([str(x) for x in parts]).elems():
        out.append(c if (c.isalnum() or c in "_.-+") else "_")
    return "".join(out)

def _build(spec, W, D = None, Dfix = None, floor = "-", room = None, fname = None):
    """The room model of `spec` at width W, the depth a variable in D=(lo, hi) or fixed at Dfix= (inches)."""
    W = float(W)
    if fname == None:
        fname = _fname("plan_furnish", room if room != None else spec["set"], spec["set"], "%dx%s" % (int(W), "var" if Dfix == None else str(int(Dfix))))
    p = opt.problem(name = fname)
    m = {"spec": spec, "W": W, "p": p, "floor": floor, "room": room if room != None else spec["set"], "R": [], "place": {}, "pts": {},
         "door_vars": {}, "window_var": None, "triangle": None, "K": [], "slides": {}}
    if Dfix != None:
        m["D"] = float(Dfix)
        m["Dvar"] = None
        m["dmax"] = float(Dfix)
    else:
        lo, hi = D if D != None else (spec["min_d"], 480.0)
        v = p.var("D", min = float(lo), max = float(hi))
        m["Dvar"] = v
        m["D"] = v
        m["dmax"] = float(hi)
    m["M"] = 2.0 * (W + m["dmax"]) + 60.0
    m["doors"] = spec["doors"]
    _items(m)
    _keepouts(m)
    _separations(m)
    if "triangle" in spec["extra"]:
        _triangle_rows(m)
    _wet_rows(m)
    return m

def _rvars(m, nm):
    p, W = m["p"], m["W"]
    return {"x0": p.var(nm + ".x0", min = -1.0, max = W + 1.0), "x1": p.var(nm + ".x1", min = -1.0, max = W + 1.0),
            "y0": p.var(nm + ".y0", min = -1.0, max = m["dmax"] + 1.0), "y1": p.var(nm + ".y1", min = -1.0, max = m["dmax"] + 1.0)}

def _eq(m, a, b, z, nm):
    """a == b when z == 1 (two rows, slack by M otherwise)."""
    M = m["M"]
    off = M * (-z + 1)
    m["p"].constrain(_sub(a, b) - off, max = 0, name = nm + " le")
    m["p"].constrain(_sub(a, b) + off, min = 0, name = nm + " ge")

def _map(m, wl, s, fx, fy, u0, v0, u1, v1):
    W, D = m["W"], m["D"]
    if wl == "S":
        return (s + u0, v0, s + u1, v1)
    if wl == "N":
        return (s + u0, _sub(D, v1), s + u1, _sub(D, v0))
    if wl == "W":
        return (v0, s + u0, v1, s + u1)
    if wl == "E":
        return (W - v1, s + u0, W - v0, s + u1)
    if wl == "F0":
        return (fx + u0, fy + v0, fx + u1, fy + v1)
    if wl == "F1":
        return (fx + v0, fy + u0, fx + v1, fy + u1)
    _fail("wall %r" % wl)

def _items(m):
    p, W, D, M, spec = m["p"], m["W"], m["D"], m["M"], m["spec"]
    for it in spec["items"]:
        name = it["name"]
        opts = {}
        order = []
        if it["walls"] == None:
            for si in range(len(it["shapes"])):
                for rot in (0, 1):
                    k = ("F%d" % rot, si)
                    opts[k] = p.binary("%s free r%d s%d" % (name, rot, si))
                    order.append(k)
        else:
            for wl in [w for w in it["walls"] if w not in spec["open_sides"]]:
                for si in range(len(it["shapes"])):
                    k = (wl, si)
                    opts[k] = p.binary("%s on %s s%d" % (name, wl, si))
                    order.append(k)
        m["place"][name] = opts
        tot = opt.sum([opts[k] for k in order])
        if it["optional"]:
            p.constrain(tot, max = 1, name = _nm(m, "piece", name + " once"))
        else:
            p.constrain(tot, equals = 1, name = _nm(m, "piece", name + " placed"))
        s = p.var(name + ".s", min = 0.0, max = max(W, m["dmax"]))
        fx = p.var(name + ".fx", min = 0.0, max = W)
        fy = p.var(name + ".fy", min = 0.0, max = m["dmax"])
        nrect = max([len(sh["rects"]) for sh in it["shapes"]])
        rv = [_rvars(m, "%s.r%d" % (name, k)) for k in range(nrect)]
        # a use space that slides along its piece (a tub's step-in): one offset variable for the item
        tsl = None
        for sh in it["shapes"]:
            for r in sh["rects"]:
                if r[4] == "slide" and tsl == None:
                    tsl = p.var(name + ".slide", min = 0.0, max = max([x["w"] for x in it["shapes"]]))
        kinds = {}
        for key in order:
            wl, si = key
            z = opts[key]
            sh = it["shapes"][si]
            L = None
            if wl in ("S", "N"):
                p.constrain(s + sh["w"] - W - M * (-z + 1), max = 0, name = _nm(m, "piece", "%s %s%d fits along" % (name, wl, si)))
                L = W
            elif wl in ("W", "E"):
                p.constrain(_sub(s + sh["w"], D) - M * (-z + 1), max = 0, name = _nm(m, "piece", "%s %s%d fits along" % (name, wl, si)))
                L = D
            if sh["corner"] == "lo" and wl in WALLS:
                p.constrain(s - M * (-z + 1), max = 0, name = _nm(m, "piece", "%s %s%d at lo corner" % (name, wl, si)))
            elif sh["corner"] == "hi" and wl in WALLS:
                _eq(m, s + sh["w"], L, z, _nm(m, "piece", "%s %s%d at hi corner" % (name, wl, si)))
            for k in range(nrect):
                if k >= len(sh["rects"]):
                    for c in ("x0", "x1", "y0", "y1"):
                        _eq(m, rv[k][c], 0.0, z, _nm(m, "piece", "%s r%d park %s %s%d" % (name, k, c, wl, si)))
                    continue
                u0, v0, u1, v1, kind, tag = sh["rects"][k]
                if kind == "slide":
                    ln = u1 - u0
                    p.constrain(tsl + ln - sh["w"] - M * (-z + 1), max = 0, name = _nm(m, "piece", "%s r%d slides within %s%d" % (name, k, wl, si)))
                    U0, U1 = tsl, tsl + ln
                    kind = "clear"
                else:
                    U0, U1 = u0, u1
                kinds.setdefault(k, {})[kind] = True
                X0, Y0, X1, Y1 = _map(m, wl, s, fx, fy, U0, v0, U1, v1)
                for c, e in (("x0", X0), ("y0", Y0), ("x1", X1), ("y1", Y1)):
                    _eq(m, rv[k][c], e, z, _nm(m, "piece", "%s r%d %s %s%d" % (name, k, c, wl, si)))
            for pn, (pu, pv) in sh["points"].items():
                key2 = (name, pn)
                if key2 not in m["pts"]:
                    m["pts"][key2] = (p.var("%s.%s.x" % key2, min = -1.0, max = W + 1), p.var("%s.%s.y" % key2, min = -1.0, max = m["dmax"] + 1))
                PX, PY, _, _ = _map(m, wl, s, fx, fy, pu, pv, pu, pv)
                _eq(m, m["pts"][key2][0], PX, z, _nm(m, "piece", "%s pt %s x %s%d" % (name, pn, wl, si)))
                _eq(m, m["pts"][key2][1], PY, z, _nm(m, "piece", "%s pt %s y %s%d" % (name, pn, wl, si)))
        for k in range(nrect):
            kd = kinds.get(k, {"solid": True})
            kind = "solid" if "solid" in kd else "clear"
            r = {"name": "%s.r%d" % (name, k), "item": name, "kind": kind, "h": it["h"], "x0": rv[k]["x0"], "x1": rv[k]["x1"], "y0": rv[k]["y0"], "y1": rv[k]["y1"]}
            p.constrain(r["x0"], min = 0.0 - 1e-6, name = _nm(m, "inside", r["name"] + " in W"))
            p.constrain(r["x1"] - W, max = 0.0, name = _nm(m, "inside", r["name"] + " in E"))
            p.constrain(r["y0"], min = 0.0, name = _nm(m, "inside", r["name"] + " in S"))
            p.constrain(_sub(r["y1"], D), max = 0.0, name = _nm(m, "inside", r["name"] + " in N"))
            m["R"].append(r)
        it["_s"] = s

def _door_off(m, i, dr):
    wl, off, op = dr["wall"], dr["off"], dr["op"]
    L = m["W"] if wl in ("S", "N") else m["D"]
    if _num(off):
        return float(off)
    if off[0] == "hi":
        return _sub(L, off[1] + op) if not _num(L) else L - off[1] - op
    if off[0] == "free":
        if i not in m["door_vars"]:
            v = m["p"].var("door%d.off" % i, min = 0.0, max = max(m["W"], m["dmax"]))
            m["p"].constrain(v, min = off[1], name = _nm(m, "door", "door%d from lo" % i))
            m["p"].constrain(_sub(v + op, L), max = -off[2], name = _nm(m, "door", "door%d from hi" % i))
            m["door_vars"][i] = v
        return m["door_vars"][i]
    _fail("door offset %r" % (off,))

def _door_rects(m):
    """Each door's keep-out: the opening on its wall and the swing inside, as deep as the leaf plus `step` (18 in to
    step past it), or a 36-in landing at an opening that does not swing in."""
    W, D = m["W"], m["D"]
    out = []
    for i, dr in enumerate(m["doors"]):
        wl, op, swing = dr["wall"], dr["op"], dr["swing"]
        off = _door_off(m, i, dr)
        depth = (dr["leaf"] + dr["step"]) if swing else dr["landing"]
        nm = "door%d" % i
        if wl == "S":
            out.append((nm, off, 0.0, off + op, depth))
        elif wl == "N":
            out.append((nm, off, _sub(D, depth), off + op, D))
        elif wl == "W":
            out.append((nm, 0.0, off, depth, off + op))
        else:
            out.append((nm, W - depth, off, W, off + op))
    return out

def _keepouts(m):
    """Keep-outs: (name, x0, y0, x1, y1, h_max, target). target "solid": a solid taller than h_max stays out (door swings
    and the step past them, landings, window zones, obstacles); "clear": a use space stays out (strict mode: the leaf's
    own swing of a door that opens into the room)."""
    p, W, D, spec = m["p"], m["W"], m["D"], m["spec"]
    K = []
    for i, (nm, a, b, c, d) in enumerate(_door_rects(m)):
        K.append((nm, a, b, c, d, -1.0, "solid"))
    if spec["strict"]:
        # the leaf's own swing (no step past it), kept clear of use spaces too
        for i, dr in enumerate(m["doors"]):
            if not dr["swing"]:
                continue
            nm, a, b, c, d = _door_rects(m)[i]
            leaf = dr["leaf"]
            if dr["wall"] == "S":
                K.append((nm + " swing", a, 0.0, c, leaf, -1.0, "clear"))
            elif dr["wall"] == "N":
                K.append((nm + " swing", a, _sub(m["D"], leaf), c, m["D"], -1.0, "clear"))
            elif dr["wall"] == "W":
                K.append((nm + " swing", 0.0, b, leaf, d, -1.0, "clear"))
            else:
                K.append((nm + " swing", W - leaf, b, W, d, -1.0, "clear"))
    if spec["window"] != None:
        wl, ln = spec["window"]
        wv = p.var("window.s", min = 0.0, max = max(W, m["dmax"]))
        if wl == "S":
            p.constrain(wv + ln - W, max = 0, name = _nm(m, "window", "window fits"))
            box = (wv, 0.0, wv + ln, 24.0)
        elif wl == "N":
            p.constrain(wv + ln - W, max = 0, name = _nm(m, "window", "window fits"))
            box = (wv, _sub(D, 24.0), wv + ln, D)
        elif wl == "E":
            p.constrain(_sub(wv + ln, D), max = 0, name = _nm(m, "window", "window fits"))
            box = (W - 24.0, wv, W, wv + ln)
        else:
            p.constrain(_sub(wv + ln, D), max = 0, name = _nm(m, "window", "window fits"))
            box = (0.0, wv, 24.0, wv + ln)
        m["window_var"] = wv
        K.append(("window", box[0], box[1], box[2], box[3], TALL, "solid"))
    for k, (wl, lo, hi, sill) in enumerate(spec["windows"]):
        if wl == "S":
            box = (lo, 0.0, hi, 24.0)
        elif wl == "N":
            box = (lo, _sub(D, 24.0), hi, D)
        elif wl == "E":
            box = (W - 24.0, lo, W, hi)
        else:
            box = (0.0, lo, 24.0, hi)
        K.append(("window%d" % k, box[0], box[1], box[2], box[3], float(sill), "solid"))
    for kp in spec["keep"]:
        nm, a, b, c, d = kp[0], kp[1], kp[2], kp[3], kp[4]
        K.append((nm, a, b, c, d, kp[5] if len(kp) > 5 else -1.0, "solid"))
    for kp in spec.get("keep_clear", []):
        K.append((kp[0], kp[1], kp[2], kp[3], kp[4], -1.0, "clear"))
    m["K"] = K

def _apart(m, A, B, nm):
    """A and B do not overlap: one of four separations holds."""
    p, M = m["p"], m["M"]
    bs = [p.binary(nm + " " + t) for t in ("L", "R", "B", "A")]
    p.constrain(opt.sum(bs), min = 1, name = _nm(m, "apart", nm + " one"))
    ax0, ay0, ax1, ay1 = A
    bx0, by0, bx1, by1 = B
    p.constrain(_sub(ax1, bx0) - M * (-bs[0] + 1), max = 0, name = _nm(m, "apart", nm + " L"))
    p.constrain(_sub(bx1, ax0) - M * (-bs[1] + 1), max = 0, name = _nm(m, "apart", nm + " R"))
    p.constrain(_sub(ay1, by0) - M * (-bs[2] + 1), max = 0, name = _nm(m, "apart", nm + " B"))
    p.constrain(_sub(by1, ay0) - M * (-bs[3] + 1), max = 0, name = _nm(m, "apart", nm + " A"))
    return bs

def _separations(m):
    R, K, strict = m["R"], m["K"], m["spec"]["strict"]
    for i in range(len(R)):
        a = R[i]
        for j in range(i + 1, len(R)):
            b = R[j]
            if a["item"] == b["item"]:
                continue
            if a["kind"] == "clear" and b["kind"] == "clear":
                continue
            _apart(m, (a["x0"], a["y0"], a["x1"], a["y1"]), (b["x0"], b["y0"], b["x1"], b["y1"]), "%s vs %s" % (a["name"], b["name"]))
        for k in K:
            if a["kind"] != k[6]:
                continue  # solids against the solid keep-outs; use spaces against the swings (strict) only
            if a["kind"] == "solid" and a["h"] <= k[5]:
                continue  # no taller than the window's sill: it may stand there
            _apart(m, (a["x0"], a["y0"], a["x1"], a["y1"]), (k[1], k[2], k[3], k[4]), "%s vs %s" % (a["name"], k[0]))

def _oct_leg(m, a, b, nm):
    """A leg in the octagonal norm: >= |dx| + 0.414|dy| and >= 0.414|dx| + |dy| (never under the straight leg, at most 8 %
    over it)."""
    p = m["p"]
    tx, ty = p.var(nm + ".dx", min = 0.0), p.var(nm + ".dy", min = 0.0)
    p.constrain(tx - (a[0] - b[0]), min = 0, name = _nm(m, "triangle", nm + " dx+"))
    p.constrain(tx + (a[0] - b[0]), min = 0, name = _nm(m, "triangle", nm + " dx-"))
    p.constrain(ty - (a[1] - b[1]), min = 0, name = _nm(m, "triangle", nm + " dy+"))
    p.constrain(ty + (a[1] - b[1]), min = 0, name = _nm(m, "triangle", nm + " dy-"))
    L = p.var(nm, min = 0.0)
    p.constrain(L - tx - OCT * ty, min = 0, name = _nm(m, "triangle", nm + " oct1"))
    p.constrain(L - OCT * tx - ty, min = 0, name = _nm(m, "triangle", nm + " oct2"))
    return L

def _triangle_rows(m):
    """NKBA KPG 3: legs between the centre-fronts of sink, range and fridge each 4 to 9 ft, the total at most 26 ft."""
    p, M = m["p"], m["M"]
    P = {}
    for k in ("sinkrun.sink", "rangerun.range", "fridge.fridge"):
        a, b = k.split(".")
        P[k] = m["pts"][(a, b)]
    legs = []
    for a, b in (("sinkrun.sink", "rangerun.range"), ("rangerun.range", "fridge.fridge"), ("fridge.fridge", "sinkrun.sink")):
        L = _oct_leg(m, P[a], P[b], "leg %s-%s" % (a, b))
        p.constrain(L, max = 108.0, name = _nm(m, "triangle", "KPG3 leg %s-%s <= 9 ft" % (a, b)))
        legs.append(L)
        bs = [p.binary("leg %s-%s far %d" % (a, b, k)) for k in range(4)]
        p.constrain(bs[0] + bs[1] + bs[2] + bs[3], min = 1, name = _nm(m, "triangle", "KPG3 leg %s-%s >= 4 ft" % (a, b)))
        dx = P[a][0] - P[b][0]
        dy = P[a][1] - P[b][1]
        p.constrain(dx - 48.0 + M * (-bs[0] + 1), min = 0, name = _nm(m, "triangle", "leg %s-%s +x" % (a, b)))
        p.constrain(-dx - 48.0 + M * (-bs[1] + 1), min = 0, name = _nm(m, "triangle", "leg %s-%s -x" % (a, b)))
        p.constrain(dy - 48.0 + M * (-bs[2] + 1), min = 0, name = _nm(m, "triangle", "leg %s-%s +y" % (a, b)))
        p.constrain(-dy - 48.0 + M * (-bs[3] + 1), min = 0, name = _nm(m, "triangle", "leg %s-%s -y" % (a, b)))
    p.constrain(legs[0] + legs[1] + legs[2], max = 312.0, name = _nm(m, "triangle", "KPG3 triangle <= 26 ft"))
    m["triangle"] = legs

def _wet_rows(m):
    """Wet fixtures on the stack walls (wet="stack", when the room has a stack), or a binary per wet wall the objective
    counts (wet="fewest", and "stack" in a room without one)."""
    spec = m["spec"]
    mode = spec["wet"]
    m["wet"] = None
    if mode == "any":
        return
    wet_items = [it for it in spec["items"] if it["wet"] and it["walls"] != None]
    if not wet_items:
        return
    p = m["p"]
    if mode == "stack" and spec["stack_walls"]:
        for it in wet_items:
            for (wl, si), z in m["place"][it["name"]].items():
                if wl not in spec["stack_walls"]:
                    p.constrain(z, max = 0, name = _nm(m, "wet", "%s on %s s%d: not a stack wall" % (it["name"], wl, si)))
        return
    wet = {}
    for w in WALLS:
        wet[w] = p.binary("wet " + w)
    for it in wet_items:
        for (wl, si), z in m["place"][it["name"]].items():
            p.constrain(wet[wl] - z, min = 0, name = _nm(m, "wet", "wet %s %s s%d" % (wl, it["name"], si)))
    m["wet"] = wet

def _l1(m, a, b, nm):
    """|a - b| as an auxiliary sum, exact when minimised."""
    p = m["p"]
    tx, ty = p.var(nm + ".dx", min = 0.0), p.var(nm + ".dy", min = 0.0)
    p.constrain(tx - _sub(a[0], b[0]), min = 0, name = _nm(m, "walk", nm + " dx+"))
    p.constrain(tx + _sub(a[0], b[0]), min = 0, name = _nm(m, "walk", nm + " dx-"))
    p.constrain(ty - _sub(a[1], b[1]), min = 0, name = _nm(m, "walk", nm + " dy+"))
    p.constrain(ty + _sub(a[1], b[1]), min = 0, name = _nm(m, "walk", nm + " dy-"))
    return tx + ty

def _door_mid(m, k = 0):
    nm, a, b, c, d = _door_rects(m)[k]
    wl = m["doors"][k]["wall"]
    if wl == "S":
        return (0.5 * (a + c), 0.0)
    if wl == "N":
        return (0.5 * (a + c), m["D"])
    if wl == "W":
        return (0.0, 0.5 * (b + d))
    return (m["W"], 0.5 * (b + d))

def _walk_proxy(m, k = 0):
    """Sum over the use points of the L1 distance from door k's middle (in)."""
    dm = _door_mid(m, k)
    out = 0.0
    for key, (xv, yv) in m["pts"].items():
        out = out + 1.0 * _l1(m, (xv, yv), dm, "walk %s.%s" % key)
    return out

# ---------------------------------------------------------------------------------------------- solving and the re-walk

def _objective(m, wp, minimize_d):
    obj = 0.0
    if minimize_d and m["Dvar"] != None:
        obj = obj + m["Dvar"]
    bonus = []
    for it in m["spec"]["items"]:
        if it["optional"] and it["bonus"]:
            bonus.append(it["bonus"] * opt.sum(list(m["place"][it["name"]].values())))
    if bonus:
        obj = _sub(obj, opt.sum(bonus))
    if wp != None:
        obj = obj + wp
    if m.get("wet") != None:
        obj = obj + WET_WALL * opt.sum([m["wet"][w] for w in WALLS])
    return obj

def _value(sol, e):
    return e if _num(e) else sol.value(e)

def _layout(m, sol):
    D = sol.value(m["Dvar"]) if m["Dvar"] != None else m["D"]
    placed = {}
    for it in m["spec"]["items"]:
        for k, z in m["place"][it["name"]].items():
            if sol.value(z) > 0.5:
                placed[it["name"]] = k
                break
    rects = []
    for r in m["R"]:
        if r["item"] not in placed:
            continue
        x0, y0, x1, y1 = sol.value(r["x0"]), sol.value(r["y0"]), sol.value(r["x1"]), sol.value(r["y1"])
        if x1 - x0 < 1e-6 and y1 - y0 < 1e-6:
            continue
        rects.append({"item": r["item"], "kind": r["kind"], "h": r["h"], "name": r["name"], "x0": x0, "y0": y0, "x1": x1, "y1": y1})
    pts = {}
    for key, (xv, yv) in m["pts"].items():
        if key[0] in placed:
            pts["%s.%s" % key] = (sol.value(xv), sol.value(yv))
    doors = [(nm, _value(sol, a), _value(sol, b), _value(sol, c), _value(sol, d)) for nm, a, b, c, d in _door_rects(m)]
    win = None
    if m["window_var"] != None:
        win = (m["spec"]["window"][0], sol.value(m["window_var"]), m["spec"]["window"][1])
    return {"W": m["W"], "D": D, "rects": rects, "points": pts, "placed": placed, "doors": doors,
            "door_specs": [(d["wall"], 0.0, d["op"], d["swing"]) for d in m["doors"]], "window": win}

def _runs(lay, nx, ny):
    """The free cells of each grid row as runs [(i0, i1)]: every solid grown by BODY blocks a box of cells."""
    blocked = [[] for _ in range(ny)]
    for r in lay["rects"]:
        if r["kind"] != "solid":
            continue
        i0 = max(0, int((r["x0"] - BODY) / CELL))
        i1 = min(nx - 1, int(math.ceil((r["x1"] + BODY) / CELL)) - 1)
        j0 = max(0, int((r["y0"] - BODY) / CELL))
        j1 = min(ny - 1, int(math.ceil((r["y1"] + BODY) / CELL)) - 1)
        if i1 < i0:
            continue
        for j in range(j0, j1 + 1):
            blocked[j].append((i0, i1))
    runs = []
    for j in range(ny):
        iv = sorted(blocked[j])
        free = []
        at = 0
        for (a, b) in iv:
            if a > at:
                free.append((at, a - 1))
            if b + 1 > at:
                at = b + 1
        if at <= nx - 1:
            free.append((at, nx - 1))
        runs.append(free)
    return runs

def _cell_of(pt, nx, ny):
    return (min(nx - 1, max(0, int(pt[0] / CELL))), min(ny - 1, max(0, int(pt[1] / CELL))))

def _nearest_free(runs, c, nx, ny):
    """The nearest free cell: the least squared distance within 40 cells, ties to the least i, then the least j."""
    i, j = c
    for (a, b) in runs[j]:
        if a <= i and i <= b:
            return c
    best = None
    for jj in range(max(0, j - 40), min(ny, j + 41)):
        for (a, b) in runs[jj]:
            lo, hi = max(a, i - 40), min(b, i + 40, nx - 1)
            if lo > hi:
                continue
            ii = i if (lo <= i and i <= hi) else (lo if i < lo else hi)
            key = ((ii - i) * (ii - i) + (jj - j) * (jj - j), ii, jj)
            if best == None or key < best:
                best = key
    return None if best == None else (best[1], best[2])

def _flood(runs, start, ny):
    """The runs reachable from `start` by straight steps (the same cells as 8-connected steps that never cut a corner)."""
    seen = {}
    i, j = start
    first = None
    for (a, b) in runs[j]:
        if a <= i and i <= b:
            first = (j, a, b)
    if first == None:
        return seen
    stack = [first]
    seen[first] = True
    for _ in range(1000000):
        if not stack:
            break
        jj, a, b = stack.pop()
        for nj in (jj - 1, jj + 1):
            if nj < 0 or nj >= ny:
                continue
            for (c, d) in runs[nj]:
                if c <= b and a <= d:
                    k = (nj, c, d)
                    if k not in seen:
                        seen[k] = True
                        stack.append(k)
    return seen

def _run_of(runs, c):
    i, j = c
    for (a, b) in runs[j]:
        if a <= i and i <= b:
            return (j, a, b)
    return None

def walk_check(lay, starts, targets):
    """Which of `targets` ({name: (x, y)} in, room frame) a person can reach from any of `starts` ([(x, y)]; None: the
    middle of the first door, 1 in inside) on the room's 2-in grid, the solids grown by 9 in. Returns the names that
    cannot be reached (a start or target in a solid moves to its nearest free cell)."""
    W, D = lay["W"], lay["D"]
    nx, ny = int(math.ceil(W / CELL)), int(math.ceil(D / CELL))
    runs = _runs(lay, nx, ny)
    if starts == None:
        starts = [_door_point(lay, 0)]
    reach = {}
    for st in starts:
        s = _nearest_free(runs, _cell_of(st, nx, ny), nx, ny)
        if s == None:
            continue
        for k in _flood(runs, s, ny).keys():
            reach[k] = True
    bad = []
    for name, pt in targets.items():
        c = _nearest_free(runs, _cell_of(pt, nx, ny), nx, ny)
        if c == None or _run_of(runs, c) not in reach:
            bad.append(name)
    return bad

def _door_point(lay, k):
    nm, a, b, c, d = lay["doors"][k]
    wl = lay["door_specs"][k][0]
    if wl == "S":
        return ((a + c) / 2.0, 1.0)
    if wl == "N":
        return ((a + c) / 2.0, lay["D"] - 1.0)
    if wl == "W":
        return (1.0, (b + d) / 2.0)
    return (lay["W"] - 1.0, (b + d) / 2.0)

def _access(lay, open_sides, n_doors):
    """Where a walk into the room starts: each door's middle, or the middle of an open side."""
    if n_doors:
        return [_door_point(lay, k) for k in range(n_doors)]
    if "W" in open_sides:
        return [(1.0, lay["D"] / 2.0)]
    if "N" in open_sides:
        return [(lay["W"] / 2.0, lay["D"] - 1.0)]
    return [(lay["W"] / 2.0, 1.0)]

def _walk_lay(m, lay):
    """The layout as the walk sees it: the room's own solids plus the spec's blocks (the parts of a bounding box outside an
    L-shaped room)."""
    blk = m["spec"].get("block", [])
    if not blk:
        return lay
    out = dict(lay)
    out["rects"] = lay["rects"] + [{"kind": "solid", "x0": b[0], "y0": b[1], "x1": b[2], "y1": b[3]} for b in blk]
    return out

def _starts(m, lay, n_doors):
    """Where the walk starts: the doors (or an open side), and the spec's extra starts (doors in an L-shaped room's inner
    walls)."""
    extra = m["spec"].get("starts", [])
    if extra and n_doors == 0:
        return list(extra)
    return _access(lay, m["spec"]["open_sides"], n_doors) + list(extra)

def _solve(m, minimize_d, walk_door = True, time_limit = 60.0, n_access = None, access_doors = None, rewalk = True):
    """The walk proxy (from the first door), the objective, then solve and re-walk, cutting a placement
    whose use points cannot all be reached, up to WALK_CUTS times."""
    wp = _walk_proxy(m, 0) if (walk_door and m["doors"]) else None
    m["p"].minimize(_objective(m, 0.02 * wp if wp != None else None, minimize_d))
    status, sol, lay, cuts, bad = None, None, None, 0, []
    nd = len(m["doors"]) if access_doors == None else access_doors
    for it in range(WALK_CUTS):
        sol = m["p"].solve(time_limit = time_limit, gap = 1e-4)
        if not sol.ok:
            status = sol.status
            lay = None
            break
        lay = _layout(m, sol)
        bad = walk_check(_walk_lay(m, lay), _starts(m, lay, nd if walk_door else 0), lay["points"]) if rewalk else []
        if not bad:
            status = sol.status
            break
        zs = []
        for opts in m["place"].values():
            for z in opts.values():
                if sol.value(z) > 0.5:
                    zs.append(z)
        m["p"].constrain(opt.sum(zs), max = len(zs) - 1, name = _nm(m, "walk", "walk cut %d" % it))
        cuts += 1
    return {"status": status, "sol": sol, "layout": lay if status == "optimal" else None, "cuts": cuts, "unreachable": bad if status != "optimal" else []}

# ---------------------------------------------------------------------------------------------- door classes and least depths

def _cls_doors(cls, kind, W):
    """The door classes: the doors, the free window zone and the open sides of a room of
    clear width W (in). `kind` is the room type."""
    if kind in ("kid", "den", "primary", "guest", "office"):
        op = OP_BED
        base = cls.split("+")[0]
        if base == "opp":
            d, win = _door("N", 3.0, op, True), ("S", 36.0)
        elif base == "adj":
            d, win = _door("W", ("hi", 3.0), op, True), ("S", 36.0)
        elif base == "core":
            d, win = _door("N", CORE, op, True), ("S", 36.0)
        elif base == "wopp":
            d, win = _door("W", 3.0, op, True), ("E", 36.0)
        elif base == "wadj":
            d, win = _door("S", 3.0, op, True), ("E", 36.0)
        else:
            _fail("door class %r" % cls)
        doors = [d]
        if cls.endswith("+garden"):
            doors.append(_door("S", ("free", 6.0, 6.0), 72.0, False))
        return doors, win, ()
    if kind in ("bath2", "bath1", "powder", "laundry", "pantry", "pbath", "closet"):
        op = OP_BATH
        swing = kind != "powder"  # a powder room this small takes a pocket door
        if cls == "wide":
            d = _door("S", 3.0, op, swing)
        elif cls == "side":
            d = _door("W", 3.0, op, swing)
        elif cls == "core":
            d = _door("S", CORE, op, swing)
        elif cls == "mid":
            d = _door("S", max(0.0, (W - op) / 2.0), op, swing)
        else:
            _fail("door class %r" % cls)
        return [d], None, ()
    if kind in ("living", "kitchen", "dining"):
        # "<facade>:<open sides>" -- front / rear (a 6-ft slider on the window wall) / mid (no window)
        fc, opn = cls.split(":")[0].split("+")[0], cls.split(":")[1]
        win = ("S", 36.0) if fc != "mid" else None
        doors = [_door("S", ("free", 6.0, 6.0), 72.0, False)] if fc == "rear" else []
        return doors, win, tuple([c for c in opn.elems()])
    if kind == "entry":
        # the front door on S, the garage's house door on E, the opening to the hall on N ("entry:a0" or "entry:a")
        hall = _door("N", 42.0, 40.0, False) if cls.endswith("a0") else _door("N", 0.0, 37.0, False)
        return [_door("S", ("free", 3.0, 3.0), OP_FRONT, True), _door("E", ("free", 3.0, 3.0), OP_HOUSE, True), hall], None, ()
    if kind == "garage":
        return [_door("W", ("free", 3.0, 3.0), OP_HOUSE, False)], None, ()
    _fail("room type %r" % kind)

# the door classes a fit table may name, by room type
DOOR_CLASSES = {
    "opp": "the door in the wall opposite the window, 3 in from a corner",
    "adj": "the door in a side wall, 3 in from the corner away from the window",
    "core": "the door opposite the window, 45 in from the corner (a side hall's core)",
    "wopp": "the window on a side wall, the door opposite it, 3 in from the corner",
    "wadj": "the window on a side wall, the door beside it, 3 in from the corner",
    "opp+garden": "opp, and a 6-ft slider anywhere on the window wall",
    "adj+garden": "adj, and a 6-ft slider anywhere on the window wall",
    "core+garden": "core, and a 6-ft slider anywhere on the window wall",
    "wide": "a bath, laundry or pantry door in the long wall, 3 in from a corner (a pocket door in a powder room)",
    "side": "the door in the short wall, 3 in from a corner",
    "mid": "the door in the middle of the long wall",
    "front:N": "a day room at the street: its window wall, open to the hall at the top (N)",
    "front:WN": "a day room at the street, open to the hall at the left and the top",
    "rear:N": "a day room at the garden: its window wall with a 6-ft slider, open at the top",
    "rear:WN": "a day room at the garden, open at the left and the top",
    "mid:NS": "a day room between two others, open at the top and the bottom, no window",
    "mid:WNS": "a day room between two others, open at the left, the top and the bottom",
    "entry:a0": "the entry: the front door, the garage's house door, the opening to the hall 42 in from the corner",
    "entry:a": "the entry: the front door, the garage's house door, the opening to the hall at the corner",
    "garage": "the garage: its house door anywhere on the left wall",
}

_SET_KIND = {"full": "kid", "twin": "kid", "bunk": "kid", "queen": "primary", "king": "primary", "queen_desk": "primary", "guest": "kid",
             "den": "den", "office": "den", "desk": "den", "tub": "bath2", "tub_irc": "bath2", "small_tub": "bath2", "small_tub_irc": "bath2", "shower": "bath1", "shower_60": "bath1", "shower_60s": "bath1",
             "pbath": "bath1", "pbath_tub": "bath1", "powder": "powder", "laundry": "laundry", "pantry": "pantry", "walkin": "closet", "living": "living",
             "dining": "dining", "dining4": "dining", "kitchen_L": "kitchen", "kitchen_island": "kitchen", "kitchen": "kitchen",
             "entry": "entry", "car": "garage", "car_storage": "garage", "car_free": "garage"}

def _fit_spec(set_name, W, door_class):
    kind = _SET_KIND.get(set_name)
    if kind == None:
        _fail("no door classes for set %r" % set_name)
    doors, win, opn = _cls_doors(door_class, kind, W)
    sp = _spec(set_name, W, doors, window = win, open_sides = opn, name = "%s|%s" % (set_name, door_class))
    return sp

def _least_rec(set_name, width_in, door_class, time_limit = 60.0, max_d = 480.0, name = None):
    sp = _fit_spec(set_name, width_in, door_class)
    m = _build(sp, width_in, D = (sp["min_d"], max_d), room = "%s|%s" % (set_name, door_class),
               fname = name if name != None else _fname("plan_furnish_fit", set_name, door_class, width_in))
    # a fit-table room is walked from its first door only
    r = _solve(m, True, walk_door = bool(sp["doors"]) and SETS[set_name]["kind"] not in ("living", "dining", "kitchen"), time_limit = time_limit, access_doors = 1)
    lay = r["layout"]
    return {"set": set_name, "door": door_class, "W": width_in, "status": r["status"], "cuts": r["cuts"],
            "D": lay["D"] if lay != None else None, "layout": lay}

def least_depth_record(set_name, width_in, door_class, *, time_limit = 60.0, name = None):
    """least_depth's whole record for a clear width in INCHES (the fit-table generator keeps its widths in
    inches): {"set", "door", "W", "status", "cuts", "D" (in) or None, "layout"}."""
    return _least_rec(set_name, width_in, door_class, time_limit = time_limit, name = name)

def least_depth(set_name, width, door_class, *, detail = False, time_limit = 60.0, name = None):
    """The least clear depth (ft) that holds `set_name` at clear width `width` (ft) with `door_class` (DOOR_CLASSES),
    every use point reachable; None when no depth up to 40 ft does. detail=True returns the whole record (the layout in
    the room's frame, inches). Its problem is the feature `name` (default plan_furnish_fit_<set>_<class>_<width in>)."""
    rec = _least_rec(set_name, width * IN, door_class, time_limit = time_limit, name = name)
    if detail:
        return rec
    return rec["D"] / IN if rec["D"] != None else None

def holds(set_name, width, depth, door_class, *, time_limit = 60.0, name = None, width_in = None, depth_in = None):
    """Does a room `width` x `depth` (clear, ft; or width_in= / depth_in= in inches) with `door_class` hold `set_name`,
    every use point reachable from its first door, the re-walk cutting up to WALK_CUTS placements? The stretch test's
    question: {"ok", "status", "cuts", "unreachable"}."""
    W = float(width_in) if width_in != None else width * IN
    D = float(depth_in) if depth_in != None else depth * IN
    sp = _fit_spec(set_name, W, door_class)
    m = _build(sp, W, Dfix = D, room = "%s|%s" % (set_name, door_class),
               fname = name if name != None else _fname("plan_furnish_holds", set_name, door_class, W, D))
    day = SETS[set_name]["kind"] in ("living", "dining", "kitchen")
    r = _solve(m, False, walk_door = bool(sp["doors"]) and not day, time_limit = time_limit, access_doors = 1)
    return {"ok": r["status"] == "optimal", "status": r["status"], "cuts": r["cuts"], "unreachable": r["unreachable"]}

def fit_table(set_name, door_class, widths, *, time_limit = 60.0):
    """A fit table: `{"set", "door", "rows": [(width ft, least depth ft or None)]}` over `widths` (ft), sorted by width."""
    rows = []
    for w in sorted(widths):
        rows.append((float(w), least_depth(set_name, w, door_class, time_limit = time_limit)))
    return {"set": set_name, "door": door_class, "rows": rows}

# ---------------------------------------------------------------------------------------------- fit_row: one row per room in a plan MILP

def _steps(table):
    """The table's step function as [(width, depth)], widths increasing, depths strictly decreasing: the running least
    depth over every width up to each (a wider room holds what a narrower one does), dropping rows that do not lower it
    and widths that hold nothing."""
    rows = sorted([r for r in table["rows"] if r[1] != None])
    out = []
    for (w, d) in rows:
        if not out or d < out[-1][1] - 1e-12:
            out.append((float(w), float(d)))
    return out

def fit_lookup(table, width):
    """The least depth (ft) a room `width` (ft) wide needs by the table: the depth at the widest table width not over
    `width`; None when `width` is narrower than every width that holds the set."""
    best = None
    for (w, d) in _steps(table):
        if w <= width + 1e-9:
            best = d
    return best

def fit_row(p, w, d, table, *, name = None, w_max = None):
    """Add to plan MILP `p` the rule "room depth d >= the table's least depth at room width w" (feet), and return the
    names of the rows it wrote.

    `w` is either the room's width as a variable or expression, or the plan's own choice of width as a list of
    (binary, width) pairs, exactly one of which is 1 (a one-hot width).

    One-hot width: ONE row, d >= sum_k z_k f(W_k), where f is the step function below. When z_k = 1 the room is W_k
    wide and the row reads d >= f(W_k); it is exact at every choice, and a width the table cannot hold forces its z_k
    to 0.

    A width variable: the table's step function f(w) = D_m for W_m <= w < W_(m+1) -- the least depth at the widest
    table width not over w, which holds because a wider room holds what a narrower one does -- with W_0 < ... < W_n
    and D_0 > ... > D_n (`_steps`: rows that do not lower the depth add nothing and are dropped). It is written with n
    binaries s_1 >= s_2 >= ... >= s_n ("w has passed W_k"):

        w >= W_0 + sum_k (W_k - W_(k-1)) s_k          (one row)
        d >= D_0 - sum_k (D_(k-1) - D_k) s_k          (one row)
        s_k <= s_(k-1)                                (n - 1 rows)

    Exactness: with the s monotone, s = (1, ..., 1, 0, ..., 0) with m ones, the first row reads w >= W_m and the second
    d >= D_m. For a given w the ones can run up to the largest m with W_m <= w and no further, and the loosest depth
    row among those choices is D_m = f(w). So the (w, d) the rows allow are exactly w >= W_0 and d >= f(w): the
    projection is the step function's epigraph, nothing more and nothing less. A room narrower than W_0 is refused (no
    table width holds the set). `w_max` (optional) is w's upper bound, to drop steps it can never reach.

    Rule names carry the family tag `fit`: `name` should be "fit:<floor>:<room>:depth"."""
    nm = name if name != None else "fit"
    steps = _steps(table)
    if not steps:
        _fail("fit_row %s: the table holds the set at no width" % nm)
    rows = []
    if type(w) == "list":
        terms = []
        for (z, wk) in w:
            f = fit_lookup(table, wk)
            if f == None:
                rows.append(p.constrain(z, max = 0, name = "%s: %s wide holds nothing" % (nm, wk)))
            else:
                terms.append(f * z)
        rows.append(p.constrain(d, min = opt.sum(terms) if terms else 0.0, name = nm))
        return rows
    if w_max != None:
        steps = [s for s in steps if s[0] <= w_max + 1e-9]
    W0, D0 = steps[0]
    if len(steps) == 1:
        rows.append(p.constrain(w, min = W0, name = nm + ": width"))
        rows.append(p.constrain(d, min = D0, name = nm))
        return rows
    s = [p.binary("%s: past %s" % (nm, steps[k][0])) for k in range(1, len(steps))]
    wterm = opt.dot([steps[k][0] - steps[k - 1][0] for k in range(1, len(steps))], s)
    dterm = opt.dot([steps[k - 1][1] - steps[k][1] for k in range(1, len(steps))], s)
    rows.append(p.constrain(w - wterm, min = W0, name = nm + ": width"))
    rows.append(p.constrain(d + dterm, min = D0, name = nm))
    for k in range(1, len(s)):
        rows.append(p.constrain(s[k] - s[k - 1], max = 0, name = "%s: step %d" % (nm, k + 1)))
    return rows

# ---------------------------------------------------------------------------------------------- furnish(): a plan room

def _local_doors(local, door_step, jamb):
    """The room stage's doors from room_local's (feet, offset = the opening's centre) on the main rect's walls. A door
    swinging in keeps its swing and `door_step` past it clear of solids; one swinging out, sliding or in a pocket keeps
    `door_step` inside it (a step to stand in); a cased opening ("open") keeps a 36-in landing."""
    out = []
    W, D = local["w"] * IN, local["d"] * IN
    for dr in local.get("doors", []):
        if dr.get("rect", 0) != 0:
            continue
        wl = dr["wall"]
        L = W if wl in ("S", "N") else D
        swing = dr.get("swing", "in") == "in"
        leaf = dr.get("width", 2.67) * IN
        op = leaf + jamb
        if dr.get("span") != None:
            lo, hi = dr["span"]
            off = ("free", max(0.0, lo * IN), max(0.0, L - hi * IN))
        else:
            off = dr["offset"] * IN - op / 2.0
            off = max(0.0, min(off, L - op))
        landing = 36.0 if dr.get("swing") == "open" else door_step
        out.append(_door(wl, off, op, swing, leaf = leaf if swing else 0.0, step = door_step, name = dr.get("key"), landing = landing))
    return out

def _local_windows(local):
    out = []
    for wn in local.get("windows", []):
        if wn.get("rect", 0) != 0:
            continue
        lo = (wn["offset"] - wn["width"] / 2.0) * IN
        hi = (wn["offset"] + wn["width"] / 2.0) * IN
        out.append((wn["wall"], lo, hi, wn.get("sill", 2.0) * IN))
    return out

def _stack_walls(local):
    """The walls of the main rect that hold a stack: each stack within 0.6 ft of the room goes to its nearest wall."""
    W, D = local["w"], local["d"]
    out = []
    for (u, v) in local.get("stacks", []):
        ds = [(abs(v), "S"), (abs(v - D), "N"), (abs(u), "W"), (abs(u - W), "E")]
        best = sorted(ds)[0][1]
        if best not in out:
            out.append(best)
    return tuple(out)

def _open_sides(local):
    """Main-rect sides that open into the room's other rects (an L-shaped room): no wall there to back a piece on."""
    out = []
    rs = local.get("rects", [])
    if len(rs) < 2:
        return ()
    m = rs[0]
    for r in rs[1:]:
        if abs(r[0] - m[1]) < 0.05 and min(r[3], m[3]) - max(r[2], m[2]) > 0.5:
            out.append("E")
        if abs(r[1] - m[0]) < 0.05 and min(r[3], m[3]) - max(r[2], m[2]) > 0.5:
            out.append("W")
        if abs(r[2] - m[3]) < 0.05 and min(r[1], m[1]) - max(r[0], m[0]) > 0.5:
            out.append("N")
        if abs(r[3] - m[2]) < 0.05 and min(r[1], m[1]) - max(r[0], m[0]) > 0.5:
            out.append("S")
    return tuple(out)

def _whole(local, door_step):
    """A room of several rects in the frame of its bounding box (room_local() gives rects, doors and
    windows relative to the main rect): returns (local', outside, starts, inner) -- the room re-framed on its bounding box
    with every door and window on the box's boundary as its own, the box's parts outside the room as rects (ft), walk
    starts at doors in the room's inner walls, and those doors' swings or steps as keep-outs (ft)."""
    rs = local.get("rects", [])
    if len(rs) < 2:
        return local, [], [], []
    bx0, bx1 = min([r[0] for r in rs]), max([r[1] for r in rs])
    by0, by1 = min([r[2] for r in rs]), max([r[3] for r in rs])
    W, D = bx1 - bx0, by1 - by0
    xs = sorted({x: True for x in [r[0] for r in rs] + [r[1] for r in rs]}.keys())
    ys = sorted({y: True for y in [r[2] for r in rs] + [r[3] for r in rs]}.keys())
    outside = []
    for j in range(len(ys) - 1):
        run = None
        for i in range(len(xs) - 1):
            cx, cy = 0.5 * (xs[i] + xs[i + 1]), 0.5 * (ys[j] + ys[j + 1])
            inside = any([r[0] <= cx and cx <= r[1] and r[2] <= cy and cy <= r[3] for r in rs])
            if not inside:
                run = (run[0], xs[i + 1]) if run != None else (xs[i], xs[i + 1])
            elif run != None:
                outside.append((run[0] - bx0, ys[j] - by0, run[1] - bx0, ys[j + 1] - by0))
                run = None
        if run != None:
            outside.append((run[0] - bx0, ys[j] - by0, run[1] - bx0, ys[j + 1] - by0))
    tol = 0.05
    doors, starts, inner = [], [], []
    for d in local.get("doors", []):
        pt = d.get("point")
        if pt == None:
            continue
        px, py = pt[0] - bx0, pt[1] - by0
        wl, w = d["wall"], d.get("width", 2.67)
        onb = (wl == "S" and abs(py) < tol) or (wl == "N" and abs(py - D) < tol) or (wl == "W" and abs(px) < tol) or (wl == "E" and abs(px - W) < tol)
        if onb:
            nd = dict(d)
            nd["offset"] = px if wl in ("S", "N") else py
            nd["rect"] = 0
            doors.append(nd)
            continue
        depth = (w + door_step / IN) if d.get("swing") == "in" else (3.0 if d.get("swing") == "open" else door_step / IN)
        inn = 1.0 / IN
        if wl == "S":
            inner.append((px - w / 2.0, py, px + w / 2.0, py + depth))
            starts.append((px, py + inn))
        elif wl == "N":
            inner.append((px - w / 2.0, py - depth, px + w / 2.0, py))
            starts.append((px, py - inn))
        elif wl == "W":
            inner.append((px, py - w / 2.0, px + depth, py + w / 2.0))
            starts.append((px + inn, py))
        else:
            inner.append((px - depth, py - w / 2.0, px, py + w / 2.0))
            starts.append((px - inn, py))
    wins = []
    for wn in local.get("windows", []):
        pt = wn.get("point")
        if pt == None:
            continue
        px, py = pt[0] - bx0, pt[1] - by0
        wl = wn["wall"]
        if (wl == "S" and abs(py) < tol) or (wl == "N" and abs(py - D) < tol) or (wl == "W" and abs(px) < tol) or (wl == "E" and abs(px - W) < tol):
            nw = dict(wn)
            nw["offset"] = px if wl in ("S", "N") else py
            nw["rect"] = 0
            wins.append(nw)
    ou, ov = local.get("origin", (0.0, 0.0))
    out = dict(local)
    out["origin"] = (ou + bx0, ov + by0)
    out["w"], out["d"] = W, D
    out["rects"] = [(0.0, W, 0.0, D)]
    out["doors"] = doors
    out["windows"] = wins
    out["stacks"] = [(u - bx0, v - by0) for (u, v) in local.get("stacks", [])]
    out["fixtures"] = [dict(f, rect = (f["rect"][0] - bx0, f["rect"][1] - bx0, f["rect"][2] - by0, f["rect"][3] - by0)) for f in local.get("fixtures", [])]
    return out, outside, starts, inner

def _fronts(wl):
    return {"S": "N", "N": "S", "W": "E", "E": "W"}.get(wl)

def _to_plan(local, x0, y0, x1, y1):
    ou, ov = local.get("origin", (0.0, 0.0))
    return (ou + x0 / IN, ou + x1 / IN, ov + y0 / IN, ov + y1 / IN)

def furnish(local, set_name = None, *, doors = None, open_sides = None, wet = "stack", strict = False, door_step = 18.0,
            jamb = 2.0, keep_fixtures = None, obstacles = (), without = (), rewalk = True, time_limit = 30.0, explain = True, name = None):
    """Pack `set_name` (a set of @cadpole/plan_furniture.star; default local["furnish"], else the room kind's) into the room
    `local` (room_local()'s shape, feet) and re-walk it at its final size. See the module docstring for the rules and
    what it returns. `doors` replaces the room's doors (room_local's door records; a door with "span": (lo, hi) may sit
    anywhere along that stretch of its wall); `open_sides` adds sides with no wall; `wet` is "stack", "fewest" or "any";
    `strict` keeps use spaces out of door swings too; `door_step` is how far past the leaf a swing is kept clear (in);
    `jamb` is how much wider the opening is than the leaf (in); `keep_fixtures` keeps the room's plan fixtures as
    obstacles (default: when the set itself places no fixtures); `obstacles` [{"rect": plan rect, "h": ft or None}] are
    more places no solid may stand (a built-in closet, its doors' space, a casement's swing), or none taller than `h`;
    `without` names items of the set to leave out (its closet, where the room has one built in); `rewalk=False` skips
    the re-walk (only to compare with a model that has none). The problem is the feature `name` (default
    plan_furnish_<room>_<set>_<width>x<depth in>, plus _strict when strict): pass name= to furnish one room twice
    with the same set and size in one document."""
    key = local.get("key", "room")
    if set_name == None:
        set_name = local.get("furnish") or SET_FOR_KIND.get(local.get("kind"))
    if set_name == None:
        return {"ok": True, "reason": "no furniture set for a %s" % local.get("kind"), "set": None, "furniture": [], "fixtures": [],
                "clears": [], "wet_walls": [], "cuts": 0, "status": "none", "layout": None}
    if set_name not in SETS:
        return {"ok": False, "reason": "no furniture set named %r in @cadpole/plan_furniture.star (have: %s)" % (set_name, ", ".join(sorted(SETS.keys()))),
                "set": set_name, "furniture": [], "fixtures": [], "clears": [], "wet_walls": [], "cuts": 0, "status": "no set", "layout": None}
    loc0 = dict(local)
    if doors != None:
        loc0["doors"] = doors
    loc, outside, starts, inner = _whole(loc0, door_step)
    W, D = loc["w"] * IN, loc["d"] * IN
    drs = _local_doors(loc, door_step, jamb)
    opn = tuple(list(open_sides or ()))
    wins = _local_windows(loc)
    sets_fixtures = any([PIECES[it["piece"]]["fixture"] for it in SETS[set_name]["items"]])
    keep = []
    if keep_fixtures if keep_fixtures != None else not sets_fixtures:
        for f in loc.get("fixtures", []):
            r = f["rect"]
            keep.append(("fixture %s" % f.get("key"), r[0] * IN, r[2] * IN, r[1] * IN, r[3] * IN))
    for k, b in enumerate(outside + inner):
        keep.append(("%s %d" % ("outside" if k < len(outside) else "inner door", k), b[0] * IN, b[1] * IN, b[2] * IN, b[3] * IN))
    ou, ov = loc.get("origin", (0.0, 0.0))
    for k, ob in enumerate(obstacles):
        r = ob["rect"]
        h = ob.get("h")
        keep.append(("obstacle %d" % k, (r[0] - ou) * IN, (r[2] - ov) * IN, (r[1] - ou) * IN, (r[3] - ov) * IN, -1.0 if h == None else h * IN))
    sw = _stack_walls(loc)
    sp = _spec(set_name, W, drs, window = None, open_sides = opn, keep = keep, windows = wins, wet = wet, stack_walls = sw, strict = strict,
               name = "%s %s" % (key, set_name), without = without, keep_clear = [("outside %d" % k, b[0] * IN, b[1] * IN, b[2] * IN, b[3] * IN) for k, b in enumerate(outside)],
               starts = [(x * IN, y * IN) for (x, y) in starts], block = [(b[0] * IN, b[1] * IN, b[2] * IN, b[3] * IN) for b in outside])
    fname = name if name != None else _fname("plan_furnish", key, set_name, "%dx%d" % (int(W), int(D)) + ("_strict" if strict else ""))
    m = _build(sp, W, Dfix = D, floor = local.get("floor") or "-", room = key, fname = fname)
    day = SETS[set_name]["kind"] in ("living", "dining", "kitchen")
    r = _solve(m, False, walk_door = bool(drs or starts) and not day, time_limit = time_limit, access_doors = len(drs), rewalk = rewalk)
    out = {"ok": False, "reason": "", "set": set_name, "furniture": [], "fixtures": [], "clears": [], "wet_walls": [], "cuts": r["cuts"],
           "status": r["status"], "layout": r["layout"], "W": W / IN, "D": D / IN, "origin": loc["origin"]}
    lay = r["layout"]
    dims = "%s x %s" % (_ftin(loc["w"]), _ftin(loc["d"])) + (" (the bounding box of its %d rects)" % len(local["rects"]) if outside else "")
    if lay == None:
        if r["status"] == "optimal" or r["unreachable"]:
            out["reason"] = "a use point stays unreachable after %d cuts: %s" % (r["cuts"], ", ".join(r["unreachable"]))
        elif r["status"] == "infeasible":
            out["reason"] = "%s does not fit %s" % (_label(set_name, without), dims)
            wet_items = [it for it in sp["items"] if it["wet"]]
            if wet == "stack" and sw and wet_items and explain:
                # is it the stack wall? the same room with the fewest wet walls instead
                r2 = furnish(local, set_name, doors = doors, open_sides = open_sides, wet = "fewest", strict = strict, door_step = door_step,
                             jamb = jamb, keep_fixtures = keep_fixtures, obstacles = obstacles, without = without, rewalk = rewalk,
                             time_limit = time_limit, explain = False, name = fname + "_wet")
                if r2["ok"]:
                    out["relaxed"] = r2
                    n = len(r2["wet_walls"])
                    out["reason"] = "%s fits %s, but not with every wet fixture on the stack wall (%s): it needs %d wet wall%s (%s)" % (
                        _label(set_name, without), dims, ", ".join(sw), n, "" if n == 1 else "s", ", ".join(r2["wet_walls"]))
                    return out
            if explain and not outside:
                need = _need_depth(sp, W, D, time_limit, fname + "_need")
                out["need_depth"] = need
                out["reason"] += (" with these doors and windows; at this width it needs %s deep" % _ftin(need)) if need != None else " at this width at any depth up to 40 ft"
        else:
            out["reason"] = "the room stage stopped: %s" % r["status"]
        return out
    out["ok"] = True
    pk = {it["name"]: it for it in sp["items"]}
    placed = lay["placed"]
    wet_walls = []
    for rc in lay["rects"]:
        it = pk[rc["item"]]
        wl = placed[rc["item"]][0]
        rect = _to_plan(loc, rc["x0"], rc["y0"], rc["x1"], rc["y1"])
        if rc["kind"] == "clear":
            out["clears"].append({"room": key, "item": rc["item"], "rect": rect})
            continue
        tag = _tag_of(sp, rc, placed)
        rec = {"key": "%s.%s.%s" % (key, rc["item"], tag), "room": key, "kind": tag, "item": rc["item"], "piece": it["piece"], "rect": rect,
               "front": _fronts(wl) if wl in WALLS else None, "h": it["h"] / IN}
        if it["fixture"] and tag not in ("nightstand", "landing"):
            out["fixtures"].append(rec)
        else:
            out["furniture"].append(rec)
        if it["wet"] and wl in WALLS and wl not in wet_walls:
            wet_walls.append(wl)
    out["wet_walls"] = wet_walls
    return out

def _label(set_name, without):
    """What was packed, in words: the set's label, or its items' pieces when some were left out."""
    if not without:
        return SETS[set_name]["label"]
    names = [PIECES[it["piece"]]["label"].split(",")[0] for it in SETS[set_name]["items"] if it["name"] not in without]
    return "; ".join(names) + " (the set %s without its %s)" % (set_name, ", ".join(without))

def _tag_of(sp, rc, placed):
    """The tag of a placed solid (bed, nightstand, wc, ...): its rect's tag in the shape the item was placed with."""
    it = [x for x in sp["items"] if x["name"] == rc["item"]][0]
    k = int(rc["name"].split(".r")[-1])
    sh = it["shapes"][placed[rc["item"]][1]]
    if k < len(sh["rects"]):
        return sh["rects"][k][5]
    return rc["item"]

def _need_depth(sp, W, D, time_limit, fname):
    """The least depth (ft) holding the set at this width with the room's own doors and windows, the north wall moved
    (the explanation of a room that is too small); None if nothing up to 40 ft does."""
    if D >= 480.0:
        return None
    m = _build(sp, W, D = (D, 480.0), room = "need", fname = fname)
    r = _solve(m, True, walk_door = bool(sp["doors"]), time_limit = time_limit)
    return r["layout"]["D"] / IN if r["layout"] != None else None

def _ftin(x):
    """Feet-inches to the nearest inch: 9.5 -> 9'-6"."""
    if x == None:
        return "-"
    n = int(math.round(x * IN))
    return "%d'-%d\"" % (n // 12, n % 12)
