"""@cadpole/plan_frame.star -- reframe(): a fixed plan's cheapest frame, as a MILP over joist bays.

    load("@cadpole/plan_frame.star", "reframe", "frame_menu", "frame_members", "frame_text", "frame_checks",
         "frame_sheets", "frame_set")

    framed = reframe(plan)                       # the plan, with its frame in it
    framed["bearing"]                            # the bearing lines, by floor
    framed["meta"]["plan_frame"]["members"]      # every joist bay, beam, header, transfer and stair header, by name
    print(frame_text(framed))

The plan is the @cadpole/plan build's plan value (feet; u east, v north; rects (u0, u1, v0, v1) at inside faces). It is
read as plain data: this module loads no other plan module, so it frames a plan however it was made.

## The model

Each floor has one **framing level** at its top: the joists over that floor, bearing on its walls. Under the next
floor up it carries that floor (40 psf live); where the next floor has a deck room, a deck (60 psf, Table R301.5);
elsewhere it is roof (20 psf, ceiling attached: Table R802.4.1(2)). A **bearing line** belongs to the floor whose
walls carry the level, the same convention as the plan's `bearing` records.

- **Direction.** Joists run along u or along v, one way for the whole building by default ("auto": the solver
  chooses), or as the menu says per floor.
- **Bays.** The level's plate is cut into bands across the joists; in each band the joists cross from one exterior
  wall to the other through the bearing lines the solver picks. Each bay (a stretch between two lines) takes the
  cheapest joist of the level's one depth that spans it: sawn joists from Table R502.3.1(2) (R802.4.1(2) under a
  roof), I-joists by rule. A level has one joist depth.
- **Lines.** A line stands at a wall of the floor (a cluster of room faces, or an explicit wall). Where a room
  straddles it, it needs a beam over that room; a door in it needs a header (R602.7); a stair or lift opening
  through the level forbids a line through it, and each side of the opening that is not on a line needs a header
  (R502.10: a single joist up to 4 ft, doubled and sized past that).
- **Transfers.** A bearing wall must sit on a bearing line of the level under it, within that level's joist depth
  (R502.4), or on grade, or be concrete (founded on its own footing); otherwise it needs a transfer. Under joists
  running the other way it needs doubled joists (R502.4's first sentence).
- **Cost.** Points, a judgment: 1 point is about one square foot of 2x10 at 16 in. The joists by material and
  joist count; a bearing wall by the foot (with a footing on grade); a beam, header or transfer by a fixed part and
  its span. `frame_menu()` holds every weight; pass `menu=` to change any.

After the solve every member is sized: joists by their table, girders and headers by R602.7(2) or (1) where the
table reaches, otherwise LVL then steel by rule of thumb (`@cadpole/plan_spans.star`), flush in the joist depth when
one fits, else dropped. **A planning aid, not engineering:** no lateral system, point loads, connections, posts or
footings are designed, and every rule-of-thumb size is marked as one.

Probe (purity): `frame_menu`.
"""

load("@cadpole/plan_draw.star", "floor_layout", "paint", "redpen", "scale_for")
load("@cadpole/plan_spans.star", "SAWN", "ftin", "header_for", "joist_options", "lvl_beam", "steel_beam")

# ------------------------------------------------------------------------------------------------- vocabulary

OUTDOOR = ["deck", "court", "well"]
OPENING_KINDS = ["stair", "lift"]
WALL_TOL = 0.9  # room faces within this of each other are one wall (a wall up to about 11 in thick)
WALL_SPAN = 1.6  # and a wall mass is at most this thick, faces and explicit walls together
GAP_CLOSE = 1.0  # a gap in a wall line shorter than this is a crossing wall, not an opening
T_DEFAULT = 0.4  # a wall's thickness when only one face of it is known (4 3/4 in)
T_EXT = 0.5  # an exterior wall's when unknown
BAND_SNAP = 1.0  # plate breakpoints closer than this are one
POS_MARGIN = 1.0  # a line stands at least this far inside the exterior walls
STRADDLE = 0.5  # a room straddles a line that passes this far inside it
SIDE_TOL = 0.75  # an opening's side is on a line this close to it
OFFSET_MAX = 16.0 / 12  # R502.4 with the deepest joist in any menu: no support is looked for farther away
WALL_PSF = 10.0  # a framed wall's weight per sq ft of wall (R301.2.2.2's cap for interior walls)

PLANNING = "A planning aid, not engineering: spans from the CRC tables named, beams by rule of thumb; no lateral system, point loads, posts, footings or connections designed."

DEFAULT_COSTS = {
    "depth_in": 0.01,  # per sq ft per inch of joist depth: the headroom it takes
    "line": 2.0,  # each interior bearing line, per band
    "wall": 0.5,  # per ft of wood wall made bearing, over a framed floor (blocking, plates)
    "wall_grade": 1.5,  # per ft of wood bearing wall on grade (a footing)
    "beam": [20.0, 2.0, 0.25],  # fixed, per ft, per ft^2 of span: a beam where a room straddles a line
    "garage": 2.0,  # times a beam's cost over a garage (the car's clear span)
    "header": [2.0, 0.0, 0.4],  # a door or window in a bearing wall, by its width
    "stair_header": [10.0, 1.0, 0.1],  # an opening's header past 4 ft (R502.10)
    "stair_header_short": 2.0,  # up to 4 ft: a single joist
    "transfer": [60.0, 3.0, 0.3],  # a bearing wall over no support, by its length
    "girder": 40.0,  # a line with no wall under it at all (a girder on posts), per band, on top of its beams
    "doubled": 1.0,  # per ft: doubled joists under a bearing wall parallel to them
    "misfit": 2.0,  # per sq ft: a joist depth that does not fit between the floors
    "ijoist_material": {"9.5": 1.2, "11.875": 1.45, "14.0": 1.7, "16.0": 2.0},  # an I-joist's lineal foot, in 2x10s
}

_MENU_KEYS = ["species", "grades", "spacings", "sawn", "ijoists", "dead", "live", "roof_live", "roof_dead",
              "deck_live", "deck_dead", "beams", "concrete", "span", "costs", "time_limit", "gap", "fit_depth", "grade_z", "slabs", "skip_kept", "lines"]


def frame_menu(menu=None, **kw):
    """The member menu reframe() frames with: the defaults, updated by `menu` (a dict) and then by keywords. Pure: it
    records nothing (the module's purity probe).

    species, grades: the sawn lumber (Table R502.3.1(2)'s names: "DF-L", "HF", "SP", "SPF"; "SS", "#1", "#2", "#3");
    default DF-L No. 2, the West Coast default. spacings: of 12, 16, 19.2, 24 in. sawn: of "2x6".."2x12". ijoists:
    depths in inches ([] for none). dead, live: the floor's loads (psf; the tables are 10 or 20 dead, 40 or 30 live).
    roof_live, roof_dead, deck_live, deck_dead: the same for roofs and decks. beams: the beam families in the order
    tried, of "sawn" (R602.7), "lvl", "steel". concrete: concrete walls are founded supports. span: "auto" (one
    direction for the building, chosen), "u", "v", or {floor key: "u" | "v"} (other floors auto). costs: weights
    (DEFAULT_COSTS). time_limit (s), gap: the solve's. fit_depth: penalize a depth that does not fit between floors.
    grade_z: a floor at or below it stands on grade beyond the floors under it. slabs: parts of levels built as
    structural concrete slabs, not joists -- [{"floor": key, "rect": (u0, u1, v0, v1)}] (the level over that floor):
    they are left out of the joist model and listed as engineered slabs. skip_kept: leave floors whose rooms are all
    kept unframed. lines: "free" (the solver picks), or "plan" (the plan's own bearing records, exactly: the frame as
    drawn, to compare with the free one; a floor with records takes the direction they imply)."""
    m = {
        "species": "DF-L",
        "grades": ["#2"],
        "spacings": [12, 16, 19.2, 24],
        "sawn": ["2x8", "2x10", "2x12"],
        "ijoists": [9.5, 11.875, 14.0, 16.0],
        "dead": 20,
        "live": 40,
        "roof_live": 20,
        "roof_dead": 20,
        "deck_live": 60,
        "deck_dead": 25,
        "beams": ["sawn", "lvl", "steel"],
        "concrete": True,
        "span": "auto",
        "costs": dict(DEFAULT_COSTS),
        "time_limit": 30.0,
        "gap": 0.0,
        "fit_depth": True,
        "grade_z": 0.0,
        "slabs": [],
        "skip_kept": True,
        "lines": "free",
    }
    for src in [menu or {}, kw]:
        for k, v in src.items():
            if k not in _MENU_KEYS:
                fail("plan_frame.frame_menu: no menu entry %r (the entries: %s)" % (k, ", ".join(_MENU_KEYS)))
            if k == "costs":
                c = dict(m["costs"])
                c.update(v)
                m["costs"] = c
            else:
                m[k] = v
    return m


# ---------------------------------------------------------------------------------------------- small helpers


def _fail(where, msg):
    fail("plan_frame.%s: %s" % (where, msg))


def _total(xs):
    t = 0.0
    for x in xs:
        t += x
    return t


def _mean(xs):
    return _total(xs) / len(xs)


def _rects(r):
    rs = r.get("rects")
    if rs == None and r.get("rect") != None:
        rs = [r["rect"]]
    out = []
    for x in rs or []:
        out.append((float(x[0]), float(x[1]), float(x[2]), float(x[3])))
    return out


def _rect_list(pl):
    if pl == None or len(pl) == 0:
        return []
    if type(pl[0]) == "int" or type(pl[0]) == "float":
        return [(float(pl[0]), float(pl[1]), float(pl[2]), float(pl[3]))]
    return [(float(x[0]), float(x[1]), float(x[2]), float(x[3])) for x in pl]


def _xy(r, d):
    """A rect in the level's frame: x along the joists, y across them."""
    if d == "u":
        return (r[0], r[1], r[2], r[3])
    return (r[2], r[3], r[0], r[1])


def _pxy(pt, d):
    if d == "u":
        return (float(pt[0]), float(pt[1]))
    return (float(pt[1]), float(pt[0]))


def _puv(x, y, d):
    if d == "u":
        return (x, y)
    return (y, x)


def _rect_uv(x0, x1, y0, y1, d):
    if d == "u":
        return (x0, x1, y0, y1)
    return (y0, y1, x0, x1)


def _other(d):
    return "v" if d == "u" else "u"


def _merge(ivs, gap=0.0):
    out = []
    for iv in sorted(ivs):
        if out and iv[0] <= out[-1][1] + gap:
            if iv[1] > out[-1][1]:
                out[-1] = (out[-1][0], iv[1])
        else:
            out.append((iv[0], iv[1]))
    return out


def _clip(ivs, lo, hi):
    out = []
    for (a, b) in ivs:
        a2, b2 = max(a, lo), min(b, hi)
        if b2 > a2 + 1e-9:
            out.append((a2, b2))
    return out


def _len(ivs):
    t = 0.0
    for (a, b) in ivs:
        t += b - a
    return t


def _minus(ivs, cuts):
    out = list(ivs)
    for (c0, c1) in cuts:
        nxt = []
        for (a, b) in out:
            if c1 <= a or c0 >= b:
                nxt.append((a, b))
                continue
            if c0 > a:
                nxt.append((a, c0))
            if c1 < b:
                nxt.append((c1, b))
        out = nxt
    return out


def _inter_area(a, b):
    w = min(a[1], b[1]) - max(a[0], b[0])
    h = min(a[3], b[3]) - max(a[2], b[2])
    if w <= 0 or h <= 0:
        return 0.0
    return w * h


def _cost3(c, L):
    return c[0] + c[1] * L + c[2] * L * L


def _dk(D):
    return format(D, ".3f").rstrip("0").rstrip(".")


def _safe(s):
    out = ""
    for ch in s.elems():
        out += ch if (ch.isalnum() or ch == "_") else "_"
    return out


# ------------------------------------------------------------------------------------------------ plan reading


def _floors(plan):
    fs = [f for f in plan.get("floors", [])]
    return sorted(fs, key=lambda f: float(f.get("z", 0.0)))


def _indoor(plan, fkey):
    return [r for r in plan.get("rooms", []) if r.get("floor") == fkey and r.get("kind") not in OUTDOOR]


def _plate(plan, f):
    pl = _rect_list(f.get("plate"))
    if pl:
        return pl
    rs = [x for r in _indoor(plan, f["key"]) for x in _rects(r)]
    if not rs:
        return []
    return [(min([x[0] for x in rs]), max([x[1] for x in rs]), min([x[2] for x in rs]), max([x[3] for x in rs]))]


