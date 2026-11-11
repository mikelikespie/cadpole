"""@cadpole/plan_fit_tables_gen.star -- the generator of @cadpole/plan_fit_tables.star: the fit-table jobs
as sets, door classes and clear widths, solved by the room stage and written out as Starlark data.

    load("@cadpole/plan_fit_tables_gen.star", "fit_jobs", "grid_jobs", "solve_jobs", "record_source", "table_source", "clear_w",
         "depth_in", "stretch_test", "STRETCH", "GRID")

    recs = solve_jobs(fit_jobs(kinds = ["bath2", "powder"]))     # each job: the least depth, solved now
    print(table_source(recs, solve_jobs(grid_jobs(["tub"]))))     # the data file's text

**Regenerating the tables.** The CLI budget is 120 s a run and the child's rooms take about 3 s each, so solve them a
few room types at a time and paste the parts together:

    # regen.star
    load("@cadpole/plan_fit_tables_gen.star", "fit_jobs", "solve_jobs", "record_source")
    for r in solve_jobs(fit_jobs(kinds = ["kid"])):
        print(record_source(r))

    cadpole eval regen.star 2>&1 | sed -n 's/^info: print: //p'

`table_source(records, grid)` wraps the records into the module's text: FIT_ENTRIES (`fit_jobs()`),
GRID_ENTRIES (`grid_jobs()`: the bed sets and the baths at every 6 in of clear width, `GRID`), FIT_TABLES and the
header. Print a long text one line at a time (cadpole keeps 200 print lines a run), and run it with the CLI.

**The jobs** (`fit_jobs()`): every room type on the
spans of a 24-ft plate whose column lines are at 0, 3.5, 7, 12, 13.6, 17, 20.5 and 24 ft (a 25-ft benchmark lot,
a synthetic site), with each door class that fits. A span's clear width is its length less half a
4.8-in partition at each end that is not a party wall (`clear_w`). 170 jobs, 154 of them feasible.
The width is kept in inches, rounded to a hundredth.

Probe for the purity test: `fit_jobs`.
"""

load("@cadpole/plan_furnish.star", "least_depth_record", "holds", "OP_BED", "OP_BATH")

PART = 0.2  # ft: half a partition; a room's clear edge sits this far inside its centre line, except at the party walls

SPANS = [
    ("kid", [(0, 12), (12, 24), (0, 13.6), (13.6, 24), (7, 17), (7, 24), (7, 20.5), (12, 20.5)]),
    ("den", [(0, 24), (0, 13.6), (7, 24), (3.5, 24), (0, 12), (12, 24)]),
    ("bath2", [(0, 7), (7, 12), (7, 13.6), (12, 17), (13.6, 20.5), (17, 24), (13.6, 24), (3.5, 12)]),
    ("bath1", [(0, 7), (7, 12), (7, 13.6), (12, 17), (17, 24), (13.6, 24), (3.5, 12), (0, 3.5)]),
    ("powder", [(0, 3.5), (0, 7), (7, 12), (7, 13.6), (12, 17), (17, 24)]),
    ("laundry", [(0, 3.5), (0, 7), (7, 12), (7, 13.6), (12, 17), (13.6, 20.5), (17, 24), (13.6, 24)]),
    ("pantry", [(0, 3.5), (0, 7), (7, 12), (7, 13.6), (12, 17), (17, 24)]),
    ("living", [(0, 24), (3.5, 24), (7, 24)]),
    ("kitchen", [(0, 24), (3.5, 24), (7, 24)]),
    ("dining", [(0, 24), (3.5, 24), (7, 24)]),
    ("entry", [(0, 13.6), (3.5, 13.6)]),
    ("garage", [(13.6, 24)]),
]
WELL_SPANS = [(7, 20.5), (12, 20.5), (13.6, 20.5)]  # spans that end on the light well: the window on the side wall

_SET = {"kid": "full", "den": "den", "bath2": "tub", "bath1": "shower", "powder": "powder", "laundry": "laundry",
        "pantry": "pantry", "living": "living", "dining": "dining", "entry": "entry"}

def clear_w(span):
    """A span's clear width in inches, rounded to a hundredth."""
    a, b = span
    x = ((b - a) - (PART if a > 0 else 0.0) - (PART if b < 24.0 else 0.0)) * 12.0
    return math.round(x * 100.0) / 100.0

