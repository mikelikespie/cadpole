"""@cadpole/plan_ask.star -- ask the solver about your own plan: pin it, cost a rule, explain a clash, edit it.

    load("@cadpole/plan_ask.star", "pin", "check", "cost", "explain", "edit", "duals", "apply",
         "set_room", "add_room", "drop_room", "set_floor", "move_zone", "add_rule", "drop_family", "set_problem",
         "describe")

A problem (`@cadpole/plan_solve.star`'s `problem()`) holds the rules; a plan value (`@cadpole/plan.star`) is a drawing, yours
or the solver's. These functions answer questions about one with the other, in plain units:

- `pin(spec, plan)`: your arrangement -- which room is beside which, on which floor -- held, and the rooms sized
  within every rule (`sizes="free"`, a small MILP: only the widths' bits), or held at their drawn sizes as well
  (`sizes="fixed"`, a check: the rules the drawing breaks, by name, and the solver picks only doors, stacks, lines
  and beams). `check(spec, plan)` is the second.
- `cost(spec, change)`: paired solves, the problem as it is and changed; answered in score, sq ft (and inches or any
  unit by `measure=`), with the rooms that move.
- `explain(spec)`: when a wish cannot be met, the rule families that clash, by floor, then the rooms in them. A
  filter over families, rebuilding the model without the ones it sets aside (every row is named
  "<family>:<floor>:<room>:<what>", so method="tags" can use the builder's drop= instead, and method="conflict" the
  builder's bounded sol.conflict() with the floor's geometry held: slower on these MILPs, and so not the default).
- `edit(spec, plan, change, minimal=True)`: the change made with the fewest feet of wall moved (each room's four
  faces, summed), your arrangement held where it can be. Its widths expand about the drawn ones (two-sided bits), so
  a room that need not move does not; that grid is a weaker LP, so pin() and duals() keep the standard one.
- `duals(spec, plan)`: what one more unit of each binding rule is worth on your plan -- LP duals, with every integer
  fixed at its value (`solve(fix=)`), so local to the arrangement and the widths, and labelled so; `resolve=` checks
  the first few by re-solving with the rule moved (a MILP's dual holds only while its arrangement does), each
  re-solve started at the base's point (`solve(start=)`).

A change is a list of edits (or one): `set_room(floor, key, area=(95, 130))`, `add_room(room(...))`,
`drop_room(floor, key)`, `set_floor(key, egress=[...])`, `move_zone(floor, zone, side, by)` (a face of a zone moved
outward by `by` feet, its escape walls and touch lines with it), `add_rule(rule(...))`, `drop_family("R310:A")`,
`set_problem(stacks=None)`. `apply(spec, change)` makes the changed problem, leaving the first untouched.

Every answer is a plan value or a dict of plain numbers; the solver's own numbers are in meta["plan_solve"], this
module's in meta["plan_ask"]. Feature names: `name=` (or plan_ask_<question>_<problem>), with a suffix per solve.
Its purity probe is `describe`.
"""

load("@cadpole/plan_solve.star", "arrangement_fix", "build", "drawn_layout", "ftin", "layout_score", "read", "run", "score", "solve",
     "start_of", _solve_floor = "floor")

# ---------------------------------------------------------------------------------------------- small helpers

def _isnum(x):
    t = type(x)
    return t == "float" or t == "int"

def _area(r):
    return (r[1] - r[0]) * (r[3] - r[2])

def _f(x, spec = ".1f"):
    return format(x, spec)

def _sq(x):
    return format(x, ",.0f") + " sq ft"

def _ps(plan):
    return plan["meta"]["plan_solve"]

def _kinds(plan):
    out = {}
    for r in plan.get("rooms", []):
        out[(r["floor"], r["key"])] = r["kind"]
    return out

def _feat(s):
    out = []
    for ch in s.elems():
        out.append("_" if ch in (":", "#", "/", " ") else ch)
    return "".join(out)

def _nm(name, what, spec):
    return name or _feat("plan_ask_%s_%s" % (what, spec["name"]))

def describe(change):
    """A change as one line of text: "bed1 on A: area (100, 150)"."""
    edits = change if type(change) == "list" else [change]
    out = []
    for e in edits:
        op = e["op"]
        if op == "set_room":
            out.append("%s on %s: %s" % (e["key"], e["floor"], ", ".join(["%s %s" % (k, v) for k, v in sorted(e["fields"].items())])))
        elif op == "add_room":
            out.append("add %s on %s" % (e["room"]["key"], e["room"]["floor"]))
        elif op == "drop_room":
            out.append("drop %s on %s" % (e["key"], e["floor"]))
        elif op == "set_floor":
            out.append("floor %s: %s" % (e["key"], ", ".join(sorted(e["fields"].keys()))))
        elif op == "move_zone":
            out.append("%s's %s face of %s moved %s" % (e["floor"], e["side"], e["zone"], ftin(e["by"])))
        elif op == "add_rule":
            out.append("rule %s on %s:%s" % (e["rule"]["what"], e["rule"]["floor"], e["rule"]["room"]))
        elif op == "drop":
            out.append("without %s" % e["family"])
        elif op == "set_problem":
            out.append("problem: %s" % ", ".join(sorted(e["fields"].keys())))
        else:
            out.append(op)
    return "; ".join(out)