def _level_plate(plan, B, menu):
    """What the level over floor B frames with joists: B's plate less the menu's slabs on it, and less every court,
    well or slot open to the sky over B (based at or under B, its top above B's ceiling)."""
    cuts = [tuple(sl["rect"]) for sl in menu.get("slabs", []) if sl.get("floor") == B["key"]]
    zb = float(B.get("z", 0.0))
    zs = {f["key"]: float(f.get("z", 0.0)) for f in plan.get("floors", [])}
    for c in plan.get("courts", []):
        base = zs.get(c.get("base"), None)
        if base != None and base <= zb + 1e-6 and float(c.get("top", 1e9)) > zb + float(B.get("ceiling", 8.0)):
            cuts.append(tuple(c["rect"]))
    if not cuts:
        return _plate(plan, B)
    out = []
    for r in _plate(plan, B):
        out += _rect_minus(r, cuts)
    return out


def _region_at(plate_xy, x, tol=0.01):
    return _merge([(r[2], r[3]) for r in plate_xy if r[0] - tol <= x and x <= r[1] + tol], 0.05)


def _bands(plate_xy, cuts=None):
    ys = []
    for r in plate_xy:
        ys += [r[2], r[3]]
    groups = []
    for y in sorted(ys):
        if groups and y - groups[-1][-1] <= BAND_SNAP:
            groups[-1].append(y)
        else:
            groups.append([y])
    ys = [_mean(g) for g in groups]
    bands = []
    for k in range(len(ys) - 1):
        y0, y1 = ys[k], ys[k + 1]
        ym = (y0 + y1) / 2.0
        spans = _merge([(r[0], r[1]) for r in plate_xy if r[2] - 0.01 <= ym and ym <= r[3] + 0.01], 1.0)
        if not spans:
            continue
        if bands and abs(bands[-1]["y1"] - y0) < 1e-9 and _same_spans(bands[-1]["spans"], spans):
            bands[-1]["y1"] = y1
        else:
            bands.append({"y0": y0, "y1": y1, "spans": spans})
    if not cuts:
        return bands
    # split at the load's own breaks (the floor above's plate, its decks), so a band carries one kind of load
    out = []
    for bd in bands:
        pts = [bd["y0"]]
        for y in sorted(cuts):
            if y > pts[-1] + BAND_SNAP and y < bd["y1"] - BAND_SNAP:
                pts.append(y)
        pts.append(bd["y1"])
        for k in range(len(pts) - 1):
            out.append({"y0": pts[k], "y1": pts[k + 1], "spans": bd["spans"]})
    return out


def _same_spans(a, b):
    if len(a) != len(b):
        return False
    for k in range(len(a)):
        if abs(a[k][0] - b[k][0]) > 0.05 or abs(a[k][1] - b[k][1]) > 0.05:
            return False
    return True


def _clusters(plan, fkey, d):
    """The floor's walls that run across the joists, as clusters of room faces and explicit walls: x (the centre
    line), t, cover (where along y a wall stands), concrete (the parts that are concrete), the rooms on each side."""
    faces = []
    for r in _indoor(plan, fkey):
        rs = [_xy(x, d) for x in _rects(r)]
        for k in range(len(rs)):
            x0, x1, y0, y1 = rs[k]
            lo_cut, hi_cut = [], []
            for k2 in range(len(rs)):
                if k2 == k:
                    continue
                o = rs[k2]
                if abs(o[1] - x0) < 0.06:
                    lo_cut.append((o[2], o[3]))
                if abs(o[0] - x1) < 0.06:
                    hi_cut.append((o[2], o[3]))
            for (a, b) in _minus([(y0, y1)], lo_cut):
                faces.append((x0, "lo", a, b, r["key"]))
            for (a, b) in _minus([(y0, y1)], hi_cut):
                faces.append((x1, "hi", a, b, r["key"]))
    explicit = []
    for w in plan.get("walls", []):
        if w.get("floor") != fkey:
            continue
        ax, ay = _pxy(w["a"], d)
        bx, by = _pxy(w["b"], d)
        if abs(ax - bx) > 0.05:
            continue
        explicit.append(((ax + bx) / 2.0, float(w.get("t", T_DEFAULT)), min(ay, by), max(ay, by), w.get("kind", "partition")))
    items = sorted([(f[0], 0, k) for k, f in enumerate(faces)] + [(e[0], 1, k) for k, e in enumerate(explicit)])
    # single linkage: a face joins the wall the last face is in when it is within WALL_TOL of it and the wall stays
    # under WALL_SPAN thick (a partition and a concrete wall side by side are one wall mass; two rooms' walls a closet
    # apart are not)
    groups = []
    for it in items:
        if groups and it[0] - groups[-1][-1][0] <= WALL_TOL and it[0] - groups[-1][0][0] <= WALL_SPAN:
            groups[-1].append(it)
        else:
            groups.append([it])
    out = []
    for g in groups:
        los = [faces[k][0] for (x, typ, k) in g if typ == 0 and faces[k][1] == "lo"]
        his = [faces[k][0] for (x, typ, k) in g if typ == 0 and faces[k][1] == "hi"]
        exps = [explicit[k] for (x, typ, k) in g if typ == 1]
        if exps:
            xc = _mean([e[0] for e in exps])
            t = max([e[1] for e in exps])
        elif los and his:
            xl, xh = _mean(his), _mean(los)
            xc, t = (xl + xh) / 2.0, max(xh - xl, 0.0)
        elif his:
            t = T_DEFAULT
            xc = _mean(his) + t / 2.0
        else:
            t = T_DEFAULT
            xc = _mean(los) - t / 2.0
        ivs = [(faces[k][2], faces[k][3]) for (x, typ, k) in g if typ == 0] + [(e[2], e[3]) for e in exps]
        out.append({
            "x": xc,
            "t": t,
            "cover": _merge(ivs, GAP_CLOSE),
            "concrete": _merge([(e[2], e[3]) for e in exps if e[4] == "concrete"], GAP_CLOSE),
            "kinds": sorted({e[4]: True for e in exps}.keys()),
            "lo_rooms": sorted({faces[k][4]: True for (x, typ, k) in g if typ == 0 and faces[k][1] == "hi"}.keys()),
            "hi_rooms": sorted({faces[k][4]: True for (x, typ, k) in g if typ == 0 and faces[k][1] == "lo"}.keys()),
        })
    return out


def _x_sides(d):
    return ("W", "E") if d == "u" else ("S", "N")


def _openings_on(plan, fkey, d, cluster, y0, y1, what):
    """The doors (what="doors") or windows of floor fkey in the wall `cluster`, within y0..y1: (key, y, width)."""
    sides = _x_sides(d)
    reach = max(0.6, cluster["t"] / 2.0 + 0.35)
    out = []
    for o in plan.get(what, []):
        if o.get("floor") != fkey or o.get("wall") not in sides or o.get("at") == None:
            continue
        x, y = _pxy(o["at"], d)
        if abs(x - cluster["x"]) > reach or y < y0 - 0.1 or y > y1 + 0.1:
            continue
        out.append((o.get("key", "?"), y, float(o.get("width", 3.0))))
    return out


def _straddled(plan, fkey, d, x, y0, y1):
    out = []
    for r in _indoor(plan, fkey):
        for rr in _rects(r):
            a = _xy(rr, d)
            if a[0] + STRADDLE < x and x < a[1] - STRADDLE:
                for (p0, p1) in _clip([(a[2], a[3])], y0, y1):
                    if p1 - p0 >= 1.0:  # shorter is the line passing a room's corner, not a beam
                        out.append((r["key"], r.get("kind", "other"), p0, p1))
    return out


def _openings(plan, B, F):
    """Stair and lift openings through the level over B: where a stair or lift room of the floor above lies over one
    of B's (the same core, or the same kind), the part they share."""
    if F == None:
        return []
    lower = [r for r in _indoor(plan, B["key"]) if r.get("kind") in OPENING_KINDS]
    out = []
    for r in _indoor(plan, F["key"]):
        if r.get("kind") not in OPENING_KINDS:
            continue
        for rr in _rects(r):
            for l in lower:
                same_core = r.get("core") != None and r.get("core") == l.get("core")
                if not same_core and l.get("kind") != r.get("kind"):
                    continue
                for lr in _rects(l):
                    ix = (max(rr[0], lr[0]), min(rr[1], lr[1]), max(rr[2], lr[2]), min(rr[3], lr[3]))
                    if ix[1] - ix[0] < 1.0 or ix[3] - ix[2] < 1.0:
                        continue
                    a_ix = (ix[1] - ix[0]) * (ix[3] - ix[2])
                    if a_ix < 0.3 * min((rr[1] - rr[0]) * (rr[3] - rr[2]), (lr[1] - lr[0]) * (lr[3] - lr[2])):
                        continue
                    out.append({"room": r["key"], "below": l["key"], "kind": r["kind"], "rect": ix})
    seen, uniq = {}, []
    for o in out:
        k = "%s|%s" % (o["room"], str(o["rect"]))
        if k not in seen:
            seen[k] = True
            uniq.append(o)
    return uniq


# --------------------------------------------------------------------------------------- the level's geometry


def _unit_cost(o, costs):
    if o["family"] == "sawn":
        mat = (o["b"] * o["depth"] / o["spacing"]) / (1.5 * 9.25 / 16.0)
        lab = 16.0 / o["spacing"]
    else:
        mat = costs["ijoist_material"].get(_dk(o["depth"]) if _dk(o["depth"]) in costs["ijoist_material"] else format(o["depth"], ".1f"), 1.6) * 16.0 / o["spacing"]
        lab = 0.8 * 16.0 / o["spacing"]
    return 0.55 * mat + 0.45 * lab


def _options(menu):
    opts = joist_options(menu["species"], menu["grades"], menu["spacings"], menu["sawn"], menu["ijoists"], menu["dead"], menu["live"])
    by = {}
    for o in opts:
        o2 = dict(o)
        o2["unit"] = _unit_cost(o, menu["costs"])
        by.setdefault(o["depth"], []).append(o2)
    return {D: sorted(by[D], key=lambda o: o["unit"]) for D in by.keys()}


def _arc_class(x0, x1, y0, y1, F_plate_xy, F_decks_xy):
    A = (x1 - x0) * (y1 - y0)
    deck = _total([_inter_area((x0, x1, y0, y1), r) for r in F_decks_xy])
    if deck > 2.0 and deck > 0.02 * A:
        return "deck"
    fl = _total([_inter_area((x0, x1, y0, y1), r) for r in F_plate_xy])
    if fl > 2.0 and fl > 0.02 * A:
        return "floor"
    return "roof"


