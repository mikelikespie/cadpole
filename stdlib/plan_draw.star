"""@cadpole/plan_draw.star -- a plan value drawn on Cadpole's sheet API, floor by floor, with a red pen.

    load("@cadpole/plan_draw.star", "plan_set", "plan_sheets", "compare_sheet", "draw_floor", "floor_layout", "paint",
         "redpen", "kind_fill", "STYLE")

    S = plan_set(p, checks=rows)                 # a sheet set: one sheet per floor (or two to a sheet), red pen on
    compare_sheet(S, [p, q], scorecards=[...])   # N plans side by side, every floor, a scorecard row each
    export(S, "plans.pdf")

Every floor is drawn the same way, in layers: courts and decks; the walls, derived from the rooms (the floor's indoor
rooms grown by `wall_t`, so the gaps between rooms read as partitions) or the plan's own `walls` when it has any on
the floor; the rooms filled by kind; stairs hatched and lifts crossed; door openings cut, with leaves and swing arcs
(slides, pockets and cased openings drawn as such); windows across the exterior wall (egress windows in blue,
skylights dashed); fixtures, furniture, stacks and bearing lines; the floor's overall dimensions; one label per room
-- its name, sq ft, and W x D in feet-inches -- placed clear of every other label, mark and fixture, shrunk or turned
upright to fit, or set outside the floor with a leader when nothing inside is free; and last the red pen: each
failing check row with an `at` or a `rect` is a numbered ring (and a dashed outline for a
rect), and the sheet lists the rows by number.

floor_layout() does the layout without a sheet and returns what it placed (for tests: labels, overlaps, marks);
paint() puts a layout on a sheet; draw_floor() is both. Lengths in the plan are feet; on paper, millimetres. The
module records nothing when loaded; its purity probe is `kind_fill`.
"""

load(
    "@cadpole/plan.star",
    "KIND_GROUPS",
    "area",
    "plan_diff",
    "courts_on",
    "exterior_walls",
    "floors_by_z",
    "ftin",
    "get_room",
    "is_outside",
    "kind_group",
    "rooms_on",
    "shared_walls",
    "sqft",
    "validate",
)

# ---------------------------------------------------------------------------------------------------------- style

STYLE = {
    "ink": "#2b2a28",
    "mute": "#6f6a62",
    "faint": "#b8b2a6",
    "paper": "#ffffff",
    "wall": "#4a4540",
    "wall_kind": {"exterior": "#4a4540", "partition": "#6d665e", "bearing": "#3b3631", "concrete": "#8e8a84", "party": "#3b3631", "fire": "#7a3b2e"},
    "opening": "#f4f1ea",
    "glass": "#5b6f80",
    "egress": "#1f6fd1",
    "court": "#e4efe9",
    "court_edge": "#6f9c86",
    "stack": "#1f4e8c",
    "bearing": "#8a4b2a",
    "fixture": "#ffffff",
    "fixture_edge": "#5d6670",
    "furniture_edge": "#9a948a",
    "dim": "#4d4943",
    "red": "#c62828",
    "accent": "#7b3fbf",
    "warn": "#d97706",
    "halo_pad": 0.5,
    "wall_t": 0.5,  # ft: the exterior wall drawn round derived rooms, and a window's depth
}

KIND_FILL = {
    "living": "#f7ebd3",
    "dining": "#f7e4c6",
    "family": "#f5e6cc",
    "den": "#f1e6d6",
    "study": "#efe5d2",
    "office": "#efe5d2",
    "play": "#f8ead0",
    "media": "#eee2cf",
    "guest": "#e9e0f0",
    "bed": "#e6dff0",
    "kitchen": "#f6dcc0",
    "bath": "#d7e7f3",
    "powder": "#dbe9f3",
    "wc": "#dbe9f3",
    "laundry": "#dcebee",
    "pantry": "#efe6d6",
    "hall": "#eeeeec",
    "entry": "#ebe9e4",
    "foyer": "#ebe9e4",
    "mud": "#e7e5e0",
    "stair": "#dcdad6",
    "lift": "#d4d2ce",
    "landing": "#e8e6e2",
    "lobby": "#e8e6e2",
    "closet": "#e9e4da",
    "storage": "#e6e0d5",
    "mech": "#e0dbd2",
    "garage": "#e1e1df",
    "workshop": "#e3ddd2",
    "deck": "#e0ecd5",
    "court": "#e4efe9",
    "well": "#e4efe9",
    "other": "#f0f0ee",
}

GROUP_NAMES = {"living": "living and bedrooms", "wet": "kitchen, baths, laundry", "circulation": "halls, stairs, lift",
               "storage": "storage, mech, garage", "outdoor": "decks and courts", "other": "other"}

def kind_fill(kind):
    """The fill colour of a room kind (the module's purity probe)."""
    return KIND_FILL.get(kind, KIND_FILL["other"])

# the architect's scales, inches to the foot, largest first
_SCALES = [(1.0, "1\" = 1'-0\""), (0.75, "3/4\" = 1'-0\""), (0.5, "1/2\" = 1'-0\""), (0.375, "3/8\" = 1'-0\""),
           (0.25, "1/4\" = 1'-0\""), (0.1875, "3/16\" = 1'-0\""), (0.125, "1/8\" = 1'-0\""), (0.09375, "3/32\" = 1'-0\""),
           (0.0625, "1/16\" = 1'-0\""), (0.03125, "1/32\" = 1'-0\"")]

def scale_for(width_ft, height_ft, box_w, box_h, *, largest = 0.5):
    """The largest architect's scale (as (mm per ft, its name)) at which width_ft x height_ft fits box_w x box_h mm,
    no larger than `largest` inches to the foot; the smallest when nothing fits."""
    for inch, name in _SCALES:
        if inch > largest + 1e-9:
            continue
        s = inch * 25.4
        if width_ft * s <= box_w and height_ft * s <= box_h:
            return (s, name)
    return (_SCALES[-1][0] * 25.4, _SCALES[-1][1])

# ---------------------------------------------------------------------------------------------------------- small geometry

def _grow(r, t):
    return (r[0] - t, r[1] + t, r[2] - t, r[3] + t)

def _pts(r):
    return [(r[0], r[2]), (r[1], r[2]), (r[1], r[3]), (r[0], r[3])]

def _seam(a, b):
    """The strip where rect b touches rect a's east or north side (an L's two pieces), a hair wide, or None."""
    if abs(b[0] - a[1]) <= 0.05:
        lo, hi = max(a[2], b[2]), min(a[3], b[3])
        if hi > lo:
            return (a[1] - 0.08, a[1] + 0.08, lo, hi)
    if abs(b[2] - a[3]) <= 0.05:
        lo, hi = max(a[0], b[0]), min(a[1], b[1])
        if hi > lo:
            return (lo, hi, a[3] - 0.08, a[3] + 0.08)
    return None

def _gap_rect(a, b):
    """The distance between two rects (0 when they touch or overlap)."""
    du = max(0.0, a[0] - b[1], b[0] - a[1])
    dv = max(0.0, a[2] - b[3], b[2] - a[3])
    return math.sqrt(du * du + dv * dv)