# ------------------------------------------------------------------------------------------------ changes

def set_room(floor, key, **fields):
    """Change a room's fields: area=, w=, d=, hall_min=, sleeping=, opens_to=, zones=, value=, knee=, ..."""
    return {"op": "set_room", "floor": floor, "key": key, "fields": fields}

def add_room(r):
    """Add a room (a plan_solve room())."""
    return {"op": "add_room", "room": r}

def drop_room(floor, key):
    """Take a room out of the program (its pieces go with it)."""
    return {"op": "drop_room", "floor": floor, "key": key}

def set_floor(key, **fields):
    """Change a floor's fields: zones=, egress=, obstacles=, fixed=, must_hold=, ..."""
    return {"op": "set_floor", "key": key, "fields": fields}

def move_zone(floor, zone, side, by):
    """Move one face of a zone outward by `by` feet (inward if negative): N and E up, S and W down. Escape walls on
    that face move with it, and those that end at it stretch; rooms' touch lines on it move too."""
    return {"op": "move_zone", "floor": floor, "zone": zone, "side": side, "by": float(by)}

def add_rule(r):
    """Add a hard rule (a plan_solve rule())."""
    return {"op": "add_rule", "rule": r}

def drop_family(family):
    """Leave a family of rules out ("R310", "R310:A", "R310:A:bed3")."""
    return {"op": "drop", "family": family}

def set_problem(**fields):
    """Change the problem's own fields: stacks=, structure=, not_over=, rules=, sym=, cantilever=, ..."""
    return {"op": "set_problem", "fields": fields}

def _norm_field(k, v):
    if k == "area" and v != None:
        return (float(v[0]), float(v[1]) if v[1] != None else None)
    if k in ("w", "d") and v != None:
        return (float(v[0]), float(v[1]))
    if k in ("opens_to", "open_to", "holds", "touches") and v != None:
        return list(v)
    if k == "value" and v != None:
        return float(v)
    return v

def _moved_coord(c, old, new):
    if _isnum(c) and abs(c - old) < 1e-6:
        return new
    return c

def _move_zone(F, zone, side, by):
    z = F["zones"][zone]
    if side == "N":
        old = z[3]
        new = z[3] + by
        nz = (z[0], z[1], z[2], new)
    elif side == "S":
        old = z[2]
        new = z[2] - by
        nz = (z[0], z[1], new, z[3])
    elif side == "E":
        old = z[1]
        new = z[1] + by
        nz = (z[0], new, z[2], z[3])
    else:
        old = z[0]
        new = z[0] - by
        nz = (new, z[1], z[2], z[3])
    G = dict(F)
    zs = dict(F["zones"])
    zs[zone] = nz
    G["zones"] = zs
    eg = []
    for wl in F["egress"]:
        w2 = dict(wl)
        if wl["side"] == side:
            w2["at"] = _moved_coord(wl["at"], old, new)
        elif (side in ("N", "S")) == (wl["side"] in ("W", "E")):
            lo = wl["span"][0]
            hi = wl["span"][1]
            w2["span"] = (new if abs(lo - old) < 1e-6 else lo, new if abs(hi - old) < 1e-6 else hi)
        eg.append(w2)
    G["egress"] = eg
    return G, old, new

