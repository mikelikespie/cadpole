"""@cadpole/plan_roof.star -- PV modules in rows on one roof plane, around its obstacles and inside the fire code's pathways
and setbacks, as a small MILP.

    load("@cadpole/plan_roof.star", "pv_rows", "fire_setbacks", "roof_probe", "LOW_SLOPE")

    roof = {"rect": (0.0, 24.0, 0.0, 16.0), "slope": 0.75, "up": "N"}     # plan feet; rises 9 in a foot to the north
    pv_rows(roof, obstacles = [{"rect": (2.0, 7.0, 6.0, 9.2), "clear": 0.5}], edge = 1.5)   # a skylight, 6 in kept round it
      -> {"ok", "count", "rows": [{"orient", "s0", "s1", "n", "segments"}], "modules": [plan rects], "coverage", ...}

**The model** (any number of obstacles, and the fire code's pathways).
The plane is measured in its own frame: `x` across the slope (plan u for a roof rising north or south), `s` up the slope
from its lowest edge (the plan depth over cos(atan(slope))). Up to `max_rows` rows stack up the slope; each is
landscape (a module's short side up the slope), portrait, or unused, and holds as many modules as fit side by side in
the clear stretches of its width. Where a row crosses an obstacle's band up the slope (the obstacle grown by its
clearance), the obstacle's width is taken out of it; a pathway running eave to ridge is taken out of every row. The
capacities are counted before the solve, for every orientation and every set of obstacles a row may cross (2^n of
them), so each row's count is one exact row: n <= sum over (orientation, obstacles crossed) of CAP * binary. Rows
stack without overlapping, inside the plane less its setbacks. The objective is the module count, a thousandth of a
foot of row height breaking ties toward the eave.

**The fire code** (California Residential Code 2022 R324.6 and California Fire Code 2022 1205.2, for one- and
two-family dwellings; readings unverified, not checked with the building or fire department):

* R324.6.1 / CFC 1205.2.1.1: at least two 36-in pathways on separate roof planes from the lowest roof edge to the
  ridge; for each plane with PV, a 36-in pathway from its lowest edge to its ridge on the same plane, an adjacent one,
  or straddling the two. `pathway=True` keeps a 36-in strip of this plane clear, eave to ridge, at the best place
  across it (see `pv_rows`); it may not cross an obstacle.
* R324.6.2 / CFC 1205.2.1.2: a clear setback on both sides of a horizontal ridge of 18 in where the array covers 33
  percent or less of the plan-view total roof area, 36 in where it covers more. R324.6.2.1 / CFC 1205.2.1.3 (an
  automatic sprinkler system, NFPA 13D or R313): 18 in up to 66 percent, 36 in above. On a shed roof the ridge is its
  high edge.
* R324.6.3 / CFC 1205.2.2: no modules on the part of a roof below an emergency escape and rescue opening, and a 36-in
  pathway to it (give that part as an obstacle).
* Low-slope roofs: R324.6 and CFC 1205.2.1 do not apply to roofs of 2 in 12 (16.7 percent) or less (`LOW_SLOPE`).
  CFC 1205.2.1 also does not apply to structures built under the CRC, which R324.6 then governs.

`fire_setbacks(roof, coverage, sprinklered)` says which apply. The edge margin (1.5 ft off every
edge, the module's wind zone and the overhang's edge) is a design choice, not a code rule.

Probe for the purity test: `roof_probe()`.
"""

LOW_SLOPE = 2.0 / 12.0  # rise per run at or under which R324.6 / CFC 1205.2.1 do not apply
PATHWAY = 3.0  # ft: 36 in
RIDGE_18, RIDGE_36 = 1.5, 3.0  # ft

def roof_probe():
    """The purity probe: a pure function (the setbacks a steep, unsprinklered roof half covered needs)."""
    return fire_setbacks({"rect": (0.0, 30.0, 0.0, 20.0), "slope": 0.5, "up": "N"}, 0.5, False)