def _geometry(plan, lev, d, menu, opts):
    """Bands, the candidate lines in each band's spans, and the bays between them, for the level over lev's floor
    with joists along d."""
    B, F = lev["floor"], lev["above"]
    plate = [_xy(r, d) for r in _level_plate(plan, B, menu)]
    F_plate = [_xy(r, d) for r in _plate(plan, F)] if F != None else []
    F_decks = [_xy(x, d) for r in plan.get("rooms", []) if F != None and r.get("floor") == F["key"] and r.get("kind") == "deck" for x in _rects(r)]
    clusters = _clusters(plan, B["key"], d)
    holes = []
    for o in lev["openings"]:
        holes.append(dict(o, xy=_xy(o["rect"], d)))
    maxspan = 0.0
    for D in opts:
        for o in opts[D]:
            maxspan = max(maxspan, o["spans"]["roof"])
    bands = []
    # where the load changes (the floor above's plate, its decks), snapped to a wall of this floor across the band
    # within 1'-6" when there is one, so the bands do not slice rooms into slivers (the heavier load still governs
    # the strip that moves: an arc's class is the heaviest it touches)
    cross = [c["x"] for c in _clusters(plan, B["key"], _other(d)) if _len(c["cover"]) >= 2.0]
    cuts = []
    for y in [r[2] for r in F_plate + F_decks] + [r[3] for r in F_plate + F_decks]:
        best = None
        for cy in cross:
            if abs(cy - y) <= 1.5 and (best == None or abs(cy - y) < abs(best - y)):
                best = cy
        cuts.append(best if best != None else y)
    for bd in _bands(plate, cuts):
        y0, y1 = bd["y0"], bd["y1"]
        spans = []
        for (a, b) in bd["spans"]:
            ends = [_end(plan, B, d, clusters, a, "a", y0, y1), _end(plan, B, d, clusters, b, "b", y0, y1)]
            inner = []
            for c in clusters:
                if not (a + POS_MARGIN < c["x"] and c["x"] < b - POS_MARGIN):
                    continue
                cov = _clip(c["cover"], y0, y1)
                if _len(cov) < min(2.0, 0.25 * (y1 - y0)):
                    continue
                if _through_hole(holes, c["x"], y0, y1):
                    continue
                inner.append(_line(plan, B, d, c, y0, y1, cov))
            inner = sorted(inner, key=lambda P: P["x"])
            pos = [ends[0]] + inner + [ends[1]]
            arcs = _arcs(pos, y0, y1, F_plate, F_decks, opts, maxspan)
            if not _reaches(pos, arcs):
                # no wall lets a joist cross this span: girder lines every 2 ft, beams the whole band
                extra = []
                for k in range(1, int((b - a) / 2.0)):
                    x = a + 2.0 * k
                    if x < a + POS_MARGIN or x > b - POS_MARGIN or _through_hole(holes, x, y0, y1):
                        continue
                    if [P for P in inner if abs(P["x"] - x) < 1.0]:
                        continue
                    extra.append(_line(plan, B, d, {"x": x, "t": 0.0, "cover": [], "concrete": [], "kinds": [], "lo_rooms": [], "hi_rooms": []}, y0, y1, []))
                inner = sorted(inner + extra, key=lambda P: P["x"])
                pos = [ends[0]] + inner + [ends[1]]
                arcs = _arcs(pos, y0, y1, F_plate, F_decks, opts, maxspan)
            for k in range(len(pos)):
                pos[k]["i"] = k
            spans.append({"a": a, "b": b, "pos": pos, "arcs": arcs})
        bands.append({"y0": y0, "y1": y1, "spans": spans})
    return {"bands": bands, "holes": holes, "clusters": clusters, "plate": plate}


def _through_hole(holes, x, y0, y1):
    for h in holes:
        q = h["xy"]
        if q[0] + 0.3 < x and x < q[1] - 0.3 and min(q[3], y1) - max(q[2], y0) > 0.5:
            return True
    return False


def _end(plan, B, d, clusters, x, which, y0, y1):
    """An exterior wall at the end of a span: the joists bear on its inside face."""
    near = [c for c in clusters if abs(c["x"] - x) <= WALL_TOL]
    c = None
    for cc in near:
        if c == None or (len(cc["kinds"]) > len(c["kinds"])) or (len(cc["kinds"]) == len(c["kinds"]) and abs(cc["x"] - x) < abs(c["x"] - x)):
            c = cc  # an explicit wall (a concrete one) first, then the nearest
    t = c["t"] if (c != None and c["t"] > 0.05) else T_EXT
    xc = (x - t / 2.0) if which == "a" else (x + t / 2.0)
    cl = c if c != None else {"x": xc, "t": t, "cover": [(y0, y1)], "concrete": [], "kinds": [], "lo_rooms": [], "hi_rooms": []}
    return {
        "x": x, "xc": xc, "t": t, "off": 0.0, "end": which, "cluster": cl,
        "wall": [(y0, y1)], "concrete": _clip(cl["concrete"], y0, y1), "beams": [], "open": [],
        "doors": _openings_on(plan, B["key"], d, cl, y0, y1, "doors"),
        "windows": _openings_on(plan, B["key"], d, cl, y0, y1, "windows"),
    }


def _line(plan, B, d, c, y0, y1, cov):
    beams = _straddled(plan, B["key"], d, c["x"], y0, y1)
    gaps = _minus([(y0, y1)], _merge(cov + [(bm[2], bm[3]) for bm in beams], GAP_CLOSE))
    # a gap inside a room (where an L-shaped room's two rects meet, so no face stands there) is a beam over that room
    rest = []
    for (g0, g1) in gaps:
        if g1 - g0 < 1.5:
            continue
        ym = (g0 + g1) / 2.0
        owner = None
        for r in _indoor(plan, B["key"]):
            for rr in _rects(r):
                a = _xy(rr, d)
                if a[0] - 0.6 <= c["x"] and c["x"] <= a[1] + 0.6 and a[2] - 0.05 <= ym and ym <= a[3] + 0.05:
                    owner = r
        if owner != None:
            beams.append((owner["key"], owner.get("kind", "other"), g0, g1))
        else:
            rest.append((g0, g1))
    gaps = rest
    return {
        "x": c["x"], "xc": c["x"], "t": c["t"], "off": c["t"] / 2.0, "end": None, "cluster": c,
        "wall": cov, "concrete": _clip(c["concrete"], y0, y1), "beams": beams,
        "open": [g for g in gaps if g[1] - g[0] >= 1.5],
        "doors": _openings_on(plan, B["key"], d, c, y0, y1, "doors"),
        "windows": _openings_on(plan, B["key"], d, c, y0, y1, "windows"),
    }


def _arcs(pos, y0, y1, F_plate, F_decks, opts, maxspan):
    arcs = []
    for i in range(len(pos)):
        for k in range(i + 1, len(pos)):
            clear = pos[k]["x"] - pos[i]["x"] - pos[i]["off"] - pos[k]["off"]
            if clear < 0.5:
                continue
            if clear > maxspan + 1e-9:
                break
            cls = _arc_class(pos[i]["x"], pos[k]["x"], y0, y1, F_plate, F_decks)
            area = (pos[k]["x"] - pos[i]["x"]) * (y1 - y0)
            choice = {}
            for D in opts:
                for o in opts[D]:
                    if o["spans"][cls] + 1e-9 >= clear:
                        choice[D] = o
                        break
            if choice:
                arcs.append({"i": i, "k": k, "clear": clear, "cls": cls, "area": area, "opts": choice})
    return arcs


def _reaches(pos, arcs):
    seen = {0: True}
    for i in range(len(pos)):
        if i not in seen:
            continue
        for a in arcs:
            if a["i"] == i:
                seen[a["k"]] = True
    return (len(pos) - 1) in seen


# ------------------------------------------------------------------------------------------------------ costs


def _line_cost(P, on_grade_ivs, menu):
    c = menu["costs"]
    total = c["line"] if P["end"] == None else 0.0
    if P["end"] == None and not P["wall"]:
        total += c["girder"]
    wood = _minus(P["wall"], P["concrete"]) if menu["concrete"] else list(P["wall"])
    if P["end"] == None:
        g = _len([iv for w in wood for iv in _clip(on_grade_ivs, w[0], w[1])])
        total += c["wall"] * (_len(wood) - g) + c["wall_grade"] * g
    for bm in P["beams"]:
        L = bm[3] - bm[2]
        total += _cost3(c["beam"], L) * (c["garage"] if bm[1] == "garage" else 1.0)
    for (y0, y1) in P["open"]:
        total += _cost3(c["beam"], y1 - y0)
    for (k, y, w) in P["doors"] + P["windows"]:
        total += _cost3(c["header"], w)
    return total


# ------------------------------------------------------------------------------------------------- the solve


def _overlap(pa, pb):
    a = 0.0
    for r in pa:
        for q in pb:
            a += _inter_area(r, q)
    return a


def _levels(plan, menu):
    """One level per floor (in z order): the floor above it and the floor under it are the nearest ones whose plates
    overlap its own, so a separate building on the lot (a kept older one) stacks on its own. A floor whose rooms are all
    kept is not framed (its frame stands), unless menu skip_kept is False."""
    fl = _floors(plan)
    plates = [_plate(plan, f) for f in fl]
    out = []
    for i in range(len(fl)):
        B = fl[i]
        up, dn = None, None
        for k in range(i + 1, len(fl)):
            if float(fl[k].get("z", 0.0)) > float(B.get("z", 0.0)) and _overlap(plates[i], plates[k]) > 1.0:
                up = k
                break
        for k in range(i - 1, -1, -1):
            if float(fl[k].get("z", 0.0)) < float(B.get("z", 0.0)) and _overlap(plates[i], plates[k]) > 1.0:
                dn = k
                break
        sp = menu["span"]
        mode = sp.get(B["key"], "auto") if type(sp) == "dict" else sp
        if mode not in ["auto", "u", "v"]:
            _fail("reframe", 'span is "auto", "u" or "v" (for floor %s it is %r)' % (B["key"], mode))
        if menu["lines"] == "plan":
            axes = {b.get("axis"): True for b in plan.get("bearing", []) if b.get("floor") == B["key"]}
            if len(axes) == 1:
                mode = "u" if list(axes.keys())[0] == "v" else "v"
        rooms = [r for r in plan.get("rooms", []) if r.get("floor") == B["key"]]
        kept = menu["skip_kept"] and len(rooms) > 0 and len([r for r in rooms if not r.get("kept", False)]) == 0
        F = fl[up] if up != None else None
        out.append({
            "t": i, "floor": B, "key": B["key"], "above": F, "below": fl[dn] if dn != None else None,
            "above_t": up, "below_t": dn, "kept": kept,
            "dirs": [] if kept else (["u", "v"] if mode == "auto" else [mode]), "auto": mode == "auto" and not kept,
            "openings": _openings(plan, B, F),
        })
    return out


def _fits(lev, D, menu):
    """Whether a joist D in deep fits between the floor's ceiling and the floor above (3/4-in subfloor, 5/8-in
    ceiling and 1/8 in to spare): None when the plan does not say."""
    B, F = lev["floor"], lev["above"]
    if F == None or B.get("ceiling") == None or F.get("z") == None or B.get("z") == None:
        return None
    room = (float(F["z"]) - float(B["z"]) - float(B["ceiling"])) * 12.0
    return D + 1.5 <= room + 1e-6


def _only(plan, floors):
    """The plan cut down to `floors`: their floors, rooms, doors, windows, walls, stacks and courts (a court by its base)."""
    keep = {k: True for k in floors}
    out = dict(plan)
    out["floors"] = [f for f in plan.get("floors", []) if f["key"] in keep]
    for k in ["rooms", "doors", "windows", "walls"]:
        out[k] = [x for x in plan.get(k, []) if x.get("floor") in keep]
    out["courts"] = [c for c in plan.get("courts", []) if c.get("base") in keep]
    out["bearing"] = [b for b in plan.get("bearing", []) if b.get("floor") in keep]
    return out