def apply(spec, change):
    """The problem with a change made (a list of edits, or one); the problem given is left as it was."""
    edits = change if type(change) == "list" else [change]
    out = dict(spec)
    out["rooms"] = list(spec["rooms"])
    out["floors"] = list(spec["floors"])
    out["rules"] = list(spec["rules"])
    out["drop"] = list(spec.get("drop") or [])
    for e in edits:
        op = e["op"]
        if op == "set_room":
            hit = False
            for i in range(len(out["rooms"])):
                r = out["rooms"][i]
                if r["floor"] == e["floor"] and r["key"] == e["key"]:
                    q = dict(r)
                    for k, v in e["fields"].items():
                        if k not in q:
                            fail("plan_ask.set_room: a room has no field %r" % k)
                        q[k] = _norm_field(k, v)
                    out["rooms"][i] = q
                    hit = True
            if not hit:
                fail("plan_ask.set_room: no room %r on %r" % (e["key"], e["floor"]))
        elif op == "add_room":
            out["rooms"].append(e["room"])
        elif op == "drop_room":
            out["rooms"] = [r for r in out["rooms"] if not (r["floor"] == e["floor"] and r["key"] == e["key"])]
            rr = []
            for r in out["rooms"]:
                if r["floor"] == e["floor"] and (e["key"] in r["opens_to"] or e["key"] in r["open_to"]):
                    r = dict(r)
                    r["opens_to"] = [c for c in r["opens_to"] if c != e["key"]]
                    r["open_to"] = [c for c in r["open_to"] if c != e["key"]]
                rr.append(r)
            out["rooms"] = rr
            out["sym"] = [s for s in out["sym"] if not (s[0] == e["floor"] and e["key"] in (s[1], s[2]))]
        elif op == "set_floor":
            hit = False
            for i in range(len(out["floors"])):
                F = out["floors"][i]
                if F["key"] == e["key"]:
                    G = dict(F)
                    for k, v in e["fields"].items():
                        if k not in G:
                            fail("plan_ask.set_floor: a floor has no field %r" % k)
                        G[k] = v
                    if "obstacles" in e["fields"] or "fixed" in e["fields"]:
                        # rects or dicts, as floor() takes them, named as floor() names them (a fixed room's kind
                        # and core): the model reads each as a dict
                        N = _solve_floor(G["key"], zones = G["zones"], obstacles = G["obstacles"], fixed = G["fixed"])
                        G["obstacles"] = N["obstacles"]
                        G["fixed"] = N["fixed"]
                    out["floors"][i] = G
                    hit = True
            if not hit:
                fail("plan_ask.set_floor: no floor %r" % e["key"])
        elif op == "move_zone":
            for i in range(len(out["floors"])):
                F = out["floors"][i]
                if F["key"] == e["floor"]:
                    G, old, new = _move_zone(F, e["zone"], e["side"], e["by"])
                    out["floors"][i] = G
                    rr = []
                    for r in out["rooms"]:
                        if r["floor"] == e["floor"] and r["touches"]:
                            r = dict(r)
                            ts = []
                            for t in r["touches"]:
                                t2 = dict(t)
                                if t["side"] == e["side"]:
                                    t2["at"] = _moved_coord(t["at"], old, new)
                                ts.append(t2)
                            r["touches"] = ts
                        rr.append(r)
                    out["rooms"] = rr
        elif op == "add_rule":
            out["rules"].append(e["rule"])
        elif op == "drop":
            out["drop"].append(e["family"])
        elif op == "set_problem":
            for k, v in e["fields"].items():
                if k not in out:
                    fail("plan_ask.set_problem: a problem has no field %r" % k)
                out[k] = v
        else:
            fail("plan_ask.apply: unknown edit %r" % op)
    return out

# ----------------------------------------------------------------------------------------------- comparing

def _main_rects(plan):
    out = {}
    for r in plan.get("rooms", []):
        out[(r["floor"], r["key"])] = r["rects"]
    return out

def _rooms_moved(before, after, keys = None, tol = 0.05):
    """Per room both plans have: its area before and after, and how far its walls moved (the largest face move and
    the sum of the four faces' moves, over its main rect)."""
    a = _main_rects(before)
    b = _main_rects(after)
    out = []
    for k, ra in a.items():
        rb = b.get(k)
        if rb == None or (keys != None and k[1] not in keys):
            continue
        A0 = 0.0
        for q in ra:
            A0 += _area(q)
        A1 = 0.0
        for q in rb:
            A1 += _area(q)
        m = 0.0
        s = 0.0
        for i in range(4):
            dd = abs(ra[0][i] - rb[0][i])
            m = max(m, dd)
            s += dd
        out.append({"floor": k[0], "key": k[1], "before": A0, "after": A1, "d_area": A1 - A0, "moved_ft": m,
                    "faces_ft": s, "size_before": "%s x %s" % (ftin(ra[0][1] - ra[0][0]), ftin(ra[0][3] - ra[0][2])),
                    "size_after": "%s x %s" % (ftin(rb[0][1] - rb[0][0]), ftin(rb[0][3] - rb[0][2])),
                    "moved": m > tol or abs(A1 - A0) > tol})
    return sorted(out, key = lambda x: -x["moved_ft"])

def _compare(spec, base, variant, measure, unit):
    sb = _ps(base)
    sv = _ps(variant)
    out = {"base": base, "variant": variant, "base_status": sb["status"], "variant_status": sv["status"]}
    if not sb["feasible"] or not sv["feasible"]:
        out["answer"] = "the %s cannot be solved (%s)" % ("problem as it is" if not sb["feasible"] else "changed problem", sb["status"] if not sb["feasible"] else sv["status"])
        out["score"] = None
        return out
    out["score"] = sv["score"] - sb["score"]
    a0 = 0.0
    a1 = 0.0
    for v in sb["areas"].values():
        a0 += v
    for v in sv["areas"].values():
        a1 += v
    out["sq_ft"] = a1 - a0
    rooms = _rooms_moved(base, variant)
    out["rooms"] = rooms
    out["moved"] = [r["key"] for r in rooms if r["moved"]]
    if measure != None:
        m0 = measure(base)
        m1 = measure(variant)
        out["measure"] = {"before": m0, "after": m1, "change": m1 - m0, "unit": unit}
    gap = max(sb["gap"] or 0.0, sv["gap"] or 0.0)
    words = "%s of score (%s)" % (_f(out["score"], "+.1f"), (_f(out["sq_ft"], "+,.0f") + " sq ft"))
    if measure != None:
        words += "; %s %s" % (_f(out["measure"]["change"], "+.2f"), unit)
    tie = abs(out["score"]) < 0.05 and abs(out["sq_ft"]) < 0.5
    out["tie"] = tie
    if out["moved"] and not tie:
        words += "; moves " + ", ".join(["%s (%s)" % (r["key"], ftin(r["moved_ft"])) for r in rooms if r["moved"]][:6])
    elif out["moved"]:
        words += "; nothing needs to move (rooms that shift are a tie: the same score either way)"
    else:
        words += "; nothing moves"
    if gap > 1e-4:
        words += " (to within a %s%% gap)" % _f(100.0 * gap, ".1f")
    out["answer"] = words
    return out