def _plane(roof):
    """(x0, x1, cos, span, to_plan): the plane across the slope (plan ft), cos of its pitch, its length up the slope,
    and a function from (x, s) to plan (u, v)."""
    u0, u1, v0, v1 = roof["rect"]
    up = roof.get("up", "N")
    k = 1.0 / math.sqrt(1.0 + roof["slope"] * roof["slope"])
    if up in ("N", "S"):
        return (u0, u1, k, (v1 - v0) / k, up)
    return (v0, v1, k, (u1 - u0) / k, up)

def _to_plan(roof, x0, x1, s0, s1):
    """A rect on the plane (x across, s up from the lowest edge) as a plan rect (u0, u1, v0, v1)."""
    u0, u1, v0, v1 = roof["rect"]
    up = roof.get("up", "N")
    k = 1.0 / math.sqrt(1.0 + roof["slope"] * roof["slope"])
    if up == "N":
        return (x0, x1, v0 + s0 * k, v0 + s1 * k)
    if up == "S":
        return (x0, x1, v1 - s1 * k, v1 - s0 * k)
    if up == "E":
        return (u0 + s0 * k, u0 + s1 * k, x0, x1)
    return (u1 - s1 * k, u1 - s0 * k, x0, x1)

def _ob_plane(roof, ob):
    """An obstacle (plan rect grown by its clearance) on the plane: (x0, x1, s0, s1)."""
    u0, u1, v0, v1 = roof["rect"]
    c = ob.get("clear", 0.0)
    a0, a1, b0, b1 = ob["rect"][0] - c, ob["rect"][1] + c, ob["rect"][2] - c, ob["rect"][3] + c
    k = 1.0 / math.sqrt(1.0 + roof["slope"] * roof["slope"])
    up = roof.get("up", "N")
    if up == "N":
        return (a0, a1, (b0 - v0) / k, (b1 - v0) / k)
    if up == "S":
        return (a0, a1, (v1 - b1) / k, (v1 - b0) / k)
    if up == "E":
        return (b0, b1, (a0 - u0) / k, (a1 - u0) / k)
    return (b0, b1, (u1 - a1) / k, (u1 - a0) / k)

def _total(xs):
    t = 0
    for x in xs:
        t += x
    return t

def _segments(x0, x1, cuts):
    """The clear stretches of [x0, x1] with the x-ranges `cuts` taken out."""
    segs = [(x0, x1)]
    for (a, b) in cuts:
        nxt = []
        for (c, d) in segs:
            if b <= c or a >= d:
                nxt.append((c, d))
                continue
            if a > c:
                nxt.append((c, a))
            if b < d:
                nxt.append((b, d))
        segs = nxt
    return segs

def _cap(segs, along, gap):
    """Modules `along` long (plus `gap` between neighbours) that fit side by side in each stretch."""
    n = 0
    for (a, b) in segs:
        n += int(math.floor((b - a + gap) / (along + gap) + 1e-9))
    return n

def fire_setbacks(roof, coverage, sprinklered = False):
    """What R324.6 asks of this plane: {"applies", "pathway", "ridge" (ft), "why"}. `coverage` is the array's share of
    the plan-view total roof area (0..1)."""
    if roof["slope"] <= LOW_SLOPE + 1e-12:
        return {"applies": False, "pathway": False, "ridge": 0.0,
                "why": "a slope of %s in 12 is 2 in 12 or less: R324.6 and CFC 1205.2.1 do not apply" % (math.round(roof["slope"] * 1200.0) / 100.0)}
    lim = 0.66 if sprinklered else 0.33
    ridge = RIDGE_18 if coverage <= lim + 1e-12 else RIDGE_36
    return {"applies": True, "pathway": True, "ridge": ridge,
            "why": "R324.6.1: a 36-in pathway eave to ridge; R324.6.2%s: %d in at the ridge for %d percent coverage (limit %d percent)" % (
                ".1 (sprinklered)" if sprinklered else "", int(ridge * 12), int(math.round(coverage * 100.0)), int(lim * 100))}