def _box_overlap(a, b):
    return a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]

def _free(box, taken):
    for t in taken:
        if _box_overlap(box, t):
            return False
    return True

def _inside(box, lim):
    return box[0] >= lim[0] - 1e-6 and box[1] >= lim[1] - 1e-6 and box[2] <= lim[2] + 1e-6 and box[3] <= lim[3] + 1e-6

def _arc(c, r, a0, a1, n = 12):
    """Points on a circular arc from a0 to a1 degrees (Cadpole's math namespace works in degrees)."""
    out = []
    for k in range(n + 1):
        a = a0 + (a1 - a0) * k / float(n)
        out.append((c[0] + r * math.cos(a), c[1] + r * math.sin(a)))
    return out

def _ring(c, r, n = 24):
    return _arc(c, r, 0.0, 360.0, n)

def _tw(s, size, bold = False):
    return text_width(s, size = size, bold = bold)

def _norm(s):
    return {"N": (0.0, 1.0), "S": (0.0, -1.0), "E": (1.0, 0.0), "W": (-1.0, 0.0)}[s]

def _tan(s):
    return (1.0, 0.0) if (s == "N" or s == "S") else (0.0, 1.0)

def _add(p, q, k = 1.0):
    return (p[0] + q[0] * k, p[1] + q[1] * k)

# ---------------------------------------------------------------------------------------------------------- the red pen

def redpen(checks, *, notes = False):
    """The failing check rows, numbered from 1 in the order given: [(n, row)]. Rows with ok True are left out, and
    rows of severity "note" unless notes=True."""
    out = []
    n = 0
    for r in checks or []:
        if r.get("ok", False):
            continue
        if r.get("severity", "fail") == "note" and not notes:
            continue
        n += 1
        out.append((n, r))
    return out

def _mark_point(p, row):
    if row.get("at") != None:
        return row["at"]
    if row.get("rect") != None:
        r = row["rect"]
        return ((r[0] + r[1]) / 2.0, (r[2] + r[3]) / 2.0)
    if row.get("room") != None:
        for rm in p.get("rooms", []):
            if rm.get("key") == row["room"]:
                m = rm["rects"][0]
                return ((m[0] + m[1]) / 2.0, (m[2] + m[3]) / 2.0)
    return None

# ---------------------------------------------------------------------------------------------------------- layout

def _sizes(s):
    name = min(2.6, max(1.5, 0.42 * s))
    return {"name": name, "sub": name * 0.8, "dim": max(1.4, name * 0.85), "mark": max(1.6, name * 0.9), "court": name * 0.75, "fix": max(1.0, name * 0.55)}