def reframe(plan, menu=None, name=None, floors=None):
    """The cheapest frame for a fixed plan: bearing lines, joist sizes and spacing, beams, transfers and headers.

    Returns a copy of the plan with `bearing` (the chosen lines, as {"floor", "axis", "at", "span"} records),
    `walls` (the plan's own walls, or walls derived from its rooms, each with its kind and "bearing": True/False)
    and meta["plan_frame"]: {"status", "members" (every member, by name, with its span and size), "levels" (per floor:
    direction, depth, bays), "counts", "score", "solve", "checks", "notes", "menu"}. A floor part beyond every floor
    under it is on grade when the floor's z is at or below menu grade_z (0), else an overhang (checked against R502.3.3).
    `menu` is a dict for
    frame_menu(); `name` names the optimize feature (default "plan_frame_<plan name>").

    The plan's rooms are fixed: only the frame is chosen. A floor without `plate` is taken as its rooms' bounding
    box. `floors` (a list of floor keys) frames those floors alone, as if the others were not there (a kept building
    on the lot, a crawlspace). Raises when the plan has no floors."""
    m = frame_menu(menu)
    whole = plan
    if floors != None:
        plan = _only(plan, floors)
    levels = _levels(plan, m)
    if not levels:
        _fail("reframe", "the plan has no floors")
    opts_all = _options(m)
    nm = name
    if nm == None:
        # one name per plan and menu, so a document may frame several plans, or one plan several ways
        nm = "plan_frame_%s_%d" % (_safe(str(plan.get("name", "plan"))), abs(hash(str(m) + str(plan.get("floors")) + str(plan.get("rooms")) + str(plan.get("walls")) + str(plan.get("doors")))) % 1000000)
    p = opt.problem(name=nm)
    obj = []

    # ---- direction
    Y = {}
    if [lev for lev in levels if lev["auto"]]:
        Y["u"] = p.binary("frame:dir:u")
        Y["v"] = p.binary("frame:dir:v")
        p.constrain(Y["u"] + Y["v"], equals=1, name="frame:dir:one")

    geo = {}
    Q = {}
    for lev in levels:
        t, B = lev["t"], lev["key"]
        for d in lev["dirs"]:
            g = _geometry(plan, lev, d, m, opts_all)
            geo[(t, d)] = g
            # depth classes with a path through every span
            Ds = []
            for D in sorted(opts_all.keys()):
                ok = True
                for bd in g["bands"]:
                    for sp in bd["spans"]:
                        if not _reaches(sp["pos"], [a for a in sp["arcs"] if D in a["opts"]]):
                            ok = False
                if ok:
                    Ds.append(D)
            g["Ds"] = Ds
            for D in Ds:
                Q[(t, d, D)] = p.binary("frame:%s:depth:%s:%s" % (B, d, _dk(D)))
            qs = [Q[(t, d, D)] for D in Ds]
            if lev["auto"]:
                if qs:
                    p.constrain(opt.sum(qs) - Y[d], equals=0, name="frame:%s:dir:%s" % (B, d))
                else:
                    p.constrain(Y[d], equals=0, name="frame:%s:dir:%s:none" % (B, d))
        if not lev["auto"] and lev["dirs"]:
            qs = [Q[(t, d, D)] for d in lev["dirs"] for D in geo[(t, d)]["Ds"]]
            if not qs:
                _fail("reframe", "no joist in the menu frames floor %s's level running along %s" % (B, lev["dirs"][0]))
            p.constrain(opt.sum(qs), equals=1, name="frame:%s:depth:one" % B)

    def qsum(t, d):
        if (t, d) not in geo:
            return None
        qs = [Q[(t, d, D)] for D in geo[(t, d)]["Ds"]]
        return opt.sum(qs) if qs else None

    # ---- the bays: one flow network per band span and depth class
    X = {}  # (t, d, j, s, D, arc index) -> binary
    into = {}  # (t, d, j, s, i) -> [binaries]
    for lev in levels:
        t, B = lev["t"], lev["key"]
        for d in lev["dirs"]:
            g = geo[(t, d)]
            for D in g["Ds"]:
                fit = _fits(lev, D, m)
                extra = m["costs"]["depth_in"] * D
                if fit == False and m["fit_depth"]:
                    extra += m["costs"]["misfit"]
                for j in range(len(g["bands"])):
                    bd = g["bands"][j]
                    for s in range(len(bd["spans"])):
                        sp = bd["spans"][s]
                        n = len(sp["pos"])
                        outs = {k: [] for k in range(n)}
                        ins = {k: [] for k in range(n)}
                        for ai in range(len(sp["arcs"])):
                            a = sp["arcs"][ai]
                            o = a["opts"].get(D)
                            if o == None:
                                continue
                            x = p.binary("frame:%s:bay:%s:%s:b%d:s%d:%d-%d" % (B, d, _dk(D), j, s, a["i"], a["k"]))
                            X[(t, d, j, s, D, ai)] = x
                            obj.append((o["unit"] + extra) * a["area"] * x)
                            outs[a["i"]].append(x)
                            ins[a["k"]].append(x)
                            into.setdefault((t, d, j, s, a["k"]), []).append(x)
                        q = Q[(t, d, D)]
                        tag = "frame:%s:flow:%s:%s:b%d:s%d" % (B, d, _dk(D), j, s)
                        p.constrain(opt.sum(outs[0]) - q, equals=0, name=tag + ":from") if outs[0] else p.constrain(q, equals=0, name=tag + ":from")
                        p.constrain(opt.sum(ins[n - 1]) - q, equals=0, name=tag + ":to") if ins[n - 1] else p.constrain(q, equals=0, name=tag + ":to")
                        for k in range(1, n - 1):
                            if ins[k] or outs[k]:
                                p.constrain(opt.sum(ins[k] + [-x for x in outs[k]]) if (ins[k] and outs[k]) else (opt.sum(ins[k]) if ins[k] else -opt.sum(outs[k])), equals=0, name="%s:n%d" % (tag, k))

    def used(t, d, j, s, i):
        """The line at position i: 1 when the level runs along d and its path stops there (an expression)."""
        sp = geo[(t, d)]["bands"][j]["spans"][s]
        if i == 0 or i == len(sp["pos"]) - 1:
            return qsum(t, d)
        xs = into.get((t, d, j, s, i))
        return opt.sum(xs) if xs else None

    # ---- the frame as drawn: interior lines exactly where the plan's bearing records are
    if m["lines"] == "plan":
        for lev in levels:
            t, B = lev["t"], lev["key"]
            recs = [b for b in plan.get("bearing", []) if b.get("floor") == B]
            for d in lev["dirs"]:
                g = geo[(t, d)]
                axis = "v" if d == "u" else "u"
                for j in range(len(g["bands"])):
                    bd = g["bands"][j]
                    for s in range(len(bd["spans"])):
                        for P in bd["spans"][s]["pos"]:
                            if P["end"] != None:
                                continue
                            e = used(t, d, j, s, P["i"])
                            if e == None:
                                continue
                            hit = False
                            for b in recs:
                                sp = b.get("span") or (-1e9, 1e9)
                                if b.get("axis") == axis and abs(float(b["at"]) - P["xc"]) <= 0.5 and min(sp[1], bd["y1"]) - max(sp[0], bd["y0"]) > 0.5:
                                    hit = True
                            if hit:
                                p.constrain(e - qsum(t, d), equals=0, name="frame:%s:drawn:%s:b%d:s%d:%d" % (B, d, j, s, P["i"]))
                            else:
                                p.constrain(e, equals=0, name="frame:%s:undrawn:%s:b%d:s%d:%d" % (B, d, j, s, P["i"]))

    # ---- what each line costs, and what an exterior wall costs when the joists bear on it
    for lev in levels:
        t, B = lev["t"], lev["key"]
        below_plate = _plate(plan, lev["below"]) if lev["below"] != None else []
        for d in lev["dirs"]:
            g = geo[(t, d)]
            bp = [_xy(r, d) for r in below_plate]
            for j in range(len(g["bands"])):
                bd = g["bands"][j]
                for s in range(len(bd["spans"])):
                    sp = bd["spans"][s]
                    for P in sp["pos"]:
                        if lev["below"] == None:
                            grade = [(bd["y0"], bd["y1"])]
                        else:
                            grade = _minus([(bd["y0"], bd["y1"])], _region_at(bp, P["xc"]))
                        P["grade"] = grade
                        c = _line_cost(P, grade, m)
                        if c <= 0:
                            continue
                        e = used(t, d, j, s, P["i"])
                        if e != None:
                            obj.append(c * e)

    # ---- stair and lift openings: a header on each side across the joists that is not on a line (R502.10)
    H = {}
    for lev in levels:
        t, B = lev["t"], lev["key"]
        for d in lev["dirs"]:
            g = geo[(t, d)]
            for hi_ in range(len(g["holes"])):
                h = g["holes"][hi_]
                q = h["xy"]
                L = q[3] - q[2]
                cost = m["costs"]["stair_header_short"] if L <= 4.0 + 1e-9 else _cost3(m["costs"]["stair_header"], L)
                for side in ["lo", "hi"]:
                    xs = q[0] if side == "lo" else q[1]
                    hv = None
                    for j in range(len(g["bands"])):
                        bd = g["bands"][j]
                        if min(q[3], bd["y1"]) - max(q[2], bd["y0"]) <= 0.5:
                            continue
                        for s in range(len(bd["spans"])):
                            sp = bd["spans"][s]
                            if xs < sp["a"] - 0.5 or xs > sp["b"] + 0.5:
                                continue
                            if abs(xs - sp["a"]) <= SIDE_TOL or abs(xs - sp["b"]) <= SIDE_TOL:
                                continue  # on the exterior wall
                            near = [used(t, d, j, s, P["i"]) for P in sp["pos"] if P["end"] == None and abs(P["x"] - xs) <= SIDE_TOL + P["t"] / 2.0]
                            near = [e for e in near if e != None]
                            if hv == None:
                                hv = p.var("frame:%s:hole:%s:%d:%s" % (B, d, hi_, side), min=0, max=1)
                                obj.append(cost * hv)
                                H[(t, d, hi_, side)] = hv
                            if near:
                                p.constrain(hv - qsum(t, d) + opt.sum(near), min=0, name="frame:%s:hole:%s:%d:%s:b%d" % (B, d, hi_, side, j))
                            else:
                                p.constrain(hv - qsum(t, d), min=0, name="frame:%s:hole:%s:%d:%s:b%d" % (B, d, hi_, side, j))

    # ---- transfers: every bearing line stands on a line of the level under it (R502.4), on grade, or on concrete
    T = {}
    for lev in levels:
        t, B = lev["t"], lev["key"]
        tb = lev["below_t"]
        if tb == None or levels[tb]["kept"]:
            continue  # on grade, or on a kept floor whose own frame carries it
        lb = levels[tb]
        for d in lev["dirs"]:
            g = geo[(t, d)]
            gb = geo.get((tb, d))
            q_other = qsum(tb, _other(d))
            for j in range(len(g["bands"])):
                bd = g["bands"][j]
                for s in range(len(bd["spans"])):
                    sp = bd["spans"][s]
                    for P in sp["pos"]:
                        e = used(t, d, j, s, P["i"])
                        if e == None:
                            continue
                        over = _minus([(bd["y0"], bd["y1"])], P["grade"])
                        if m["concrete"]:
                            over = _minus(over, P["concrete"])
                        over = [iv for iv in over if iv[1] - iv[0] >= 1.0]
                        if not over:
                            continue
                        tline = None
                        for (o0, o1) in over:
                            # the level below running the other way: doubled joists under this wall
                            if q_other != None:
                                pv = p.var("frame:%s:doubled:%s:b%d:s%d:%d:%s" % (B, d, j, s, P["i"], _dk(o0)), min=0, max=1)
                                p.constrain(pv - e - q_other, min=-1, name="frame:%s:doubled:%s:b%d:s%d:%d:%s" % (B, d, j, s, P["i"], _dk(o0)))
                                obj.append(m["costs"]["doubled"] * (o1 - o0) * pv)
                                T[("doubled", t, d, j, s, P["i"], o0)] = (pv, o0, o1)
                            if gb == None:
                                continue
                            for jb in range(len(gb["bands"])):
                                bb = gb["bands"][jb]
                                l0, l1 = max(o0, bb["y0"]), min(o1, bb["y1"])
                                if l1 - l0 < 1.0:
                                    continue
                                ws = []
                                for sb in range(len(bb["spans"])):
                                    spb = bb["spans"][sb]
                                    if P["xc"] < spb["a"] - 1.0 or P["xc"] > spb["b"] + 1.0:
                                        continue
                                    for Pb in spb["pos"]:
                                        dd = abs(Pb["xc"] - P["xc"])
                                        if dd > OFFSET_MAX + 1e-9:
                                            continue
                                        eb = used(tb, d, jb, sb, Pb["i"])
                                        if eb == None:
                                            continue
                                        if dd <= 0.15:
                                            ws.append(eb)
                                            continue
                                        okD = [Q[(tb, d, D)] for D in gb["Ds"] if D / 12.0 + 1e-9 >= dd]
                                        if not okD:
                                            continue
                                        w = p.var("frame:%s:sits:%s:b%d:s%d:%d:%d:%d:%d" % (B, d, j, s, P["i"], jb, sb, Pb["i"]), min=0, max=1)
                                        p.constrain(w - eb, max=0, name="frame:%s:sits:%s:b%d:s%d:%d:%d:%d:%d:line" % (B, d, j, s, P["i"], jb, sb, Pb["i"]))
                                        p.constrain(w - opt.sum(okD), max=0, name="frame:%s:sits:%s:b%d:s%d:%d:%d:%d:%d:depth" % (B, d, j, s, P["i"], jb, sb, Pb["i"]))
                                        ws.append(w)
                                tv = p.var("frame:%s:transfer:%s:b%d:s%d:%d:%d" % (B, d, j, s, P["i"], jb), min=0, max=1)
                                terms = [tv, -e] + ws + ([q_other] if q_other != None else [])
                                p.constrain(opt.sum(terms), min=0, name="frame:%s:transfer:%s:b%d:s%d:%d:%d" % (B, d, j, s, P["i"], jb))
                                c = m["costs"]["transfer"]
                                obj.append((c[1] * (l1 - l0) + c[2] * (l1 - l0) * (l1 - l0)) * tv)
                                if tline == None:
                                    tline = p.var("frame:%s:transfer:%s:b%d:s%d:%d" % (B, d, j, s, P["i"]), min=0, max=1)
                                    obj.append(c[0] * tline)
                                p.constrain(tline - tv, min=0, name="frame:%s:transfer:%s:b%d:s%d:%d:%d:any" % (B, d, j, s, P["i"], jb))
                                T[("transfer", t, d, j, s, P["i"], jb)] = (tv, l0, l1)

    p.minimize(opt.sum(obj))
    sol = p.solve(time_limit=m["time_limit"], gap=m["gap"])
    meta = dict(whole.get("meta") or {})
    out = dict(whole)
    stats = {"status": sol.status, "objective": sol.objective if sol.ok else None, "gap": sol.gap, "nodes": sol.nodes,
             "binaries": len(X) + len(Q) + len(Y), "levels": len(levels)}
    if not sol.ok:
        meta["plan_frame"] = {"status": sol.status, "solve": stats, "members": [], "levels": [], "notes": [PLANNING],
                              "checks": [_row("frame:solve", "Frame solve", False, "the frame solve ended %s" % sol.status, "plan_frame", "fail")],
                              "counts": {}, "menu": m}
        out["meta"] = meta
        return out
    return _extract(plan, out, meta, levels, geo, Q, X, H, T, sol, m, stats)