# ------------------------------------------------------------------------------------------------------ pin

def pin(spec, plan, *, sizes = "free", doors = True, windows = True, mode = "exact", step = 0.25, time_limit = None,
        gap = None, max_nodes = None, name = None, floors = None):
    """Your plan's arrangement held, its rooms sized within every rule (sizes="free"), or held at their drawn sizes
    too (sizes="fixed": a check). The drawn doors and escape windows are held when given (doors=False, windows=False
    leave them to the solver). The answer is the sized plan, with meta["plan_ask"]: the drawing's own score under
    the problem, the sized score, the rules the drawing breaks (fixed sizes), the rooms that changed and the notes
    on what the drawing and the problem do not share."""
    nm = _nm(name, "pin", spec)
    spec2, lay, notes = drawn_layout(spec, plan, floors = floors)
    drawn_score = layout_score(spec2, lay)
    if sizes == "fixed":
        pl = solve(spec2, pin = lay, broken = "skip", mode = mode, step = step, time_limit = time_limit, gap = gap,
                   floors = floors, name = nm)
        fixn = 0
    elif sizes == "free":
        fix, n2 = arrangement_fix(spec2, lay, doors = plan.get("doors") if doors else None,
                                  windows = plan.get("windows") if windows else None, floors = floors, kinds = _kinds(plan))
        notes = notes + n2
        pl = solve(spec2, fix = fix, mode = mode, step = step, time_limit = time_limit, gap = gap, floors = floors, name = nm,
                   max_nodes = max_nodes)
        fixn = len(fix)
    else:
        fail("plan_ask.pin: sizes is \"free\" or \"fixed\", got %r" % sizes)
    s = _ps(pl)
    ask = {"question": "pin", "sizes": sizes, "drawn_score": drawn_score, "notes": notes, "held_binaries": fixn,
           "broken": [b["rule"] for b in s["broken"]]}
    if s["feasible"]:
        ask["score"] = s["score"]
        ask["gain"] = s["score"] - drawn_score
        ask["rooms"] = _rooms_moved(plan, pl)
        ask["changed"] = [r["key"] for r in ask["rooms"] if r["moved"]]
    pl["meta"]["plan_ask"] = ask
    return pl

def check(spec, plan, **kw):
    """pin(spec, plan, sizes="fixed"): the rules a drawn plan breaks, its score, and the doors, stacks, lines and beams
    the solver gives it."""
    return pin(spec, plan, sizes = "fixed", **kw)

# ----------------------------------------------------------------------------------------------------- cost

def cost(spec, change, *, base = None, plan = None, arrangement = "free", measure = None, unit = "score",
         mode = "exact", step = 0.25, time_limit = None, gap = None, max_nodes = None, name = None, floors = None):
    """What a rule costs: two solves, the problem as it is and with `change` made, and the difference -- in score, in
    sq ft and, with `measure=` (a function of a plan) in `unit`, plus the rooms that move. arrangement="free" lets the
    plan rearrange; "pinned" holds `plan`'s arrangement in both (pin's sizes="free"). `base=` reuses a solved plan for
    the first. The answer is a dict: score, sq_ft, rooms (each with before/after and how far it moved), moved (keys),
    answer (one line), base and variant (the two plans)."""
    nm = _nm(name, "cost", spec)
    spec2 = apply(spec, change)
    if arrangement == "pinned":
        if plan == None:
            fail("plan_ask.cost: arrangement=\"pinned\" needs plan= (the plan whose arrangement to hold)")
        b = base or pin(spec, plan, mode = mode, step = step, time_limit = time_limit, gap = gap, name = nm + "_base", floors = floors)
        v = pin(spec2, plan, mode = mode, step = step, time_limit = time_limit, gap = gap, name = nm + "_variant", floors = floors)
    elif arrangement == "free":
        b = base or solve(spec, mode = mode, step = step, time_limit = time_limit, gap = gap, name = nm + "_base", floors = floors)
        v = solve(spec2, mode = mode, step = step, time_limit = time_limit, gap = gap, name = nm + "_variant", floors = floors)
    else:
        fail("plan_ask.cost: arrangement is \"free\" or \"pinned\", got %r" % arrangement)
    out = _compare(spec, b, v, measure, unit)
    out["change"] = describe(change)
    out["arrangement"] = arrangement
    return out

# ----------------------------------------------------------------------------------------------------- edit

