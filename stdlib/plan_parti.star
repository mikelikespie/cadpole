"""@cadpole/plan_parti.star -- parti first: you pick the organizing idea, one MILP fits it on every floor.

    load("@cadpole/plan_parti.star", "fit", "compare", "partis_sheet", "author", "explain", "options", "precheck",
         "library", "resolve", "compile_template", "grid", "ftin", "character", "FAMILIES")
    load("@cadpole/plan_templates.star", "SITE", "PROGRAM", "side_hall")

    plan = fit(side_hall(), SITE, PROGRAM, name="plan_parti_side_hall")
    plan["meta"]["plan_parti"]["objective"]     # 1091.93: the proven optimum
    fits = [fit(t, SITE, PROGRAM, name="plan_parti_" + t["key"]) for t in library()]   # 5 to 60 s each
    partis_sheet(S, fits)                       # one sheet: every floor of each, its scorecard and its red pen

A template (`@cadpole/plan_templates.star`) is a column grid across the lot, a stair, an optional court, which rooms may
take which columns and open off which, the front-to-back orders it insists on and the patterns it turns on. `fit()`
compiles it with a site and a program into **one** mixed-integer program over every floor -- the floor depths, the
stair's place (one footprint on every floor), each room's span and depth, the doors, the plumbing stacks -- solves it
with the `opt` builder, and returns a plan value in the `@cadpole/plan` format (feet, u east, v north),
with its scorecard in `plan["meta"]["plan_parti"]`.

**The model** is a parti kernel that takes any number of floors, wells on either party wall, kept rooms, and
extra and forbidden doors:

* doors are a graph: every room has one *access* door from the rooms it may open off (which spans the house from the
  street and each floor's arrival landing), plus required doors (`must`: the garage into the house), weighted wishes
  (`wish`: the pantry beside the kitchen) and bans (`ban`). Parents need not be listed before their children: an
  out-of-order parent gets depth rows that keep the access doors a tree;
* columns may end short of the floor's rear wall instead of filling up with stores (`short`);
* `precheck()` tests, before any MILP, that every room can take a span, reach a door, daylight and a stack;
* kept rooms (`kept=True` or pinned in `fixed`) are exempt from the design rules;
* `explain()` runs a deletion filter by rule family (and then by floor) by rebuilding the problem without each in
  turn, and names the cheapest family to drop. Every rule is named `"<family>:<floor>:<room>:<what>"`;
* options come by template and knob (`@cadpole/plan_templates.star`), never by raw no-good cuts.

Options a fit takes (the kernel alone is the default): `fit_rows=True`
(@cadpole/plan_furnish.star's fit rows for each room's furniture, instead of the kernel's fixture depths),
`light="measured"` (@cadpole/plan_light.star's sky table for how deep a court lights, instead of an assumed 12 ft),
`rules=` (@cadpole/plan_rules.star's menu); every fit's scorecard carries the measured dark floor, and
`partis_sheet()` draws fits with plan_draw's compare_sheet and a red pen from @cadpole/plan_audit.star's audit.

Every rule is linear. Areas are exact: a room's width is one of its spans' widths (a constant), so its area is
`sum(W_span * p_span)` with `p_span = s_span * d` written as four exact rows of a binary and a bounded depth.

Loading this module records nothing; `fit()`, `author()` and `explain()` make `opt.problem()` features, named
`name=` (default `"plan_parti_<template key>"`); a document that fits one template twice passes two names.
"""

load("@cadpole/building.star", _mm = "ft")
load("@cadpole/plan.star", "normalize")
load("@cadpole/plan_audit.star", "audit")
load("@cadpole/plan_draw.star", "compare_sheet", "kind_fill")
load("@cadpole/plan_fit_tables.star", "FIT_TABLES")
load("@cadpole/plan_furnish.star", "fit_lookup", "fit_row")
load("@cadpole/plan_light.star", "lit_share", "sky_row")
load("@cadpole/plan_templates.star", "EXTRA_ROOMS", "PROGRAM", "PROGRAM_KEYS", "SITE", "copy_template", "tidy",
     _library = "library")
load("@cadpole/plan_rules.star", "check_rules", "rule_families", "standard")

# ------------------------------------------------------------------ constants

# Code and design constants of the kernel (feet). A fit's `options=` may override any of them.
CODE = {
    "head": 8.0,          # window head (9-ft ceilings under 10-ft floor to floor)
    "lit": 16.0,          # daylit depth from a facade: twice the head height (Title 24's secondary sidelit zone)
    "door": 2.67,         # a 32-in door and its jambs
    "win_well": 5.0,      # a window onto a light well: at least 5 ft of the room's wall faces the well
    "stair_w": 3.5,       # 36 in clear between walls
    "flight": 12.5,       # 16 risers of 7-1/2 in, 15 treads of 10 in (R311.7: at most 7-3/4 in, at least 10 in)
    "land": 3.0,          # R311.7.6: 36 in in the direction of travel
    "cant": 2.0,          # an upper floor may cantilever 2 ft past the floor below
    "deck_min": 8.0,      # the main floor's rear deck sits on the ground floor's roof, at least 8 ft deep
    "deck_val": 12.0,     # deck depth that earns value
    "deck_weight": 0.4,   # value per sq ft of deck
    "knee": 0.25,         # a room's area past its target is worth a quarter
    "parsimony": 0.15,    # each gross sq ft built costs this much (so empty depth is not built)
    "big_m": 64.0,        # big-M for lengths (feet)
    "hall_max": 5.5,      # a hall wider than this is a cross hall no deeper than this
    "min_floor": 24.0,    # the least inside depth of a floor
    "door_cost": 0.5,     # each door beyond a room's access door costs this (so wishes, not ties, add doors)
}

FLOOR_NAMES = {"G": "Ground", "M": "Main", "U": "Upper", "T": "Top"}

# Which of the room stage's fit tables a room reads with fit_rows=True (@cadpole/plan_fit_tables.star): by room key, then
# by plan kind: (furniture set, door classes). The kernel does not know where along a wall a door lands, so a room
# reads the worst of the door classes its doors could take (the deepest least depth at each width).
FIT_SETS = {
    "bathG": ("shower", ["side", "wide"]),
    "bath": ("tub", ["side", "wide"]),
    "powder": ("powder", ["side", "wide"]),
    "laundry": ("laundry", ["side", "wide"]),
    "pantry": ("pantry", ["side", "wide"]),
    "bed": ("full", ["opp", "adj"]),
    "den": ("den", ["opp+garden", "adj+garden"]),
    "garage": ("car", ["garage"]),
}

def fit_table(set_, classes):
    """One table from the room stage's fit tables for a furniture set over several door classes: at each width the
    deepest least depth, or None where any class cannot hold the set. {"set", "door", "rows": [(w, d or None)]}."""
    tabs = [FIT_TABLES["%s|%s" % (set_, c)] for c in classes if ("%s|%s" % (set_, c)) in FIT_TABLES]
    if not tabs:
        fail("plan_parti.fit_table: no fit table for %s with door classes %s" % (set_, classes))
    widths = sorted({x[0]: 1 for t in tabs for x in t["rows"]}.keys())
    rows = []
    for W in widths:
        ds = [fit_lookup(t, W) for t in tabs]
        rows.append((W, None if None in ds else max(ds)))
    return {"set": set_, "door": "/".join(classes), "rows": rows}

# Rule families: every rule's name starts with one. "def" rows define the model and are never dropped.
FAMILIES = ["tiling", "order", "facade", "fixtures", "shape", "program", "stair", "well", "access", "doors", "light",
            "stack", "floors", "cap", "garden", "pattern", "fixed", "site"]

ROOM_DEFAULTS = {
    "area": (0.0, 1e9), "target": None, "value": 0.0, "d": (3.0, 999.0), "wmin": 0.0, "mind": None, "lit": False,
    "wet": False, "parents": [], "front": False, "rear": False, "aspect": None, "open_to": [], "optional": False,
    "day": False, "through": False, "borrow": None, "garden": False, "label": None, "hall": False, "knee": 0.25,
    "kept": False, "role": None, "kind": "other", "name": None, "unit": "main", "core": None, "furnish": None,
}