def pv_rows(roof, *, module = (5.65, 3.72), orient = ("L", "P"), uniform = False, edge = 1.5, ridge = None, obstacles = (),
            pathway = None, gap = 0.0, max_rows = 8, time_limit = 30.0, name = None):
    """The most modules this roof plane takes, in rows. `roof` = {"rect": plan (u0, u1, v0, v1), "slope": rise per foot,
    "up": the side it rises toward}. `module` (long, short) ft; `orient` the orientations allowed ("L": the long side
    across the slope); `uniform` one orientation for every row (the drawn grid's look); `edge` ft kept off every edge;
    `ridge` ft kept off the high edge (when more than `edge`); `obstacles` [{"rect": plan, "clear": ft}]; `pathway`: a
    36-in strip clear eave to ridge (True; or None/False: none); `gap` ft between modules side by side.

    With a pathway, every place across the plane where a module could start a stretch is tried (the count is a step
    function of where the strip sits, and it can only step up where a stretch's start lines up with a module edge), and
    the best is kept; the strip may not cross an obstacle.

    Returns {"ok", "count", "rows": [{"orient", "s0", "s1", "n", "cross"}], "modules": [plan rects], "pathway": (x0, x1)
    or None, "coverage": array plan area / roof plan area, "status"}."""
    x0r, x1r, k, span, up = _plane(roof)
    X0, X1 = x0r + edge, x1r - edge
    S0, S1 = edge, span - max(edge, ridge if ridge != None else 0.0)
    obs = [_ob_plane(roof, ob) for ob in obstacles]
    if not pathway:
        res = _solve(roof, module, orient, uniform, X0, X1, S0, S1, obs, None, gap, max_rows, time_limit,
                     name if name != None else "plan_roof_pv")
        return res
    # Where the strip may sit: the count is a step function of its place p, stepping up only where the stretch west of
    # it gains a module -- p = a stretch's start (the inset edge, an obstacle's far side) plus m modules -- and down
    # elsewhere; so its best is at one of those places or at the start of a stretch of places the strip may take (the
    # plane's edge, an obstacle's far side). Trying them all is exact.
    starts = [X0] + [o[1] for o in obs]
    cands = {}
    for x in [x0r] + [o[1] for o in obs]:
        cands[math.round(x * 1e6) / 1e6] = True
    for st in starts:
        for ln in (module[0], module[1]):
            for m in range(0, 64):
                x = st + m * (ln + gap) - (gap if m > 0 else 0.0)
                if x + PATHWAY > x1r + 1e-9:
                    break
                if x >= x0r - 1e-9:
                    cands[math.round(x * 1e6) / 1e6] = True
    best = None
    i = 0
    for x in sorted(cands.keys()):
        strip = (x, x + PATHWAY)
        if any([strip[0] < o[1] and strip[1] > o[0] for o in obs]):
            continue  # a pathway may not cross a skylight or a hatch
        r = _solve(roof, module, orient, uniform, X0, X1, S0, S1, obs, strip, gap, max_rows, time_limit,
                   (name if name != None else "plan_roof_pv") + "_path%d" % i)
        i += 1
        if r["ok"] and (best == None or r["count"] > best["count"]):
            best = r
    if best == None:
        return {"ok": False, "count": 0, "rows": [], "modules": [], "pathway": None, "coverage": 0.0, "status": "no pathway place"}
    return best