def edit(spec, plan, change, *, minimal = True, arrangement = "auto", within = None, doors = True, windows = True,
         mode = "exact", step = 0.25, time_limit = None, gap = None, name = None, floors = None):
    """Make a change to your plan. minimal=True: the plan that meets every rule and the change with the fewest feet
    of wall moved (each room's four faces, summed against the drawing), the arrangement held (arrangement="pinned"),
    let go (="free"), or held if it can be and else let go (="auto"); `within` a score it must keep. minimal=False:
    the best plan with the change (pinned or free as asked). meta["plan_ask"] says how far the walls moved, which
    rooms changed (before and after) and the score either side."""
    nm = _nm(name, "edit", spec)
    spec2 = apply(spec, change)
    spec3, lay, notes = drawn_layout(spec2, plan, floors = floors)
    objective = "move" if minimal else "score"
    tried = []
    pl = None
    if arrangement in ("pinned", "auto"):
        fix, n2 = arrangement_fix(spec3, lay, doors = plan.get("doors") if doors else None,
                                  windows = plan.get("windows") if windows else None, floors = floors, kinds = _kinds(plan))
        notes = notes + n2
        pl = solve(spec3, fix = fix, objective = objective, anchor = lay, within = within, mode = mode, step = step,
                   time_limit = time_limit, gap = gap, floors = floors, name = nm + "_pinned", widths = lay)
        tried.append("pinned: " + _ps(pl)["status"])
    if arrangement == "free" or (arrangement == "auto" and not _ps(pl)["feasible"]):
        pl = solve(spec3, objective = objective, anchor = lay, within = within, mode = mode, step = step,
                   time_limit = time_limit, gap = gap, floors = floors, name = nm + "_free", widths = lay)
        tried.append("free: " + _ps(pl)["status"])
    if arrangement not in ("pinned", "free", "auto"):
        fail("plan_ask.edit: arrangement is \"pinned\", \"free\" or \"auto\", got %r" % arrangement)
    s = _ps(pl)
    ask = {"question": "edit", "change": describe(change), "tried": tried, "notes": notes, "minimal": minimal,
           "drawn_score": layout_score(spec3, lay)}
    if s["feasible"]:
        rooms = _rooms_moved(plan, pl)
        total = 0.0
        for k, R in lay.items():
            for key, q in R.items():
                q2 = s["layout"].get(k, {}).get(key)
                if q2 == None:
                    continue
                for i in range(4):
                    total += abs(q[i] - q2[i])
        ask["walls_moved_ft"] = total
        ask["rooms"] = rooms
        ask["changed"] = [r["key"] for r in rooms if r["moved"]]
        ask["score"] = s["score"]
        ask["answer"] = "%s moves %s of wall (%s); score %s -> %s" % (
            describe(change), ftin(total), ", ".join(["%s %s -> %s" % (r["key"], r["size_before"], r["size_after"]) for r in rooms if r["moved"]][:6]) or "no room",
            _f(ask["drawn_score"]), _f(s["score"]))
    pl["meta"]["plan_ask"] = ask
    return pl

# ---------------------------------------------------------------------------------------------------- duals

def duals(spec, plan, *, sizes = "free", top = 12, resolve = 0, resolve_by = 1.0, families = None, mode = "exact",
          step = 0.25, time_limit = None, gap = None, name = None, floors = None, arrangement = "free"):
    """What one more unit of each binding rule is worth on your plan (score per sq ft or per ft). The plan is
    pinned (sizes="free": its arrangement; "fixed": its rects), solved, and then solved again with every integer
    fixed at its value (opt's fix=), which leaves an LP whose duals price the binding rules -- local to that
    arrangement and those widths when the pinned problem had integers (a MILP), and labelled so; the LP of a fully
    fixed plan is often degenerate, where several duals are equally valid. `resolve=n`: the first n rules re-solved
    with the rule moved by `resolve_by` (relaxed), the arrangement free (arrangement="free") or pinned, against the
    problem as it is -- what the rule is really worth when the plan can change. Returns a dict: rows (rule, dual,
    local, slack, resolved, started: the binaries each re-solve started from), the score, the solve's status."""
    nm = _nm(name, "duals", spec)
    spec2, lay, notes = drawn_layout(spec, plan, floors = floors)
    fix = None
    if sizes == "fixed":
        M = build(spec2, pin = lay, broken = "skip", mode = mode, step = step, floors = floors, name = nm, tie = 0.0)
    else:
        fix, n2 = arrangement_fix(spec2, lay, doors = plan.get("doors"), windows = plan.get("windows"), floors = floors, kinds = _kinds(plan))
        notes = notes + n2
        M = build(spec2, fix = fix, mode = mode, step = step, floors = floors, name = nm, tie = 0.0)
    s = run(M, time_limit = time_limit, gap = gap)
    out = {"status": s.status, "notes": notes, "rows": []}
    if not s.feasible:
        return out
    ints = {}
    for n, b in M["bins"].items():
        ints[b] = s.value(b)
    local = run(M, fix = ints) if ints else s
    is_local = len(ints) > 0
    out["local"] = is_local
    out["score"] = read(M, s)["meta"]["plan_solve"]["score"]
    out["lp_status"] = local.status
    rows = []
    for n in local.binding():
        fam = n.split(":")[0]
        if fam in ("def", "pair", "door", "tree", "flip"):
            continue
        if families != None and fam not in families:
            continue
        d = local.dual(n)
        if d == None or abs(d) < 1e-6:
            continue
        rows.append({"rule": n, "dual": d, "local": is_local, "slack": local.slack(n)})
    rows = sorted(rows, key = lambda r: -abs(r["dual"]))[:top]
    if resolve > 0:
        # the re-solves: the problem as it is, then with each rule relaxed, free (or pinned) -- no tie-break, so the
        # difference is the score's own
        sp = spec if arrangement == "free" else spec2
        fx = None if arrangement == "free" else fix
        M0 = build(sp, fix = fx, pin = lay if (arrangement != "free" and sizes == "fixed") else None, mode = mode, step = step,
                   floors = floors, name = nm + "_resolve_base", tie = 0.0)
        s0 = run(M0, time_limit = time_limit, gap = gap)
        out["resolve_base"] = {"status": s0.status, "objective": s0.objective, "gap": s0.gap}
        for i in range(min(resolve, len(rows))):
            r = rows[i]
            M2 = build(sp, fix = fx, pin = lay if (arrangement != "free" and sizes == "fixed") else None, shift = {r["rule"]: resolve_by},
                       mode = mode, step = step, floors = floors, name = nm + "_resolve_%d" % (i + 1), tie = 0.0)
            # the rule relaxed, the base's point still holds: it starts the search (`start=`), so a re-solve a
            # limit stops is never worse than the base
            st = start_of(M2, M0, s0)
            s2 = run(M2, time_limit = time_limit, gap = gap, start = st)
            r["started"] = len(st or {})
            r["shifted"] = len(M2["shifted"]) > 0
            if s2.feasible and s0.feasible and r["shifted"]:
                r["resolved"] = (s2.objective - s0.objective) / resolve_by
                r["resolved_gap"] = max(s2.gap or 0.0, s0.gap or 0.0)
            else:
                r["resolved"] = None
    out["rows"] = rows
    return out