# ----------------------------------------------------------------------------------------------- the solution


def _row(key, name, ok, detail, rule, severity, floor=None, room=None, at=None, rect=None):
    r = {"key": key, "name": name, "ok": ok, "detail": detail, "rule": rule, "severity": severity, "floor": floor, "room": room}
    if rect != None:
        r["rect"] = rect
    elif at != None:
        r["at"] = at
    return r


def _psf(cls, m):
    if cls == "deck":
        return float(m["deck_live"] + m["deck_dead"]), float(m["deck_live"])
    if cls == "roof":
        return float(m["roof_live"] + m["roof_dead"]), float(m["roof_live"])
    return float(m["live"] + m["dead"]), float(m["live"])


def _size_beam(L, plf, live, D, m, width=None, floors=1):
    """The member for a girder or beam of span L ft under plf (live part `live`) in a level D in deep, in the menu's
    family order: flush first (no deeper than the joists; a steel flange may stand 1/2 in proud for a nailer), then
    dropped. Sawn girders come from R602.7(2) (one floor or two, by building width); LVL and steel by rule."""
    cands = []
    for fam in m["beams"]:
        if fam == "sawn":
            if floors <= 2 and width != None:
                h = header_for(L, "interior", "One floor only" if floors <= 1 else "Two floors", max(width, 1.0))
                if h != None:
                    dd = SAWN[h["size"].split("-")[1]][1]
                    cands.append({"size": h["size"], "family": "sawn", "depth": dd, "jacks": h["jacks"], "util": None,
                                  "rule": "R602.7(2), %s" % ("one floor" if floors <= 1 else "two floors")})
        elif fam == "lvl":
            b = lvl_beam(L, plf, live, max_depth=D)
            if b == None:
                b = lvl_beam(L, plf, live)
            if b != None:
                cands.append(dict(b, family="lvl"))
        elif fam == "steel":
            b = steel_beam(L, plf, live, max_depth=D + 0.5)
            if b == None:
                b = steel_beam(L, plf, live)
            if b != None:
                cands.append(dict(b, family="steel"))
    for c in cands:
        c["flush"] = c["depth"] <= D + (0.5 if c["family"] == "steel" else 1e-9)
    for c in cands:
        if c["flush"]:
            return c
    return cands[0] if cands else None


def _member_size_text(z):
    if z == None:
        return "beyond the rule: an engineer's"
    s = z["size"]
    if z.get("family") in ("lvl", "steel"):
        s += " (rule of thumb)"
    if not z.get("flush", True):
        if z.get("_D") != None:
            drop = math.round((z["depth"] - z["_D"]) * 4.0) / 4.0
            s += ", dropped %s below the joists" % _inches(drop)
        else:
            s += ", dropped"
    return s


