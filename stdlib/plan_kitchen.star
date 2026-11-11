"""@cadpole/plan_kitchen.star -- one kitchen run against NKBA's landing areas, as a small MILP.

    load("@cadpole/plan_kitchen.star", "kitchen_run", "check_run", "NKBA", "kitchen_probe")

    kitchen_run()                       # the shortest run that meets every landing rule: 13'-0" for a 30-in range,
                                        # a 24-in sink and a 24-in fridge
    kitchen_run(length = 126.0 / 12)    # at 10'-6", the order and counters with the least total shortfall (30 in)
    check_run(["range", "sink", "fridge"], [12, 18, 18, 0])   # a drawn run's shortfall, counter by counter (inches)

A straight run between two walls: three appliances -- a range, a sink and a fridge -- in some order, with counters
c0 | a | c1 | b | c2 | c | c3 between and beside them, and a dishwasher under the counter beside the sink. The rules
(NKBA Kitchen Planning Guidelines; readings unverified):

* KPG 11, the sink: 24 in of landing on one side and 18 in on the other;
* KPG 17, the cooking surface: 12 in on one side and 15 in on the other;
* KPG 16, the refrigerator: 15 in on its handle side;
* KPG 24, a counter between two appliances that both need landing there: the longer of the two plus 12 in;
* KPG 13, the dishwasher within 36 in of the sink: here, under a counter next to the sink, so that counter is at least
  18 in (the dishwasher's width).

The end counters stand against walls and serve one appliance each. Which side of the sink takes its 24 in, which side
of the range its 15, and which way the fridge's door swings are binaries; the order is one binary per permutation (6),
so every counter's neighbours are explicit. With `length=` the landing rules go soft: each counter's shortfall (the
most any of its rules misses by) is a variable, and the objective is their sum.

The model is in feet (every NKBA length is a whole number of quarter-feet, so nothing rounds); what it returns is in
inches, the unit the guidelines speak. Probe for the purity test: `kitchen_probe()`.
"""

IN = 12.0
_BIG = 200.0 / IN  # ft: longer than any counter

NKBA = {
    "sink": (24.0, 18.0),     # in: its two landings (KPG 11)
    "range": (12.0, 15.0),    # in: its two landings (KPG 17)
    "fridge": 15.0,           # in: on the handle side (KPG 16)
    "combined": 12.0,         # in: two landings in one counter: the longer plus this (KPG 24)
    "dishwasher": 18.0,       # in: the dishwasher's width, under a counter next to the sink (KPG 13)
}
SIZES = {"range": 30.0, "sink": 24.0, "fridge": 24.0}  # in: a 30-in range, a 24-in sink base, a 24-in counter-depth fridge
_PERMS = [("range", "sink", "fridge"), ("range", "fridge", "sink"), ("sink", "range", "fridge"),
          ("sink", "fridge", "range"), ("fridge", "range", "sink"), ("fridge", "sink", "range")]

def kitchen_probe():
    """The purity probe: a pure function (the three appliances' widths, in)."""
    return SIZES["range"] + SIZES["sink"] + SIZES["fridge"]