# -------------------------------------------------------------------------------------------------- explain

FAMILY_WORDS = {
    "zone": "rooms inside the floor's zones",
    "pair": "rooms kept apart, with the wall between them",
    "size": "the program's room sizes",
    "area": "the program's areas",
    "R304": "habitable rooms 7 ft wide and 70 sq ft (R304)",
    "R311": "halls 36 in wide (R311.6)",
    "R307": "fixture clearances (R307)",
    "R310": "an escape window for every sleeping room (R310)",
    "shape": "room proportions (aspect)",
    "contain": "rooms that must hold a landing, a car, the steps",
    "touch": "rooms on the walls they must run along",
    "annex": "L-shaped rooms' second pieces",
    "door": "a door from every room onto its parent",
    "tree": "every room reaching the stair through doors",
    "owner": "the owner's rules",
    "value": "value curves",
    "sym": "symmetry between interchangeable rooms",
    "plumb": "wet rooms on full-height stacks",
    "struct": "bearing lines and spans",
    "acoustic": "rooms not over others",
    "envelope": "the envelope",
    "cap": "the floor-area cap",
    "flip": "options' cuts",
}

ORDER = ["zone", "pair", "size", "R304", "R311", "R307", "area", "shape", "R310", "contain", "touch", "annex",
         "owner", "envelope", "cap", "plumb", "struct", "acoustic", "value", "sym", "flip", "door", "tree"]

def _probe(spec, keep, units, extra_drop, nm, k, mode, step, probe_time, probe_nodes, floors, pin, fix, sub = None):
    """One feasibility probe: the problem with every unit not in `keep` left out (rebuilt without it)."""
    drop = [u for u in units if u not in keep] + list(extra_drop)
    if sub != None:
        drop = drop + sub
    M = build(spec, drop = drop, objective = "none", mode = mode, step = step, floors = floors, pin = pin, fix = fix,
              name = "%s_p%d" % (nm, k))
    s = run(M, time_limit = probe_time, max_nodes = probe_nodes)
    if s.status == "infeasible":
        return "infeasible"
    if s.feasible:
        return "feasible"
    return "unknown"

def _filter(items, test, budget):
    """Chinneck's additive filter, then a deletion pass: an irreducible subset of `items` whose test is "infeasible",
    given test(subset) -> "infeasible" | "feasible" | "unknown". Returns (members, undecided, probes)."""
    C = []
    undecided = []
    n = [0]
    def t(sub):
        n[0] += 1
        return test(sub)
    pool = list(items)
    for _ in range(len(items) + 1):
        if n[0] >= budget:
            break
        if C and t(C) == "infeasible":
            break
        hit = None
        T = list(C)
        for u in pool:
            if u in C:
                continue
            if n[0] >= budget:
                break
            T.append(u)
            r = t(T)
            if r == "infeasible":
                hit = u
                break
            if r == "unknown" and u not in undecided:
                undecided.append(u)
        if hit == None:
            break
        C.append(hit)
        pool = pool[:pool.index(hit)]
    # deletion pass: each member needed?
    out = []
    for u in C:
        if n[0] >= budget:
            out.append(u)
            undecided.append(u)
            continue
        rest = [x for x in C if x != u]
        r = t(rest) if rest else "feasible"
        if r == "infeasible":
            continue
        if r == "unknown" and u not in undecided:
            undecided.append(u)
        out.append(u)
    return out, undecided, n[0]

def _selector(u):
    parts = u.split(":")
    out = {"family": parts[0]}
    if len(parts) > 1 and parts[1]:
        out["floor"] = parts[1]
    if len(parts) > 2:
        out["room"] = ":".join(parts[2:])
    return out