def _extract(plan, out, meta, levels, geo, Q, X, H, T, sol, m, stats):
    members, levels_out, bearing, checks, notes = [], [], [], [], [PLANNING]
    counts = {"beams": 0, "transfers": 0, "headers": 0, "stair_headers": 0, "lines": 0, "doubled": 0, "dropped": 0, "engineer": 0}
    if m["dead"] == 20:
        notes.append("Floors are planned at 20 psf dead (Table R502.3.1(2)'s heavier column). In Seismic Design Categories D0-D2, R301.2.2.2 caps a prescriptive floor at 10 psf, so a floor this heavy is engineered there. Unverified reading.")
    for lev in levels:
        if lev["kept"]:
            notes.append("Floor %s: every room on it is kept, so its frame stands as built and is not redesigned; walls over it are taken as carried." % lev["key"])
    chosen = {}
    for lev in levels:
        t = lev["t"]
        for d in lev["dirs"]:
            for D in geo[(t, d)]["Ds"]:
                if sol.value(Q[(t, d, D)]) > 0.5:
                    chosen[t] = (d, D)
    # every line used, with its bays either side, per level
    lines = {}  # t -> list of line pieces
    bays_by = {}
    for lev in levels:
        t, B = lev["t"], lev["key"]
        if t not in chosen:
            continue
        d, D = chosen[t]
        g = geo[(t, d)]
        pieces, bays = [], []
        for j in range(len(g["bands"])):
            bd = g["bands"][j]
            for s in range(len(bd["spans"])):
                sp = bd["spans"][s]
                path = []
                for ai in range(len(sp["arcs"])):
                    x = X.get((t, d, j, s, D, ai))
                    if x != None and sol.value(x) > 0.5:
                        path.append(sp["arcs"][ai])
                path = sorted(path, key=lambda a: a["i"])
                for a in path:
                    o = a["opts"][D]
                    Pi, Pk = sp["pos"][a["i"]], sp["pos"][a["k"]]
                    bays.append({"j": j, "s": s, "x0": Pi["x"], "x1": Pk["x"], "y0": bd["y0"], "y1": bd["y1"], "clear": a["clear"],
                                 "cls": a["cls"], "option": o, "i": a["i"], "k": a["k"]})
                for idx in range(len(path) + 1):
                    i = path[idx]["i"] if idx < len(path) else path[-1]["k"]
                    P = sp["pos"][i]
                    left = path[idx - 1] if idx > 0 else None
                    right = path[idx] if idx < len(path) else None
                    pieces.append({"j": j, "s": s, "P": P, "y0": bd["y0"], "y1": bd["y1"], "left": left, "right": right, "pos": sp["pos"]})
        lines[t] = pieces
        bays_by[t] = bays
    # loads: each line's own reaction, then what stacks on it from the floors above (top down), each with its extent
    for t in sorted(lines.keys(), reverse=True):
        d, D = chosen[t]
        for pc in lines[t]:
            own, live = 0.0, 0.0
            for a in [pc["left"], pc["right"]]:
                if a == None:
                    continue
                L = pc["pos"][a["k"]]["x"] - pc["pos"][a["i"]]["x"]
                tot, lv = _psf(a["cls"], m)
                own += L / 2.0 * tot
                live += L / 2.0 * lv
            pc["own"], pc["live"], pc["ups"] = own, live, []
        for tu in [lv2["t"] for lv2 in levels if lv2["below_t"] == t and lv2["t"] in lines and chosen.get(lv2["t"], (None,))[0] == d]:
            ceiling = float(levels[tu]["floor"].get("ceiling", 8.0))
            for up in lines[tu]:
                ut, ul, uf = _load_on(up, up["y0"], up["y1"])
                for pc in lines[t]:
                    if abs(up["P"]["xc"] - pc["P"]["xc"]) > D / 12.0 + 0.15:
                        continue
                    o0, o1 = max(up["y0"], pc["y0"]), min(up["y1"], pc["y1"])
                    if o1 - o0 < 0.5:
                        continue
                    pc["ups"].append((o0, o1, ut + WALL_PSF * ceiling, ul, uf))
    # members, level by level; each named by the level it is in ("G B2"), a transfer by the level under the wall
    counters = {}
    for lev in levels:
        t, B = lev["t"], lev["key"]
        if t not in chosen:
            continue
        d, D = chosen[t]
        axis = "v" if d == "u" else "u"
        g = geo[(t, d)]
        def nxt(kind, floor=None):
            f = floor if floor != None else B
            c = counters.setdefault(f, {})
            c[kind] = c.get(kind, 0) + 1
            return "%s %s%d" % (f, kind, c[kind])

        carries = lev["above"]["key"] if lev["above"] != None else "roof"
        # joist bays, merged across bands where they match
        merged = []
        for b in sorted(bays_by[t], key=lambda b: (b["x0"], b["x1"], b["y0"])):
            o = b["option"]
            if merged and abs(merged[-1]["x0"] - b["x0"]) < 0.05 and abs(merged[-1]["x1"] - b["x1"]) < 0.05 and \
               merged[-1]["option"]["size"] == o["size"] and merged[-1]["option"]["spacing"] == o["spacing"] and \
               abs(merged[-1]["y1"] - b["y0"]) < 0.05 and merged[-1]["cls"] == b["cls"]:
                merged[-1]["y1"] = b["y1"]
                merged[-1]["clear"] = max(merged[-1]["clear"], b["clear"])
            else:
                merged.append(dict(b))
        bays_out = []
        for b in merged:
            o = b["option"]
            mx = o["spans"][b["cls"]]
            name = nxt("J")
            members.append({
                "name": name, "kind": "joists", "floor": B, "level": "over " + B, "carries": carries if b["cls"] != "deck" else "deck",
                "load": b["cls"], "size": "%s @ %s in" % (o["size"], _dk(float(o["spacing"]))), "family": o["family"],
                "grade": o["grade"], "depth": o["depth"], "spacing": o["spacing"], "span": b["clear"], "max_span": mx,
                "rect": _rect_uv(b["x0"], b["x1"], b["y0"], b["y1"], d), "axis": d,
                "rule": ("R502.3.1(2)" if b["cls"] == "floor" else ("R802.4.1(2)" if b["cls"] == "roof" else "R502.3.1(2) x0.86 (deck rule)")) if o["family"] == "sawn" else "rule of thumb",
                "text": "%s, %s clear (%s allowed)" % ("%s @ %s in" % (o["size"], _dk(float(o["spacing"]))), ftin(b["clear"]), ftin(mx)),
            })
            bays_out.append(name)
        for sl in m.get("slabs", []):
            if sl.get("floor") != B:
                continue
            q = tuple(sl["rect"])
            members.append({
                "name": nxt("C"), "kind": "slab", "floor": B, "level": "over " + B, "carries": carries, "rect": q, "axis": d,
                "span": min(q[1] - q[0], q[3] - q[2]), "size": "structural concrete slab (engineered)", "rule": "engineered",
                "text": "structural concrete slab, %s x %s, spanning %s (engineered)" % (ftin(q[1] - q[0]), ftin(q[3] - q[2]), ftin(min(q[1] - q[0], q[3] - q[2]))),
            })
        # lines: bearing walls, beams, headers
        used_lines = {}
        bpieces = []
        seen_openings = {}
        for pc in lines[t]:
            P, y0, y1 = pc["P"], pc["y0"], pc["y1"]
            width = 0.0
            for a in [pc["left"], pc["right"]]:
                if a != None:
                    width += pc["pos"][a["k"]]["x"] - pc["pos"][a["i"]]["x"]
            if P["end"] == None:
                width = width  # an interior line carries half of each bay: R602.7(2)'s building width is the two bays
            key = "%s|%s" % (_dk(P["xc"]), P["end"] or "")
            used_lines.setdefault(key, []).append((y0, y1, P))
            if P["end"] == None:
                counts["lines"] += 1
            for bm in P["beams"]:
                tot, live, fl = _load_on(pc, bm[2], bm[3])
                bpieces.append({"xc": P["xc"], "room": bm[0], "kind": bm[1], "y0": bm[2], "y1": bm[3], "tot": tot, "live": live, "fl": fl, "width": width})
            for (y_a, y_b) in P["open"]:
                tot, live, fl = _load_on(pc, y_a, y_b)
                bpieces.append({"xc": P["xc"], "room": None, "kind": None, "y0": y_a, "y1": y_b, "tot": tot, "live": live, "fl": fl, "width": width})
            wall_kind = "exterior" if P["end"] != None else "interior"
            for (k, y, w) in P["doors"] + P["windows"]:
                if k in seen_openings:
                    continue  # an opening on a band's edge is found by both bands
                seen_openings[k] = True
                rough = w + 0.5
                tot, live, fl = _load_on(pc, y - rough / 2.0, y + rough / 2.0)
                if P["end"] == None:
                    h = header_for(rough, "interior", "One floor only" if fl <= 1 else "Two floors", max(width, 1.0)) if fl <= 2 else None
                else:
                    above = 0
                    for F2 in _floors(plan)[t + 1:]:
                        pt = _puv(P["xc"], y, d)
                        if [r for r in _plate(plan, F2) if r[0] - 0.6 <= pt[0] and pt[0] <= r[1] + 0.6 and r[2] - 0.6 <= pt[1] and pt[1] <= r[3] + 0.6]:
                            above += 1
                    center = len([pp for pp in lines[t] if pp["P"]["end"] == None and pp["j"] == pc["j"] and pp["s"] == pc["s"]]) > 0
                    case = {(0, True): "Roof and ceiling", (0, False): "Roof and ceiling", (1, True): "Roof, ceiling and one center bearing floor",
                            (1, False): "Roof, ceiling and one clear span floor", (2, True): "Roof, ceiling and two center bearing floors",
                            (2, False): "Roof, ceiling, and two clear span floors"}.get((above, center))
                    bw = pc["pos"][-1]["x"] - pc["pos"][0]["x"]
                    h = header_for(rough, "exterior", case, bw) if case != None else None
                in_conc = [iv for iv in P["concrete"] if iv[0] - 0.1 <= y and y <= iv[1] + 0.1]
                counts["headers"] += 1
                nmh = nxt("H")
                if in_conc and m["concrete"]:
                    size, rule = "a concrete lintel (engineered)", "concrete wall"
                elif h != None:
                    size, rule = "%s, %d jack%s each end" % (h["size"], h["jacks"], "" if h["jacks"] == 1 else "s"), h["table"]
                else:
                    z = _size_beam(rough, tot, live, 99.0, m, width=None, floors=3)
                    size, rule = _member_size_text(z), (z["rule"] if z != None else "none: engineer")
                members.append({
                    "name": nmh, "kind": "header", "floor": B, "level": "over " + B, "opening": k, "wall": wall_kind,
                    "axis": axis, "at": P["xc"], "range": (y - w / 2.0, y + w / 2.0), "a": _puv(P["xc"], y - w / 2.0, d),
                    "b": _puv(P["xc"], y + w / 2.0, d), "span": rough, "plf": tot, "size": size, "rule": rule,
                    "text": "%s over %s (%s rough opening) in the %s bearing wall at %s %s" % (size, k, ftin(rough), wall_kind, "u" if d == "u" else "v", ftin(P["xc"])),
                })
        # beams: the pieces each band found, merged along their line where they meet over the same room
        for bm in _merge_pieces(bpieces, ["room"]):
            L = bm["y1"] - bm["y0"]
            z = _size_beam(L, bm["tot"], bm["live"], D, m, width=bm["width"], floors=bm["fl"])
            if z != None:
                z["_D"] = D
            counts["beams"] += 1
            if z == None:
                counts["engineer"] += 1
            elif not z.get("flush", True):
                counts["dropped"] += 1
            nmb = nxt("B")
            where = bm["room"] if bm["room"] != None else "an open stretch"
            members.append({
                "name": nmb, "kind": "beam", "floor": B, "level": "over " + B, "over": bm["room"], "room_kind": bm["kind"],
                "axis": axis, "at": bm["xc"], "range": (bm["y0"], bm["y1"]), "a": _puv(bm["xc"], bm["y0"], d), "b": _puv(bm["xc"], bm["y1"], d),
                "span": L, "plf": bm["tot"], "size": _member_size_text(z), "member": z, "rule": z["rule"] if z != None else "none: engineer",
                "text": "%s over %s at %s %s, %s span, %s plf" % (_member_size_text(z), where, "u" if d == "u" else "v", ftin(bm["xc"]), ftin(L), format(bm["tot"], ",.0f")),
            })
            if z == None:
                checks.append(_row("frame:%s:%s:engineer" % (B, nmb), "%s needs an engineer" % nmb, False,
                                   "the beam over %s spans %s under %s plf: past the rule of thumb" % (where, ftin(L), format(bm["tot"], ",.0f")),
                                   "plan_frame", "fail", B, bm["room"], at=_puv(bm["xc"], (bm["y0"] + bm["y1"]) / 2.0, d)))
        # the bearing lines, merged along each line
        for key in sorted(used_lines.keys()):
            segs = sorted(used_lines[key], key=lambda s: s[0])
            P = segs[0][2]
            spans_ = _merge([(s[0], s[1]) for s in segs], 0.05)
            for (y0, y1) in spans_:
                bearing.append({"floor": B, "axis": axis, "at": P["xc"], "span": (y0, y1),
                                "kind": "exterior" if P["end"] != None else ("concrete" if _len(_clip(P["concrete"], y0, y1)) > 0.8 * (y1 - y0) else "interior"),
                                "carries": carries})
        # stair and lift headers
        for (tt, dd, hi_, side), hv in H.items():
            if tt != t or dd != d or sol.value(hv) < 0.5:
                continue
            h = g["holes"][hi_]
            q = h["xy"]
            L = q[3] - q[2]
            xs = q[0] if side == "lo" else q[1]
            tail = _tail(bays_by[t], xs, q, side)
            tot, live = _psf("floor", m)
            plf = tail / 2.0 * tot
            counts["stair_headers"] += 1
            nms = nxt("S")
            if L <= 4.0 + 1e-9:
                size, rule = "a single joist, the joists' size", "R502.10"
            else:
                z = _size_beam(L, plf, tail / 2.0 * live, D, m)
                if z != None:
                    z["_D"] = D
                size = "doubled header and trimmers: " + _member_size_text(z)
                rule = "R502.10; " + (z["rule"] if z != None else "none: engineer")
            members.append({
                "name": nms, "kind": "stair_header", "floor": B, "level": "over " + B, "opening": h["room"], "side": side,
                "axis": axis, "at": xs, "range": (q[2], q[3]), "a": _puv(xs, q[2], d), "b": _puv(xs, q[3], d), "span": L,
                "plf": plf, "size": size, "rule": rule,
                "text": "%s on the %s side of the %s opening (%s), %s tail joists" % (size, "low" if side == "lo" else "high", h["kind"], ftin(L), ftin(tail)),
            })
        # transfers and doubled joists: the pieces the bands found, merged along their line
        tpieces = []
        for kk in sorted([k for k in T.keys() if k[1] == t and k[2] == d], key=lambda k: (k[0], k[3], k[4], k[5])):
            v, l0, l1 = T[kk]
            if sol.value(v) < 0.5:
                continue
            j, s, i = kk[3], kk[4], kk[5]
            P = g["bands"][j]["spans"][s]["pos"][i]
            pc = [x for x in lines[t] if x["j"] == j and x["s"] == s and x["P"]["i"] == i]
            tl = _load_on(pc[0], l0, l1) if pc else (0.0, 0.0, 1)
            tpieces.append({"what": kk[0], "xc": P["xc"], "y0": l0, "y1": l1, "fl": 1, "width": 0.0,
                            "tot": tl[0] + WALL_PSF * float(lev["floor"].get("ceiling", 8.0)), "live": tl[1]})
        if tpieces:
            lb = levels[lev["below_t"]]
            Db = chosen.get(lev["below_t"], (None, 12.0))[1]
        for tp in _merge_pieces(tpieces, ["what"]):
            l0, l1, plf, live = tp["y0"], tp["y1"], tp["tot"], tp["live"]
            L = l1 - l0
            if tp["what"] == "doubled":
                counts["doubled"] += 1
                nmd = nxt("D", lb["key"])
                members.append({
                    "name": nmd, "kind": "doubled_joists", "floor": lb["key"], "level": "over " + lb["key"], "under": B,
                    "axis": axis, "at": tp["xc"], "range": (l0, l1), "a": _puv(tp["xc"], l0, d), "b": _puv(tp["xc"], l1, d), "span": L,
                    "plf": plf, "size": "doubled joists under the wall", "rule": "R502.4",
                    "text": "doubled joists in the level over %s, under %s's bearing wall at %s %s, %s" % (lb["key"], B, "u" if d == "u" else "v", ftin(tp["xc"]), ftin(L)),
                })
                continue
            counts["transfers"] += 1
            nmt = nxt("T", lb["key"])
            z = _size_beam(L, plf, live, Db, m)
            if z != None:
                z["_D"] = Db
            near = _nearest_support(lines.get(lev["below_t"], []), tp["xc"], l0, l1)
            members.append({
                "name": nmt, "kind": "transfer", "floor": lb["key"], "level": "over " + lb["key"], "under": B,
                "axis": axis, "at": tp["xc"], "range": (l0, l1), "a": _puv(tp["xc"], l0, d), "b": _puv(tp["xc"], l1, d), "span": L,
                "plf": plf, "offset": near, "size": _member_size_text(z), "member": z, "rule": "R502.4; " + (z["rule"] if z != None else "none: engineer"),
                "text": "%s under %s's bearing wall at %s %s (%s long, %s plf): %s" % (
                    _member_size_text(z), B, "u" if d == "u" else "v", ftin(tp["xc"]), ftin(L), format(plf, ",.0f"),
                    ("the nearest line below is %s away; R502.4 allows %s" % (ftin(near), ftin(Db / 12.0))) if near != None else "no line below it"),
            })
            checks.append(_row("frame:%s:%s" % (lb["key"], nmt), "%s: a transfer" % nmt, False,
                               "%s's bearing wall at %s %s (%s long) does not land on a line of the level over %s (R502.4 allows %s): %s" % (
                                   B, "u" if d == "u" else "v", ftin(tp["xc"]), ftin(L), lb["key"], ftin(Db / 12.0), _member_size_text(z)),
                               "R502.4", "warn", lb["key"], None, at=_puv(tp["xc"], (l0 + l1) / 2.0, d)))
        # R502.3.3: the floor above standing out past this floor's walls, and not on grade, is a cantilever
        F = lev["above"]
        if F != None and float(F.get("z", 0.0)) > m["grade_z"] + 1e-9:
            lower = [r for lv2 in levels[:t + 1] for r in _plate(plan, lv2["floor"])]
            for fr_ in _plate(plan, F):
                for q in _rect_minus(fr_, lower):
                    dd = min(q[1] - q[0], q[3] - q[2])
                    if dd <= D / 12.0 + 0.05 or max(q[1] - q[0], q[3] - q[2]) < 1.0:
                        continue
                    checks.append(_row("frame:%s:cantilever:%s" % (B, _dk(q[0]) + "," + _dk(q[2])), "%s overhangs %s" % (F["key"], B), False,
                                       "%s's floor stands %s past %s's walls (%s x %s): a cantilever beyond the %s joist depth (R502.3.3) is engineered, or follows Table R502.3.3(1) under a bearing wall and roof only" % (
                                           F["key"], ftin(dd), B, ftin(q[1] - q[0]), ftin(q[3] - q[2]), _inches(D)),
                                       "R502.3.3", "warn", B, None, rect=q))
        fit = _fits(lev, D, m)
        if fit == False:
            checks.append(_row("frame:%s:depth" % B, "%s's joists fit" % B, False,
                               "%s joists, a 3/4-in subfloor and a 5/8-in ceiling do not fit between %s's %s ceiling and the floor above" % (
                                   _inches(D), B, ftin(float(lev["floor"]["ceiling"]))), "plan_frame", "fail", B))
        levels_out.append({"floor": B, "carries": carries, "direction": d, "lines_axis": axis, "depth": D,
                           "bays": bays_out, "openings": [h["room"] for h in g["holes"]], "fits": fit})
    walls = _walls_out(plan, levels, geo, chosen, bearing)
    checks.append(_row("frame:planning", "The frame is a planning aid", True, PLANNING, "plan_frame", "note"))
    score = sol.objective
    framed = {lev["key"]: True for lev in levels}
    out["bearing"] = bearing + [b for b in (out.get("bearing") or []) if b.get("floor") not in framed]
    out["walls"] = walls + [w for w in (out.get("walls") or []) if w.get("floor") not in framed]
    meta["plan_frame"] = {
        "status": sol.status, "members": members, "levels": levels_out, "counts": counts, "score": score,
        "solve": stats, "checks": checks, "notes": notes, "menu": m,
    }
    out["meta"] = meta
    return out