def floor_layout(
        p,
        floor,
        *,
        scale = 6.35,
        checks = None,
        numbers = None,
        dims = True,
        labels = True,
        fixtures = True,
        furniture = True,
        details = True,
        highlight = None,
        style = None):
    """Lay out one floor of plan p at `scale` paper mm to the foot (6.35 is 1/4" = 1'-0"), without a sheet.

    checks are check rows; the failing ones on this floor become numbered red-pen marks, numbered
    as redpen(checks) numbers them (or by `numbers`, a list of (n, row) from redpen over a wider list). details=False
    leaves out the area and size lines of the labels and the fixtures, for small drawings. highlight: room keys to
    outline in the accent colour (the comparison sheet's "changed from the first plan"). Returns a dict:
    w, h (paper mm), s (mm per ft), origin and margin (plan point (u, v) lands on paper at margin + (q - origin) * s,
    from the layout's lower-left), bbox (the floor's extent in plan feet, outside faces), shapes and texts (for paint),
    labels (room key -> its box and how it was placed: "inside", "shrunk", "upright" or "outside"), overlaps (labels
    that found no free place), marks ([(n, row, paper point)]) and rows (the numbered rows on this floor)."""
    st = dict(STYLE)
    st.update(style or {})
    s = float(scale)
    sz = _sizes(s)
    t = st["wall_t"]
    rooms = rooms_on(p, floor)
    indoor = [r for r in rooms if kind_group(r.get("kind")) != "outdoor"]
    outdoor = [r for r in rooms if kind_group(r.get("kind")) == "outdoor"]
    near = [x for r in rooms for x in r["rects"]]
    # a court open at this floor is drawn when it comes within 5 ft of one of its rooms (a court beside another
    # building on the lot is not this floor's)
    cts = [c for c in courts_on(p, floor) if [x for x in near if _gap_rect(c["rect"], x) <= 5.0]]
    rects = [_grow(x, t) for r in indoor for x in r["rects"]] + [x for r in outdoor for x in r["rects"]] + [c["rect"] for c in cts]
    if not rects:
        fail("plan_draw.floor_layout: floor %s has no rooms" % floor)
    bx = (min([r[0] for r in rects]), max([r[1] for r in rects]), min([r[2] for r in rects]), max([r[3] for r in rects]))

    # margins: the dimensions on the left and the bottom, room for labels set outside on the right and the left
    dim_m = (sz["dim"] * 2.0 + 7.0) if dims else 3.0
    side = 3.0
    ml, mb, mr, mt = dim_m + side, dim_m, side, 3.0
    origin_mm = (ml, mb)

    def P(q):
        return (origin_mm[0] + (q[0] - bx[0]) * s, origin_mm[1] + (q[1] - bx[2]) * s)

    def R(r):
        a = P((r[0], r[2]))
        b = P((r[1], r[3]))
        return (a[0], a[1], b[0], b[1])

    shapes = []
    texts = []

    def fillr(z, r, color):
        b = R(r)
        shapes.append({"z": z, "kind": "fill", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])], "color": color})

    def line(z, pts, w, color, dash = None):
        shapes.append({"z": z, "kind": "poly", "pts": [P(q) for q in pts], "weight": w, "color": color, "dash": dash})

    # courts, wells and slots open at this floor
    for c in cts:
        fillr(10, c["rect"], st["court"])
        line(11, _pts(c["rect"]) + [_pts(c["rect"])[0]], 0.25, st["court_edge"], [1.6, 1.0])
    # decks and other outdoor rooms
    for r in outdoor:
        for x in r["rects"]:
            fillr(12, x, kind_fill(r["kind"]))
            line(13, _pts(x) + [_pts(x)[0]], 0.25, st["mute"], [1.2, 0.8])

    # walls: the plan's own, or derived from the rooms
    own = [w for w in p.get("walls", []) if w.get("floor") == floor]
    if own:
        for w in own:
            a, b = w["a"], w["b"]
            h = w.get("t", 0.5) / 2.0
            if abs(a[1] - b[1]) < 1e-9:
                r = (min(a[0], b[0]) - h, max(a[0], b[0]) + h, a[1] - h, a[1] + h)
            elif abs(a[0] - b[0]) < 1e-9:
                r = (a[0] - h, a[0] + h, min(a[1], b[1]) - h, max(a[1], b[1]) + h)
            else:
                dx, dy = b[0] - a[0], b[1] - a[1]
                L = math.sqrt(dx * dx + dy * dy)
                nx, ny = -dy / L * h, dx / L * h
                shapes.append({"z": 20, "kind": "fill", "pts": [P((a[0] + nx, a[1] + ny)), P((b[0] + nx, b[1] + ny)), P((b[0] - nx, b[1] - ny)), P((a[0] - nx, a[1] - ny))], "color": st["wall_kind"].get(w.get("kind"), st["wall"])})
                continue
            fillr(20, r, st["wall_kind"].get(w.get("kind"), st["wall"]))
    else:
        # the floor's plate (inside faces) is wall wherever no room is: thick walls and chases read solid
        fl = [f for f in p.get("floors", []) if f.get("key") == floor]
        for x in (fl[0].get("plate") or []) if fl else []:
            fillr(19, x, st["wall"])
        for r in indoor:
            for x in r["rects"]:
                fillr(20, _grow(x, t), st["wall"])

    # rooms by kind; stairs hatched across their run, lifts crossed
    for r in indoor:
        for x in r["rects"]:
            fillr(25, x, kind_fill(r["kind"]))
        for i in range(len(r["rects"])):
            for j in range(len(r["rects"])):
                if i != j:
                    seam = _seam(r["rects"][i], r["rects"][j])
                    if seam != None:
                        fillr(25.5, seam, kind_fill(r["kind"]))
        if r["kind"] == "stair":
            for x in r["rects"]:
                b = R(x)
                shapes.append({"z": 26, "kind": "hatch", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])], "spacing": max(0.6, 0.9 * s), "angle": 90.0 if (x[1] - x[0]) >= (x[3] - x[2]) else 0.0, "color": st["faint"]})
        if r["kind"] == "lift":
            m = r["rects"][0]
            line(26, [(m[0], m[2]), (m[1], m[3])], 0.2, st["mute"])
            line(26, [(m[0], m[3]), (m[1], m[2])], 0.2, st["mute"])

    # rooms to point out (a comparison's changes): a heavy outline in the accent colour
    for r in rooms:
        if highlight != None and r["key"] in highlight:
            for x in r["rects"]:
                b = R(x)
                shapes.append({"z": 58, "kind": "poly", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3]), (b[0], b[1])], "weight": 0.6, "color": st["accent"], "dash": None})

    # bearing lines
    for bl in p.get("bearing", []):
        if bl.get("floor") != floor:
            continue
        sp = bl.get("span") or ((bx[2], bx[3]) if bl["axis"] == "v" else (bx[0], bx[1]))
        if bl["axis"] == "v":
            line(50, [(bl["at"], sp[0]), (bl["at"], sp[1])], 0.3, st["bearing"], [4.0, 1.0, 0.8, 1.0])
        else:
            line(50, [(sp[0], bl["at"]), (sp[1], bl["at"])], 0.3, st["bearing"], [4.0, 1.0, 0.8, 1.0])

    # doors
    by = {r["key"]: r for r in rooms}
    for d in p.get("doors", []):
        if d.get("floor") != floor or d.get("at") == None or d.get("a") not in by:
            continue
        _door_shapes(p, d, by, t, P, shapes, st)

    # windows; a skylight keeps its room's label off it where the room has room elsewhere
    obstacles = {}
    soft = {}
    for w in p.get("windows", []):
        if w.get("floor") != floor or w.get("room") not in by:
            continue
        _window_shapes(w, t, P, shapes, st)
        if w.get("source") == "roof":
            h = w.get("width", 3.0) / 2.0
            soft.setdefault(w["room"], []).append(R(w.get("rect") or (w["at"][0] - h, w["at"][0] + h, w["at"][1] - h, w["at"][1] + h)))

    # fixtures and furniture
    if fixtures and details:
        for f in p.get("fixtures", []):
            if f.get("room") not in by:
                continue
            _fixture_shapes(f, P, R, shapes, texts, st, sz)
            obstacles.setdefault(f["room"], []).append(R(f["rect"]))
    if furniture and details:
        for f in p.get("furniture", []):
            if f.get("room") not in by:
                continue
            b = R(f["rect"])
            shapes.append({"z": 41, "kind": "fill", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])], "color": st["paper"]})
            shapes.append({"z": 42, "kind": "poly", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3]), (b[0], b[1])], "weight": 0.18, "color": st["furniture_edge"], "dash": None})
            obstacles.setdefault(f["room"], []).append(b)

    # stacks
    for k in p.get("stacks", []):
        if floor not in k.get("floors", []):
            continue
        c = P(k["at"])
        rr = max(0.7, 0.3 * s)
        shapes.append({"z": 50, "kind": "fillpoly", "pts": _ring(c, rr + 0.35, 16), "color": st["paper"]})
        shapes.append({"z": 51, "kind": "fillpoly", "pts": _ring(c, rr, 16), "color": st["stack"]})

    # overall dimensions, outside faces
    if dims:
        _dims(bx, P, shapes, texts, st, sz)

    # the red pen's marks go first among the movable things, so the labels keep off them
    numbered = numbers if numbers != None else redpen(checks)
    rows = []
    marks = []
    taken = []
    for n, row in numbered:
        if row.get("floor") != floor:
            continue
        q = _mark_point(p, row)
        rows.append((n, row))
        if q == None:
            continue
        c = P(q)
        rr = sz["mark"] * 1.15
        for k2 in range(8):
            box = (c[0] - rr, c[1] - rr, c[0] + rr, c[1] + rr)
            if _free(box, [m[3] for m in marks]):
                break
            c = (c[0] + rr * 1.6, c[1] + rr * 0.6 * (1 if k2 % 2 == 0 else -1))
        col = st["warn"] if row.get("severity") == "warn" else st["red"]
        if row.get("rect") != None and row.get("at") == None:
            b = R(row["rect"])
            shapes.append({"z": 70, "kind": "poly", "pts": [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3]), (b[0], b[1])], "weight": 0.45, "color": col, "dash": [1.4, 0.9]})
        box = (c[0] - rr, c[1] - rr, c[0] + rr, c[1] + rr)
        marks.append((n, row, c, box))
        taken.append(box)
        shapes.append({"z": 75, "kind": "fillpoly", "pts": _ring(c, rr, 20), "color": st["paper"]})
        shapes.append({"z": 76, "kind": "poly", "pts": _ring(c, rr, 20), "weight": 0.45, "color": col, "dash": None})
        texts.append({"z": 77, "s": str(n), "at": (c[0], c[1]), "size": sz["mark"] * 0.95, "bold": True, "color": col, "align": "center", "valign": "middle", "rotation": 0.0, "halo": None})

    # labels: one per room, biggest rooms first
    placed = {}
    overlaps = []
    outside = []
    if labels:
        for r in sorted(rooms, key = lambda r: -area(r)):
            hard = obstacles.get(r["key"], [])
            how = _place_label(p, r, R, sz, details, taken, hard + soft.get(r["key"], []), texts, st, upright = False)
            if how == None:
                how = _place_label(p, r, R, sz, details, taken, hard, texts, st)
            if how == None:
                outside.append(r)
            else:
                placed[r["key"]] = how
        for c in cts:
            b = R(c["rect"])
            tx = "%s %s" % (c["kind"], c["key"])
            w = _tw(tx, sz["court"])
            cx, cy = (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0
            bb = (cx - w / 2.0 - 0.4, cy - sz["court"] / 2.0 - 0.4, cx + w / 2.0 + 0.4, cy + sz["court"] / 2.0 + 0.4)
            if _inside(bb, (b[0], b[1], b[2], b[3])) and _free(bb, taken):
                taken.append(bb)
                texts.append({"z": 61, "s": tx, "at": (cx, cy), "size": sz["court"], "bold": False, "color": st["court_edge"], "align": "center", "valign": "middle", "rotation": 0.0, "halo": None})
    W = origin_mm[0] + (bx[1] - bx[0]) * s + mr
    H = origin_mm[1] + (bx[3] - bx[2]) * s + mt
    # labels that fit nowhere inside go outside, beside the floor, with a leader
    if outside:
        W = _place_outside(outside, R, sz, details, taken, texts, shapes, st, W, H, placed, overlaps)
    return {
        "floor": floor,
        "w": W,
        "h": H,
        "s": s,
        "origin": (bx[0], bx[2]),
        "margin": origin_mm,
        "bbox": bx,
        "shapes": shapes,
        "texts": texts,
        "labels": placed,
        "overlaps": overlaps,
        "marks": [(m[0], m[1], m[2]) for m in marks],
        "rows": rows,
    }

def _door_shapes(p, d, by, t, P, shapes, st):
    a = by[d["a"]]
    s = d["wall"]
    n = _norm(s)
    tg = _tan(s)
    w = d.get("width", 2.67)
    at = d["at"]
    gap = t
    if not is_outside(d.get("b")) and d.get("b") in by:
        al = at[0] if (s == "N" or s == "S") else at[1]
        best = None
        for x in shared_walls(p, a, by[d["b"]]):
            if x["wall"] == s and x["span"][0] - 0.05 <= al and al <= x["span"][1] + 0.05:
                best = x
        if best != None:
            gap = max(0.0, best["gap"])
    lo = _add(at, tg, -w / 2.0)
    hi = _add(at, tg, w / 2.0)
    if gap > 0.0:
        cut = [P(lo), P(hi), P(_add(hi, n, gap)), P(_add(lo, n, gap))]
        shapes.append({"z": 30, "kind": "fillpoly", "pts": cut, "color": st["opening"]})
    sw = d.get("swing", "a")
    ink = st["ink"]
    if sw == "a" or sw == "b":
        hinge, other = (lo, hi) if d.get("hinge", "lo") == "lo" else (hi, lo)
        sign = -1.0 if sw == "a" else 1.0
        base = 0.0 if sw == "a" else gap
        h0 = _add(hinge, n, base)
        o0 = _add(other, n, base)
        tip = _add(h0, n, sign * w)
        shapes.append({"z": 45, "kind": "poly", "pts": [P(h0), P(tip)], "weight": 0.35, "color": ink, "dash": None})
        # the arc from the open leaf's tip round to the closed position, a quarter turn (degrees)
        a_tip = math.atan2(tip[1] - h0[1], tip[0] - h0[0])
        a_cl = math.atan2(o0[1] - h0[1], o0[0] - h0[0])
        da = a_cl - a_tip
        if da > 180.0:
            da -= 360.0
        if da < -180.0:
            da += 360.0
        pts = _arc(h0, w, a_tip, a_tip + da, 14)
        shapes.append({"z": 45, "kind": "poly", "pts": [P(q) for q in pts], "weight": 0.18, "color": ink, "dash": None})
    elif sw == "slide":
        g2 = gap if gap > 0.0 else 0.3
        b0 = 0.0 if gap > 0.0 else -0.15
        mid = _add(at, tg, 0.0)
        shapes.append({"z": 45, "kind": "poly", "pts": [P(_add(lo, n, b0 + g2 / 3.0)), P(_add(_add(mid, tg, 0.25), n, b0 + g2 / 3.0))], "weight": 0.4, "color": ink, "dash": None})
        shapes.append({"z": 45, "kind": "poly", "pts": [P(_add(_add(mid, tg, -0.25), n, b0 + 2.0 * g2 / 3.0)), P(_add(hi, n, b0 + 2.0 * g2 / 3.0))], "weight": 0.4, "color": ink, "dash": None})
    elif sw == "pocket":
        mid = gap / 2.0
        shapes.append({"z": 45, "kind": "poly", "pts": [P(_add(lo, n, mid)), P(_add(_add(lo, tg, -w), n, mid))], "weight": 0.3, "color": ink, "dash": [1.0, 0.7]})
        shapes.append({"z": 45, "kind": "poly", "pts": [P(_add(lo, n, mid)), P(_add(_add(lo, tg, 0.2), n, mid))], "weight": 0.4, "color": ink, "dash": None})

def _window_shapes(w, t, P, shapes, st):
    at = w["at"]
    wd = w.get("width", 3.0)
    col = st["egress"] if w.get("egress") else st["glass"]
    if w.get("source") == "roof":
        h = wd / 2.0
        r = w.get("rect") or (at[0] - h, at[0] + h, at[1] - h, at[1] + h)  # a skylight's own rect when it has one
        b = [(r[0], r[2]), (r[1], r[2]), (r[1], r[3]), (r[0], r[3]), (r[0], r[2])]
        shapes.append({"z": 46, "kind": "poly", "pts": [P(q) for q in b], "weight": 0.25, "color": col, "dash": [1.2, 0.8]})
        return
    s = w.get("wall")
    if s not in ["N", "S", "E", "W"]:
        return
    n = _norm(s)
    tg = _tan(s)
    lo = _add(at, tg, -wd / 2.0)
    hi = _add(at, tg, wd / 2.0)
    shapes.append({"z": 31, "kind": "fillpoly", "pts": [P(lo), P(hi), P(_add(hi, n, t)), P(_add(lo, n, t))], "color": st["paper"]})
    for f, wt, c in [(0.0, 0.18, st["mute"]), (t, 0.18, st["mute"]), (t / 2.0, 0.45 if w.get("egress") else 0.3, col)]:
        shapes.append({"z": 35, "kind": "poly", "pts": [P(_add(lo, n, f)), P(_add(hi, n, f))], "weight": wt, "color": c, "dash": None})
    for q in [lo, hi]:
        shapes.append({"z": 35, "kind": "poly", "pts": [P(q), P(_add(q, n, t))], "weight": 0.18, "color": st["mute"], "dash": None})

def _fixture_shapes(f, P, R, shapes, texts, st, sz):
    r = f["rect"]
    b = R(r)
    box = [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])]
    shapes.append({"z": 40, "kind": "fillpoly", "pts": box, "color": st["fixture"]})
    shapes.append({"z": 41, "kind": "poly", "pts": box + [box[0]], "weight": 0.2, "color": st["fixture_edge"], "dash": None})
    k = f.get("kind", "")
    cx, cy = (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0
    w, h = b[2] - b[0], b[3] - b[1]
    if k in ["wc", "toilet"]:
        # the bowl, off the tank at the back (opposite the front)
        fr = f.get("front", "N")
        n = _norm(fr)
        rr = 0.3 * min(w, h)
        c = (cx + n[0] * 0.15 * w, cy + n[1] * 0.15 * h)
        shapes.append({"z": 41, "kind": "poly", "pts": _ring(c, rr, 16), "weight": 0.2, "color": st["fixture_edge"], "dash": None})
    elif k in ["lav", "sink", "bar_sink"]:
        shapes.append({"z": 41, "kind": "poly", "pts": _ring((cx, cy), 0.32 * min(w, h), 16), "weight": 0.2, "color": st["fixture_edge"], "dash": None})
    elif k in ["tub", "bath"]:
        i = 0.12 * min(w, h)
        ib = [(b[0] + i, b[1] + i), (b[2] - i, b[1] + i), (b[2] - i, b[3] - i), (b[0] + i, b[3] - i)]
        shapes.append({"z": 41, "kind": "poly", "pts": ib + [ib[0]], "weight": 0.18, "color": st["fixture_edge"], "dash": None})
    elif k in ["shower"]:
        shapes.append({"z": 41, "kind": "poly", "pts": [(b[0], b[1]), (b[2], b[3])], "weight": 0.15, "color": st["fixture_edge"], "dash": None})
        shapes.append({"z": 41, "kind": "poly", "pts": [(b[0], b[3]), (b[2], b[1])], "weight": 0.15, "color": st["fixture_edge"], "dash": None})
    elif k in ["range", "cooktop"]:
        for dx in [-0.25, 0.25]:
            for dy in [-0.25, 0.25]:
                shapes.append({"z": 41, "kind": "poly", "pts": _ring((cx + dx * w, cy + dy * h), 0.12 * min(w, h), 10), "weight": 0.15, "color": st["fixture_edge"], "dash": None})
    elif w > _tw(k, sz["fix"]) + 1.0 and h > sz["fix"] + 1.0:
        texts.append({"z": 42, "s": k, "at": (b[0] + 0.6, b[1] + 0.6), "size": sz["fix"], "bold": False, "color": st["fixture_edge"], "align": "left", "valign": "bottom", "rotation": 0.0, "halo": None})

def _dims(bx, P, shapes, texts, st, sz):
    a = P((bx[0], bx[2]))
    b = P((bx[1], bx[2]))
    c = P((bx[0], bx[3]))
    off = sz["dim"] + 3.0
    col = st["dim"]
    # across, under the floor
    y = a[1] - off
    shapes.append({"z": 55, "kind": "poly", "pts": [(a[0], a[1] - 0.8), (a[0], y - 1.2)], "weight": 0.15, "color": col, "dash": None})
    shapes.append({"z": 55, "kind": "poly", "pts": [(b[0], b[1] - 0.8), (b[0], y - 1.2)], "weight": 0.15, "color": col, "dash": None})
    shapes.append({"z": 55, "kind": "poly", "pts": [(a[0] - 1.2, y), (b[0] + 1.2, y)], "weight": 0.22, "color": col, "dash": None})
    for x in [a[0], b[0]]:
        shapes.append({"z": 55, "kind": "poly", "pts": [(x - 0.8, y - 0.8), (x + 0.8, y + 0.8)], "weight": 0.4, "color": col, "dash": None})
    texts.append({"z": 56, "s": ftin(bx[1] - bx[0]), "at": ((a[0] + b[0]) / 2.0, y + 0.9), "size": sz["dim"], "bold": False, "color": col, "align": "center", "valign": "baseline", "rotation": 0.0, "halo": None})
    # up, left of the floor
    x = a[0] - off
    shapes.append({"z": 55, "kind": "poly", "pts": [(a[0] - 0.8, a[1]), (x - 1.2, a[1])], "weight": 0.15, "color": col, "dash": None})
    shapes.append({"z": 55, "kind": "poly", "pts": [(c[0] - 0.8, c[1]), (x - 1.2, c[1])], "weight": 0.15, "color": col, "dash": None})
    shapes.append({"z": 55, "kind": "poly", "pts": [(x, a[1] - 1.2), (x, c[1] + 1.2)], "weight": 0.22, "color": col, "dash": None})
    for yy in [a[1], c[1]]:
        shapes.append({"z": 55, "kind": "poly", "pts": [(x - 0.8, yy - 0.8), (x + 0.8, yy + 0.8)], "weight": 0.4, "color": col, "dash": None})
    texts.append({"z": 56, "s": ftin(bx[3] - bx[2]), "at": (x - 0.9, (a[1] + c[1]) / 2.0), "size": sz["dim"], "bold": False, "color": col, "align": "center", "valign": "baseline", "rotation": 90.0, "halo": None})

def _label_lines(r, details):
    lines = [(r.get("name") or r["key"], True)]
    if details:
        lines.append((sqft(area(r)), False))
        m = r["rects"][0]
        lines.append(("%s x %s" % (ftin(m[1] - m[0], down = True), ftin(m[3] - m[2], down = True)), False))  # clear: rounded down
    return lines

def _block(lines, sz, f):
    """Width, height and the lines' (text, size, bold, baseline offset from the block's top) at shrink factor f."""
    out = []
    y = 0.0
    w = 0.0
    for i, (tx, bold) in enumerate(lines):
        size = (sz["name"] if i == 0 else sz["sub"]) * f
        y += size if i == 0 else size * 1.5
        out.append((tx, size, bold, y))
        w = max(w, _tw(tx, size, bold))
    return (w, y + 0.3 * sz["sub"] * f, out)

def _place_label(p, r, R, sz, details, taken, obstacles, texts, st, upright = True):
    rects = [R(x) for x in r["rects"]]
    all_lines = _label_lines(r, details)
    tries = []
    for keep in range(len(all_lines), 0, -1):
        for f in [1.0, 0.85, 0.72]:
            tries.append((all_lines[:keep], f))
    pad = st["halo_pad"]
    for lines, f in tries:
        bw, bh, ls = _block(lines, sz, f)
        for i, b in enumerate(rects):
            lim = (b[0] + 0.4, b[1] + 0.4, b[2] - 0.4, b[3] - 0.4)
            cx, cy = (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0
            hh = b[3] - b[1]
            for dy in [0.0, hh * 0.22, -hh * 0.22, hh * 0.36, -hh * 0.36]:
                for dx in [0.0, (b[2] - b[0]) * 0.2, -(b[2] - b[0]) * 0.2]:
                    x, y = cx + dx, cy + dy
                    box = (x - bw / 2.0 - pad, y - bh / 2.0 - pad, x + bw / 2.0 + pad, y + bh / 2.0 + pad)
                    if _inside(box, lim) and _free(box, taken) and _free(box, obstacles):
                        taken.append(box)
                        top = y + bh / 2.0
                        for tx, size, bold, off in ls:
                            texts.append({"z": 62, "s": tx, "at": (x, top - off), "size": size, "bold": bold, "color": st["ink"] if bold else st["mute"], "align": "center", "valign": "baseline", "rotation": 0.0, "halo": kind_fill(r["kind"])})
                        how = "inside" if (f == 1.0 and len(lines) == len(all_lines)) else "shrunk"
                        return {"how": how, "box": box, "rect": i, "lines": len(lines), "f": f}
    if not upright:
        return None
    # tall and narrow: one upright line
    one = r.get("name") or r["key"]
    if details:
        one = "%s %s" % (one, sqft(area(r)))
    for f in [1.0, 0.85, 0.72]:
        size = sz["name"] * f
        w = _tw(one, size, False)
        for i, b in enumerate(rects):
            cx, cy = (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0
            box = (cx - size * 0.65 - pad, cy - w / 2.0 - pad, cx + size * 0.65 + pad, cy + w / 2.0 + pad)
            lim = (b[0] + 0.3, b[1] + 0.3, b[2] - 0.3, b[3] - 0.3)
            if _inside(box, lim) and _free(box, taken) and _free(box, obstacles):
                taken.append(box)
                texts.append({"z": 62, "s": one, "at": (cx + size * 0.36, cy), "size": size, "bold": False, "color": st["ink"], "align": "center", "valign": "baseline", "rotation": 90.0, "halo": kind_fill(r["kind"])})
                return {"how": "upright", "box": box, "rect": i, "lines": 1, "f": f}
    return None

def _place_outside(rooms, R, sz, details, taken, texts, shapes, st, W, H, placed, overlaps):
    """Labels that fit nowhere inside: in a column right of the floor, each level with its room where it can be, with
    a leader to the room."""
    x0 = W + 4.0
    col_w = 0.0
    used = []
    for r in sorted(rooms, key = lambda r: -R(r["rects"][0])[1]):
        lines = _label_lines(r, details)
        bw, bh, ls = _block(lines, sz, 0.85)
        b = R(r["rects"][0])
        target = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
        y = target[1]
        box = None
        for k in range(60):
            for sgn in [1.0, -1.0]:
                yy = y + sgn * k * 1.2
                cand = (x0, yy - bh / 2.0, x0 + bw + 1.0, yy + bh / 2.0)
                if cand[1] >= 0.0 and _free(cand, used):
                    box = cand
                    break
            if box != None:
                break
        if box == None:
            box = (x0, y - bh / 2.0, x0 + bw + 1.0, y + bh / 2.0)
            overlaps.append(r["key"])
        used.append(box)
        top = box[3]
        for tx, size, bold, off in ls:
            texts.append({"z": 62, "s": tx, "at": (box[0] + 0.5, top - off), "size": size, "bold": bold, "color": st["ink"] if bold else st["mute"], "align": "left", "valign": "baseline", "rotation": 0.0, "halo": None})
        ly = (box[1] + box[3]) / 2.0
        shapes.append({"z": 63, "kind": "poly", "pts": [(box[0] - 0.4, ly), target], "weight": 0.18, "color": st["mute"], "dash": None})
        shapes.append({"z": 63, "kind": "fillpoly", "pts": _ring(target, 0.45, 8), "color": st["mute"]})
        placed[r["key"]] = {"how": "outside", "box": box, "rect": 0, "lines": len(lines), "f": 0.85}
        col_w = max(col_w, bw + 1.5)
    return W + 4.0 + col_w + 2.0

# ---------------------------------------------------------------------------------------------------------- painting

def paint(sheet, lay, at):
    """Put a floor_layout() on a sheet, its lower-left corner at paper point `at` (mm). Returns the paper box
    (x0, y0, x1, y1) it took."""
    ox, oy = at[0], at[1]

    def Q(q):
        return (ox + q[0], oy + q[1])

    for sh in sorted(lay["shapes"], key = lambda e: e["z"]):
        k = sh["kind"]
        if k == "fill" or k == "fillpoly":
            sheet.fill([Q(q) for q in sh["pts"]], color = sh["color"])
        elif k == "poly":
            if sh.get("dash") != None:
                sheet.polyline([Q(q) for q in sh["pts"]], weight = sh["weight"], color = sh["color"], dash = sh["dash"])
            else:
                sheet.polyline([Q(q) for q in sh["pts"]], weight = sh["weight"], color = sh["color"])
        elif k == "hatch":
            sheet.hatch([Q(q) for q in sh["pts"]], pattern = "lines", spacing = sh["spacing"], angle = sh["angle"], weight = 0.15, color = sh["color"])
    for tx in sorted(lay["texts"], key = lambda e: e["z"]):
        p = Q(tx["at"])
        if tx.get("halo") != None and tx["rotation"] == 0.0:
            w = _tw(tx["s"], tx["size"], tx["bold"])
            x0 = p[0] - (w / 2.0 if tx["align"] == "center" else (w if tx["align"] == "right" else 0.0))
            sheet.fill([(x0 - 0.4, p[1] - 0.3 * tx["size"] - 0.25), (x0 + w + 0.4, p[1] - 0.3 * tx["size"] - 0.25), (x0 + w + 0.4, p[1] + tx["size"] + 0.3), (x0 - 0.4, p[1] + tx["size"] + 0.3)], color = tx["halo"])
        sheet.text(tx["s"], at = p, size = tx["size"], bold = tx["bold"], color = tx["color"], align = tx["align"], valign = tx["valign"], rotation = tx["rotation"])
    return (ox, oy, ox + lay["w"], oy + lay["h"])

def draw_floor(sheet, p, floor, *, at, scale = 6.35, checks = None, numbers = None, title = None, number = None, dims = True, labels = True, details = True, style = None):
    """Lay out one floor and paint it with its lower-left corner at `at` (paper mm); with title=, a view title under
    it (number= its ring). Returns the layout (floor_layout's dict) with "box", the paper box it took."""
    lay = floor_layout(p, floor, scale = scale, checks = checks, numbers = numbers, dims = dims, labels = labels, details = details, style = style)
    box = paint(sheet, lay, at)
    if title != None:
        sheet.view_title(number or "1", title, at = (at[0], at[1] - 9.0), scale = "", width = min(160.0, max(60.0, lay["w"])))
    return dict(lay, box = box)

def _legend_items(p, floors):
    kinds = {}
    for r in p.get("rooms", []):
        if r.get("floor") in floors:
            kinds[r["kind"]] = 1
    out = []
    for g in ["living", "wet", "circulation", "storage", "outdoor", "other"]:
        for k in KIND_GROUPS[g]:
            if k in kinds:
                out.append(k)
    return out

def _paint_key(sheet, p, floors, at, width, st):
    """The kinds on these floors as swatches, then the symbols; returns the y under it."""
    x, y = at
    sheet.text("ROOMS BY KIND", at = (x, y), size = 2.2, bold = True)
    y -= 4.5
    ks = _legend_items(p, floors)
    cols = 2 if width > 90 else 1
    cw = width / cols
    for i, k in enumerate(ks):
        cx = x + (i % cols) * cw
        cy = y - (i // cols) * 4.2
        sheet.fill([(cx, cy - 0.5), (cx + 6.0, cy - 0.5), (cx + 6.0, cy + 2.5), (cx, cy + 2.5)], color = kind_fill(k))
        sheet.polyline([(cx, cy - 0.5), (cx + 6.0, cy - 0.5), (cx + 6.0, cy + 2.5), (cx, cy + 2.5), (cx, cy - 0.5)], weight = 0.15, color = st["faint"])
        sheet.text(k, at = (cx + 8.0, cy), size = 2.0)
    y -= ((len(ks) + cols - 1) // cols) * 4.2 + 2.5
    sym = [("egress", "egress window"), ("glass", "window"), ("stack", "plumbing stack"), ("bearing", "bearing line"), ("red", "red pen: a failing check")]
    for key, label in sym:
        if key == "stack":
            sheet.fill(_ring((x + 3.0, y + 1.0), 1.0, 12), color = st["stack"])
        elif key == "red":
            sheet.polyline(_ring((x + 3.0, y + 1.0), 1.6, 16), weight = 0.4, color = st["red"])
        elif key == "bearing":
            sheet.polyline([(x, y + 1.0), (x + 6.0, y + 1.0)], weight = 0.3, color = st["bearing"], dash = [2.0, 0.7, 0.5, 0.7])
        else:
            sheet.polyline([(x, y + 1.0), (x + 6.0, y + 1.0)], weight = 0.45, color = st[key])
        sheet.text(label, at = (x + 8.0, y), size = 2.0)
        y -= 4.2
    return y

def _paint_rows(sheet, rows, at, width, st, title = "RED PEN"):
    """The numbered rows as a list: n, then the row's name and detail, wrapped. Returns the y under it."""
    x, y = at
    if not rows:
        return y
    sheet.text(title, at = (x, y), size = 2.2, bold = True, color = st["red"])
    y -= 4.5
    for n, row in rows:
        col = st["warn"] if row.get("severity") == "warn" else st["red"]
        sheet.text(str(n), at = (x + 2.0, y), size = 2.0, bold = True, color = col, align = "center")
        head = row.get("name") or row.get("key") or ""
        if row.get("rule"):
            head = "%s (%s)" % (head, row["rule"])
        y = sheet.note(head + ": " + (row.get("detail") or ""), at = (x + 5.5, y), width = width - 6.0, size = 1.9)
        y -= 1.2
    return y

# ---------------------------------------------------------------------------------------------------------- sheets

def plan_sheets(S, p, *, checks = None, floors = None, prefix = "A", first = 1, per_sheet = None, scale = None, title = None, style = None):
    """Add plan p's floors (those with rooms) to sheet set S: `per_sheet` floors to a sheet (default: as many as fit side by side at
    1/4" = 1'-0", at most 3), each at the largest architect's scale that fits (or `scale`, mm to the foot), with the
    kinds' key and the red pen's numbered list in a column on the right. checks are check rows; the failing ones are
    numbered once over the whole plan. Sheets are numbered prefix + "1.<k>" from `first`. Returns the sheets."""
    st = dict(STYLE)
    st.update(style or {})
    fl = [f for f in floors_by_z(p) if (floors == None or f["key"] in floors) and rooms_on(p, f["key"])]
    if not fl:
        return []  # nothing to draw (a plan with no rooms on these floors)
    numbered = redpen(checks)
    x0, y0, x1, y1 = S.area()
    col = 105.0
    box_w = x1 - x0 - col - 16.0
    box_h = y1 - y0 - 34.0
    allow_w, allow_h = 30.0, 16.0  # mm a floor needs beyond its extent: the dimensions and margins
    feet = []
    for f in fl:
        b = floor_layout(p, f["key"], scale = 1.0, numbers = numbered, labels = False, style = style)["bbox"]
        feet.append((b[1] - b[0], b[3] - b[2]))
    n = per_sheet
    if n == None:
        n = 1
        for k in [3, 2]:
            if len(feet) < k:
                continue
            ok = True
            for g0 in range(0, len(feet), k):
                grp = feet[g0:g0 + k]
                if _sum([fw for fw, _ in grp]) * 6.35 + len(grp) * allow_w + 14.0 * (len(grp) - 1) > box_w or max([fh for _, fh in grp]) * 6.35 + allow_h > box_h:
                    ok = False
            if ok:
                n = k
                break
    sheets = []
    for k0 in range(0, len(fl), n):
        group = fl[k0:k0 + n]
        gfeet = feet[k0:k0 + n]
        cands = [(scale, "AS NOTED")] if scale != None else [(inch * 25.4, nm) for inch, nm in _SCALES if inch <= 0.5]
        lays = None
        sname = None
        for s, nm in cands:
            if scale == None and (_sum([fw for fw, _ in gfeet]) * s + len(group) * allow_w + 14.0 * (len(group) - 1) > box_w or max([fh for _, fh in gfeet]) * s + allow_h > box_h):
                continue
            ls = [floor_layout(p, f["key"], scale = s, numbers = numbered, style = style) for f in group]
            if scale != None or (_sum([l["w"] for l in ls]) + 14.0 * (len(ls) - 1) <= box_w and max([l["h"] for l in ls]) <= box_h):
                lays, sname = ls, nm
                break
        if lays == None:
            s, sname = _SCALES[-1][0] * 25.4, _SCALES[-1][1]
            lays = [floor_layout(p, f["key"], scale = s, numbers = numbered, style = style) for f in group]
        names = ", ".join([f.get("name", f["key"]) for f in group])
        num = "%s1.%d" % (prefix, first + len(sheets))
        sh = S.sheet(num, (title + ": " if title else "") + names.upper(), scale = sname)
        total = _sum([l["w"] for l in lays]) + 14.0 * (len(lays) - 1)
        x = x0 + 8.0 + max(0.0, (box_w - total) / 2.0)
        rows_here = []
        for i, f in enumerate(group):
            lay = lays[i]
            yb = y0 + 22.0 + max(0.0, (box_h - lay["h"]) / 2.0)
            paint(sh, lay, (x, yb))
            sh.view_title(str(i + 1), "%s  %s" % (f["key"], f.get("name", "")), at = (x, yb - 9.0), width = min(150.0, max(70.0, lay["w"])), scale = sname)
            rows_here.extend(lay["rows"])
            x += lay["w"] + 14.0
        cx = x1 - col
        yk = _paint_key(sh, p, [f["key"] for f in group], (cx, y1 - 10.0), col - 6.0, st)
        general = [(nn, r) for nn, r in numbered if r.get("floor") == None] if len(sheets) == 0 else []
        yk = _paint_rows(sh, rows_here + general, (cx, yk - 4.0), col - 6.0, st)
        north = (p.get("site") or {}).get("north_deg", 0.0)
        sh.north_arrow(at = (x1 - col / 2.0, y0 + 18.0), r = 7.0, angle = north, label = north != 0.0)
        sheets.append(sh)
    return sheets

def _sum(xs):
    t = 0.0
    for x in xs:
        t += x
    return t

def plan_set(p, *, name = None, checks = None, size = None, project = None, stamp = None, per_sheet = None, scale = None):
    """A sheet set of plan p, one sheet per floor or a few floors to a sheet (plan_sheets), with a title block naming
    the plan and its source. Export it: export(plan_set(p), "plan.pdf")."""
    nm = name or p.get("name", "plan")
    S = sheet_set(
        nm,
        size = size if size != None else sheet.arch_d,
        units = "ft-in",
        title_block = title_block(project = project or nm.upper().replace("_", " "), stamp = stamp or ["PLAN STUDY", "NOT FOR CONSTRUCTION"], notes = "From %s." % (p.get("source") or "a plan value")),
    )
    plan_sheets(S, p, checks = checks, per_sheet = per_sheet, scale = scale)
    return S

def scorecard(p, checks = None):
    """A default scorecard row for a plan: rooms, built area (indoor rooms' net sq ft), bedrooms, and the problems
    validate() and the failing checks count."""
    indoor = [r for r in p.get("rooms", []) if kind_group(r.get("kind")) != "outdoor"]
    beds = [r for r in indoor if r.get("sleeping")]
    fails = len(redpen(checks)) if checks != None else None
    return [
        ("rooms", str(len(indoor))),
        ("net sq ft", format(_sum([area(r) for r in indoor]), ",.0f")),
        ("bedrooms", str(len(beds))),
        ("plan problems", str(len(validate(p)))),
        ("failing checks", "-" if fails == None else str(fails)),
    ]

def compare_sheet(S, plans, *, number = "C1.0", name = "COMPARISON", scorecards = None, checks = None, labels = None, floors = None, style = None):
    """One sheet of N plans side by side, every floor of each, at one scale, with a scorecard table under them: a row
    per plan. In every plan after the first, the rooms added or changed against the first are outlined in the accent
    colour (plan_diff). Each plan is a panel of its floors (in a row, lowest on the left, or stacked, lowest at the bottom),
    and the panels go in the grid of columns that gives the largest architect's scale. scorecards: a list (one per
    plan) of [(column, value), ...] (default scorecard(plan, checks)); checks: a list (one per plan) of check rows for
    the red pen, or None; labels: the panels' titles (default the plans' names). Returns the sheet."""
    st = dict(STYLE)
    st.update(style or {})
    if not plans:
        fail("plan_draw.compare_sheet: no plans")
    N = len(plans)
    cks = checks if checks != None else [None for _ in plans]
    cards = scorecards if scorecards != None else [scorecard(plans[i], cks[i]) for i in range(N)]
    names = labels if labels != None else [q.get("name", "plan %d" % (i + 1)) for i, q in enumerate(plans)]
    # each plan's floors with rooms, lowest first, and their extents in feet
    keys = []
    ext = {}
    for i, q in enumerate(plans):
        ks = []
        for f in floors_by_z(q):
            if (floors == None or f["key"] in floors) and [r for r in q.get("rooms", []) if r.get("floor") == f["key"]]:
                ks.append(f)
                b = floor_layout(q, f["key"], scale = 1.0, dims = False, labels = False)["bbox"]
                ext[(i, f["key"])] = (b[1] - b[0], b[3] - b[2])
        keys.append(ks)
    x0, y0, x1, y1 = S.area()
    table_h = 8.0 + 4.6 * (N + 1)
    W = x1 - x0 - 16.0
    Ht = y1 - y0 - table_h - 18.0
    cap = 7.0  # a floor's caption under it
    best = None
    for c in range(1, N + 1):
        r = (N + c - 1) // c
        cell_w = (W - 16.0 * (c - 1)) / c
        cell_h = (Ht - 10.0 * r - 8.0 * (r - 1)) / r
        for orient in ["row", "stack"]:
            smax = 1e9
            for i in range(N):
                fws = [ext[(i, f["key"])][0] for f in keys[i]] or [1.0]
                fhs = [ext[(i, f["key"])][1] for f in keys[i]] or [1.0]
                n = len(fws)
                if orient == "row":
                    smax = min(smax, (cell_w - n * 9.0 - (n - 1) * 10.0) / _sum(fws), (cell_h - 6.0 - cap) / max(fhs))
                else:
                    smax = min(smax, (cell_w - 9.0) / max(fws), (cell_h - n * (6.0 + cap) - (n - 1) * 4.0) / _sum(fhs))
            if best == None or smax > best[0] + 1e-9:
                best = (smax, c, r, orient, cell_w, cell_h)
    smax, c, r, orient, cell_w, cell_h = best
    s, sname = _SCALES[-1][0] * 25.4, _SCALES[-1][1]
    for inch, nm in _SCALES:
        if inch * 25.4 <= smax + 1e-9:
            s, sname = inch * 25.4, nm
            break
    sh = S.sheet(number, name, scale = sname)
    for i, q in enumerate(plans):
        numbered = redpen(cks[i])
        hl = None
        if i > 0:
            df = plan_diff(plans[0], q)
            hl = {k: 1 for k in df["added"]}
            for ch in df["changed"]:
                hl[ch["key"]] = 1
        lays = [floor_layout(q, f["key"], scale = s, numbers = numbered, dims = False, details = s >= 4.7, fixtures = s >= 4.7, furniture = s >= 4.7, highlight = hl, style = style) for f in keys[i]]
        col, row = i % c, i // c
        cx = x0 + 8.0 + col * (cell_w + 16.0)
        ctop = y1 - 6.0 - row * (cell_h + 18.0)
        if orient == "row":
            total = _sum([l["w"] for l in lays]) + 10.0 * (len(lays) - 1)
            x = cx + max(0.0, (cell_w - total) / 2.0)
            hmax = max([l["h"] for l in lays] or [0.0])
            yb = ctop - 8.0 - hmax - max(0.0, (cell_h - 8.0 - hmax - cap) / 2.0)
            sh.text(names[i], at = (cx + cell_w / 2.0, yb + hmax + 4.0), size = 3.2, bold = True, align = "center")
            for k, lay in enumerate(lays):
                paint(sh, lay, (x, yb + (hmax - lay["h"])))
                f = keys[i][k]
                sh.text("%s  %s" % (f["key"], f.get("name", "")), at = (x + lay["margin"][0], yb - 4.5 + (hmax - lay["h"])), size = 2.0, color = st["mute"])
                x += lay["w"] + 10.0
        else:
            sh.text(names[i], at = (cx + cell_w / 2.0, ctop - 4.0), size = 3.2, bold = True, align = "center")
            y = ctop - 8.0
            for k in range(len(lays) - 1, -1, -1):
                lay = lays[k]
                y -= lay["h"]
                x = cx + max(0.0, (cell_w - lay["w"]) / 2.0)
                paint(sh, lay, (x, y))
                f = keys[i][k]
                sh.text("%s  %s" % (f["key"], f.get("name", "")), at = (x + lay["margin"][0], y - 4.5), size = 2.0, color = st["mute"])
                y -= cap + 4.0
    # the scorecard: a row per plan
    cols = [cc for cc, _ in cards[0]]
    rows = [["plan"] + cols]
    for i in range(N):
        d = {cc: v for cc, v in cards[i]}
        rows.append([names[i]] + [d.get(cc, "") for cc in cols])
    tw = min(W, 60.0 + 34.0 * len(cols))
    cw = [60.0] + [(tw - 60.0) / max(1, len(cols)) for _ in cols]
    sh.table(rows, at = (x0 + 8.0, y0 + table_h + 2.0), col_widths = cw, size = 2.2, align = ["left"] + ["right" for _ in cols])
    return sh