def _answer(out):
    words = []
    for u in out["families"]:
        fam = u.split(":")[0]
        fl = u[len(fam) + 1:]
        rr = out["rooms"].get(u) or []
        words.append("%s on %s%s" % (FAMILY_WORDS.get(fam, fam), fl, (" (" + ", ".join(rr) + ")") if rr else ""))
    a = ("these cannot all hold: " + "; ".join(words)) if words else "no irreducible set found within the probes"
    if words and out.get("background"):
        fams = []
        for u in out["background"]:
            f = u.split(":")[0]
            if f not in fams and f in ("zone", "pair", "door", "tree"):
                fams.append(f)
        if fams:
            a += " -- with " + ", ".join([FAMILY_WORDS.get(f, f) for f in fams]) + " held throughout"
    if out["undecided"]:
        a += " (undecided within the probes' limits: %s)" % ", ".join(out["undecided"])
    if out.get("complete", True) == False:
        a += " (the probe budget ran out: the set may not be irreducible)"
    return a

def _by_conflict(s0, out, bg, cap, rank, search_time):
    """The builder's sol.conflict() on the explained problem: its deletion filter drops whole families while the rest
    stays infeasible, then halves of each family that had to stay, down to single rules. Built with the background
    untagged, the background is one group, tried last, so the families it names are the ones that clash with the
    background held (the rebuild filter's question); the background rules it names are reported as held."""
    rows = s0.conflict(max_probes = cap, time_limit = search_time)
    out["rows"] = rows
    out["held"] = []
    held_units = []
    for n in rows:
        parts = n.split(":")
        if len(parts) < 4 or parts[0] in bg:
            out["held"].append(n)
            if len(parts) >= 4 and (parts[0] + ":" + parts[1]) not in held_units:
                held_units.append(parts[0] + ":" + parts[1])
            continue
        u = parts[0] + ":" + parts[1]
        if u not in out["rooms"]:
            out["families"].append(u)
            out["rooms"][u] = []
        if parts[2] != "-" and parts[2] not in out["rooms"][u]:
            out["rooms"][u].append(parts[2])
    out["families"] = sorted(out["families"], key = lambda u: (rank.get(u.split(":")[0], len(rank)), u))
    for u in out["families"]:
        out["rooms"][u] = sorted(out["rooms"][u])
    out["background"] = sorted(held_units)
    # whether the builder stopped early (at max_probes or the budget) is its info's to say, not the answer's
    out["complete"] = None
    out["answer"] = _answer(out)
    return out

BACKGROUND = ["zone", "pair", "sym", "value", "annex", "def"]

METHODS = ["rebuild", "tags", "conflict", "conflict_families", "conflict_all"]