def fit_jobs(kinds = None):
    """The jobs, in order: [{"kind", "span", "cls", "set", "door", "W"}] (W in inches). `kinds` picks room
    types (kid den bath2 bath1 powder laundry pantry living kitchen dining entry garage)."""
    out = []
    for kind, spans in SPANS:
        if kinds != None and kind not in kinds:
            continue
        for sp in spans:
            a, b = sp
            W = clear_w(sp)
            if kind in ("kid", "den"):
                cls = ["wopp", "wadj"] if sp in WELL_SPANS else (["opp", "adj"] + (["core"] if a == 0 else []))
                for c in cls:
                    if kind == "den":
                        out.append(_job(kind, sp, c, "den", c, W))
                        out.append(_job(kind, sp, c + "+garden", "den", c + "+garden", W))
                    else:
                        out.append(_job(kind, sp, c, "full", c, W))
            elif kind in ("bath2", "bath1", "powder", "laundry", "pantry"):
                op = OP_BATH
                cl = [("wide", "S", 3.0), ("side", "W", 3.0)]
                if a == 0:
                    cl.append(("core", "S", 45.0))
                if kind in ("laundry", "pantry"):
                    cl.append(("mid", "S", max(0.0, (W - op) / 2.0)))
                for c, wl, off in cl:
                    if off + op > W + 1e-6 and wl in ("S", "N"):
                        continue
                    out.append(_job(kind, sp, c, _SET[kind], c, W))
            elif kind in ("living", "kitchen", "dining"):
                opn = "N" if a == 0 else "WN"
                facades = ["front", "rear"] + (["mid"] if kind == "dining" else [])
                for fc in facades:
                    variants = ["L", "island"] if kind == "kitchen" else [""]
                    for v in variants:
                        o = opn if fc != "mid" else opn + "S"
                        st = ("kitchen_" + v) if kind == "kitchen" else _SET[kind]
                        out.append(_job(kind, sp, fc + ("+" + v if v else ""), st, fc + ":" + o, W))
            elif kind == "entry":
                out.append(_job(kind, sp, "front", "entry", "entry:a0" if a == 0 else "entry:a", W))
            elif kind == "garage":
                for v in ("car", "car+storage"):
                    out.append(_job(kind, sp, v, "car" if v == "car" else "car_storage", "garage", W))
    return out

# A regular grid beside the jobs: the bed sets and the baths at every 6 in of clear width, for plan MILPs whose
# rooms take any width (a fit row reads the widest grid width not over the room's).
GRID = [
    ("full", ("opp", "adj"), 8.0, 16.0),
    ("twin", ("opp", "adj"), 7.0, 14.0),
    ("queen", ("opp", "adj"), 10.0, 18.0),
    ("tub", ("wide", "side"), 5.0, 9.0),
    ("shower", ("wide", "side"), 5.0, 9.0),
    ("powder", ("wide", "side"), 3.0, 6.0),
]

def grid_jobs(sets = None):
    """The grid's jobs: [{"kind": "grid", "span": (0, 0), "cls", "set", "door", "W"}], W in inches at 6-in steps."""
    out = []
    for st, classes, w0, w1 in GRID:
        if sets != None and st not in sets:
            continue
        n = int(math.round((w1 - w0) * 2.0))
        for door in classes:
            for k in range(n + 1):
                W = (w0 + 0.5 * k) * 12.0
                out.append(_job("grid", (0, 0), door, st, door, W))
    return out

def _job(kind, span, cls, set_name, door, W):
    return {"kind": kind, "span": span, "cls": cls, "set": set_name, "door": door, "W": W}

def solve_jobs(jobs, time_limit = 60.0):
    """Solve each job's least depth: [{"kind", "span", "cls", "set", "door", "W", "D" (in) or None, "status", "cuts"}]."""
    out = []
    for i, j in enumerate(jobs):
        nm = "plan_fit_tables_gen_%d_%s_%s_%s-%s_%d" % (i, j["set"], j["door"].replace(":", "_"), j["span"][0], j["span"][1], int(math.round(j["W"] * 100.0)))
        r = least_depth_record(j["set"], j["W"], j["door"], time_limit = time_limit, name = nm)
        rec = dict(j)
        rec["D"] = r["D"]
        rec["status"] = r["status"]
        rec["cuts"] = r["cuts"]
        out.append(rec)
    return out

STRETCH = [0.0, 0.5, 1.0, 2.0, 4.0, 8.0]  # ft deeper than the least depth: the stretch test