def _solve(roof, module, orient, uniform, X0, X1, S0, S1, obs, strip, gap, K, time_limit, name):
    L, Wm = module
    H = {"L": Wm, "P": L}  # a row's height up the slope
    A = {"L": L, "P": Wm}  # a module's length across
    n_ob = len(obs)
    subsets = []
    for mask in range(1 << n_ob):
        subsets.append([j for j in range(n_ob) if (mask >> j) & 1])
    base_cuts = [] if strip == None else [strip]
    CAP = {}
    for t in orient:
        for si in range(len(subsets)):
            cuts = base_cuts + [(obs[j][0], obs[j][1]) for j in subsets[si]]
            CAP[(t, si)] = _cap(_segments(X0, X1, cuts), A[t], gap)
    p = opt.problem(name = name)
    M = S1 + 10.0
    rows = []
    for r in range(K):
        q = {}
        for t in orient:
            for si in range(len(subsets)):
                q[(t, si)] = p.binary("r%d.%s.%d" % (r, t, si))
        p.constrain(opt.sum(list(q.values())), max = 1, name = "pv:roof:r%d:one kind" % r)
        o = {}
        for t in orient:
            o[t] = opt.sum([q[(t, si)] for si in range(len(subsets))])
        s = p.var("r%d.s" % r, min = S0, max = S1)
        h = opt.sum([H[t] * o[t] for t in orient])
        p.constrain(s + h, max = S1, name = "pv:roof:r%d:inside" % r)
        for j in range(n_ob):
            below, above = p.binary("r%d.below%d" % (r, j)), p.binary("r%d.above%d" % (r, j))
            # unless the row's kind counts obstacle j as crossed, the row is wholly below or above its band
            out_j = opt.sum([q[(t, si)] for t in orient for si in range(len(subsets)) if j not in subsets[si]])
            p.constrain(below + above - out_j, min = 0, name = "pv:roof:r%d:clear of %d" % (r, j))
            p.constrain(s + h - M * (-below + 1), max = obs[j][2], name = "pv:roof:r%d:below %d" % (r, j))
            p.constrain(s + M * (-above + 1), min = obs[j][3], name = "pv:roof:r%d:above %d" % (r, j))
        n = p.var("r%d.n" % r, min = 0, max = max(CAP.values()) if CAP else 0, integer = True)
        p.constrain(n - opt.sum([CAP[key] * q[key] for key in q]), max = 0, name = "pv:roof:r%d:capacity" % r)
        rows.append({"q": q, "o": o, "s": s, "h": h, "n": n})
    for r in range(K - 1):
        p.constrain(rows[r + 1]["s"] - rows[r]["s"] - rows[r]["h"], min = 0, name = "pv:roof:r%d:stacked" % r)
    if uniform and len(orient) > 1:
        g = p.binary("all " + orient[0])
        for r in range(K):
            p.constrain(rows[r]["o"][orient[1]] + g, max = 1, name = "pv:roof:r%d:uniform %s" % (r, orient[1]))
            p.constrain(rows[r]["o"][orient[0]] - g, max = 0, name = "pv:roof:r%d:uniform %s" % (r, orient[0]))
    p.maximize(opt.sum([r["n"] for r in rows]) - 1e-3 * opt.sum([r["s"] for r in rows]))
    sol = p.solve(time_limit = time_limit)
    if not sol.ok:
        return {"ok": False, "count": 0, "rows": [], "modules": [], "pathway": strip, "coverage": 0.0, "status": sol.status}
    out_rows = []
    mods = []
    for r in rows:
        nn = sol.value(r["n"])
        if nn == 0:
            continue
        key = [kk for kk in r["q"] if sol.value(r["q"][kk]) > 0.5][0]
        t, si = key
        s0 = sol.value(r["s"])
        s1 = s0 + H[t]
        segs = _segments(X0, X1, base_cuts + [(obs[j][0], obs[j][1]) for j in subsets[si]])
        placed = 0
        for (a, b) in segs:
            for m in range(int(math.floor((b - a + gap) / (A[t] + gap) + 1e-9))):
                if placed >= nn:
                    break
                xa = a + m * (A[t] + gap)
                mods.append(_to_plan(roof, xa, xa + A[t], s0, s1))
                placed += 1
        out_rows.append({"orient": t, "s0": s0, "s1": s1, "n": nn, "cross": subsets[si]})
    u0, u1, v0, v1 = roof["rect"]
    area = (u1 - u0) * (v1 - v0)
    cov = _total([(m[1] - m[0]) * (m[3] - m[2]) for m in mods]) / area if area > 0 else 0.0
    return {"ok": True, "count": _total([r["n"] for r in out_rows]), "rows": out_rows, "modules": mods, "pathway": strip,
            "coverage": cov, "status": sol.status, "span": S1 - S0}