def explain(spec, *, rooms = True, order = None, focus = None, background = None, max_probes = 80, room_probes = 60,
            probe_time = 5.0, probe_nodes = None, mode = "exact", step = 0.25, name = None, floors = None, pin = None,
            fix = None, method = "rebuild", search_time = 60.0):
    """Why a wish cannot be met: the rule families that clash, by floor ("R310:A"), then the rooms in each.

    The problem is solved for any feasible point; if there is none, a filter over rule units (a family on a floor)
    finds an irreducible set -- these together cannot hold, and without any one of them the rest can. Every size
    limit is a rule of its own here (bounds="loose"), so the program's sizes can be named. Each probe is a
    feasibility solve of at most `probe_time` seconds (and `probe_nodes` nodes); `max_probes` bounds them all
    (the CLI's 120-s budget is the hard limit). A probe that stops without an answer leaves its unit "undecided":
    kept, and said so. Then, with rooms=True, the same filter over the rooms of each clashing unit names them.
    `order` is the families' order (the filter favours the earlier); `focus` families go first (the question's own:
    "R311" for "why not a 36-in landing"); `background` families are always held and never named (default: the
    floor's zones, walls between rooms, symmetry, value curves, L-pieces and definitions -- every clash involves
    rooms competing for room, so the answer names the rules a designer can change); `pin`/`fix` explain a pinned
    problem.
    method:
      "rebuild" (the default): Chinneck's additive filter then a deletion pass, each probe a model built without
        the units it leaves out;
      "tags": the same filter, each probe a solve of one problem with opt's drop= selector (no rebuild);
      "conflict": the builder's sol.conflict() on one problem, the background written untagged so that it is one
        group, tried last (held while the families are filtered); its rows name the rooms. One call, bounded by
        max_probes + room_probes probes; "complete" is None (the builder's info says when it stopped early);
      "conflict_families": the builder's conflict(families_only=True) for the families, then the rooms by drop=;
      "conflict_all": the builder's conflict() with every family a candidate, the background too.
    `search_time` bounds the builder's whole search (seconds; the conflict methods only), so it ends inside the
    evaluation's budget with time to spare.

    Returns {"feasible", "status", "families": [unit...], "rooms": {unit: [room...]}, "undecided", "probes",
    "answer", "method"} -- feasible True (nothing to explain), False, or None (the full problem was not proven
    infeasible within one probe); the conflict methods add "rows" (the builder's) and "held" (its background rows)."""
    nm = _nm(name, "explain", spec)
    if method not in METHODS:
        fail("plan_ask.explain: method is %s; got %r" % (", ".join(METHODS), method))
    bg = BACKGROUND if background == None else list(background)
    # first the problem as solve() builds it: there is something to explain only when that model has no point
    Mt = build(spec, objective = "none", mode = mode, step = step, floors = floors, pin = pin, fix = fix, name = nm + "_check")
    st = run(Mt, time_limit = probe_time, max_nodes = probe_nodes)
    out = {"status": st.status, "families": [], "rooms": {}, "undecided": [], "probes": 1, "method": method}
    if st.feasible:
        out["feasible"] = True
        out["answer"] = "the problem can be solved: nothing to explain"
        return out
    if st.status != "infeasible":
        out["feasible"] = None
        out["answer"] = "not proven impossible within one probe (%s): raise probe_time or probe_nodes" % st.status
        return out
    out["feasible"] = False
    # the same problem with every size limit a rule of its own (bounds="loose", the same width lattice), for the
    # units and for the methods that probe one problem
    held = method in ("conflict", "conflict_families")
    M0 = build(spec, objective = "none", mode = mode, step = step, floors = floors, pin = pin, fix = fix, name = nm + "_full",
               bounds = "loose", untag = bg if held else None)
    s0 = None
    if method != "rebuild":
        s0 = run(M0, time_limit = probe_time, max_nodes = probe_nodes)
        out["probes"] += 1
        if s0.status != "infeasible":
            out["note"] = "the one-problem model was not proven infeasible (%s): the rebuild filter instead" % s0.status
            method = "rebuild"
            out["method"] = method
    rank = {}
    fo = list(focus or []) + [f for f in (order or ORDER) if f not in (focus or [])]
    for i in range(len(fo)):
        rank[fo[i]] = i
    if method in ("conflict", "conflict_all"):
        return _by_conflict(s0, out, bg if method == "conflict" else [], max_probes + room_probes, rank, search_time)
    allu = [u for u in M0["units"].keys() if u.split(":")[0] != "def"]
    held_u = [u for u in allu if u.split(":")[0] in bg]
    units = [u for u in allu if u.split(":")[0] not in bg]
    units = sorted(units, key = lambda u: (rank.get(u.split(":")[0], len(fo)), u))
    out["background"] = sorted(held_u)
    budget = max_probes - 1

    def by_tags(keep, sub = None):
        sel = [_selector(u) for u in units if u not in keep] + [_selector(x) for x in (sub or [])]
        if not sel:
            sel = None
        s = run(M0, objective = False, drop = sel, time_limit = probe_time, max_nodes = probe_nodes)
        if s.status == "infeasible":
            return "infeasible"
        return "feasible" if s.feasible else "unknown"

    def test_units(keep):
        k = out["probes"]
        out["probes"] += 1
        if method == "tags":
            return by_tags(keep)
        return _probe(spec, keep, units, [], nm, k, mode, step, probe_time, probe_nodes, floors, pin, fix)

    if method == "conflict_families":
        fams = []
        for n in s0.conflict(families_only = True, max_probes = max_probes, time_limit = search_time):
            parts = n.split(":")
            if len(parts) >= 4 and parts[0] not in bg and (parts[0] + ":" + parts[1]) in units:
                u = parts[0] + ":" + parts[1]
                if u not in fams:
                    fams.append(u)
        fams = sorted(fams, key = lambda u: (rank.get(u.split(":")[0], len(fo)), u))
        und = []
    else:
        fams, und, n = _filter(units, test_units, budget)
    out["families"] = fams
    out["undecided"] = und
    # the builder's family pass says in its info whether it stopped early; the filter's own count says here
    out["complete"] = None if method == "conflict_families" else (out["probes"] < max_probes and not und)
    if rooms:
        named = {}
        for u in fams:
            for r in M0["units"].get(u, {}).keys():
                for q in r.split("|"):
                    named[q] = named.get(q, 0) + 1
        stop = out["probes"] + room_probes
        for u in fams:
            rms = [r for r in M0["units"].get(u, {}).keys() if r != "-"]
            # the rooms other clashing units name come first, the filter favouring the earlier
            def weight(r):
                n = 0
                for q in r.split("|"):
                    n += named.get(q, 0)
                return (-n, r)
            rms = sorted(rms, key = weight)
            if len(rms) < 2 or out["probes"] >= stop:
                out["rooms"][u] = rms
                continue
            others = [x for x in fams if x != u]
            def test_rooms(keep, u = u, rms = rms, others = others):
                k = out["probes"]
                out["probes"] += 1
                sub = ["%s:%s" % (u, r) for r in rms if r not in keep]
                if method in ("tags", "conflict_families"):
                    return by_tags(others + [u], sub)
                return _probe(spec, others + [u], units, [], nm, k, mode, step, probe_time, probe_nodes, floors, pin, fix, sub = sub)
            mem, und2, n2 = _filter(rms, test_rooms, stop - out["probes"])
            out["rooms"][u] = sorted(mem)
            out["undecided"] += ["%s:%s" % (u, r) for r in und2]
            if und2:
                out["complete"] = False
    out["answer"] = _answer(out)
    return out