def _rect_minus(r, cuts):
    """Rect r less the union of rects `cuts`, as rects on the grid of their edges (merged along each column)."""
    xs = sorted({x: 1 for x in [r[0], r[1]] + [c[0] for c in cuts if r[0] < c[0] and c[0] < r[1]] + [c[1] for c in cuts if r[0] < c[1] and c[1] < r[1]]}.keys())
    ys = sorted({y: 1 for y in [r[2], r[3]] + [c[2] for c in cuts if r[2] < c[2] and c[2] < r[3]] + [c[3] for c in cuts if r[2] < c[3] and c[3] < r[3]]}.keys())
    out = []
    for i in range(len(xs) - 1):
        col = []
        for j in range(len(ys) - 1):
            cx, cy = (xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0
            inside = False
            for c in cuts:
                if c[0] - 0.05 <= cx and cx <= c[1] + 0.05 and c[2] - 0.05 <= cy and cy <= c[3] + 0.05:
                    inside = True
                    break
            if inside:
                continue
            if col and abs(col[-1][3] - ys[j]) < 1e-9:
                col[-1] = (col[-1][0], col[-1][1], col[-1][2], ys[j + 1])
            else:
                col.append((xs[i], xs[i + 1], ys[j], ys[j + 1]))
        out += col
    # and side by side, where neighbouring columns' pieces run the same way
    merged = []
    for q in sorted(out, key=lambda q: (q[2], q[3], q[0])):
        if merged and abs(merged[-1][1] - q[0]) < 1e-9 and abs(merged[-1][2] - q[2]) < 1e-9 and abs(merged[-1][3] - q[3]) < 1e-9:
            merged[-1] = (merged[-1][0], q[1], q[2], q[3])
        else:
            merged.append(q)
    return merged


def _merge_pieces(pieces, same):
    """Pieces of one member that the bands cut apart -- the same line (x within 0.05 ft), the same `same` fields, and
    ranges that meet (within 0.6 ft: a crossing wall) -- as one, with the heaviest load of its pieces."""
    out = []
    for pc in sorted(pieces, key=lambda q: (q["xc"], str([q.get(k) for k in same]), q["y0"])):
        last = out[-1] if out else None
        if last != None and abs(last["xc"] - pc["xc"]) < 0.05 and [last.get(k) for k in same] == [pc.get(k) for k in same] and pc["y0"] <= last["y1"] + 0.6:
            last["y1"] = max(last["y1"], pc["y1"])
            for k in ["tot", "live"]:
                last[k] = max(last[k], pc[k])
            last["fl"] = max(last["fl"], pc["fl"])
            last["width"] = max(last["width"], pc["width"])
        else:
            out.append(dict(pc))
    return out


def _load_on(pc, r0, r1):
    """The uniform load (total plf, live plf) on a member from r0 to r1 of a line piece: the line's own reaction, plus
    each wall standing on it from above in proportion to how much of the member it covers; and how many floors the
    member carries (its own, plus one for each stacked wall over at least a quarter of it)."""
    L = max(r1 - r0, 0.01)
    tot, live, fl = pc["own"], pc["live"], 1
    for (a, b, w, lv, f) in pc.get("ups", []):
        ov = min(b, r1) - max(a, r0)
        if ov <= 0.05:
            continue
        frac = min(1.0, ov / L)
        tot += w * frac
        live += lv * frac
        if frac >= 0.25:
            fl = max(fl, f + 1)
    return tot, live, fl


def _tail(bays, xs, q, side):
    """The length of the tail joists that frame into an opening's header: the bay beyond the side, from the side to
    that bay's far line."""
    best = 0.0
    for b in bays:
        if min(b["y1"], q[3]) - max(b["y0"], q[2]) <= 0.5:
            continue
        if b["x0"] - 0.05 <= xs and xs <= b["x1"] + 0.05:
            L = (xs - b["x0"]) if side == "lo" else (b["x1"] - xs)
            best = max(best, L)
    return best


def _nearest_support(pieces, xc, l0, l1):
    best = None
    for pc in pieces:
        if min(pc["y1"], l1) - max(pc["y0"], l0) < 0.5:
            continue
        dd = abs(pc["P"]["xc"] - xc)
        if best == None or dd < best:
            best = dd
    return best


def _walls_out(plan, levels, geo, chosen, bearing):
    """The plan's walls with "bearing" marked: its own walls if it has any, else walls derived from its rooms (one per
    cluster run, both ways), each with a kind."""
    def is_bearing(fkey, a, b):
        for br in bearing:
            if br["floor"] != fkey:
                continue
            if br["axis"] == "v" and abs(a[0] - b[0]) < 0.05 and abs(a[0] - br["at"]) < 0.6:
                if min(max(a[1], b[1]), br["span"][1]) - max(min(a[1], b[1]), br["span"][0]) > 0.5:
                    return True
            if br["axis"] == "u" and abs(a[1] - b[1]) < 0.05 and abs(a[1] - br["at"]) < 0.6:
                if min(max(a[0], b[0]), br["span"][1]) - max(min(a[0], b[0]), br["span"][0]) > 0.5:
                    return True
        return False

    own = plan.get("walls") or []
    out = []
    for w in own:
        x = dict(w)
        x["bearing"] = is_bearing(w.get("floor"), w["a"], w["b"])
        if x["bearing"] and x.get("kind", "partition") == "partition":
            x["kind"] = "bearing"
        out.append(x)
    # and walls derived from the rooms wherever the plan has none of its own (a cluster holding an explicit wall is
    # that wall)
    for lev in levels:
        B = lev["key"]
        plate = _plate(plan, lev["floor"])
        for d in ["u", "v"]:
            pxy = [_xy(r, d) for r in plate]
            for c in _clusters(plan, B, d):
                if c["kinds"]:
                    continue
                for (y0, y1) in c["cover"]:
                    if y1 - y0 < 0.5:
                        continue
                    a, b = _puv(c["x"], y0, d), _puv(c["x"], y1, d)
                    ext = False
                    for (r0, r1) in [(r[0], r[1]) for r in pxy if r[2] - 0.5 <= y1 and y0 <= r[3] + 0.5]:
                        if abs(c["x"] - r0) < c["t"] / 2.0 + 0.6 or abs(c["x"] - r1) < c["t"] / 2.0 + 0.6:
                            ext = True
                    kind = "concrete" if "concrete" in c["kinds"] else ("exterior" if ext else "partition")
                    brg = is_bearing(B, a, b)
                    if brg and kind == "partition":
                        kind = "bearing"
                    out.append({"floor": B, "a": a, "b": b, "t": c["t"] if c["t"] > 0.05 else T_DEFAULT, "kind": kind, "bearing": brg})
    return out


# ---------------------------------------------------------------------------------------------------- reading


def frame_members(plan, kind=None, floor=None):
    """The members reframe() put in meta["plan_frame"], optionally of one kind ("joists", "beam", "header",
    "stair_header", "transfer", "doubled_joists", "slab") or one floor's level."""
    fr = (plan.get("meta") or {}).get("plan_frame")
    if fr == None:
        _fail("frame_members", "the plan has no frame: call reframe() first")
    return [x for x in fr["members"] if (kind == None or x["kind"] == kind) and (floor == None or x["floor"] == floor)]


def frame_checks(plan):
    """The frame's check rows (the @cadpole/plan build's row form): transfers, members past the rule, joists that do
    not fit, and the planning note."""
    fr = (plan.get("meta") or {}).get("plan_frame")
    if fr == None:
        _fail("frame_checks", "the plan has no frame: call reframe() first")
    return fr["checks"]


def frame_text(plan):
    """The frame as plain lines: per level its direction and depth, then every member by name, span and size."""
    fr = (plan.get("meta") or {}).get("plan_frame")
    if fr == None:
        _fail("frame_text", "the plan has no frame: call reframe() first")
    lines = ["Frame of %s (%s): %s" % (plan.get("name", "plan"), fr["status"], PLANNING)]
    c = fr["counts"]
    lines.append("  %d interior bearing line pieces, %d beams, %d transfers, %d headers, %d stair headers%s" % (
        c.get("lines", 0), c.get("beams", 0), c.get("transfers", 0), c.get("headers", 0), c.get("stair_headers", 0),
        (", %d need an engineer" % c["engineer"]) if c.get("engineer") else ""))
    for lv in fr["levels"]:
        lines.append("Level over %s (carries %s): joists along %s, %s in deep" % (lv["floor"], lv["carries"], lv["direction"], _dk(lv["depth"])))
        for x in fr["members"]:
            if x["level"] == "over " + lv["floor"]:
                lines.append("  %s %s: %s" % (x["name"], x["kind"].replace("_", " "), x["text"]))
    return "\n".join(lines)


# ----------------------------------------------------------------------------------------------- framing plans

FRAME_STYLE = {
    "joist": "#5c4a32",
    "bearing": "#8a4b2a",
    "beam": "#c26a00",
    "header": "#6a3fb0",
    "stair_header": "#007a7a",
    "transfer": "#c62828",
    "doubled_joists": "#d1631f",
    "tag_fill": "#ffffff",
}

_TAG_KINDS = {"beam": "beam", "header": "header", "stair_header": "stair_header", "transfer": "transfer", "doubled_joists": "doubled_joists"}


def _tw(s, size):
    return text_width(s, size=size)


def _box_hit(a, b):
    return a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]


def _free_box(box, taken):
    for t in taken:
        if _box_hit(box, t):
            return False
    return True


def _arrow_head(tip, d, size):
    """A filled arrowhead at `tip` pointing along unit direction d."""
    nx, ny = -d[1], d[0]
    b = (tip[0] - d[0] * size, tip[1] - d[1] * size)
    return [tip, (b[0] + nx * size * 0.4, b[1] + ny * size * 0.4), (b[0] - nx * size * 0.4, b[1] - ny * size * 0.4)]