def _build(name, sizes, length, soft, fixed):
    p = opt.problem(name = name)
    z = [p.binary("order %s" % "-".join(o)) for o in _PERMS]
    p.constrain(opt.sum(z), equals = 1, name = "kitchen:run:order:one order")
    seg = [p.var("c%d" % k, min = 0.0, max = _BIG) for k in range(4)]  # c0 | a | c1 | b | c2 | c | c3
    r15R, s24R, fhR = p.binary("range 15 on right"), p.binary("sink 24 on right"), p.binary("fridge handle right")
    s0, s1 = NKBA["sink"][0] / IN, NKBA["sink"][1] / IN
    g0, g1 = NKBA["range"][0] / IN, NKBA["range"][1] / IN
    f = NKBA["fridge"] / IN
    need = {
        "range": (g0 + (g1 - g0) * r15R, -(g1 - g0) * r15R + g1),
        "sink": (-(s0 - s1) * s24R + s0, s1 + (s0 - s1) * s24R),
        "fridge": (-f * fhR + f, f * fhR),
    }
    has = {"range": (1.0, 1.0), "sink": (1.0, 1.0), "fridge": (-fhR + 1.0, fhR)}
    short = []
    sv = {}
    cb = NKBA["combined"] / IN
    dw = NKBA["dishwasher"] / IN

    def rule(expr_ge, nm, zk, key):
        # expr >= 0 when order zk is on (slack by BIG otherwise); soft: one shortfall variable per counter
        off = _BIG * (-zk + 1)
        if soft:
            if key not in sv:
                sv[key] = p.var("short " + key, min = 0.0, max = _BIG)
                short.append((key, sv[key]))
            p.constrain(expr_ge + sv[key] + off, min = 0.0, name = "kitchen:run:landing:" + nm)
        else:
            p.constrain(expr_ge + off, min = 0.0, name = "kitchen:run:landing:" + nm)

    for k in range(len(_PERMS)):
        o, zk = _PERMS[k], z[k]
        a, b, c = o
        tag = "-".join(o)
        rule(seg[0] - need[a][0], "%s: %s left landing" % (tag, a), zk, "%s c0" % tag)
        rule(seg[3] - need[c][1], "%s: %s right landing" % (tag, c), zk, "%s c3" % tag)
        for (x, y, cs, ci) in ((a, b, seg[1], 1), (b, c, seg[2], 2)):
            both = p.binary("%s: %s|%s both" % (tag, x, y))
            p.constrain(both - has[x][1] - has[y][0], min = -1.0, name = "kitchen:run:landing:%s: %s|%s both" % (tag, x, y))
            key = "%s c%d" % (tag, ci)
            rule(cs - need[x][1], "%s: %s right landing" % (tag, x), zk, key)
            rule(cs - need[y][0], "%s: %s left landing" % (tag, y), zk, key)
            rule(cs - need[x][1] - cb * both, "%s: %s|%s the longer plus 12" % (tag, x, y), zk, key)
            rule(cs - need[y][0] - cb * both, "%s: %s|%s the longer plus 12 (b)" % (tag, x, y), zk, key)
        i = list(o).index("sink")
        left, right = seg[i], seg[i + 1]
        d = p.binary("%s: dw right of sink" % tag)
        p.constrain(right - dw * d + _BIG * (-zk + 1), min = 0.0, name = "kitchen:run:dishwasher:%s right" % tag)
        p.constrain(left - dw * (-d + 1) + _BIG * (-zk + 1) + _BIG * d, min = 0.0, name = "kitchen:run:dishwasher:%s left" % tag)
    total = (sizes["range"] + sizes["sink"] + sizes["fridge"]) / IN
    length_e = opt.sum(seg) + total
    if length != None:
        p.constrain(length_e, equals = length, name = "kitchen:run:length")
    if fixed != None:
        order, counters = fixed
        p.constrain(z[_PERMS.index(tuple(order))], equals = 1, name = "kitchen:run:drawn order")
        for k in range(4):
            p.constrain(seg[k], equals = counters[k] / IN, name = "kitchen:run:drawn c%d" % k)
    return {"p": p, "z": z, "seg": seg, "sides": (r15R, s24R, fhR), "short": short, "length": length_e}

def _report(m, sol, soft):
    if not sol.ok:
        return {"ok": False, "status": sol.status}
    order = [_PERMS[k] for k in range(len(_PERMS)) if sol.value(m["z"][k]) > 0.5][0]
    c = [math.round(sol.value(s) * IN * 1000.0) / 1000.0 for s in m["seg"]]
    r15R, s24R, fhR = [sol.value(x) for x in m["sides"]]
    out = {"ok": True, "status": sol.status, "order": list(order), "counters": c, "length": math.round(sol.value(m["length"]) * IN * 1000.0) / 1000.0,
           "sides": {"range_15_right": r15R == 1, "sink_24_right": s24R == 1, "fridge_handle_right": fhR == 1}}
    if soft:
        tag = "-".join(order)
        short = {}
        tot = 0.0
        for key, v in m["short"]:
            if key.startswith(tag + " "):
                x = math.round(sol.value(v) * IN * 1000.0) / 1000.0
                if x > 1e-6:
                    short[key[len(tag) + 1:]] = x
                    tot += x
        out["short"] = short
        out["shortfall"] = tot
    return out

def kitchen_run(length = None, *, sizes = None, name = None):
    """The run, as a MILP: with no `length`, the shortest run (ft) that meets every landing rule; with `length` (ft),
    the order, counters and sides with the least total shortfall at exactly that length. Returns {"ok", "order" (west to
    east), "counters" [c0, c1, c2, c3] (in), "length" (in), "sides", and with a length "short" {counter: in} and
    "shortfall" (in)}. `sizes` overrides SIZES (in)."""
    sz = dict(SIZES)
    if sizes != None:
        sz.update(sizes)
    soft = length != None
    m = _build(name if name != None else ("plan_kitchen_run" if length == None else "plan_kitchen_run_%s" % length), sz, length, soft, None)
    if soft:
        m["p"].minimize(opt.sum([v for _, v in m["short"]]))
    else:
        m["p"].minimize(m["length"])
    return _report(m, m["p"].solve(), soft)

def check_run(order, counters, *, sizes = None, name = None):
    """A drawn run's shortfall: `order` (three of range, sink, fridge, west to east), `counters` (in, c0..c3). Returns
    kitchen_run's record for that run (the sides chosen to miss least)."""
    sz = dict(SIZES)
    if sizes != None:
        sz.update(sizes)
    L = (sz["range"] + sz["sink"] + sz["fridge"] + counters[0] + counters[1] + counters[2] + counters[3]) / IN
    m = _build(name if name != None else "plan_kitchen_check", sz, L, True, (order, counters))
    m["p"].minimize(opt.sum([v for _, v in m["short"]]))
    return _report(m, m["p"].solve(), True)