def stretch_test(entries, a = 0, b = None, extra = STRETCH):
    """The stretch test: re-solve each feasible entry of `entries` (FIT_ENTRIES' shape) from index a to b at
    its least depth plus each of `extra` (ft), its doors as before, and re-walk it. A layout "holds" when the set fits
    and every use point is reachable after at most WALK_CUTS placements are cut.
    Returns [(index, extra ft, ok, status, cuts, unreachable)]. Cadpole prints at most 200 lines a run: print a summary
    line per entry, and run the 154 entries in chunks (about 3 minutes of solving in all)."""
    out = []
    for i in range(a, min(len(entries), b if b != None else len(entries))):
        e = entries[i]
        if e[3] == None:
            continue
        for k, x in enumerate(extra):
            r = holds(e[0], 0, 0, e[1], width_in = e[8], depth_in = e[9] + 12.0 * x, name = "plan_fit_tables_gen_stretch_%d_%d" % (i, k))
            out.append((i, x, r["ok"], r["status"], r["cuts"], r["unreachable"]))
    return out

def _f(x):
    return "None" if x == None else str(x)

def depth_in(D):
    """A solved least depth (in) without the solver's float noise: to a ten-thousandth of an inch."""
    return None if D == None else math.round(D * 10000.0) / 10000.0

def record_source(r):
    """One FIT_ENTRIES line: (set, door, width ft, depth ft or None, room type, span, class, cuts, W in, D in).
    The width in feet is rounded to a millionth of a foot, the depth in feet rounded UP to one (a fit row may only err
    deep); the inches are the solved values, the depth to a ten-thousandth of an inch."""
    d = depth_in(r["D"])
    d_ft = None if d == None else math.ceil(d / 12.0 * 1e6 - 1e-6) / 1e6
    w_ft = math.round(r["W"] / 12.0 * 1e6) / 1e6
    return "    (%r, %r, %s, %s, %r, (%s, %s), %r, %d, %s, %s)," % (r["set"], r["door"], _f(w_ft), _f(d_ft), r["kind"],
                                                                    _f(r["span"][0]), _f(r["span"][1]), r["cls"], r["cuts"], _f(r["W"]), _f(d))

def table_source(records, grid = ()):
    """The text of @cadpole/plan_fit_tables.star for the jobs' records and the grid's."""
    lines = [_HEADER, "FIT_ENTRIES = ["]
    for r in records:
        lines.append(record_source(r))
    lines.append("]")
    lines.append("")
    lines.append("GRID_ENTRIES = [")
    for r in grid:
        lines.append(record_source(r))
    lines.append("]")
    lines.append(_FOOTER)
    return "\n".join(lines)

_HEADER = '''"""@cadpole/plan_fit_tables.star -- the room stage's fit tables, generated by @cadpole/plan_fit_tables_gen.star.
DO NOT EDIT: regenerate (see that module's docstring).

    load("@cadpole/plan_fit_tables.star", "FIT_TABLES", "FIT_ENTRIES", "fit_tables_probe")
    load("@cadpole/plan_furnish.star", "fit_row")
    fit_row(p, w, d, FIT_TABLES["full|opp"], name = "fit:L1:bed2:depth")

FIT_ENTRIES: one per job, (set, door class, clear width ft, least clear depth ft or None, room type, its
span on the benchmark plate (ft), its class, walk cuts, width in, depth in). The jobs are the generator's
`fit_jobs()` on a synthetic 25-ft benchmark plate.
GRID_ENTRIES: the same shape (room type "grid", span (0, 0)) for the bed sets and the baths at every 6 in of width.
FIT_TABLES: the entries by "<set>|<door class>", {"set", "door", "rows": [(width ft, depth ft or None)]} sorted by width,
what fit_row() and fit_lookup() read. A depth in feet is rounded up to a millionth of a foot.
Probe for the purity test: fit_tables_probe()."""
'''

_FOOTER = '''
def _tables():
    t = {}
    for e in FIT_ENTRIES + GRID_ENTRIES:
        key = e[0] + "|" + e[1]
        if key not in t:
            t[key] = {"set": e[0], "door": e[1], "rows": []}
        if (e[2], e[3]) not in t[key]["rows"]:
            t[key]["rows"].append((e[2], e[3]))
    for k in t:
        t[k]["rows"] = sorted(t[k]["rows"])
    return t

FIT_TABLES = _tables()

def fit_tables_probe():
    """The purity probe: the number of entries and of tables."""
    return (len(FIT_ENTRIES), len(FIT_TABLES))
'''