def _overlay(lay, p, fr, lv):
    """The level's frame on a floor_layout(): bearing walls heavy, a span arrow per joist bay with its size, and every
    beam, header, stair header, transfer and doubled joist drawn and tagged clear of the room labels."""
    st = FRAME_STYLE
    s = lay["s"]
    mx, my = lay["margin"]
    ox, oy = lay["origin"]

    def P(q):
        return (mx + (q[0] - ox) * s, my + (q[1] - oy) * s)

    taken = [l["box"] for l in lay["labels"].values()] + [(m[2][0] - 2.5, m[2][1] - 2.5, m[2][0] + 2.5, m[2][1] + 2.5) for m in lay["marks"]]
    B = lv["floor"]
    sh, tx = lay["shapes"], lay["texts"]
    size = max(1.5, min(2.1, 0.33 * s))
    # bearing walls, heavy, under everything else of the frame
    for b in p.get("bearing", []):
        if b["floor"] != B:
            continue
        a0, a1 = (b["at"], b["span"][0]), (b["at"], b["span"][1])
        if b["axis"] == "u":
            a0, a1 = (b["span"][0], b["at"]), (b["span"][1], b["at"])
        sh.append({"z": 52, "kind": "poly", "pts": [P(a0), P(a1)], "weight": 0.9 if b["kind"] != "exterior" else 0.6, "color": st["bearing"], "dash": None})
    # structural slabs, hatched
    for m in fr["members"]:
        if m["kind"] != "slab" or m["floor"] != B:
            continue
        q = m["rect"]
        a, b = P((q[0], q[2])), P((q[1], q[3]))
        sh.append({"z": 52, "kind": "hatch", "pts": [(a[0], a[1]), (b[0], a[1]), (b[0], b[1]), (a[0], b[1])], "spacing": 1.2, "angle": 45.0, "color": "#8e8a84"})
        sh.append({"z": 52, "kind": "poly", "pts": [(a[0], a[1]), (b[0], a[1]), (b[0], b[1]), (a[0], b[1]), (a[0], a[1])], "weight": 0.35, "color": "#8e8a84", "dash": None})
        tx.append({"z": 64, "s": "%s  slab (engineered)" % m["name"].split(" ")[1], "at": ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0), "size": size, "bold": False, "color": "#5f5b55", "align": "center", "valign": "middle", "rotation": 0.0, "halo": "#ffffff"})
    # joist bays: an arrow across each, its size beside it
    for m in fr["members"]:
        if m["kind"] != "joists" or m["floor"] != B:
            continue
        r = m["rect"]
        along_u = m["axis"] == "u"
        L0, L1 = (r[0], r[1]) if along_u else (r[2], r[3])
        C0, C1 = (r[2], r[3]) if along_u else (r[0], r[1])
        mark = m["name"].split(" ")[1]
        dvec = (1.0, 0.0) if along_u else (0.0, 1.0)
        ah = max(1.2, min(2.0, 0.3 * s))
        drawn = None
        for label in ["%s  %s" % (mark, m["size"]), mark]:
            lw = _tw(label, size)
            for f in [0.5, 0.32, 0.68, 0.18, 0.82]:
                c = C0 + (C1 - C0) * f
                a, b = ((L0 + 0.25, c), (L1 - 0.25, c)) if along_u else ((c, L0 + 0.25), (c, L1 - 0.25))
                pa, pb = P(a), P(b)
                length = abs(pb[0] - pa[0]) + abs(pb[1] - pa[1])
                mid = ((pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0)
                if along_u:
                    box = (mid[0] - lw / 2.0 - 0.5, mid[1] + 0.5, mid[0] + lw / 2.0 + 0.5, mid[1] + 0.5 + size + 1.0)
                else:
                    box = (mid[0] - size - 1.5, mid[1] - lw / 2.0 - 0.5, mid[0] - 0.5, mid[1] + lw / 2.0 + 0.5)
                if lw + 2.0 <= length and _free_box(box, taken):
                    drawn = (pa, pb, mid, label, box)
                    break
            if drawn != None:
                break
        if drawn == None:
            # nowhere clear for even the mark: the arrow alone at mid-bay (the schedule has the bay)
            c = (C0 + C1) / 2.0
            a, b = ((L0 + 0.25, c), (L1 - 0.25, c)) if along_u else ((c, L0 + 0.25), (c, L1 - 0.25))
            pa, pb = P(a), P(b)
            drawn = (pa, pb, None, None, None)
        pa, pb, mid, label, box = drawn
        sh.append({"z": 53, "kind": "poly", "pts": [pa, pb], "weight": 0.25, "color": st["joist"], "dash": None})
        sh.append({"z": 53, "kind": "fillpoly", "pts": _arrow_head(pb, dvec, ah), "color": st["joist"]})
        sh.append({"z": 53, "kind": "fillpoly", "pts": _arrow_head(pa, (-dvec[0], -dvec[1]), ah), "color": st["joist"]})
        if label != None:
            if along_u:
                tx.append({"z": 64, "s": label, "at": (mid[0], mid[1] + 0.8), "size": size, "bold": False, "color": st["joist"], "align": "center", "valign": "bottom", "rotation": 0.0, "halo": "#ffffff"})
            else:
                tx.append({"z": 64, "s": label, "at": (mid[0] - 0.8, mid[1]), "size": size, "bold": False, "color": st["joist"], "align": "center", "valign": "bottom", "rotation": 90.0, "halo": None})
            taken.append(box)
    # beams, headers, stair headers, transfers, doubled joists
    for m in fr["members"]:
        k = m["kind"]
        if k not in _TAG_KINDS or m["floor"] != B:
            continue
        col = st[_TAG_KINDS[k]]
        pa, pb = P(m["a"]), P(m["b"])
        w = {"beam": 0.8, "header": 0.9, "stair_header": 0.8, "transfer": 1.0, "doubled_joists": 0.5}[k]
        dash = {"beam": [3.0, 1.0], "transfer": [2.0, 0.8], "doubled_joists": [1.0, 0.6]}.get(k)
        sh.append({"z": 54, "kind": "poly", "pts": [pa, pb], "weight": w, "color": col, "dash": dash})
        tag = m["name"].split(" ")[1]
        tw = _tw(tag, size) + 1.2
        mid = ((pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0)
        vert = abs(pb[0] - pa[0]) < abs(pb[1] - pa[1])
        cands = []
        for off in [2.2, 4.5, 7.0, 10.0]:
            for sgn in [1.0, -1.0]:
                for slide in [0.0, 0.3, -0.3]:
                    cx = mid[0] + (sgn * off if vert else (pb[0] - pa[0]) * slide)
                    cy = mid[1] + ((pb[1] - pa[1]) * slide if vert else sgn * off)
                    cands.append((cx, cy))
        best = cands[0]
        for c in cands:
            bx = (c[0] - tw / 2.0, c[1] - size * 0.75, c[0] + tw / 2.0, c[1] + size * 0.75)
            if _free_box(bx, taken):
                best = c
                break
        bx = (best[0] - tw / 2.0, best[1] - size * 0.75, best[0] + tw / 2.0, best[1] + size * 0.75)
        taken.append(bx)
        sh.append({"z": 65, "kind": "poly", "pts": [mid, best], "weight": 0.15, "color": col, "dash": None})
        sh.append({"z": 66, "kind": "fill", "pts": [(bx[0], bx[1]), (bx[2], bx[1]), (bx[2], bx[3]), (bx[0], bx[3])], "color": st["tag_fill"]})
        sh.append({"z": 66, "kind": "poly", "pts": [(bx[0], bx[1]), (bx[2], bx[1]), (bx[2], bx[3]), (bx[0], bx[3]), (bx[0], bx[1])], "weight": 0.25, "color": col, "dash": None})
        tx.append({"z": 67, "s": tag, "at": best, "size": size, "bold": True, "color": col, "align": "center", "valign": "middle", "rotation": 0.0, "halo": None})
    return lay


def _schedule_rows(fr, B):
    rows = [["MARK", "MEMBER", "SIZE", "SPAN", "RULE"]]
    for m in fr["members"]:
        if m["floor"] != B:
            continue
        what = {"joists": "joists", "slab": "structural slab", "beam": "beam over %s" % (m.get("over") or "an open stretch"), "header": "header, %s" % m.get("opening"),
                "stair_header": "header, %s opening" % m.get("opening"), "transfer": "transfer under %s's wall" % m.get("under"),
                "doubled_joists": "doubled joists under %s's wall" % m.get("under")}[m["kind"]]
        rows.append([m["name"].split(" ")[1], what, m["size"], ftin(m["span"]), m["rule"]])
    return rows


def _inches(d):
    whole = int(d + 1e-9)
    part = d - whole
    for (num, den) in ((1, 4), (1, 2), (3, 4), (1, 8), (3, 8), (5, 8), (7, 8)):
        if abs(part - num / float(den)) < 1e-6:
            return ("%d %d/%d in" % (whole, num, den)) if whole else ("%d/%d in" % (num, den))
    return "%d in" % whole


_SCALE_LIST = [(0.5, "1/2\" = 1'-0\""), (0.375, "3/8\" = 1'-0\""), (0.25, "1/4\" = 1'-0\""), (0.1875, "3/16\" = 1'-0\""),
               (0.125, "1/8\" = 1'-0\""), (0.09375, "3/32\" = 1'-0\""), (0.0625, "1/16\" = 1'-0\"")]


def frame_sheets(S, p, floors=None, prefix="S", first=1, scale=None, per_sheet=None):
    """Add framing plans for plan p (reframe()'s output) to sheet set S: each level drawn over the floor whose walls
    carry it (plan_draw's floor_layout), bearing walls heavy, a span arrow per joist bay with its mark and size, every
    beam, header, stair header and transfer drawn and tagged, and the red pen for the frame's failing rows; up to three
    levels side by side on a sheet (per_sheet), at the largest architect's scale that fits (or `scale`, mm to the
    foot); in a column beside them the member schedule by level and the planning-aid note. Returns the sheets."""
    fr = (p.get("meta") or {}).get("plan_frame")
    if fr == None or fr.get("status") != "optimal":
        _fail("frame_sheets", "the plan has no frame to draw: call reframe() first (status %s)" % (fr or {}).get("status"))
    x0, y0, x1, y1 = S.area()
    col = 150.0
    box_w = x1 - x0 - col - 18.0
    box_h = y1 - y0 - 46.0
    numbered = redpen(fr["checks"])
    lvs = [lv for lv in fr["levels"] if floors == None or lv["floor"] in floors]
    feet = []
    for lv in lvs:
        bb = floor_layout(p, lv["floor"], scale=1.0, labels=False, dims=False)["bbox"]
        feet.append((bb[1] - bb[0] + 9.0, bb[3] - bb[2] + 6.0))
    gap = 16.0

    def fits(group, s):
        return _total([f[0] for f in group]) * s + gap * (len(group) - 1) <= box_w and max([f[1] for f in group]) * s <= box_h

    n = per_sheet
    if n == None:
        n = 1
        for k in [3, 2]:
            if len(feet) >= k and not [g0 for g0 in range(0, len(feet), k) if not fits(feet[g0:g0 + k], 0.25 * 25.4)]:
                n = k
                break
    sheets = []
    for g0 in range(0, len(lvs), n):
        group, gfeet = lvs[g0:g0 + n], feet[g0:g0 + n]
        sc, sname = (scale, "AS NOTED") if scale != None else (_SCALE_LIST[-1][0] * 25.4, _SCALE_LIST[-1][1])
        if scale == None:
            for inch, nm in _SCALE_LIST:
                if fits(gfeet, inch * 25.4):
                    sc, sname = inch * 25.4, nm
                    break
        lays = []
        for lv in group:
            lay = floor_layout(p, lv["floor"], scale=sc, numbers=numbered, details=False, fixtures=False, furniture=False)
            lays.append(_overlay(lay, p, fr, lv))
        num = "%s%d.%d" % (prefix, 1, first + len(sheets))
        sh = S.sheet(num, "FRAMING OVER " + ", ".join([lv["floor"] for lv in group]), scale=sname)
        total = _total([l["w"] for l in lays]) + gap * (len(lays) - 1)
        x = x0 + 8.0 + max(0.0, (box_w - total) / 2.0)
        rows, bold = [["MARK", "MEMBER", "SIZE", "SPAN", "RULE"]], [0]
        for i in range(len(group)):
            lv, lay = group[i], lays[i]
            yb = y0 + 34.0 + max(0.0, (box_h - lay["h"]) / 2.0)
            paint(sh, lay, (x, yb))
            sh.view_title(str(i + 1), "OVER %s, CARRYING %s: JOISTS ALONG %s, %s" % (lv["floor"], str(lv["carries"]).upper(), lv["direction"].upper(), _inches(lv["depth"]).upper()),
                          at=(x, yb - 10.0), width=min(170.0, max(90.0, lay["w"])), scale=sname)
            x += lay["w"] + gap
            bold.append(len(rows))
            rows.append(["%s" % lv["floor"], "the level over %s" % lv["floor"], "", "", ""])
            rows += _schedule_rows(fr, lv["floor"])[1:]
        cx = x1 - col
        y = y1 - 10.0
        sh.text("MEMBERS", at=(cx, y), size=2.4, bold=True)
        y = sh.table(rows, at=(cx, y - 3.0), col_widths=[11.0, 40.0, 46.0, 14.0, 33.0], size=1.6, bold_rows=bold) - 5.0
        legend = [("heavy", "bearing wall: carries the level"), ("line", "joists: span, mark and size"),
                  ("dashed", "beam (B) or transfer (T)"), ("heavy", "header (H) or stair header (S)")]
        y = sh.legend(legend, at=(cx, y), size=1.8, title="FRAMING") - 4.0
        rows_here = [(nn, r) for nn, r in numbered if r.get("floor") in [lv["floor"] for lv in group]]
        if rows_here:
            sh.text("RED PEN", at=(cx, y), size=2.2, bold=True, color="#c62828")
            y -= 4.0
            for nn, r in rows_here:
                y = sh.note("%d. %s: %s" % (nn, r["name"], r["detail"]), at=(cx, y), width=col - 6.0, size=1.7) - 1.0
            y -= 2.0
        y = sh.note(PLANNING, at=(cx, y), width=col - 6.0, size=1.8, bold=True) - 1.0
        for nt in fr["notes"][1:]:
            y = sh.note(nt, at=(cx, y), width=col - 6.0, size=1.6) - 1.0
        sheets.append(sh)
    return sheets


def frame_set(p, name=None, project=None, stamp=None, scale=None, per_sheet=None):
    """A sheet set of plan p's framing plans (frame_sheets), ARCH D, with a title block. Export it:
    export(frame_set(reframe(plan)), "frame.pdf")."""
    nm = name or ("%s_frame" % p.get("name", "plan"))
    S = sheet_set(nm, size=sheet.arch_d, units="ft-in",
                  title_block=title_block(project=project or ("%s: FRAMING" % p.get("name", "plan")).upper().replace("_", " "),
                                          stamp=stamp or ["PLANNING AID", "NOT ENGINEERING"],
                                          notes="Framed by @cadpole/plan_frame.star reframe() from %s. Spans: CRC 2022 Tables R502.3.1(2), R802.4.1(2), R602.7. LVL and steel by rule of thumb." % (p.get("source") or "a plan value")))
    frame_sheets(S, p, scale=scale, per_sheet=per_sheet)
    return S