def ftin(x):
    """Feet as feet-inches text, to the nearest inch: 7.42 -> 7'-5"."""
    n = int(abs(x) * 12.0 + 0.5)
    return "%s%d'-%d\"" % ("-" if x < 0 else "", n // 12, n % 12)

def sqft(a):
    return format(a, ",.0f") + " sq ft"

def grid(lines, width = 24.0, partition = 0.4):
    """The column grid: `lines` are partition centre lines (u) across an inside `width`. Column k runs between line
    k and line k+1, inside the partitions: `cols[k] = (u0, u1)`. `line_u[k]` is line k's u (0 and `width` are the
    party walls' inside faces). This is the module's pure probe."""
    P = partition
    edges = [-P / 2.0] + list(lines) + [width + P / 2.0]
    cols = [(edges[i] + P / 2.0, edges[i + 1] - P / 2.0) for i in range(len(edges) - 1)]
    return {"lines": list(lines), "cols": cols, "C": len(cols), "line_u": [0.0] + list(lines) + [width],
            "width": width, "P": P}

def _width(g, sp):
    return g["cols"][sp[1]][1] - g["cols"][sp[0]][0]

def _uv(g, sp):
    return g["cols"][sp[0]][0], g["cols"][sp[1]][1]

def _constants(site, options):
    K = dict(CODE)
    K.update(options or {})
    lot = site["lot"]
    sb = site.get("setbacks", {})
    K["WALL"] = site.get("wall", 0.5)
    K["P"] = site.get("partition", 0.4)
    K["LOT_W"] = lot[1] - lot[0]
    K["W"] = K["LOT_W"] - 2 * K["WALL"]
    gross_max = (lot[3] - lot[2]) - sb.get("front", 0.0) - sb.get("rear", 0.0)
    K["LMAX"] = gross_max - 2 * K["WALL"]
    K["FAR_CAP"] = site.get("far_cap", 1e9)
    K["X0"] = lot[0] + K["WALL"] + sb.get("west", 0.0)
    K["Y0"] = lot[2] + sb.get("front", 0.0) + K["WALL"]
    K["FTF"] = site.get("floor_to_floor", 10.0)
    K["CEIL"] = site.get("ceiling", 9.0)
    return K

# ------------------------------------------------------------------ resolving a template with a program

def library(keys = None, **kw):
    """The templates of @cadpole/plan_templates.star named by `keys` (all four 25-ft partis by default), each made with
    the parameters `kw` (mirror=, floors=, split=, court=, light_well=)."""
    return _library(keys, **kw)

def resolve(template, site = None, program = None):
    """The template with the program's sizes merged into its placements: every room a full record (ROOM_DEFAULTS
    filled), program rooms the program does not ask for dropped (their parents and orders tidied), and
    `t["unplaced"]` the program rooms the template has no place for. Pure data; nothing is solved."""
    t = copy_template(template)
    t["site"] = site or SITE
    prog = dict(PROGRAM if program == None else program)
    for k in template.get("program_add", []):
        if k not in prog:
            prog[k] = EXTRA_ROOMS[k]
    rooms = []
    placed = {}
    for r in t["rooms"]:
        if r.get("program"):
            if r["key"] not in prog:
                continue
            merged = dict(ROOM_DEFAULTS)
            merged.update(prog[r["key"]])
            for k, v in r.items():
                if k not in ("program", "if_program"):
                    merged[k] = v
            merged["program"] = True
        else:
            merged = dict(ROOM_DEFAULTS)
            merged.update(r)
            merged["program"] = False
        if merged["name"] == None:
            merged["name"] = merged["key"]
        merged["spans"] = [tuple(s) for s in merged["spans"]]
        merged["parents"] = list(merged["parents"])
        merged["area"] = tuple(merged["area"])
        merged["d"] = tuple(merged["d"])
        if merged["mind"] != None:
            merged["mind"] = tuple(merged["mind"])
        rooms.append(merged)
        placed[merged["key"]] = True
    t["rooms"] = rooms
    tidy(t, strict = False)
    t["unplaced"] = [] if template.get("partial") else [k for k in prog if k not in placed]
    t["program_keys"] = [k for k in PROGRAM_KEYS if k in placed]
    return t

# ------------------------------------------------------------------ the model

def _b(m, name):
    m["n"]["binaries"] += 1
    m["n"]["vars"] += 1
    return m["p"].binary(name)

def _v(m, name, min = None, max = None):
    m["n"]["vars"] += 1
    return m["p"].var(name, min = min, max = max)

def _c(m, expr, min = None, max = None, equals = None, name = None, tags = None):
    m["n"]["rows"] += 1
    if name == None:
        return m["p"].constrain(expr, min = min, max = max, equals = equals)
    if tags != None:
        return m["p"].constrain(expr, min = min, max = max, equals = equals, name = name, tags = tags)
    return m["p"].constrain(expr, min = min, max = max, equals = equals, name = name)

def _rule(m, fam, floor, room, what, expr, min = None, max = None, equals = None):
    """A named rule: "<family>:<floor>:<room>:<what>". Rules of a family (or a "family:floor") in m["drop"] are left
    out, which is how explain()'s deletion filter tests a family."""
    if fam in m["drop"] or (fam + ":" + floor) in m["drop"]:
        return None
    name = "%s:%s:%s:%s" % (fam, floor, room, what)
    # the model's own definitions and its symmetry rows carry no tags: no explanation drops them
    _c(m, expr, min = min, max = max, equals = equals, name = name, tags = {} if fam in ("def", "sym") else None)
    m["rows"].setdefault(fam, []).append(name)
    return name

def _exempt(m, r):
    """Kept rooms are exempt from design rules."""
    return r.get("kept") or r["key"] in m["t"].get("fixed", {}) and not r.get("site")

def _cols_of(m, k):
    out = {}
    for sp in m["V"][k]["spans"]:
        for c in range(sp[0], sp[1] + 1):
            out[c] = True
    return out

def _u(m, k, c):
    """Occupancy of column c by room k: an expression, 0 or 1."""
    return opt.sum([sv for sp, sv in m["V"][k]["s"].items() if sp[0] <= c and c <= sp[1]])

def _e(m, k):
    return m["V"][k]["e"]

def _need_d(r, w):
    a, b = sorted(r["mind"])
    if w >= b - 1e-9:
        return a
    if w >= a - 1e-9:
        return b
    return None

def _room_spans(m, r):
    g = m["g"]
    out = []
    for sp in r["spans"]:
        w = _width(g, sp)
        if w < r["wmin"] - 1e-9:
            continue
        if [k for k in range(sp[0] + 1, sp[1] + 1) if k in m["bearing"]]:
            continue
        if r["mind"] != None and _need_d(r, w) == None:
            continue
        out.append(sp)
    return out

def _room(m, r):
    p, g, K = m["p"], m["g"], m["K"]
    M = K["big_m"]
    f, k = r["floor"], r["key"]
    spans = _room_spans(m, r)
    if not spans:
        fail("plan_parti: %s: no span fits (spans %s)" % (k, r["spans"]))
    dlo, dhi = r["d"][0], min(r["d"][1], K["LMAX"])
    optional = r["optional"]
    e = _b(m, k + ".on") if optional else 1.0
    d = _v(m, k + ".d", min = 0.0 if optional else dlo, max = dhi)
    y = _v(m, k + ".y", min = 0.0, max = K["LMAX"])
    if optional:
        _rule(m, "def", f, k, "min depth", d - dlo * e, min = 0)
        _rule(m, "def", f, k, "absent: no depth", d - dhi * e, max = 0)
        m["arr_bins"].append(e)
    s = {}
    if len(spans) == 1 and not optional:
        s[spans[0]] = 1.0
    else:
        for sp in spans:
            s[sp] = _b(m, "%s.span%d-%d" % (k, sp[0], sp[1]))
            m["arr_bins"].append(s[sp])
        _rule(m, "def", f, k, "one span", opt.sum(s.values()) - e, equals = 0)
    pr = {}
    for sp in spans:
        sv = s[sp]
        if type(sv) == "float":
            pr[sp] = d
        else:  # q = s * d exactly (s binary, 0 <= d <= dhi)
            q = _v(m, "%s.p%d-%d" % (k, sp[0], sp[1]), min = 0.0, max = dhi)
            _c(m, q - dhi * sv, max = 0)
            _c(m, q - d, max = 0)
            _c(m, q - d - dhi * sv, min = -dhi)
            _c(m, q - dlo * sv, min = 0)
            pr[sp] = q
    area = opt.sum([_width(g, sp) * pr[sp] for sp in spans])
    bay = m["t"].get("bay")
    if bay and bay["host"].get(f) == k:
        area = area + _bay_area(bay)
    wexpr = opt.sum([_width(g, sp) * s[sp] for sp in spans])
    WL = opt.sum([sp[0] * s[sp] for sp in spans])
    EL = opt.sum([(sp[1] + 1) * s[sp] for sp in spans])
    m["V"][k] = {"y": y, "d": d, "e": e, "s": s, "pr": pr, "area": area, "w": wexpr, "WL": WL, "EL": EL, "spans": spans}
    _in_floor(m, r)
    exempt = _exempt(m, r)
    if r["front"] and not exempt:
        _rule(m, "facade", f, k, "on the street facade", y, max = 0)
    if r["rear"] and not exempt:
        _rear_rows(m, r)
    fs = _fit_set(m, r)
    if fs != None and not exempt:
        # the fit row: at least the least depth that holds the room's furniture at its width (one row; a
        # width the table cannot vouch for is ruled out)
        tab = fit_table(fs[0], fs[1])
        names = fit_row(m["p"], [(s[sp], _width(g, sp)) for sp in spans], d, tab, name = "fit:%s:%s:depth" % (f, k))
        m["n"]["rows"] += len(names)
        m["rows"].setdefault("fit", []).extend(names)
        m["fit_used"][k] = {"set": fs[0], "door": fs[1], "dropped": [sp for sp in spans if fit_lookup(tab, _width(g, sp)) == None]}
    elif r["mind"] != None and not exempt:
        _rule(m, "fixtures", f, k, "fixture depth (R307)",
              d - opt.sum([_need_d(r, _width(g, sp)) * s[sp] for sp in spans]), min = 0)
    if r["role"] == "circ" and not r["through"] and k != "vestibule" and not exempt:
        # a hall is narrow one way: a span wider than hall_max makes it at most hall_max deep (a cross hall)
        for sp in spans:
            if _width(g, sp) > K["hall_max"] + 1e-9:
                _rule(m, "shape", f, k, "a hall (%d-%d)" % sp, d - M * (-s[sp] + 1), max = K["hall_max"])
    if r["aspect"] and not exempt:
        _rule(m, "shape", f, k, "proportion (d)", d - r["aspect"] * wexpr, max = 0)
        _rule(m, "shape", f, k, "proportion (w)", wexpr - r["aspect"] * d, max = 0)
    lo, hi = r["area"]
    if not exempt:
        if lo > 0:
            _rule(m, "program", f, k, "min area", area - lo * e, min = 0)
        if hi < 1e8:
            _rule(m, "program", f, k, "max area", area, max = hi)
    if r["value"] > 0:
        if r["target"]:
            a1 = _v(m, k + ".a1", min = 0.0, max = r["target"])
            a2 = _v(m, k + ".a2", min = 0.0)
            _c(m, a1 + a2 - area, equals = 0)
            m["kernel_terms"].append(r["value"] * a1 + r["knee"] * r["value"] * a2)
        else:
            m["kernel_terms"].append(r["value"] * area)

def _fit_set(m, r):
    fr = m["fit_rows"]
    if not fr:
        return None
    table = FIT_SETS if fr == True else fr
    return table.get(r["key"]) or table.get(r["kind"])

def _well_lit(m, w, f):
    """How deep a well or court lights the rooms beside it on floor f: the template's assumed depth, or with
    light="measured" @cadpole/plan_light.star's SKY_TABLE (view_cone's sky, by court width, wall height and floor)."""
    if m["light"] != "measured":
        return w.get("lit_depth", 12.0)
    FL = m["FL"]
    base = w["floors"][0]
    width = m["g"]["cols"][w["col"]][1] - m["g"]["cols"][w["col"]][0]
    h = (FL.index(w["floors"][-1]) + 1 - FL.index(base)) * m["K"]["FTF"]
    r = sky_row(width, h, FL.index(f) - FL.index(base))
    return r["lit"] if r != None else 0.0

def _depth_of(m, f, c):
    """The inside depth column c reaches on floor f: the floor's depth, or the column's own where it may end short."""
    return m["Lc"].get((f, c), m["L"][f])

def _in_floor(m, r):
    """The room ends inside its floor; in a column that may end short, inside that column's depth too (with the
    column's tiling this leaves no hole in it)."""
    f, k = r["floor"], r["key"]
    v = m["V"][k]
    M = m["K"]["big_m"]
    _rule(m, "def", f, k, "in floor", v["y"] + v["d"] - m["L"][f], max = 0)
    for sp in v["spans"]:
        for c in range(sp[0], sp[1] + 1):
            if (f, c) in m["Lc"]:
                _rule(m, "def", f, k, "in column %d (%d-%d)" % (c, sp[0], sp[1]),
                      v["y"] + v["d"] - m["Lc"][(f, c)] - M * (-v["s"][sp] + 1), max = 0)

def _rear_rows(m, r):
    """The room reaches the garden facade: its far end is the floor's rear wall (or, in a column that may end short,
    that column's end)."""
    f, k = r["floor"], r["key"]
    v = m["V"][k]
    M = m["K"]["big_m"]
    shorts = [c for c in _cols_of(m, k) if (f, c) in m["Lc"]]
    if not shorts:
        _rule(m, "facade", f, k, "on the garden facade", v["y"] + v["d"] - m["L"][f] + M * (-v["e"] + 1), min = 0)
        return
    for sp in v["spans"]:
        for c in range(sp[0], sp[1] + 1):
            _rule(m, "facade", f, k, "on the garden facade (c%d, %d-%d)" % (c, sp[0], sp[1]),
                  v["y"] + v["d"] - _depth_of(m, f, c) + M * (-v["s"][sp] + 1), min = 0)

def _fixed(m):
    for k, fx in m["t"].get("fixed", {}).items():
        if k not in m["V"]:
            continue
        v = m["V"][k]
        r = m["R"][k]
        fam = "site" if r.get("site") else "fixed"
        sp = tuple(fx["span"])
        if sp not in v["s"]:
            fail("plan_parti: %s: pinned span %s is not one of its spans %s" % (k, sp, v["spans"]))
        sv = v["s"][sp]
        if type(sv) != "float":
            _rule(m, fam, r["floor"], k, "existing span", sv, equals = 1)
        if "y" in fx:
            _rule(m, fam, r["floor"], k, "existing y", v["y"], equals = fx["y"])
        if "d" in fx:
            _rule(m, fam, r["floor"], k, "existing d", v["d"], equals = fx["d"])

def _tiling(m, f):
    """Every column is tiled front to back on every floor by the rooms that take it, a partition apart."""
    P = m["K"]["P"]
    for c in range(m["g"]["C"]):
        terms = []
        for r in m["F"][f]:
            v = m["V"][r["key"]]
            for sp in v["spans"]:
                if sp[0] <= c and c <= sp[1]:
                    terms.append(v["pr"][sp] + P * v["s"][sp])
        if not terms:
            fail("plan_parti: floor %s column %d: no room may occupy it" % (f, c))
        _rule(m, "tiling", f, "c%d" % c, "tiled", opt.sum(terms) - _depth_of(m, f, c), equals = P)

def _pairs(m, f):
    """Two rooms that share a column lie one in front of the other (a partition apart)."""
    p, K = m["p"], m["K"]
    M, P = K["big_m"], K["P"]
    rooms = m["F"][f]
    fixed = {}
    for o in m["t"]["orders"]:
        if o[0] == f:
            fixed[(o[1], o[2])] = True
    for i in range(len(rooms)):
        ri = rooms[i]
        ci = _cols_of(m, ri["key"])
        vi = m["V"][ri["key"]]
        for j in range(i + 1, len(rooms)):
            rj = rooms[j]
            cj = _cols_of(m, rj["key"])
            common = sorted([c for c in ci if c in cj])
            if not common:
                continue
            if (ri["key"], rj["key"]) in fixed or (rj["key"], ri["key"]) in fixed:
                continue  # the template fixed their order: a rule in _orders
            vj = m["V"][rj["key"]]
            must = (not ri["optional"] and not rj["optional"] and
                    all([max(a1, a2) <= min(b1, b2) for (a1, b1) in vi["spans"] for (a2, b2) in vj["spans"]]))
            if must:
                sh = 1.0
            else:
                sh = _b(m, "sh|%s|%s" % (ri["key"], rj["key"]))
                for c in common:
                    _c(m, sh - _u(m, ri["key"], c) - _u(m, rj["key"], c), min = -1)
            o = _b(m, "ord|%s<%s" % (ri["key"], rj["key"]))
            m["arr_bins"].append(o)
            # o = 1: i in front of j; o = 0: j in front of i (both only when they share a column)
            _c(m, vi["y"] + vi["d"] - vj["y"] - M * (-o + 1) - M * (-sh + 1), max = -P)
            _c(m, vj["y"] + vj["d"] - vi["y"] - M * o - M * (-sh + 1), max = -P)
            m["sh"][(ri["key"], rj["key"])] = (sh, o)

def _symmetry(m):
    """Interchangeable rooms (the same floor, role, spans, parents and sizes: two closets) would let the solver find
    each plan twice; order them front to back, and the first is the one that exists if only one does."""
    M = m["K"]["big_m"]
    groups = {}
    for r in m["t"]["rooms"]:
        sig = (r["floor"], r["role"], tuple(r["spans"]), tuple(r["parents"]), r["area"], r["d"], r["mind"],
               r["optional"], r["value"], r["target"])
        groups.setdefault(sig, []).append(r)
    for rs in groups.values():
        for i in range(len(rs) - 1):
            a, b = rs[i], rs[i + 1]
            va, vb = m["V"][a["key"]], m["V"][b["key"]]
            if a["optional"]:
                _rule(m, "sym", a["floor"], a["key"], "on before " + b["key"], va["e"] - vb["e"], min = 0)
                _rule(m, "sym", a["floor"], a["key"], "before " + b["key"], va["y"] - vb["y"] - M * (-vb["e"] + 1), max = 0)
            else:
                _rule(m, "sym", a["floor"], a["key"], "before " + b["key"], va["y"] - vb["y"], max = 0)
            m["sym_pairs"].append((a["key"], b["key"]))

def _orders(m):
    K = m["K"]
    M, P = K["big_m"], K["P"]
    for o in m["t"]["orders"]:
        f, a, b = o[0], o[1], o[2]
        va, vb = m["V"][a], m["V"][b]
        ex = va["y"] + va["d"] - vb["y"]
        if m["R"][a]["optional"]:
            ex = ex - M * (-va["e"] + 1)
        if m["R"][b]["optional"]:
            ex = ex - M * (-vb["e"] + 1)
        _rule(m, "order", f, a, "in front of " + b, ex, max = -P)

def _stair(m):
    """The stair's rectangles share one v on every floor (aligned, R311.7). Side stair: flights along v in one column,
    rising toward the rear and stacked; a middle floor holds the arrival landing (rear end) and the next flight's
    bottom landing (front end). Cross stair: flights along u across the house, landings at the two ends."""
    st, K, Ys = m["t"]["stair"], m["K"], m["Ys"]
    LAND, FLIGHT, STAIR_W = K["land"], K["flight"], K["stair_w"]
    FL = m["FL"]
    for f in FL:
        m["landings"][f] = {}
    if st == None:
        return  # a one-storey template (a test toy) has no stair
    for i, f in enumerate(FL):
        k = "stair" + f
        v = m["V"][k]
        first, last = i == 0, i == len(FL) - 1
        Lg = {}
        if st["type"] == "side":
            if first:
                off, ln = 0.0, LAND + FLIGHT
            elif last:
                off, ln = LAND, FLIGHT + LAND
            else:
                off, ln = 0.0, 2 * LAND + FLIGHT
            _rule(m, "stair", f, k, "aligned (R311.7)", v["y"] - Ys, equals = off)
            _rule(m, "stair", f, k, "length (R311.7)", v["d"], equals = ln)
            c = st["col"]
            if not last:
                Lg["bot"] = {"col": c, "v0": Ys + 0.0, "v1": Ys + LAND, "S": True, "N": False}
            if not first:
                Lg["top"] = {"col": c, "v0": Ys + LAND + FLIGHT, "v1": Ys + 2 * LAND + FLIGHT, "S": False, "N": True}
        else:
            _rule(m, "stair", f, k, "aligned (R311.7)", v["y"] - Ys, equals = 0.0)
            _rule(m, "stair", f, k, "width (R311.7)", v["d"], equals = STAIR_W)
            if not last:
                Lg["bot"] = {"col": st["land_w"], "v0": Ys + 0.0, "v1": Ys + STAIR_W, "S": True, "N": True}
            if not first:
                Lg["top"] = {"col": st["land_e"], "v0": Ys + 0.0, "v1": Ys + STAIR_W, "S": True, "N": True}
        m["landings"][f] = Lg

def _wells(m):
    """A light well or court: the same rectangle on each floor it is on (a rear notch reaches each floor's rear)."""
    for w in m["t"]["wells"]:
        fl = w["floors"]
        pre = w["pre"]
        v0 = m["V"][pre + fl[0]]
        for f in fl[1:]:
            vf = m["V"][pre + f]
            _rule(m, "well", f, pre, "stacked", vf["y"] - v0["y"], equals = 0)
            if not w.get("rear"):
                _rule(m, "well", f, pre, "same length", vf["d"] - v0["d"], equals = 0)

# ------------------------------------------------------------------ doors: a tree from the street and each landing, and more

def _adj_rows(m, rk, pk, kind, a, name, overlap = None):
    """Room rk touches room pk with `overlap` of contact; kind = where rk lies: S (in front of pk), N (behind),
    E (east of), W (west of). The rows hold when binary a = 1."""
    p, K = m["p"], m["K"]
    M, P = K["big_m"], K["P"]
    if overlap == None:
        overlap = K["door"]
    vr, vp = m["V"][rk], m["V"][pk]
    if m["R"][pk]["optional"]:
        _c(m, a - vp["e"], max = 0)
    if kind in ("S", "N"):
        cp = _cols_of(m, pk)
        common = sorted([c for c in _cols_of(m, rk) if c in cp])
        if not common:
            return False
        ws = []
        for c in common:
            wv = _v(m, "w|%s|%s|%d" % (name, kind, c), min = 0.0, max = 1.0)
            _c(m, wv - _u(m, rk, c), max = 0)
            _c(m, wv - _u(m, pk, c), max = 0)
            ws.append(wv)
        _c(m, opt.sum(ws) - a, min = 0)
        gap = (vp["y"] - vr["y"] - vr["d"]) if kind == "S" else (vr["y"] - vp["y"] - vp["d"])
        _c(m, gap + M * (-a + 1), min = P)
        _c(m, gap - M * (-a + 1), max = P)
        return True
    KK = m["g"]["C"] + 1
    if kind == "E":
        ok = [1 for sr in vr["spans"] for sp in vp["spans"] if sr[0] == sp[1] + 1]
        dl = vr["WL"] - vp["EL"]
    else:
        ok = [1 for sr in vr["spans"] for sp in vp["spans"] if sr[1] + 1 == sp[0]]
        dl = vr["EL"] - vp["WL"]
    if not ok:
        return False
    _c(m, dl - KK * (-a + 1), max = 0)
    _c(m, dl + KK * (-a + 1), min = 0)
    _c(m, vr["y"] + vr["d"] - vp["y"] + M * (-a + 1), min = overlap)
    _c(m, vp["y"] + vp["d"] - vr["y"] + M * (-a + 1), min = overlap)
    return True

def _landing_rows(m, rk, f, which, kind, a):
    p, K = m["p"], m["K"]
    M, P = K["big_m"], K["P"]
    Lg = m["landings"][f].get(which)
    if Lg == None:
        return False
    vr = m["V"][rk]
    c = Lg["col"]
    if kind in ("S", "N"):
        if not Lg[kind] or c not in _cols_of(m, rk):
            return False
        _c(m, _u(m, rk, c) - a, min = 0)
        gap = (Lg["v0"] - vr["y"] - vr["d"]) if kind == "S" else (vr["y"] - Lg["v1"])
        _c(m, gap + M * (-a + 1), min = P)
        _c(m, gap - M * (-a + 1), max = P)
        return True
    KK = m["g"]["C"] + 1
    if kind == "E":
        if not [1 for sr in vr["spans"] if sr[0] == c + 1]:
            return False
        dl = vr["WL"] - (c + 1)
    else:
        if not [1 for sr in vr["spans"] if sr[1] + 1 == c]:
            return False
        dl = vr["EL"] - c
    _c(m, dl - KK * (-a + 1), max = 0)
    _c(m, dl + KK * (-a + 1), min = 0)
    _c(m, vr["y"] + vr["d"] - Lg["v0"] + M * (-a + 1), min = K["door"])
    _c(m, Lg["v1"] - vr["y"] + M * (-a + 1), min = K["door"])
    return True

def _banned(m, a, b):
    for x in m["t"]["doors"].get("ban", []):
        if (x[0] == a and x[1] == b) or (x[0] == b and x[1] == a):
            return True
    return False

def _doors(m):
    p = m["p"]
    for r in m["t"]["rooms"]:
        if r["role"] in ("stair", "void"):
            continue
        k, f = r["key"], r["floor"]
        floor_keys = [q["key"] for q in m["F"][f]]
        opts = []
        for pk in r["parents"]:
            if pk == "stair.bot":
                fail("plan_parti: %s: the bottom landing is a departure, not a door parent" % k)
            if pk not in ("ROOT", "stair.top") and pk not in floor_keys:
                fail("plan_parti: %s: parent %s is not on floor %s" % (k, pk, f))
            if pk == "ROOT":
                opts.append(("ROOT", "street", None))
                continue
            for kind in ("S", "N", "E", "W"):
                a = _b(m, "door|%s|%s|%s" % (k, pk, kind))
                if _banned(m, k, pk):
                    _c(m, a, max = 0)
                    continue
                if pk in ("stair.bot", "stair.top"):
                    ok = _landing_rows(m, k, f, pk.split(".")[1], kind, a)
                else:
                    ok = _adj_rows(m, k, pk, kind, a, "%s<%s" % (k, pk))
                if ok:
                    opts.append((pk, kind, a))
                    m["arr_bins"].append(a)
                else:
                    _c(m, a, max = 0)
        if not opts:
            fail("plan_parti: %s: no door is possible" % k)
        m["door_opts"][k] = opts
        if [o for o in opts if o[0] == "ROOT"]:
            continue
        _rule(m, "access", f, k, "has a door", opt.sum([o[2] for o in opts]) - _e(m, k), equals = 0)
    _door_depths(m)
    # the next flight up starts from a hall (or an open room) on every floor but the top
    for f in (m["FL"][:-1] if m["t"]["stair"] != None else []):
        opts = []
        for r in m["F"][f]:
            if not (r["role"] == "circ" or r["through"]):
                continue
            for kind in ("S", "N", "E", "W"):
                a = _b(m, "depart|%s|%s" % (r["key"], kind))
                if _landing_rows(m, r["key"], f, "bot", kind, a):
                    if r["optional"]:
                        _c(m, a - _e(m, r["key"]), max = 0)
                    opts.append((r["key"], kind, a))
                else:
                    _c(m, a, max = 0)
        _rule(m, "access", f, "stair" + f, "the flight up starts from a hall", opt.sum([o[2] for o in opts]), min = 1)
        m["touch_opts"].append((f, opts))

def _door_depths(m):
    """Access doors form a tree hung from the street and each landing. A template that lists every room's parents
    before it needs nothing more; a parent listed after its child gets depth rows (MTZ) on that floor, so no cycle of
    access doors can close."""
    p = m["p"]
    for f in m["FL"]:
        rooms = m["F"][f]
        pos = {}
        for i, r in enumerate(rooms):
            pos[r["key"]] = i
        back = [(r["key"], pk) for r in rooms for pk in r["parents"] if pk in pos and pos[pk] >= pos[r["key"]]]
        if not back:
            continue
        N = len(rooms) + 1
        depth = {}
        for r in rooms:
            if r["role"] in ("stair", "void"):
                continue
            depth[r["key"]] = _v(m, "depth|%s" % r["key"], min = 0.0, max = N)
        for r in rooms:
            k = r["key"]
            if k not in m["door_opts"]:
                continue
            for o in m["door_opts"][k]:
                pk, a = o[0], o[2]
                if a == None or pk not in depth:
                    continue
                # a = 1: k hangs off pk, one step deeper
                _rule(m, "def", f, k, "deeper than %s (%s)" % (pk, o[1]), depth[k] - depth[pk] + N * (-a + 1), min = 1)

def _extra_doors(m):
    """Required doors outside the access tree (`must`: (room, [others])): the room also opens into one of the
    others -- the garage into the house."""
    p = m["p"]
    for x in m["t"]["doors"].get("must", []):
        rk, pks = x[0], x[1]
        if type(pks) == "string":
            pks = [pks]
        if rk not in m["V"]:
            continue
        opts = []
        seen = {}
        for pk in pks:
            if pk not in m["V"] or pk in seen or m["R"][pk]["floor"] != m["R"][rk]["floor"]:
                continue
            seen[pk] = True
            for kind in ("S", "N", "E", "W"):
                a = _b(m, "xdoor|%s|%s|%s" % (rk, pk, kind))
                if _adj_rows(m, rk, pk, kind, a, "x%s<%s" % (rk, pk)):
                    opts.append((pk, kind, a))
                else:
                    _c(m, a, max = 0)
        if not opts:
            fail("plan_parti: the extra door %s - %s is impossible" % (rk, pks))
        _rule(m, "doors", m["R"][rk]["floor"], rk, "also opens into " + "/".join(pks),
              opt.sum([o[2] for o in opts]) - _e(m, rk), min = 0)
        m["extra_opts"].append((rk, opts))

def _door_wishes(m):
    """Weighted door wishes ((a, b, weight)): a binary that may be 1 only if a door joins a and b -- an access door
    either way, or an extra door made for the wish -- and earns `weight`. A wish of weight "hard" is a must."""
    p = m["p"]
    for x in m["t"]["doors"].get("wish", []):
        a, b, w = x[0], x[1], x[2]
        if a not in m["V"] or b not in m["V"] or m["R"][a]["floor"] != m["R"][b]["floor"]:
            continue
        have = []
        for o in m["door_opts"].get(a, []):
            if o[0] == b:
                have.append(o[2])
        for o in m["door_opts"].get(b, []):
            if o[0] == a:
                have.append(o[2])
        for kind in ("S", "N", "E", "W"):
            z = _b(m, "wdoor|%s|%s|%s" % (a, b, kind))
            if _adj_rows(m, a, b, kind, z, "w%s<%s" % (a, b)):
                have.append(z)
                m["wish_doors"].append((a, b, kind, z))
                m["door_cost_terms"].append(z)
            else:
                _c(m, z, max = 0)
        x_ = _b(m, "wish|%s|%s" % (a, b))
        _c(m, opt.sum(have) - x_, min = 0)
        if w == "hard":
            _rule(m, "doors", m["R"][a]["floor"], a, "a door to " + b, x_ - _e(m, a), min = 0)
        else:
            m["wish_terms"].append(("door %s - %s" % (a, b), w, x_))

# ------------------------------------------------------------------ daylight and egress

def _light(m):
    """Each room that must be daylit (2H rule; a sleeping room's escape window, R310) or that earns light on two
    sides. touch|* binaries: the room touches a source (front glass, rear glass, a face of a well with 5 ft of wall);
    lit|* binaries: the source lights it (no deeper than 2H from the glass; from a well: within its lit depth, and no
    wider than its face plus half a head height each side). Lit: sum lit >= 1. Two-sided (Alexander 159): sum touch
    >= 2."""
    p, K, t = m["p"], m["K"], m["t"]
    M = K["big_m"]
    for r in t["rooms"]:
        if not (r["lit"] or r["day"] or r["borrow"] or r["hall"]):
            continue
        k, f = r["key"], r["floor"]
        v = m["V"][k]
        Lf = m["L"][f]
        touch, lit = [], []
        for side in ("F", "R"):
            tb = _b(m, "touch|%s|%s" % (k, side))
            if side == "F":
                _c(m, v["y"] - M * (-tb + 1), max = 0)
            else:
                _rear_touch(m, r, tb)
            touch.append((side, tb))
            if r["hall"]:
                continue
            a = _b(m, "lit|%s|%s" % (k, side))
            if r["borrow"]:  # an open room lit through its open neighbour: its open edge within `borrow` of the glass
                near = v["y"] if side == "F" else (_borrow_end(m, r) - v["y"] - v["d"])
                _c(m, near - M * (-a + 1), max = r["borrow"])
            else:
                _c(m, a - tb, max = 0)
                _c(m, v["d"] - M * (-a + 1), max = K["lit"])
            lit.append((side, a))
        for w in t["wells"]:
            if f not in w["floors"]:
                continue
            pre = w["pre"]
            wk = pre + f
            vw = m["V"][wk]
            wd = _well_lit(m, w, f)
            m["well_lit"]["%s:%s" % (pre, f)] = wd
            cw = w["col"]
            ww = m["g"]["cols"][cw][1] - m["g"]["cols"][cw][0]
            side_kind = "W" if w.get("side", "E") == "E" else "E"  # rooms lie west of an east well, east of a west one
            for kind in (side_kind, "S", "N"):
                tb = _b(m, "touch|%s|%s%s" % (k, pre, kind))
                if not _adj_rows(m, k, wk, kind, tb, "%s|%s" % (k, pre),
                                 overlap = K["win_well"] if kind in ("W", "E") else K["door"]):
                    _c(m, tb, max = 0)
                    continue
                touch.append((pre + kind, tb))
                if r["hall"]:
                    continue
                a = _b(m, "lit|%s|%s%s" % (k, pre, kind))
                _c(m, a - tb, max = 0)
                if kind in ("W", "E"):
                    _c(m, v["w"] - M * (-a + 1), max = wd)
                    _c(m, v["d"] - vw["d"] - M * (-a + 1), max = K["head"])
                else:
                    _c(m, v["d"] - M * (-a + 1), max = wd)
                    _c(m, v["w"] - M * (-a + 1), max = ww + K["head"])
                lit.append((pre + kind, a))
        exempt = _exempt(m, r)
        if r["hall"]:
            if not exempt:
                _rule(m, "light", f, k, "reaches the glass", opt.sum([x[1] for x in touch]) - _e(m, k), min = 0)
        elif (r["lit"] or r["borrow"]) and not exempt:
            what = "daylit (2H)" + (" / escape window (R310)" if r["role"] == "sleep" else "")
            _rule(m, "light", f, k, what, opt.sum([x[1] for x in lit]) - _e(m, k), min = 0)
        m["lit_opts"][k] = lit
        m["touch"][k] = touch
        if r["day"] and not exempt:
            bay = m["t"].get("bay")
            bayv = 1.0 if (bay and bay["host"].get(f) == k) else 0.0
            tw = _b(m, "two|" + k)
            _c(m, opt.sum([x[1] for x in touch]) + bayv - 2 * tw, min = 0)
            m["two"][k] = tw

def _borrow_end(m, r):
    """The rear glass a borrowed-light room looks toward: the floor's rear wall, or its column's where it ends short."""
    shorts = [c for c in _cols_of(m, r["key"]) if (r["floor"], c) in m["Lc"]]
    if shorts:
        return m["Lc"][(r["floor"], shorts[0])]
    return m["L"][r["floor"]]

def _rear_touch(m, r, tb):
    f, k = r["floor"], r["key"]
    v = m["V"][k]
    M = m["K"]["big_m"]
    shorts = [c for c in _cols_of(m, k) if (f, c) in m["Lc"]]
    if not shorts:
        _c(m, v["y"] + v["d"] - m["L"][f] + M * (-tb + 1), min = 0)
        return
    for sp in v["spans"]:
        for c in range(sp[0], sp[1] + 1):
            _c(m, v["y"] + v["d"] - _depth_of(m, f, c) + M * (-tb + 1) + M * (-v["s"][sp] + 1), min = 0)

# ------------------------------------------------------------------ plumbing stacks (vertical: one v on every floor)

def _stacks(m):
    p, K, t = m["p"], m["K"], m["t"]
    M, P = K["big_m"], K["P"]
    m["stack_v"] = [_v(m, "stack%d.v" % si, min = 1.0, max = K["LMAX"] - 1.0) for si in range(len(t["stacks"]))]
    m["stack_on"] = [_b(m, "stack%d.used" % si) for si in range(len(t["stacks"]))]  # an unused candidate asks nothing
    for r in t["rooms"]:
        if not r["wet"]:
            continue
        k = r["key"]
        v = m["V"][k]
        zs = []
        for si, kline in enumerate(t["stacks"]):
            edge = [sv for sp, sv in v["s"].items() if sp[0] == kline or sp[1] + 1 == kline]
            if not edge:
                continue
            z = _b(m, "stack|%s|%d" % (k, si))
            _c(m, opt.sum(edge) - z, min = 0)
            _c(m, m["stack_on"][si] - z, min = 0)
            Vs = m["stack_v"][si]
            # the stack lies on the room's wall line, within its length or in its end wall (back-to-back wet rooms share)
            _c(m, Vs - v["y"] + M * (-z + 1), min = -P / 2.0)
            _c(m, Vs - v["y"] - v["d"] - M * (-z + 1), max = P / 2.0)
            zs.append((si, z))
        if not zs:
            fail("plan_parti: %s: wet, but no stack line is an edge of any of its spans" % k)
        if not (_exempt(m, r)):
            _rule(m, "stack", r["floor"], k, "on a stack", opt.sum([z[1] for z in zs]) - _e(m, k), min = 0)
        m["stack_use"][k] = zs
    # a stack runs in walls on every floor: never through a room that spans across its line
    for si, kline in enumerate(t["stacks"]):
        Vs = m["stack_v"][si]
        for r in t["rooms"]:
            v = m["V"][r["key"]]
            cr = [sv for sp, sv in v["s"].items() if sp[0] < kline and kline <= sp[1]]
            if not cr:
                continue
            cross = opt.sum(cr)
            b = _b(m, "stackclear|%d|%s" % (si, r["key"]))
            on = m["stack_on"][si]
            _c(m, Vs - v["y"] - M * (-b + 1) - M * (-cross + 1) - M * (-on + 1), max = -0.25)
            _c(m, Vs - v["y"] - v["d"] + M * b + M * (-cross + 1) + M * (-on + 1), min = 0.25)

# ------------------------------------------------------------------ floors, area cap, deck, garden door

def _bay_area(b):
    w0 = b["u1"] - b["u0"]
    w1 = max(w0 - 2 * b["proj"], 0.0)
    return 0.5 * (w0 + w1) * b["proj"]

def _col_gross_widths(m):
    """Each column's share of the lot width, wall faces included (they sum to the lot width)."""
    g, K = m["g"], m["K"]
    xs = [0.0] + [x + K["WALL"] for x in g["lines"]] + [K["LOT_W"]]
    return [xs[i + 1] - xs[i] for i in range(g["C"])]

def _floors(m):
    p, K, t, g = m["p"], m["K"], m["t"], m["g"]
    M, P, WALL = K["big_m"], K["P"], K["WALL"]
    L, FL = m["L"], m["FL"]
    shorts01 = len(FL) > 1 and [c for c in range(g["C"]) if (FL[0], c) in m["Lc"] or (FL[1], c) in m["Lc"]]
    if len(FL) > 1:
        _rule(m, "floors", FL[1], "deck", "main floor %s short of the ground floor" % ftin(K["deck_min"]),
              L[FL[1]] - L[FL[0]], max = -K["deck_min"])
        # where columns may end short, the deck is behind every column: each column of the main floor stops
        # deck_min short of the same column below
        for c in (shorts01 or []):
            _rule(m, "floors", FL[1], "deck", "c%d %s short of the ground floor" % (c, ftin(K["deck_min"])),
                  _depth_of(m, FL[1], c) - _depth_of(m, FL[0], c), max = -K["deck_min"])
    for i in range(1, len(FL) - 1):
        _rule(m, "floors", FL[i + 1], "plate", "over %s (cantilever <= %s)" % (FL[i], ftin(K["cant"])),
              L[FL[i + 1]] - L[FL[i]], max = K["cant"])
    # where a column may end short on a floor or the floor below, it stands over the same column below,
    # overhanging it by at most `cant` (each floor inside the one below)
    for i in range(1, len(FL)):
        f, below = FL[i], FL[i - 1]
        for c in range(g["C"]):
            if (f, c) in m["Lc"] or (below, c) in m["Lc"]:
                _rule(m, "floors", f, "c%d" % c, "over %s (cantilever <= %s)" % (below, ftin(K["cant"])),
                      _depth_of(m, f, c) - _depth_of(m, below, c), max = K["cant"])
    if m["Lc"]:
        cw = _col_gross_widths(m)
        gross = opt.sum([cw[c] * (_depth_of(m, f, c) + 2 * WALL) for f in FL for c in range(g["C"])])
    else:
        gross = opt.sum([K["LOT_W"] * (L[f] + 2 * WALL) for f in FL])
    garage_gross = 0.0
    if "garage" in m["V"]:
        gar = m["V"]["garage"]
        garage_gross = opt.sum([(_width(g, sp) + 0.7) * (gar["pr"][sp] + 0.7 * gar["s"][sp]) for sp in gar["spans"]])
    wg = []
    for wl in t["wells"]:
        cw_ = wl["col"]
        ww = g["cols"][cw_][1] - g["cols"][cw_][0] + P / 2.0 + WALL
        for f in wl["floors"]:
            wg.append(ww * (m["V"][wl["pre"] + f]["d"] + P))
    well_gross = opt.sum(wg)
    bay = t.get("bay")
    bay_area = len(bay["host"]) * _bay_area(bay) if bay else 0.0
    m["gross"] = gross - garage_gross - well_gross + bay_area
    _rule(m, "cap", "all", "house", "floor area cap %s (garage excluded)" % sqft(K["FAR_CAP"]), m["gross"],
          max = K["FAR_CAP"])
    if len(FL) > 1:
        m["deck_d"] = _v(m, "deck.val_depth", min = 0.0, max = K["deck_val"])
        _c(m, m["deck_d"] - L[FL[0]] + L[FL[1]], max = 0)
        for c in (shorts01 or []):  # a full-width deck: as deep as its shallowest column
            _c(m, m["deck_d"] - _depth_of(m, FL[0], c) + _depth_of(m, FL[1], c), max = 0)
        m["kernel_terms"].append(K["deck_weight"] * K["W"] * m["deck_d"])
    m["built"] = gross - well_gross + bay_area
    m["kernel_terms"].append(-K["parsimony"] * m["built"])
    gs = []
    for r in m["F"][FL[0]]:
        if r["garden"]:
            b = _b(m, "garden|" + r["key"])
            v = m["V"][r["key"]]
            _rear_touch(m, r, b)  # the garden door is in the room's rear wall, at its columns' end
            if r["optional"]:
                _c(m, b - _e(m, r["key"]), max = 0)
            gs.append((r["key"], b))
    if gs:
        _rule(m, "garden", FL[0], "house", "a door to the garden", opt.sum([x[1] for x in gs]), min = 1)
    m["garden_opts"] = gs

# ------------------------------------------------------------------ patterns and the objective

def _patterns(m):
    pts = m["t"].get("patterns", {})
    if "two_sided" in pts and m["two"]:
        mode, w = pts["two_sided"][0], pts["two_sided"][1]
        ex = opt.sum(list(m["two"].values()))
        if mode == "hard":
            _rule(m, "pattern", "all", "house", "light on two sides (159): at least %d day rooms" % int(w), ex, min = w)
        else:
            m["pattern_terms"].append(("light on two sides (159)", w, ex))
    if "entrance_turn" in pts:
        mode, w = pts["entrance_turn"][0], pts["entrance_turn"][1]
        turns = []
        for r in m["t"]["rooms"]:
            for o in m["door_opts"].get(r["key"], []):
                if o[0] == "vestibule" and o[1] in ("E", "W"):
                    turns.append(o[2])
        if turns:
            ex = opt.sum(turns)
            if mode == "hard":
                _rule(m, "pattern", "all", "vestibule", "entrance transition (112): a turn", ex, min = 1)
            else:
                m["pattern_terms"].append(("entrance transition (112): a turn", w, ex))

def _objective(m, patterns = True):
    m["kernel"] = opt.sum(m["kernel_terms"])
    obj = m["kernel"]
    if patterns:
        for x in m["pattern_terms"]:
            obj = obj + x[1] * x[2]
    for x in m["wish_terms"]:
        obj = obj + x[1] * x[2]
    for x in m["rule_terms"]:
        obj = obj + x[1] * x[2]
    if m["door_cost_terms"]:
        obj = obj - m["K"]["door_cost"] * opt.sum(m["door_cost_terms"])
    m["objective"] = obj
    m["p"].maximize(obj)

def _short_columns(m):
    """Columns that may end short of the floor's rear wall: their own depth variable, at most the floor's."""
    p, K = m["p"], m["K"]
    for s in m["t"].get("short", []):
        if type(s) == "tuple" or type(s) == "list":
            pairs = [(s[0], s[1])]
        else:
            pairs = [(f, s) for f in m["FL"]]
        for f, c in pairs:
            if f not in m["L"] or (f, c) in m["Lc"]:
                continue
            lc = _v(m, "Lc.%s.%d" % (f, c), min = 0.0, max = K["LMAX"])
            _c(m, lc - m["L"][f], max = 0)
            m["Lc"][(f, c)] = lc

def _penalty(m, rule_, what, expr):
    """A weighted rule's price: `weight` per unit of `expr` (a shortfall or an excess, at least 0)."""
    m["rule_terms"].append(("%s: %s" % (rule_["name"], what), -rule_["weight"], expr))

def _excess(m, name, expr, cap):
    """A variable at least max(0, expr - cap)."""
    x = _v(m, name, min = 0.0)
    _c(m, x - expr, min = -cap)
    return x

def _door_link(m, rule_, a, bs, what):
    """A door between room a and one of the rooms bs, by an access door either way or one made for the rule:
    hard (a must) or weighted (a wish worth the rule's weight). Returns the link binary, or None."""
    if a not in m["V"]:
        return None
    have = []
    for b in bs:
        if b not in m["V"] or m["R"][b]["floor"] != m["R"][a]["floor"]:
            continue
        for o in m["door_opts"].get(a, []):
            if o[0] == b:
                have.append(o[2])
        for o in m["door_opts"].get(b, []):
            if o[0] == a:
                have.append(o[2])
        for kind in ("S", "N", "E", "W"):
            z = _b(m, "rdoor|%s|%s|%s|%s" % (rule_["name"], a, b, kind))
            if _adj_rows(m, a, b, kind, z, "r%s<%s" % (a, b)):
                have.append(z)
                m["wish_doors"].append((a, b, kind, z))
                m["door_cost_terms"].append(z)
            else:
                _c(m, z, max = 0)
    x = _b(m, "link|%s|%s" % (rule_["name"], a))
    _c(m, opt.sum(have) - x, min = 0)
    if rule_["hard"]:
        _rule(m, rule_["family"], m["R"][a]["floor"], a, what, x - _e(m, a), min = 0)
    else:
        m["rule_terms"].append(("%s: %s" % (rule_["name"], what), rule_["weight"], x))
    return x

def _apply_rules(m, rules):
    """Compile @cadpole/plan_rules.star rules into the parti model, each under its own family. Kept rooms are exempt."""
    K = m["K"]
    M = K["big_m"]
    g = m["g"]
    t = m["t"]
    for r in rules:
        n, fam, a = r["name"], r["family"], r["args"]
        if n == "garage_size":
            for q in t["rooms"]:
                if q["kind"] != "garage" or _exempt(m, q):
                    continue
                v, k, f = m["V"][q["key"]], q["key"], q["floor"]
                if r["hard"]:
                    for sp in v["spans"]:
                        if _width(g, sp) > a["wmax"] + 1e-9:
                            _rule(m, fam, f, k, "no wider than %s (%d-%d)" % (ftin(a["wmax"]), sp[0], sp[1]), v["s"][sp], max = 0)
                    _rule(m, fam, f, k, "no deeper than %s" % ftin(a["dmax"]), v["d"], max = a["dmax"])
                else:
                    _penalty(m, r, "%s past %s x %s" % (k, ftin(a["wmax"]), ftin(a["dmax"])),
                             _excess(m, "gx|" + k, v["area"], a["wmax"] * a["dmax"]))
        elif n == "hall_width":
            for q in t["rooms"]:
                if q["role"] != "circ" or q["key"] == "vestibule" or q["through"] or q.get("hall_room") or _exempt(m, q):
                    continue
                v, k, f = m["V"][q["key"]], q["key"], q["floor"]
                for sp in v["spans"]:
                    if _width(g, sp) > a["wmax"] + 1e-9:
                        _rule(m, fam, f, k, "narrow side at most %s (%d-%d)" % (ftin(a["wmax"]), sp[0], sp[1]),
                              v["d"] - M * (-v["s"][sp] + 1), max = a["wmax"])
        elif n == "storage_cap":
            for q in t["rooms"]:
                if q["kind"] not in a["caps"] or _exempt(m, q):
                    continue
                v, k, f = m["V"][q["key"]], q["key"], q["floor"]
                cap = a["caps"][q["kind"]]
                if r["hard"]:
                    _rule(m, fam, f, k, "at most %s" % sqft(cap), v["area"], max = cap)
                else:
                    _penalty(m, r, "%s past %s" % (k, sqft(cap)), _excess(m, "sx|" + k, v["area"], cap))
        elif n == "well_band":
            b0, b1 = a["band"][0], a["band"][1]
            for w in t["wells"]:
                if w.get("rear"):
                    continue  # a rear court opens to the yard: not a light well
                for f in w["floors"]:
                    k = w["pre"] + f
                    if k not in m["V"]:
                        continue
                    v = m["V"][k]
                    L = m["L"][f]
                    if not m["R"][k].get("site"):
                        _rule(m, fam, f, k, "in the middle band (front)", 2 * v["y"] + v["d"] - 2 * b0 * L, min = 0)
                        _rule(m, fam, f, k, "in the middle band (back)", 2 * v["y"] + v["d"] - 2 * b1 * L, max = 0)
                    lit = []
                    for q in m["F"][f]:
                        if q["role"] not in ("hab", "sleep"):
                            continue
                        for x in m["lit_opts"].get(q["key"], []):
                            if x[0].startswith(w["pre"]) and x[0][len(w["pre"]):] in ("W", "E", "S", "N"):
                                lit.append(x[1])
                    _rule(m, fam, f, k, "lights a habitable room", opt.sum(lit) - _e(m, k), min = 0)
        elif n == "pantry_by_kitchen":
            if a["pantry"] in m["V"] and a["kitchen"] in m["V"] and not _exempt(m, m["R"][a["pantry"]]):
                _door_link(m, r, a["pantry"], [a["kitchen"]], "opens off the " + a["kitchen"])
        elif n == "stack_day":
            pass  # the kernel's stacks run on column lines and clear every room they would cross: a check, not a row
        elif n == "garage_door":
            for q in t["rooms"]:
                if q["kind"] != "garage" or _exempt(m, q):
                    continue
                into = a.get("into") or [x["key"] for x in m["F"][q["floor"]] if x["role"] == "circ"]
                _door_link(m, r, q["key"], into, "a door into the house")
        elif n == "no_garage_only":
            for q in t["rooms"]:
                if q["kind"] not in a["kinds"] or _exempt(m, q):
                    continue
                for o in m["door_opts"].get(q["key"], []):
                    if o[0] in m["R"] and m["R"][o[0]]["kind"] == "garage":
                        _rule(m, fam, q["floor"], q["key"], "not through the garage (%s)" % o[1], o[2], max = 0)
        elif n in ("wet_off_hall", "bed_off_hall"):
            kinds = a.get("kinds", ["bed", "guest"])
            for q in t["rooms"]:
                if q["kind"] not in kinds or _exempt(m, q):
                    continue
                bad = [o[2] for o in m["door_opts"].get(q["key"], []) if o[0] in m["R"] and
                       m["R"][o[0]]["kind"] in a["not_off"]]
                if not bad:
                    continue
                if r["hard"]:
                    _rule(m, fam, q["floor"], q["key"], "not off a day room", opt.sum(bad), max = 0)
                else:
                    _penalty(m, r, "%s off a day room" % q["key"], opt.sum(bad))
        elif n == "egress_facade":
            want = {"street": "F", "rear": "R"}
            for q in t["rooms"]:
                if q["role"] != "sleep" or _exempt(m, q):
                    continue
                ts = [x[1] for x in m["touch"].get(q["key"], []) if x[0] in [want[s] for s in a["sources"] if s in want]]
                _rule(m, fam, q["floor"], q["key"], "an escape window on a facade (R310.1)", opt.sum(ts) - _e(m, q["key"]), min = 0)
        elif n == "not_over":
            # a room of an upper kind and one of a lower kind on the floor below that may share a column lie one in
            # front of the other wherever they do share one (as rooms on one floor do)
            M = K["big_m"]
            FL = m["FL"]
            for i in range(1, len(FL)):
                for qa in m["F"][FL[i]]:
                    if qa["kind"] not in a["upper"] or _exempt(m, qa):
                        continue
                    for qb in m["F"][FL[i - 1]]:
                        if qb["kind"] not in a["lower"] or _exempt(m, qb):
                            continue
                        cb = _cols_of(m, qb["key"])
                        common = sorted([c for c in _cols_of(m, qa["key"]) if c in cb])
                        if not common:
                            continue
                        va, vb = m["V"][qa["key"]], m["V"][qb["key"]]
                        sh = _b(m, "over|%s|%s" % (qa["key"], qb["key"]))
                        for c in common:
                            _c(m, sh - _u(m, qa["key"], c) - _u(m, qb["key"], c), min = -1)
                        o = _b(m, "overo|%s|%s" % (qa["key"], qb["key"]))
                        _rule(m, fam, FL[i], qa["key"], "not over %s (front)" % qb["key"],
                              va["y"] + va["d"] - vb["y"] - M * (-o + 1) - M * (-sh + 1), max = 0)
                        _rule(m, fam, FL[i], qa["key"], "not over %s (back)" % qb["key"],
                              vb["y"] + vb["d"] - va["y"] - M * o - M * (-sh + 1), max = 0)
        elif n == "bed_width":
            for q in t["rooms"]:
                if q["role"] != "sleep" or _exempt(m, q):
                    continue
                v, k, f = m["V"][q["key"]], q["key"], q["floor"]
                narrow = [sp for sp in v["spans"] if _width(g, sp) < a["wmin"] - 1e-9]
                if r["hard"]:
                    for sp in narrow:
                        _rule(m, fam, f, k, "at least %s wide (%d-%d)" % (ftin(a["wmin"]), sp[0], sp[1]), v["s"][sp], max = 0)
                    _rule(m, fam, f, k, "at least %s deep" % ftin(a["wmin"]), v["d"], min = a["wmin"] * 1.0)
                else:
                    if narrow:
                        _penalty(m, r, "%s narrower than %s" % (k, ftin(a["wmin"])),
                                 opt.sum([(a["wmin"] - _width(g, sp)) * v["s"][sp] for sp in narrow]))
                    sd = _v(m, "bx|" + k, min = 0.0)
                    _c(m, sd + v["d"], min = a["wmin"] * 1.0)
                    _penalty(m, r, "%s shallower than %s" % (k, ftin(a["wmin"])), sd)
        else:
            fail("plan_parti: rule %r is not one the parti kernel compiles" % n)

def compile_template(t, drop = None, objective = True, patterns = True, name = None, options = None, rules = None,
                     fit_rows = False, light = "assumed"):
    """Compile a resolved template (resolve()) into an opt problem. Returns the model: a dict whose "p" is the
    problem and whose other entries map rooms, doors, light and stacks to their variables. `drop` is a list of rule
    families (or "family:floor") to leave out. `rules` are @cadpole/plan_rules.star rules to add."""
    site = t["site"]
    K = _constants(site, options)
    g = grid(t["lines"], K["W"], K["P"])
    FL = list(t["floors"])
    p = opt.problem(name = name or ("plan_parti_" + t["key"]))
    dropd = {}
    for x in (drop or []):
        dropd[x] = True
    R = {}
    F = {}
    for f in FL:
        F[f] = []
    for r in t["rooms"]:
        if r["key"] in R:
            fail("plan_parti: duplicate room key %s" % r["key"])
        if r["floor"] not in F:
            fail("plan_parti: %s is on floor %s, which the template does not have (%s)" % (r["key"], r["floor"], FL))
        R[r["key"]] = r
        F[r["floor"]].append(r)
    m = {"p": p, "t": t, "K": K, "g": g, "FL": FL, "R": R, "F": F, "drop": dropd, "bearing": {},
         "V": {}, "door_opts": {}, "extra_opts": [], "lit_opts": {}, "touch": {}, "two": {}, "pattern_terms": [],
         "kernel_terms": [], "wish_terms": [], "rule_terms": [], "door_cost_terms": [], "wish_doors": [],
         "touch_opts": [], "sh": {}, "arr_bins": [], "landings": {}, "stack_v": [], "stack_on": [], "stack_use": {},
         "sym_pairs": [], "rows": {}, "Lc": {}, "garden_opts": [], "deck_d": None,
         "n": {"vars": 0, "binaries": 0, "rows": 0}, "fit_rows": fit_rows, "fit_used": {}, "light": light,
         "well_lit": {}}
    for b in t.get("bearing", []):
        m["bearing"][b] = True
    m["Ys"] = _v(m, "stair.y", min = 0.0, max = K["LMAX"] - 3.0)
    L = {}
    for f in FL:
        L[f] = _v(m, "L." + f, min = K["min_floor"], max = K["LMAX"])
    m["L"] = L
    for f, val in t.get("fix_depth", {}).items():
        _rule(m, "fixed", f, "plate", "existing depth", L[f], equals = val)
    _short_columns(m)
    for r in t["rooms"]:
        _room(m, r)
    _fixed(m)
    for f in FL:
        _tiling(m, f)
        _pairs(m, f)
    _orders(m)
    _symmetry(m)
    _stair(m)
    _wells(m)
    _doors(m)
    _extra_doors(m)
    _door_wishes(m)
    _light(m)
    _stacks(m)
    _floors(m)
    if rules:
        _apply_rules(m, rules)
    _patterns(m)
    if objective:
        _objective(m, patterns)
    return m

# ------------------------------------------------------------------ reading a solution back

def _val(sol, x):
    if type(x) == "float" or type(x) == "int":
        return float(x)
    return float(sol.value(x))

def _solution_rooms(m, sol):
    """Rooms that exist in the solution, in the kernel frame: u across from the west wall's inside face, v back from
    the front wall's inside face."""
    g, t = m["g"], m["t"]
    out = []
    bay = t.get("bay")
    for r in t["rooms"]:
        v = m["V"][r["key"]]
        if _val(sol, v["e"]) < 0.5:
            continue
        best, bv = None, -1.0
        for sp in v["spans"]:
            x = _val(sol, v["s"][sp])
            if x > bv:
                best, bv = sp, x
        u0, u1 = _uv(g, best)
        out.append({"key": r["key"], "name": r["name"], "label": r["label"] or r["name"], "floor": r["floor"],
                    "role": r["role"], "kind": r["kind"], "span": best, "u0": u0, "u1": u1,
                    "y": _val(sol, v["y"]), "d": _val(sol, v["d"]), "area": _val(sol, v["area"]),
                    "wet": r["wet"], "lit": r["lit"], "day": r["day"], "through": r["through"],
                    "open_to": list(r["open_to"]), "hall": r["hall"], "borrow": r["borrow"], "program": r["program"],
                    "kept": r.get("kept", False) or r["key"] in t.get("fixed", {}), "site": r.get("site", False),
                    "bay": bool(bay and bay["host"].get(r["floor"]) == r["key"]), "well": r.get("well"),
                    "core": r.get("core"), "unit": r.get("unit", "main"), "furnish": r.get("furnish")})
    return out

def _solution_doors(m, sol):
    out = []
    for rk, opts in m["door_opts"].items():
        for o in opts:
            if o[0] == "ROOT":
                out.append({"room": rk, "parent": "ROOT", "kind": "street", "access": True})
                break
            if _val(sol, o[2]) > 0.5:
                out.append({"room": rk, "parent": o[0], "kind": o[1], "access": True})
    for x in m["extra_opts"]:
        for o in x[1]:
            if _val(sol, o[2]) > 0.5:
                out.append({"room": x[0], "parent": o[0], "kind": o[1], "extra": True})
    for x in m["wish_doors"]:
        if _val(sol, x[3]) > 0.5:
            out.append({"room": x[0], "parent": x[1], "kind": x[2], "extra": True, "wish": True})
    for x in m["touch_opts"]:
        for o in x[1]:
            if _val(sol, o[2]) > 0.5:
                out.append({"room": o[0], "parent": "stair.bot", "kind": o[1], "depart": True})
    return out

def _stats(m):
    return {"rooms": len(m["t"]["rooms"]), "vars": m["n"]["vars"], "binaries": m["n"]["binaries"],
            "rows": m["n"]["rows"], "arrangement": len(m["arr_bins"])}

def _ext(m, sol):
    """The solution as kernel-frame records: rooms, doors, stacks, depths. What the scorecard and the plan read."""
    t, g, K = m["t"], m["g"], m["K"]
    L = {}
    for f in m["FL"]:
        L[f] = _val(sol, m["L"][f])
    Lc = {}
    for key in m["Lc"]:
        Lc[key] = _val(sol, m["Lc"][key])
    stacks = []
    for si, kline in enumerate(t["stacks"]):
        if _val(sol, m["stack_on"][si]) < 0.5:
            continue
        users = [rk for rk, zs in m["stack_use"].items() for z in zs if z[0] == si and _val(sol, z[1]) > 0.5]
        stacks.append({"si": si, "line": kline, "u": g["line_u"][kline], "v": _val(sol, m["stack_v"][si]), "rooms": users})
    ext = {"key": t["key"], "floors": m["FL"], "L": L, "Lc": Lc, "Ys": _val(sol, m["Ys"]),
           "cols": g["cols"], "lines": g["lines"], "line_u": g["line_u"], "W": K["W"], "P": K["P"],
           "stair": t["stair"], "wells": t["wells"], "bay": t.get("bay"),
           "rooms": _solution_rooms(m, sol), "doors": _solution_doors(m, sol), "stacks": stacks,
           "garden": [x[0] for x in m["garden_opts"] if _val(sol, x[1]) > 0.5],
           "gross": _val(sol, m["gross"]), "built": _val(sol, m["built"]),
           "deck": (L[m["FL"][0]] - L[m["FL"][1]]) if len(m["FL"]) > 1 else 0.0,
           "K": K}
    return ext

def _col_depth(ext, f, c):
    return ext["Lc"].get((f, c), ext["L"][f])

def _rooms_on(ext, f):
    return [r for r in ext["rooms"] if r["floor"] == f]

def _well_rects(ext, f):
    return [r for r in _rooms_on(ext, f) if r["role"] == "void"]

def _well_side(ext, r, w):
    """How room r meets well w: "side" (beside it, across a column line), "endS"/"endN" (in front of / behind it)."""
    P = ext["P"]
    ov_v = min(r["y"] + r["d"], w["y"] + w["d"]) - max(r["y"], w["y"])
    ov_u = min(r["u1"], w["u1"]) - max(r["u0"], w["u0"])
    side = abs(r["u1"] + P - w["u0"]) < 0.05 or abs(w["u1"] + P - r["u0"]) < 0.05
    if side and ov_v >= ext["K"]["win_well"] - 0.05:
        return "side"
    if abs(r["y"] + r["d"] + P - w["y"]) < 0.05 and ov_u >= 2.0:
        return "endS"
    if abs(w["y"] + w["d"] + P - r["y"]) < 0.05 and ov_u >= 2.0:
        return "endN"
    return None

def _rear_of(ext, r):
    """The room's far end is its columns' rear wall."""
    for c in range(r["span"][0], r["span"][1] + 1):
        if r["y"] + r["d"] < _col_depth(ext, r["floor"], c) - 0.05:
            return False
    return True

def sources(ext, r):
    """Light sources of a room, from geometry: front glass, rear glass, a well (a face of it), the bay."""
    src = []
    if r["y"] < 0.05:
        src.append("front")
    if _rear_of(ext, r):
        src.append("rear")
    if r["role"] != "void":
        for w in _well_rects(ext, r["floor"]):
            if _well_side(ext, r, w):
                src.append("well")
                break
    if r.get("bay"):
        src.append("bay")
    return src

def _dark_area(ext, r, step = 0.5):
    """Floor area of a room farther than 2H (16 ft) from every glass face it touches (a well: its assumed lit depth,
    12 ft -- an assumption view_cone refutes; the measured light replaces it)."""
    K = ext["K"]
    L = ext["L"][r["floor"]]
    wd = 12.0
    if ext["wells"]:
        wd = ext["wells"][0].get("lit_depth", 12.0)
    src = sources(ext, r)
    ws = [(w, _well_side(ext, r, w)) for w in _well_rects(ext, r["floor"])]
    hh = K["head"] / 2.0
    nu = int((r["u1"] - r["u0"]) / step)
    nv = int(r["d"] / step)
    n, tot = 0, 0
    for i in range(nu):
        u = r["u0"] + step / 2 + i * step
        for j in range(nv):
            v = r["y"] + step / 2 + j * step
            tot += 1
            lit = False
            if "front" in src and v <= K["lit"]:
                lit = True
            if not lit and "rear" in src and r["y"] + r["d"] - v <= K["lit"]:
                lit = True
            if not lit:
                for x in ws:
                    w, how = x[0], x[1]
                    if how == "side" and w["y"] - hh <= v and v <= w["y"] + w["d"] + hh:
                        dist = (w["u0"] - u) if w["u0"] > u else (u - w["u1"])
                        if dist <= wd:
                            lit = True
                    if how == "endS" and w["y"] - v <= wd and w["u0"] - hh <= u and u <= w["u1"] + hh:
                        lit = True
                    if how == "endN" and v - (w["y"] + w["d"]) <= wd and w["u0"] - hh <= u and u <= w["u1"] + hh:
                        lit = True
            if not lit:
                n += 1
    return r["area"] * n / max(tot, 1)

def _touching(a, b, P, tol = 0.05):
    ov_u = min(a["u1"], b["u1"]) - max(a["u0"], b["u0"])
    ov_v = min(a["y"] + a["d"], b["y"] + b["d"]) - max(a["y"], b["y"])
    if ov_u > 1.0 and (abs(a["y"] + a["d"] + P - b["y"]) < tol or abs(b["y"] + b["d"] + P - a["y"]) < tol):
        return True
    if ov_v > 1.0 and (abs(a["u1"] + P - b["u0"]) < tol or abs(b["u1"] + P - a["u0"]) < tol):
        return True
    return False

def _graph(ext, f):
    """Openings on a floor: the doors, and open-plan links (rooms open to each other that touch)."""
    E = []
    keys = {}
    for r in _rooms_on(ext, f):
        keys[r["key"]] = r
    for d in ext["doors"]:
        if d.get("depart"):
            continue
        if d["room"] not in keys:
            continue
        E.append((d["room"], d["parent"]))
    for r in keys.values():
        for o in r["open_to"]:
            if o in keys and _touching(r, keys[o], ext["P"]) and (o, r["key"]) not in E and (r["key"], o) not in E:
                E.append((r["key"], o))
    return E

def _depths(ext, f):
    E = _graph(ext, f)
    root = "ROOT" if f == ext["floors"][0] else "stair.top"
    adj = {}
    for e in E:
        adj.setdefault(e[0], []).append(e[1])
        adj.setdefault(e[1], []).append(e[0])
    dep = {root: 0}
    q = [root]
    for _ in range(1000):
        if not q:
            break
        x = q.pop(0)
        for y in adj.get(x, []):
            if y not in dep:
                dep[y] = dep[x] + 1
                q.append(y)
    return dep

def _has_loop(ext, f):
    E = [e for e in _graph(ext, f) if e[0] not in ("ROOT", "stair.top") and e[1] not in ("ROOT", "stair.top")]
    parent = {}
    for e in E:
        parent[e[0]] = e[0]
        parent[e[1]] = e[1]

    def find(x):
        for _ in range(1000):
            if parent[x] == x:
                break
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for e in E:
        ra, rb = find(e[0]), find(e[1])
        if ra == rb:
            return True
        parent[ra] = rb
    return False

def _sightline(ext, f):
    """The longest clear view from the front glass to the rear glass along one column: every room met is circulation
    or open plan, and each touches the next through a door or an open-plan link."""
    if f not in ext["L"]:
        return 0.0
    E = {}
    for e in _graph(ext, f):
        E[(e[0], e[1])] = True
        E[(e[1], e[0])] = True
    best = 0.0
    for c in range(len(ext["cols"])):
        col = sorted([r for r in _rooms_on(ext, f) if r["span"][0] <= c and c <= r["span"][1]], key = lambda r: r["y"])
        if not col or col[0]["y"] > 0.05 or col[-1]["y"] + col[-1]["d"] < ext["L"][f] - 0.05:
            continue
        ok = len([r for r in col if not (r["role"] == "circ" or r["through"] or r["open_to"] or
                                         r["key"] in ("living", "dining", "kitchen"))]) == 0
        ok = ok and len([i for i in range(len(col) - 1) if (col[i]["key"], col[i + 1]["key"]) not in E]) == 0
        if ok:
            best = max(best, ext["L"][f])
    return best

def character(ext, keys = None):
    """A plan's character, room by room (the program rooms): its floor, its third of the floor's depth (street,
    middle, garden), which party walls it touches, its light sources, and what its door opens from (a hall, the
    stair, or which room). Options that differ in character differ in organization, not in inches."""
    rs = {}
    for r in ext["rooms"]:
        rs[r["key"]] = r
    par = {}
    for d in ext["doors"]:
        if not d.get("depart") and not d.get("extra") and d["room"] not in par:
            par[d["room"]] = d["parent"]
    out = {}
    W = ext["W"]
    for k in (keys or PROGRAM_KEYS):
        r = rs.get(k)
        if r == None:
            continue
        L = ext["L"][r["floor"]]
        c = r["y"] + r["d"] / 2.0
        third = "street" if c < L / 3.0 else ("garden" if c > 2 * L / 3.0 else "middle")
        walls = ("W" if r["u0"] < 0.05 else "") + ("E" if r["u1"] > W - 0.05 else "")
        src = ",".join(sorted([s for s in sources(ext, r) if s != "bay"]))
        p = par.get(k, "?")
        if p in rs and rs[p]["role"] == "circ" and rs[p]["key"] != "vestibule":
            p = "hall"
        elif p.startswith("stair"):
            p = "stair"
        out[k] = (r["floor"], third, walls, src, p)
    return out

def metrics(ext):
    """The scorecard's measures, from the solution's geometry."""
    mt = {}
    hab = [r for r in ext["rooms"] if r["role"] in ("hab", "sleep")]
    mt["two_sided"] = [r["key"] for r in hab if len([s for s in sources(ext, r)]) >= 2]
    dark = {}
    tot = 0.0
    for r in hab:
        a = _dark_area(ext, r)
        tot += a
        if a > 1.0:
            dark[r["key"]] = a
    mt["dark_sqft_assumed"] = tot
    mt["dark_rooms_assumed"] = dark
    fl = ext["floors"]
    mt["sightline_main"] = _sightline(ext, fl[1]) if len(fl) > 1 else 0.0
    mt["loop_main"] = _has_loop(ext, fl[1]) if len(fl) > 1 else False
    mt["door_depth"] = {f: _depths(ext, f) for f in fl}
    mt["entrance_turn"] = len([d for d in ext["doors"] if d["parent"] == "vestibule" and d["kind"] in ("E", "W")]) > 0
    rs = {}
    for r in ext["rooms"]:
        rs[r["key"]] = r
    mt["kitchen_garden"] = "kitchen" in rs and "rear" in sources(ext, rs["kitchen"])
    mt["primary_garden"] = "primary" in rs and "rear" in sources(ext, rs["primary"])
    mt["halls_lit"] = [r["key"] for r in ext["rooms"] if r["role"] == "circ" and
                       [s for s in sources(ext, r) if s in ("front", "rear", "well")]]
    mt["circ_sqft"] = sum_([r["area"] for r in ext["rooms"] if r["role"] == "circ"])
    mt["net_sqft"] = sum_([r["area"] for r in ext["rooms"] if r["role"] not in ("void", "garage")])
    return mt

def sum_(xs):
    t = 0.0
    for x in xs:
        t += x
    return t

# ------------------------------------------------------------------ the plan value

def _pt(K, u, v):
    return (u + K["X0"], v + K["Y0"])

def _rect(K, u0, u1, v0, v1):
    return (u0 + K["X0"], u1 + K["X0"], v0 + K["Y0"], v1 + K["Y0"])

def _landing_rect(ext, f, which):
    st = ext["stair"]
    K = ext["K"]
    Ys = ext["Ys"]
    sr = [r for r in _rooms_on(ext, f) if r["role"] == "stair"][0]
    if st["type"] == "side":
        if which == "bot":
            return {"u0": sr["u0"], "u1": sr["u1"], "y": Ys, "d": K["land"]}
        return {"u0": sr["u0"], "u1": sr["u1"], "y": Ys + K["land"] + K["flight"], "d": K["land"]}
    c = st["land_w"] if which == "bot" else st["land_e"]
    cu = ext["cols"][c]
    return {"u0": cu[0], "u1": cu[1], "y": Ys, "d": K["stair_w"]}

def _door_geom(ext, a, b, kind):
    """Where the opening between room a and room-or-landing b lies: (wall of a, (u, v) centre, (lo, hi) along it)."""
    P = ext["P"]
    if kind in ("S", "N"):
        lo, hi = max(a["u0"], b["u0"]), min(a["u1"], b["u1"])
        v = a["y"] + a["d"] + P / 2.0 if kind == "S" else a["y"] - P / 2.0
        return ("N" if kind == "S" else "S"), (0.5 * (lo + hi), v), (lo, hi), "u"
    lo, hi = max(a["y"], b["y"]), min(a["y"] + a["d"], b["y"] + b["d"])
    u = a["u0"] - P / 2.0 if kind == "E" else a["u1"] + P / 2.0
    return ("W" if kind == "E" else "E"), (u, 0.5 * (lo + hi)), (lo, hi), "v"

def _plan_doors(ext):
    K = ext["K"]
    rs = {}
    for r in ext["rooms"]:
        rs[r["key"]] = r
    out = []
    n = 0
    for d in ext["doors"]:
        a = rs.get(d["room"])
        if a == None:
            continue
        f = a["floor"]
        n += 1
        key = "d%d_%s" % (n, d["room"])
        if d["parent"] == "ROOT":
            at = (0.5 * (a["u0"] + a["u1"]), -K["WALL"] / 2.0)
            out.append({"key": key, "floor": f, "a": a["key"], "b": "outside:street", "wall": "S", "at": _pt(K, at[0], at[1]),
                        "width": K["door"], "swing": "a", "hinge": "lo",
                        "span": (a["u0"] + K["X0"], a["u1"] + K["X0"]), "access": True})
            continue
        if d["parent"] in ("stair.top", "stair.bot"):
            b = _landing_rect(ext, f, d["parent"].split(".")[1])
            bkey = [r["key"] for r in _rooms_on(ext, f) if r["role"] == "stair"][0]
        else:
            b = rs.get(d["parent"])
            if b == None:
                continue
            bkey = b["key"]
        wall, c, span, axis = _door_geom(ext, a, b, d["kind"])
        off = K["X0"] if axis == "u" else K["Y0"]
        out.append({"key": key, "floor": f, "a": a["key"], "b": bkey, "wall": wall, "at": _pt(K, c[0], c[1]),
                    "width": K["door"], "swing": "open" if d.get("depart") else "a", "hinge": "lo",
                    "span": (span[0] + off, span[1] + off), "access": bool(d.get("access")),
                    "extra": bool(d.get("extra")), "depart": bool(d.get("depart"))})
    for k in ext["garden"]:
        a = rs[k]
        n += 1
        at = (0.5 * (a["u0"] + a["u1"]), a["y"] + a["d"] + K["WALL"] / 2.0)
        out.append({"key": "d%d_%s_garden" % (n, k), "floor": a["floor"], "a": k, "b": "outside:yard", "wall": "N",
                    "at": _pt(K, at[0], at[1]), "width": K["door"], "swing": "a", "hinge": "lo",
                    "span": (a["u0"] + K["X0"], a["u1"] + K["X0"])})
    # open-plan links: cased openings
    for f in ext["floors"]:
        keys = {}
        for r in _rooms_on(ext, f):
            keys[r["key"]] = r
        done = {}
        for r in keys.values():
            for o in r["open_to"]:
                b = keys.get(o)
                if b == None or (o, r["key"]) in done or not _touching(r, b, ext["P"]):
                    continue
                done[(r["key"], o)] = True
                kind = None
                if abs(r["y"] + r["d"] + ext["P"] - b["y"]) < 0.05:
                    kind = "S"
                elif abs(b["y"] + b["d"] + ext["P"] - r["y"]) < 0.05:
                    kind = "N"
                elif abs(r["u0"] - ext["P"] - b["u1"]) < 0.05:
                    kind = "E"
                else:
                    kind = "W"
                wall, c, span, axis = _door_geom(ext, r, b, kind)
                off = K["X0"] if axis == "u" else K["Y0"]
                n += 1
                out.append({"key": "o%d_%s_%s" % (n, r["key"], o), "floor": f, "a": r["key"], "b": o, "wall": wall,
                            "at": _pt(K, c[0], c[1]), "width": span[1] - span[0], "swing": "open", "hinge": "lo",
                            "span": (span[0] + off, span[1] + off)})
    return out

def _plan_windows(ext):
    K = ext["K"]
    out = []
    wkind = {}
    for w in ext["wells"]:
        wkind[w["pre"]] = ("court:" if w.get("rear") else "well:") + w["pre"]
    for r in ext["rooms"]:
        if r["role"] not in ("hab", "sleep", "circ"):
            continue
        if r["role"] == "circ" and not r["hall"] and r["key"] != "vestibule":
            continue
        src = sources(ext, r)
        w0 = r["u1"] - r["u0"]
        width = min(max(w0 - 2.0, 2.0), 8.0)
        sleeping = r["role"] == "sleep"
        if "front" in src and r["bay"]:
            # the bay is the room's street glass: its opening in the facade, the bay's three sides beyond it
            b = ext["bay"]
            out.append({"key": "w_%s_bay" % r["key"], "floor": r["floor"], "room": r["key"], "wall": "S",
                        "at": _pt(K, 0.5 * (b["u0"] + b["u1"]), -K["WALL"] / 2.0),
                        "width": min(b["u1"] - b["u0"], w0), "sill": 2.0, "head": K["head"], "source": "street",
                        "egress": sleeping, "bay": True})
        elif "front" in src:
            out.append({"key": "w_%s_s" % r["key"], "floor": r["floor"], "room": r["key"], "wall": "S",
                        "at": _pt(K, 0.5 * (r["u0"] + r["u1"]), -K["WALL"] / 2.0), "width": width, "sill": 2.0,
                        "head": K["head"], "source": "street", "egress": sleeping})
        if "rear" in src:
            out.append({"key": "w_%s_n" % r["key"], "floor": r["floor"], "room": r["key"], "wall": "N",
                        "at": _pt(K, 0.5 * (r["u0"] + r["u1"]), r["y"] + r["d"] + K["WALL"] / 2.0), "width": width,
                        "sill": 2.0, "head": K["head"], "source": "rear", "egress": sleeping})
        for w in _well_rects(ext, r["floor"]):
            how = _well_side(ext, r, w)
            if how == None or r["role"] == "circ":
                continue
            srcname = wkind.get(w.get("well"), "well:" + w["key"])
            reaches = srcname.startswith("court:")
            if how == "side":
                lo, hi = max(r["y"], w["y"]), min(r["y"] + r["d"], w["y"] + w["d"])
                east = abs(r["u1"] + ext["P"] - w["u0"]) < 0.05
                u = r["u1"] + ext["P"] / 2.0 if east else r["u0"] - ext["P"] / 2.0
                out.append({"key": "w_%s_%s" % (r["key"], w["key"]), "floor": r["floor"], "room": r["key"],
                            "wall": "E" if east else "W", "at": _pt(K, u, 0.5 * (lo + hi)),
                            "width": min(max(hi - lo - 1.0, 2.0), 8.0), "sill": 2.0, "head": K["head"],
                            "source": srcname, "egress": sleeping and reaches})
            else:
                lo, hi = max(r["u0"], w["u0"]), min(r["u1"], w["u1"])
                v = r["y"] + r["d"] + ext["P"] / 2.0 if how == "endS" else r["y"] - ext["P"] / 2.0
                out.append({"key": "w_%s_%s" % (r["key"], w["key"]), "floor": r["floor"], "room": r["key"],
                            "wall": "N" if how == "endS" else "S", "at": _pt(K, 0.5 * (lo + hi), v),
                            "width": min(max(hi - lo - 1.0, 2.0), 8.0), "sill": 2.0, "head": K["head"],
                            "source": srcname, "egress": sleeping and reaches})
    return out

def _plan_rooms(ext):
    K = ext["K"]
    out = []
    for r in ext["rooms"]:
        if r["role"] == "void":
            kind = "court" if r["kind"] == "court" else "well"
        else:
            kind = r["kind"]
        rec = {"key": r["key"], "name": r["name"], "floor": r["floor"], "kind": kind,
               "rects": [_rect(K, r["u0"], r["u1"], r["y"], r["y"] + r["d"])], "unit": r["unit"],
               "kept": bool(r["kept"]), "core": r["core"], "furnish": r["furnish"]}
        if r["role"] in ("hab", "sleep"):
            rec["habitable"] = True
        if r["role"] == "sleep":
            rec["sleeping"] = True
        if r["role"] in ("circ", "serv", "wet", "garage", "stair", "void") and r["kind"] in ("living", "dining", "den", "study", "office", "family", "kitchen"):
            rec["habitable"] = False
        out.append(rec)
    fl = ext["floors"]
    if len(fl) > 1 and ext["deck"] > 0.05:
        LM, LG = ext["L"][fl[1]], ext["L"][fl[0]]
        out.append({"key": "deck", "name": "Deck", "floor": fl[1], "kind": "deck",
                    "rects": [_rect(K, 0.0, ext["W"], LM + K["WALL"], LG + K["WALL"])], "unit": "main", "kept": False,
                    "core": None, "furnish": None, "habitable": False})
    return out

def _plan_floors(ext):
    K = ext["K"]
    out = []
    C = len(ext["cols"])
    for i, f in enumerate(ext["floors"]):
        # the built footprint, column by column: each column to its own depth, less the courts cut into it
        strips = []
        for c in range(C):
            u0 = ext["line_u"][c]  # partition centre lines: the strips meet, and merge where their depths agree
            u1 = ext["line_u"][c + 1]
            top = _col_depth(ext, f, c)
            holes = sorted([(w["y"], w["y"] + w["d"]) for w in _well_rects(ext, f) if w["span"][0] <= c and c <= w["span"][1]])
            v = 0.0
            for h in holes:
                if h[0] > v + 0.05:
                    strips.append((u0, u1, v, h[0]))
                v = max(v, h[1])
            if top > v + 0.05:
                strips.append((u0, u1, v, top))
        # merge side-by-side strips with the same depths
        merged = []
        for s in strips:
            if merged and abs(merged[-1][1] - s[0]) < 1e-9 and abs(merged[-1][2] - s[2]) < 1e-9 and abs(merged[-1][3] - s[3]) < 1e-9:
                last = merged.pop()
                merged.append((last[0], s[1], s[2], s[3]))
            else:
                merged.append(s)
        plate = [_rect(K, s[0], s[1], s[2], s[3]) for s in merged]
        out.append({"key": f, "name": FLOOR_NAMES.get(f, f), "z": i * K["FTF"], "ceiling": K["CEIL"], "plate": plate})
    return out

def _plan_stacks(ext):
    K = ext["K"]
    return [{"key": "s%d" % (i + 1), "at": _pt(K, s["u"], s["v"]), "floors": list(ext["floors"]), "kind": "dwv",
             "rooms": s["rooms"]} for i, s in enumerate(ext["stacks"])]

def _plan_courts(ext):
    K = ext["K"]
    out = []
    for w in ext["wells"]:
        base = w["floors"][0]
        rs = [r for r in ext["rooms"] if r["key"] == w["pre"] + base]
        if not rs:
            continue
        r = rs[0]
        top_i = ext["floors"].index(w["floors"][-1]) + 1
        out.append({"key": w["pre"], "kind": "court" if w.get("rear") else "well",
                    "rect": _rect(K, r["u0"], r["u1"], r["y"], r["y"] + r["d"]), "base": base,
                    "top": top_i * K["FTF"], "reaches": "rear" if w.get("rear") else None,
                    "floors": list(w["floors"])})
    return out

def _plan_bearing(ext, t):
    K = ext["K"]
    out = []
    for f in ext["floors"]:
        for k in t.get("bearing", []):
            out.append({"floor": f, "axis": "v", "at": ext["line_u"][k] + K["X0"], "span": (K["Y0"], K["Y0"] + ext["L"][f])})
    return out

def _site_of(site):
    out = {"lot": site["lot"], "street": site.get("street", "S"), "north_deg": site.get("north_deg", 0.0)}
    if "setbacks" in site:
        out["setbacks"] = site["setbacks"]
    if "neighbors" in site:
        out["neighbors"] = site["neighbors"]
    return out

def measured_light(plan):
    """The light measure (@cadpole/plan_light.star) on a plan value, without boxes: each court or well window lit
    as deep as SKY_TABLE says (view_cone's sky by court width, wall height and floor), facade glass to 2H, and each
    habitable room's floor within lit depth of its own glass. Uncalibrated, as every light row says.
    {"dark": sq ft, "share": 0..1, "rooms": {key: (sq ft, lit sq ft)}, "court_lit": {window key: ft}}"""
    courts = {}
    for c in plan.get("courts", []):
        courts[c["key"]] = c
    zs = {}
    for f in plan.get("floors", []):
        zs[f["key"]] = f.get("z", 0.0)
    court_lit = {}
    for w in plan.get("windows", []):
        src = w.get("source") or ""
        if not (src.startswith("court:") or src.startswith("well:")):
            continue
        c = courts.get(src.split(":", 1)[1])
        if c == None:
            continue
        q = c["rect"]
        width = (q[1] - q[0]) if w["wall"] in ("E", "W") else (q[3] - q[2])
        bz = zs.get(c["base"], 0.0)
        k = int((zs.get(w["floor"], 0.0) - bz) / 10.0 + 0.5)
        r = sky_row(width, c["top"] - bz, max(k, 0))
        if r != None:
            court_lit[w["key"]] = r["lit"]
    tot, lit = 0.0, 0.0
    rooms = {}
    for f in plan.get("floors", []):
        sh = lit_share(plan, f["key"], court_lit = court_lit)
        tot += sh["total"]
        lit += sh["lit"]
        rooms.update(sh["rooms"])
    return {"dark": tot - lit, "share": lit / tot if tot > 0 else 1.0, "rooms": rooms, "court_lit": court_lit}

def plan_of(m, sol, extra = None):
    """The solution as a plan value (the @cadpole/plan format), with the scorecard in meta["plan_parti"]."""
    t = m["t"]
    ext = _ext(m, sol)
    mt = metrics(ext)
    pattern_terms = [(x[0], x[1], _val(sol, x[2])) for x in m["pattern_terms"]]
    wish_terms = [(x[0], x[1], _val(sol, x[2])) for x in m["wish_terms"]]
    rule_terms = [(x[0], x[1], _val(sol, x[2])) for x in m["rule_terms"]]
    binding = {}
    for nm in sol.binding():
        fam = nm.split(":")[0]
        if fam in ("def", "sym") or nm.startswith("constraint_"):
            continue
        binding.setdefault(fam, []).append(nm)
    plan = {
        "name": t["name"],
        "unit": "ft",
        "source": "plan_parti:" + t["key"],
        "site": _site_of(t["site"]),
        "floors": _plan_floors(ext),
        "rooms": _plan_rooms(ext),
        "doors": _plan_doors(ext),
        "windows": _plan_windows(ext),
        "fixtures": [],
        "furniture": [],
        "stacks": _plan_stacks(ext),
        "courts": _plan_courts(ext),
        "walls": [],
        "bearing": _plan_bearing(ext, t),
        "meta": {},
    }
    card = {
        "template": t["key"], "name": t["name"], "idea": t.get("idea", ""), "precedent": t.get("precedent", ""),
        "joists": t.get("joists", ""), "variant": t.get("variant", []),
        "status": sol.status, "objective": sol.objective, "kernel": _val(sol, m["kernel"]),
        "pattern_terms": pattern_terms, "wish_terms": wish_terms, "rule_terms": rule_terms,
        "gap": sol.gap, "bound": sol.bound, "nodes": sol.nodes, "stats": _stats(m), "binaries": m["n"]["binaries"],
        "depths": ext["L"], "column_depths": {"%s.%d" % (k[0], k[1]): v for k, v in ext["Lc"].items()},
        "deck": ext["deck"], "stair_v": ext["Ys"], "gross": ext["gross"], "built": ext["built"],
        "far_cap": m["K"]["FAR_CAP"], "metrics": mt, "character": character(ext, t.get("program_keys")),
        "binding": binding, "bays": [t["bay"]] if t.get("bay") else [],
        "rooms": [{"key": r["key"], "floor": r["floor"], "span": r["span"], "u0": r["u0"], "u1": r["u1"],
                   "v0": r["y"], "v1": r["y"] + r["d"], "area": r["area"], "sources": sources(ext, r)}
                  for r in ext["rooms"]],
        "doors_kernel": ext["doors"],
        "frame": {"x0": m["K"]["X0"], "y0": m["K"]["Y0"], "note": "kernel u = x - x0, v = y - y0"},
    }
    if extra:
        card.update(extra)
    plan["meta"]["plan_parti"] = card
    plan = normalize(plan)  # @cadpole/plan.star's defaults, as its constructors fill them
    card = plan["meta"]["plan_parti"]
    ml = measured_light(plan)
    card["light"] = {"dark_sqft": ml["dark"], "share": ml["share"], "court_lit": ml["court_lit"],
                     "dark_rooms": {k: v[0] - v[1] for k, v in ml["rooms"].items() if v[0] - v[1] > 1.0},
                     "how": "plan_light.lit_share, court windows lit to SKY_TABLE's depth; uncalibrated"}
    return plan

_SLIM_DROP = ["doors_kernel", "precheck", "frame", "idea", "precedent", "problem", "rooms"]

def _round(x, depth = 0):
    t = type(x)
    if t == "float":
        return float(format(x, ".4f"))
    if t == "list":
        return [_round(y, depth + 1) for y in x]
    if t == "tuple":
        return tuple([_round(y, depth + 1) for y in x])
    if t == "dict":
        out = {}
        for k, v in x.items():
            out[k] = _round(v, depth + 1)
        return out
    return x

def slim(plan):
    """A fit's plan value to store as data: the geometry and the scorecard, every length rounded to 1/10,000 ft, and
    the bulky working records (the kernel-frame doors, the precheck rows, the door-graph depths) left out. A stored
    plan is geometry, never solver values: re-fit to change it."""
    out = dict(plan)
    card = dict(plan["meta"]["plan_parti"])
    for k in _SLIM_DROP:
        card.pop(k, None)
    if "metrics" in card:
        mt = dict(card["metrics"])
        mt.pop("door_depth", None)
        card["metrics"] = mt
    if "binding" in card:
        card["binding"] = {k: len(v) for k, v in card["binding"].items()}
    out["meta"] = dict(plan["meta"], plan_parti = card)
    return _round(out)

def _no_plan(t, status, rows, extra = None):
    card = {"template": t["key"], "name": t["name"], "status": status, "objective": None, "precheck": rows,
            "variant": t.get("variant", [])}
    if extra:
        card.update(extra)
    return {"name": t["name"], "unit": "ft", "source": "plan_parti:" + t["key"], "site": _site_of(t["site"]),
            "floors": [], "rooms": [], "doors": [], "windows": [], "fixtures": [], "furniture": [], "stacks": [],
            "courts": [], "walls": [], "bearing": [], "meta": {"plan_parti": card}}

# ------------------------------------------------------------------ fit

def solve(p, time_limit = None, gap = None, max_nodes = None, relax = None, objective = True, fix = None, drop = None):
    """p.solve() with only the options given (the builder refuses an explicit None for some of them)."""
    kw = {}
    if drop:
        kw["drop"] = drop
    if time_limit != None:
        kw["time_limit"] = time_limit
    if gap != None:
        kw["gap"] = gap
    if max_nodes != None:
        kw["max_nodes"] = max_nodes
    if relax:
        kw["relax"] = relax
    if not objective:
        kw["objective"] = False
    if fix:
        kw["fix"] = fix
    return p.solve(**kw)

def fit(template, site = None, program = None, rules = None, time_limit = None, gap = 1e-6, name = None,
        patterns = True, options = None, check = True, max_nodes = None, fit_rows = False, light = "assumed"):
    """Fit `template` to `site` with `program`: one MILP over every floor, solved to `gap` (1e-6 by default: proven
    optimal for these plans' scores). `rules`: @cadpole/plan_rules.star rules to add (the vetted menu); None adds none,
    which is the kernel alone. Returns a plan value; its meta["plan_parti"] holds the status, the
    objective and kernel score, the pattern terms, the depths, the gross area, the measures, each program room's
    character and the binding rules by family. A template that fails the precheck, or a problem that is infeasible,
    returns a plan with no rooms and the reasons in meta["plan_parti"] ("precheck", "why"; explain() names the clash)."""
    t = resolve(template, site, program)
    pre = precheck(t, resolved = True)
    fatal = [x for x in pre if not x["ok"] and x["severity"] == "fail"]
    if fatal:
        return _no_plan(t, "precheck", pre)
    m = compile_template(t, patterns = patterns, name = name, options = options, rules = rules, fit_rows = fit_rows,
                         light = light)
    sol = solve(m["p"], time_limit = time_limit, gap = gap, max_nodes = max_nodes)
    grafts = {"fit_rows": m["fit_used"], "light": light, "well_lit": m["well_lit"]}
    if sol.objective == None:
        # no point: explain() says why (sol.conflict() on a MILP is a deletion filter over every rule, which
        # a fit should not run unasked)
        return _no_plan(t, sol.status, pre, {"problem": m["p"].id, "grafts": grafts,
                                             "why": "infeasible: explain() names the rule families that clash"
                                                    if sol.status == "infeasible" else sol.status})
    plan = plan_of(m, sol, {"precheck": pre, "problem": m["p"].id, "rules": [x["name"] for x in (rules or [])],
                            "grafts": grafts})
    if check and rules:
        plan["meta"]["plan_parti"]["redpen"] = check_rules(plan, rules)
    return plan

# ------------------------------------------------------------------ the precheck

def _row_out(key, ok, detail, rule, severity = "fail", floor = None, room = None):
    return {"key": key, "name": key, "ok": ok, "detail": detail, "rule": rule, "severity": severity, "floor": floor,
            "room": room}

def precheck(template, site = None, program = None, resolved = False):
    """Before any MILP: can every room take a span, reach a door, daylight and a stack, and is every column taken?
    Returns check rows (key, name, ok, detail, rule, severity, floor, room); a failed row is a template that cannot
    fit, said in its own terms. These are the conditions the compiler would otherwise stop on, plus the ones that
    make a MILP infeasible for a reason a person can read."""
    t = template if resolved else resolve(template, site, program)
    K = _constants(t["site"], None)
    g = grid(t["lines"], K["W"], K["P"])
    rows = []
    keys = {}
    for r in t["rooms"]:
        keys[r["key"]] = r
    bearing = {}
    for b in t.get("bearing", []):
        bearing[b] = True
    for k in t.get("unplaced", []):
        rows.append(_row_out("place:" + k, False, "the template has no place for %s" % k, "program"))
    for k in t.get("orphans", []):
        rows.append(_row_out("door:" + k, False, "%s: every room it may open off is gone" % k, "access", room = k))
    floors = list(t["floors"])
    spans_of = {}
    for r in t["rooms"]:
        k, f = r["key"], r["floor"]
        if f not in floors:
            rows.append(_row_out("floor:" + k, False, "%s is on floor %s, which the template does not have" % (k, f),
                                 "template", room = k))
            continue
        sp = []
        for s in r["spans"]:
            w = _width(g, s)
            if w < r["wmin"] - 1e-9:
                continue
            if [x for x in range(s[0] + 1, s[1] + 1) if x in bearing]:
                continue
            if r["mind"] != None and _need_d(r, w) == None:
                continue
            sp.append(s)
        spans_of[k] = sp
        if not sp:
            rows.append(_row_out("span:" + k, False, "%s: no span is wide enough (spans %s; least width %s)" %
                                 (k, r["spans"], ftin(r["wmin"])), "program", floor = f, room = k))
            continue
        dhi = min(r["d"][1], K["LMAX"])
        amax = max([_width(g, s) * dhi for s in sp])
        amin = min([_width(g, s) * r["d"][0] for s in sp])
        lo, hi = r["area"]
        if amax < lo - 1e-6 and not r.get("kept"):
            rows.append(_row_out("area:" + k, False, "%s: %s is the most any span gives; it needs %s" %
                                 (k, sqft(amax), sqft(lo)), "program", floor = f, room = k))
        if amin > hi + 1e-6 and not r.get("kept"):
            rows.append(_row_out("area:" + k, False, "%s: %s is the least any span gives; it may have %s" %
                                 (k, sqft(amin), sqft(hi)), "program", floor = f, room = k))
        if r["wet"]:
            if not [s for s in sp for kl in t["stacks"] if s[0] == kl or s[1] + 1 == kl]:
                rows.append(_row_out("stack:" + k, False, "%s is wet, but no stack line is an edge of any of its spans" % k,
                                     "stack", floor = f, room = k))
    for f in floors:
        for c in range(g["C"]):
            if not [r for r in t["rooms"] if r["floor"] == f and r["key"] in spans_of and
                    [s for s in spans_of[r["key"]] if s[0] <= c and c <= s[1]]]:
                rows.append(_row_out("column:%s:%d" % (f, c), False, "floor %s, column %d: no room may take it" % (f, c),
                                     "tiling", floor = f))
    if t["stair"] != None:
        for f in floors:
            if "stair" + f not in keys:
                rows.append(_row_out("stair:" + f, False, "floor %s has no stair" % f, "stair", floor = f))
    for w in t["wells"]:
        for f in w["floors"]:
            if w["pre"] + f not in keys:
                rows.append(_row_out("well:%s:%s" % (w["pre"], f), False, "well %s has no room on floor %s" % (w["pre"], f),
                                     "well", floor = f))
    # doors: each room has a parent that can touch it
    for r in t["rooms"]:
        k, f = r["key"], r["floor"]
        if r["role"] in ("stair", "void") or k not in spans_of or not spans_of[k] or k in t.get("orphans", []):
            continue
        ok = False
        for pk in r["parents"]:
            if pk == "ROOT":
                ok = True
            elif pk == "stair.top":
                ok = ok or f != floors[0]
            elif pk in spans_of and keys[pk]["floor"] == f and spans_of[pk]:
                ok = ok or _could_touch(spans_of[k], spans_of[pk])
            elif pk not in keys:
                rows.append(_row_out("parent:%s:%s" % (k, pk), False, "%s opens off %s, which is not in the plan" % (k, pk),
                                     "access", floor = f, room = k))
        if not ok:
            rows.append(_row_out("door:" + k, False, "%s: no room it may open off can touch it" % k, "access",
                                 floor = f, room = k))
    # light: a room that must be daylit can reach the front, the rear or a well in some span
    for r in t["rooms"]:
        k, f = r["key"], r["floor"]
        if k not in spans_of or not spans_of[k] or r.get("kept"):
            continue
        if not (r["lit"] or r["hall"]):
            continue
        if r["lit"] and r["d"][0] > K["lit"] and not t["wells"]:
            rows.append(_row_out("light:" + k, False, "%s must be at least %s deep, past 2H (%s) of any window" %
                                 (k, ftin(r["d"][0]), ftin(K["lit"])), "light", floor = f, room = k))
        sources_ = _light_reach(t, keys, spans_of, r)
        if not sources_:
            rows.append(_row_out("light:" + k, False, "%s: in every span it may take, rooms pinned to the street and the "
                                 % k + "garden (or the template's orders) stand between it and any glass, and no well "
                                 + "is on its floor", "light", floor = f, room = k))
    return rows

def _could_touch(sa, sb):
    for a in sa:
        for b in sb:
            if max(a[0], b[0]) <= min(a[1], b[1]):
                return True
            if a[0] == b[1] + 1 or b[0] == a[1] + 1:
                return True
    return False

def _light_reach(t, keys, spans_of, r):
    """Which sources room r could reach in some span: the street unless a room the template orders in front of it is
    always built, or a column of the span always holds a room pinned to the street; the garden likewise; a well on its
    floor; borrowed light. Never claims a source is out of reach when it is not."""
    k, f = r["key"], r["floor"]
    out = []
    pinned_front = {}
    pinned_rear = {}
    for q in t["rooms"]:
        if q["floor"] != f or q["key"] == k or q["optional"] or q["key"] not in spans_of or not spans_of[q["key"]]:
            continue
        sq = spans_of[q["key"]]
        cols = None
        for s in sq:
            cs = {}
            for c in range(s[0], s[1] + 1):
                cs[c] = True
            cols = cs if cols == None else {c: True for c in cols if c in cs}
        if q["front"]:
            for c in (cols or {}):
                pinned_front[c] = True
        if q["rear"]:
            for c in (cols or {}):
                pinned_rear[c] = True
    # a template's orders hold whatever columns the rooms take: a room behind an always-built room never reaches
    # the street, one in front of an always-built room never reaches the garden
    present = {}
    for q in t["rooms"]:
        if q["floor"] == f and not q["optional"] and q["key"] in spans_of and spans_of[q["key"]]:
            present[q["key"]] = True
    no_front = len([o for o in t["orders"] if o[0] == f and o[2] == k and o[1] in present]) > 0
    no_rear = len([o for o in t["orders"] if o[0] == f and o[1] == k and o[2] in present]) > 0
    for s in spans_of[k]:
        cs = range(s[0], s[1] + 1)
        if r["front"] or (not no_front and not [c for c in cs if c in pinned_front]):
            out.append("front")
        if r["rear"] or (not no_rear and not [c for c in cs if c in pinned_rear]):
            out.append("rear")
    if [w for w in t["wells"] if f in w["floors"]]:
        out.append("well")
    if r.get("borrow"):
        out.append("borrow")
    return out

# ------------------------------------------------------------------ compare, explain, author

def _mirrored(ch):
    """A room's character on the plan flipped east-west: its party walls swap."""
    w = ch[2]
    flip = ("W" if "E" in w else "") + ("E" if "W" in w else "")
    return (ch[0], ch[1], flip, ch[3], ch[4])

def compare(fits, keys = None):
    """Compare fits (plan values from fit()): each one's scorecard line, and for every pair how many program rooms
    differ in character (floor, depth third, party walls, light, what the door opens from) and how far their centres
    moved. Returns {"fits": [...], "pairs": [{"a", "b", "differ", "rooms", "moved"}], "best"}."""
    rows = []
    best = None
    for p in fits:
        c = p["meta"]["plan_parti"]
        if c.get("objective") != None and (best == None or c["kernel"] > best):
            best = c["kernel"]
    for p in fits:
        c = p["meta"]["plan_parti"]
        if c.get("objective") == None:
            rows.append({"template": c["template"], "status": c["status"]})
            continue
        mt = c["metrics"]
        rows.append({"template": c["template"], "name": c["name"], "status": c["status"], "kernel": c["kernel"],
                     "objective": c["objective"], "versus_best": c["kernel"] - best, "gross": c["gross"],
                     "two_sided": len(mt["two_sided"]), "dark_sqft_assumed": mt["dark_sqft_assumed"],
                     "kitchen_garden": mt["kitchen_garden"], "nodes": c["nodes"], "binaries": c["binaries"]})
    pairs = []
    for i in range(len(fits)):
        for j in range(i + 1, len(fits)):
            ci, cj = fits[i]["meta"]["plan_parti"], fits[j]["meta"]["plan_parti"]
            if ci.get("objective") == None or cj.get("objective") == None:
                continue
            a, b = ci["character"], cj["character"]
            ks = keys or [k for k in PROGRAM_KEYS if k in a and k in b]
            diff = [k for k in ks if k in a and k in b and a[k] != b[k]]
            mdiff = [k for k in ks if k in a and k in b and a[k] != _mirrored(b[k])]
            mirror_of = len(mdiff) == 0 and len(diff) > 0
            if mirror_of:
                diff = mdiff  # one is the other's mirror image: they count as one plan
            ra = {}
            for r in fits[i]["rooms"]:
                ra[r["key"]] = r["rects"][0]
            moved = 0.0
            for r in fits[j]["rooms"]:
                q = ra.get(r["key"])
                if q == None or r["key"] not in ks:
                    continue
                b = r["rects"][0]
                du = 0.5 * (b[0] + b[1]) - 0.5 * (q[0] + q[1])
                dv = 0.5 * (b[2] + b[3]) - 0.5 * (q[2] + q[3])
                moved += math.sqrt(du * du + dv * dv)
            pairs.append({"a": ci["template"], "b": cj["template"], "differ": len(diff), "of": len(ks), "rooms": diff,
                          "moved": moved, "mirrored": mirror_of})
    return {"fits": rows, "pairs": pairs, "best": best}

def red_pen(plan, rules = None, audit_rows = True):
    """A fit's red pen, as check rows for plan_draw: the audit's marks (doors, stacks, fillers, code rows -- without
    its furniture re-pack and measured sky, which a document can run itself) and the failing checks of `rules`
    (@cadpole/plan_rules.star; the standing set by default, less the pantry rule the audit already marks)."""
    rows = []
    if audit_rows and plan.get("rooms"):
        rows.extend(audit(plan, furnish = False, light = False)["marks"])
    for r in check_rules(plan, rules if rules != None else standard()):
        if not r["ok"] and not (audit_rows and r["rule"] == "pantry_by_kitchen"):
            rows.append(r)
    return rows

def scorecard_row(plan, best = None, checks = None):
    """A fit's row on a comparison sheet: score (and its distance from the best), gross area, two-sided rooms, dark
    floor measured (@cadpole/plan_light.star) and as assumed, the red pen's count, and the proof."""
    c = plan["meta"]["plan_parti"]
    if c.get("objective") == None:
        return [("score", c.get("status", "-"))]
    fails = len([r for r in (checks or []) if not r["ok"] and r.get("severity", "fail") != "note"])
    vs = "" if best == None or c["kernel"] >= best - 0.5 else " (%s)" % format(c["kernel"] - best, "+,.0f")
    return [
        ("score", format(c["kernel"], ",.0f") + vs),
        ("gross sq ft", format(c["gross"], ",.0f")),
        ("two-sided rooms", str(len(c["metrics"]["two_sided"]))),
        ("dark sq ft, measured", format(c["light"]["dark_sqft"], ",.0f")),
        ("dark, if courts lit 12 ft", format(c["metrics"]["dark_sqft_assumed"], ",.0f")),
        ("red pen", str(fails) if checks != None else "-"),
        ("proof", "%s, %s nodes" % (c["status"], format(c["nodes"], ",d"))),
    ]

def partis_sheet(S, fits, labels = None, rules = None, audit_rows = True, number = "C1.0", name = "PARTIS"):
    """One comparison sheet of fits in sheet set S (plan_draw.compare_sheet): every floor of each fit side by side at
    one scale, a scorecard row each and its red pen. Returns {"sheet", "checks", "compare"}."""
    keep = [i for i, f in enumerate(fits) if f["meta"]["plan_parti"].get("objective") != None]
    if not keep:
        fail("plan_parti.partis_sheet: none of the fits has a plan")
    ok = [fits[i] for i in keep]  # a fit with no plan (precheck, infeasible) has nothing to draw
    cmp = compare(ok)
    checks = [red_pen(f, rules, audit_rows) for f in ok]
    cards = [scorecard_row(f, cmp["best"], checks[i]) for i, f in enumerate(ok)]
    names = [labels[i] for i in keep] if labels != None else [f["meta"]["plan_parti"]["name"] for f in ok]
    sh = compare_sheet(S, ok, number = number, name = name, scorecards = cards, checks = checks, labels = names)
    # what the marks mean, above the scorecard table
    x0, y0 = S.area()[0], S.area()[1]
    sh.text("Numbered rings: each plan's red pen (red: fails, orange: warnings). Purple outlines: rooms placed " +
            "differently from the first plan's. Score: the program's value less the cost of what is built.",
            at = (x0 + 8.0, y0 + 8.0 + 4.6 * (len(ok) + 1) + 7.0), size = 2.2, color = "#6f6a62")
    return {"sheet": sh, "checks": checks, "compare": cmp}

def massing(plan, name = None, at = (0.0, 0.0), gap = 0.25):
    """A plan value as blocks, to look at in the viewer: each indoor room a box from its floor up to its ceiling, inset
    `gap` ft (3 in) so the partitions read, coloured by kind as plan_draw fills it; a deck a 6-in slab; courts left
    open. `at` (feet) moves it on the site. Bodies are in mm, named "plan_parti_<name>_<room>"; returns them."""
    nm = "plan_parti_" + (name or str(plan.get("name", "plan")).replace(" ", "_").replace(",", ""))
    zs = {}
    ceil = {}
    for f in plan.get("floors", []):
        zs[f["key"]] = f.get("z", 0.0)
        ceil[f["key"]] = f.get("ceiling") or 9.0
    out = []
    for r in plan.get("rooms", []):
        kind = r.get("kind")
        if kind in ("court", "well"):
            continue
        q = r["rects"][0]
        w, d = q[1] - q[0] - 2 * gap, q[3] - q[2] - 2 * gap
        if w <= 0.0 or d <= 0.0:
            continue
        h = 0.5 if kind == "deck" else (r.get("ceiling") or ceil.get(r["floor"], 9.0))
        b = box(_mm(w), _mm(d), _mm(h), at = (_mm(q[0] + gap + at[0]), _mm(q[2] + gap + at[1]), _mm(zs.get(r["floor"], 0.0))),
                name = "%s_%s" % (nm, r["key"]))
        b.material(color = kind_fill(kind))
        out.append(b)
    return out

def _group_rows(m, g):
    """The names of a rule group: a family ("light") or a family on one floor ("light:U")."""
    if ":" in g:
        return [n for n in m["rows"].get(g.split(":")[0], []) if n.startswith(g + ":")]
    return list(m["rows"].get(g, []))

def _groups_rows(m, groups):
    out = []
    for g in groups:
        out.extend(_group_rows(m, g))
    return out

# Families that are never a choice to drop: the model's definitions, symmetry, and tiling (no holes, no overlaps).
NEVER_DROP = ["def", "sym", "tiling"]

# A feasibility probe (no objective) cannot be unbounded, so HiGHS's "infeasible_or_unbounded" is infeasible.
_INFEASIBLE = ("infeasible", "infeasible_or_unbounded")

def _selector(groups):
    """The tag selector for rule groups ("light", "light:U"): the rules named "<family>:<floor>:..." carry family
    and floor tags."""
    out = []
    for g in groups:
        if ":" in g:
            out.append({"family": g.split(":")[0], "floor": g.split(":")[1]})
        else:
            out.append({"family": g})
    return out

def explain_model(m, families = None, by_floor = True, cheapest = True, time_limit = None, max_nodes = None,
                  engine = None):
    """explain() on a compiled model. engine "starlark" (the default): the filter here, by family and then by (family,
    floor), each probe a feasibility solve that leaves the dropped groups out (`drop=` by tag), over the candidate
    `families` when given; the model's own definitions and symmetry rows are never candidates. engine "rust": one
    infeasible solve, then the MILP deletion filter (`sol.conflict()`: whole families first, the untagged rows --
    here the definitions and symmetry rows -- as one family more, then halves of the families kept, down to single
    rules), read back by family and floor from the rule names; with by_floor=False it stops after the families
    (`families_only=True`). The probes take the first solve's limits, and the evaluation's budget bounds the search.
    Either way an irreducible set; where there is a choice the two may name different ones. A probe that a limit
    stopped before it answered keeps its group in -- the set still cannot all hold, but may not be irreducible -- and
    is counted in "undecided"."""
    p = m["p"]
    eng = engine or "starlark"
    n = [0]
    undecided = [0]

    def status(groups, objective = False):
        n[0] += 1
        return solve(p, drop = _selector(groups), objective = objective, time_limit = time_limit, max_nodes = max_nodes)

    def clash(groups):
        """Still infeasible with these groups left out? A probe stopped with no point and no verdict counts as no."""
        s = status(groups)
        if s.status in _INFEASIBLE:
            return True
        if not s.feasible:
            undecided[0] += 1
        return False

    first = status([])
    if first.status not in _INFEASIBLE:
        return {"feasible": first.status, "families": [], "groups": [], "cheapest": None, "solves": n[0], "engine": eng}
    rules = []
    untagged = 0
    if eng == "rust":
        found = first.conflict(families_only = True) if not by_floor else first.conflict()
        keep, groups = [], []
        for x in found:
            parts = x.split(":")
            if len(parts) < 4 or parts[0] in ("def", "sym"):
                untagged += 1  # the untagged family: the model's definitions and symmetry rows
                continue
            rules.append(x)
            if parts[0] not in keep:
                keep.append(parts[0])
            if by_floor and parts[0] + ":" + parts[1] not in groups:
                groups.append(parts[0] + ":" + parts[1])
    else:
        fams = [f for f in (families or [x for x in m["rows"].keys() if x not in ("def", "sym")]) if m["rows"].get(f)]
        keep, dropped = [], []
        for f in fams:
            if clash(dropped + [f]):
                dropped.append(f)  # still infeasible without it: not part of the clash
            else:
                keep.append(f)
        groups = []
        if by_floor:
            gdrop = list(dropped)
            floors = list(m["FL"]) + ["all"]
            cand = [f + ":" + fl for f in keep for fl in floors if _group_rows(m, f + ":" + fl)]
            for g in cand:
                if clash(gdrop + [g]):
                    gdrop.append(g)
                else:
                    groups.append(g)
    best = None
    tried = []
    if cheapest:
        for g in (groups or keep):
            if g.split(":")[0] in NEVER_DROP:
                continue
            s = status([g], objective = True)
            tried.append((g, s.status, s.objective))
            if s.objective != None and (best == None or s.objective > best["objective"]):
                best = {"group": g, "objective": s.objective, "status": s.status, "rules": len(_group_rows(m, g))}
    return {"feasible": "infeasible", "families": keep, "groups": groups, "rules": rules, "untagged": untagged,
            "undecided": undecided[0], "cheapest": best, "tried": tried, "solves": n[0], "engine": eng}

def explain(template, site = None, program = None, rules = None, families = None, by_floor = True, name = None,
            options = None, time_limit = None, cheapest = True, max_nodes = None, fit_rows = False, light = "assumed",
            engine = None):
    """Why a template does not fit: a deletion filter over rule families, then over (family, floor) groups, on one
    compiled problem -- each probe a feasibility solve that leaves a group's rules out (`drop=`, by row tags;
    engine="rust" runs the solver's own filter, `sol.conflict()`, instead). Returns {"feasible", "families": an irreducible
    set of families that clash, "groups": the (family, floor) groups within them, "cheapest": the group whose rules
    dropped alone leave the best plan, with its objective, "solves", "engine", "undecided": probes a limit stopped
    before they answered, whose groups stay in (with any, the set may not be irreducible)}; a template the precheck rejects
    returns its rows instead. The problem is named `name` (default "plan_parti_x_<template>")."""
    t = resolve(template, site, program)
    pre = precheck(t, resolved = True)
    fatal = [x for x in pre if not x["ok"] and x["severity"] == "fail"]
    if fatal:  # the template cannot even be compiled: the precheck says why, in its own terms
        return {"feasible": "precheck", "families": [], "groups": [], "cheapest": None, "solves": 0, "precheck": fatal}
    m = compile_template(t, name = name or ("plan_parti_x_" + t["key"]), options = options, rules = rules,
                         fit_rows = fit_rows, light = light)
    out = explain_model(m, families, by_floor, cheapest, time_limit, max_nodes, engine)
    out["problem"] = m["p"].id
    return out

def author(template, site = None, program = None, rules = None, name = None, time_limit = None, explain_ = True,
           max_nodes = None, fit_rows = False, light = "assumed"):
    """What drafting a template needs: pre-check each room, compile, test feasibility, and when it is infeasible run
    the deletion filter by family and floor. Returns {"precheck": rows, "feasible": status, "stats", "explain":
    explain()'s answer or None}. A precheck failure stops before any MILP."""
    t = resolve(template, site, program)
    pre = precheck(t, resolved = True)
    fatal = [x for x in pre if not x["ok"] and x["severity"] == "fail"]
    if fatal:
        return {"precheck": pre, "feasible": "precheck", "stats": None, "explain": None}
    m = compile_template(t, name = name or ("plan_parti_a_" + t["key"]), rules = rules, fit_rows = fit_rows, light = light)
    s = solve(m["p"], objective = False, time_limit = time_limit, max_nodes = max_nodes)
    out = {"precheck": pre, "feasible": s.status, "stats": _stats(m), "explain": None, "problem": m["p"].id}
    if s.status == "infeasible" and explain_:
        out["explain"] = explain_model(m, cheapest = False, time_limit = time_limit, max_nodes = max_nodes)
    return out

def options(template, knobs = None, site = None, program = None, rules = None, name = None, time_limit = None,
            gap = 1e-6, max_nodes = None, fit_rows = False, light = "assumed"):
    """Options by template and knob, never by raw no-good cuts: the template's fit, then one fit per knob setting
    ((knob name, value) pairs, or names meaning True; plan_templates.knob turns them), and their comparison.
    Returns {"fits": [...], "compare": compare(fits)}."""
    out = [fit(template, site, program, rules = rules, time_limit = time_limit, gap = gap, max_nodes = max_nodes,
               name = (name or ("plan_parti_" + template["key"])) + "_base", fit_rows = fit_rows, light = light)]
    for i, k in enumerate(knobs or []):
        kn, kv = (k, True) if type(k) == "string" else (k[0], k[1])
        t2 = template["knobs"][kn](template, kv)
        out.append(fit(t2, site, program, rules = rules, time_limit = time_limit, gap = gap, max_nodes = max_nodes,
                       name = (name or ("plan_parti_" + template["key"])) + "_k%d" % (i + 1), fit_rows = fit_rows,
                       light = light))
    return {"fits": out, "compare": compare(out)}
