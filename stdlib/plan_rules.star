"""@cadpole/plan_rules.star -- the vetted rule menu for generated plans.

    load("@cadpole/plan_rules.star", "menu", "rule", "standard", "architect", "rule_families", "check_rules")

    rules = standard()                                   # the standing set
    rules = standard() + [rule("bed_width", wmin=8.5)]   # a rule from the menu, an argument changed
    rules = [rule("garage_size", hard=True)]             # a size rule made hard

A rule is plain data:

    {"name": "garage_size", "family": "garage_size", "class": "size", "hard": False, "weight": 2.0,
     "args": {"wmax": 11.5, "dmax": 22.0}, "why": "...", "exploit": "..."}

**Typology rules are hard; size rules are weighted**, and so is door etiquette (which room a powder room or a bedroom
opens off: a preference, priced), so every critique shows up as a binding rule or a price in the scorecard, never
as a plan that silently ignores it. A generator compiles the rules it supports into its MILP --
`@cadpole/plan_parti.star` compiles every rule here, each under its own family name, so `explain()` names the rule that
clashes -- and `check_rules(plan, rules)` measures any plan value (the @cadpole/plan format) against them, returning
check rows (key, name, ok, detail, rule, severity, floor, room, at | rect) for the red pen: the same menu audits a
generated plan and a hand-drawn one. Kept rooms (`kept=True`) are exempt from every rule here.

## The standing set

Each closes an exploit a solver takes when the rule is left unwritten, and each has
a regression test in `stdlib/plan_rules_test.star`:

* `garage_size`: a one-car garage within about 11'-6" x 22'-0" (left free, garages stretched to 28'-9" deep or
  15'-8" wide);
* `hall_width`: halls about 4 ft wide unless they are rooms (a 5'-8" x 42'-8" corridor; a 15'-8" x 5'-6" dead-end
  hall);
* `storage_cap`: caps on storage and workshop area (a 231-sq-ft workshop and 133-sq-ft stores filling leftover
  depth);
* `well_band`: light wells only in the middle band of the floor, and only where they light a habitable room;
* `pantry_by_kitchen`: the pantry opens off the kitchen (a 69-ft walk from the kitchen to the pantry);
* `stack_day`: no plumbing stack through a day room (stacks in the open in a living or dining room).

`architect()` adds an architect's door and room rules: a door from the garage into the house, baths
not reached through the garage, powder rooms and baths off a hall, bedrooms off a hall, an escape window on a
facade (R310.1, unverified with the building department), and bedrooms wide enough for a full bed.
`rule("not_over")` is the floors' "not over" rows: no wet room over a living room or a bedroom on the floor below,
or any pair of kinds (a media room over the bedrooms: `rule("not_over", upper=["media"], lower=["bed"])`).

Code readings here are unverified: R310.1's yard-or-court wording and CPC 402.5 are open questions for the building
department.
Loading this module records nothing; it has no solver of its own. Its purity probe is `rule`.
"""

_DAY = ["living", "dining", "family", "den", "study", "office", "play", "media", "kitchen"]

_MENU = {
    # ---- the standing set
    "garage_size": {"class": "size", "hard": False, "weight": 2.0, "args": {"wmax": 11.5, "dmax": 22.0},
                    "why": "a one-car garage within about 11'-6\" x 22'-0\"",
                    "exploit": "left free, garages stretched to 28'-9\" deep or 15'-8\" wide"},
    "hall_width": {"class": "typology", "hard": True, "weight": 0.0, "args": {"wmax": 4.5},
                   "why": "halls about 4 ft wide unless they are rooms (a hall's narrow side at most 4'-6\")",
                   "exploit": "a 5'-8\" x 42'-8\" corridor; a 15'-8\" x 5'-6\" dead-end hall"},
    "storage_cap": {"class": "size", "hard": False, "weight": 1.0,
                    "args": {"caps": {"storage": 100.0, "workshop": 150.0, "mud": 60.0, "mech": 60.0}},
                    "why": "caps on storage and workshop area, per room",
                    "exploit": "a 231-sq-ft workshop and 133-sq-ft stores filling leftover depth"},
    "well_band": {"class": "typology", "hard": True, "weight": 0.0, "args": {"band": (1.0 / 3.0, 2.0 / 3.0)},
                  "why": "light wells only in the middle band of the floor, and only where they light a habitable room",
                  "exploit": "wells cut at the front or back, where a facade already lights, or beside a bath"},
    "pantry_by_kitchen": {"class": "typology", "hard": True, "weight": 0.0, "args": {"pantry": "pantry", "kitchen": "kitchen"},
                          "why": "the pantry opens off the kitchen",
                          "exploit": "a 69-ft walk from the kitchen to the pantry"},
    "stack_day": {"class": "typology", "hard": True, "weight": 0.0, "args": {"kinds": _DAY},
                  "why": "no plumbing stack through a day room: stacks in walls or chases",
                  "exploit": "stacks standing in the open in a living or dining room"},
    # ---- the architect's rules
    "garage_door": {"class": "typology", "hard": True, "weight": 0.0, "args": {},
                    "why": "a door from the garage into the house",
                    "exploit": "no door from the garage into the house"},
    "no_garage_only": {"class": "typology", "hard": True, "weight": 0.0, "args": {"kinds": ["bath", "powder", "wc"] + _DAY},
                       "why": "no bath or day room reached only through the garage",
                       "exploit": "a bath whose only door is from the garage"},
    "wet_off_hall": {"class": "etiquette", "hard": False, "weight": 10.0, "args": {"kinds": ["powder", "bath", "wc"],
                                                                              "not_off": ["living", "dining", "kitchen"]},
                     "why": "powder rooms and baths open off a hall, not a day room",
                     "exploit": "the powder room opening into the dining room"},
    "bed_off_hall": {"class": "etiquette", "hard": False, "weight": 10.0, "args": {"not_off": _DAY},
                     "why": "bedrooms open off a hall or the stair, not a day room",
                     "exploit": "a bedroom reached through the living room"},
    "egress_facade": {"class": "typology", "hard": True, "weight": 0.0, "args": {"sources": ["street", "rear"]},
                      "why": "each sleeping room's escape window on a facade (R310.1: a public way, or a yard or court " +
                             "that opens to one; an enclosed light well does not) -- unverified, a question for the building department",
                      "exploit": "an escape window onto an enclosed light well"},
    "not_over": {"class": "typology", "hard": True, "weight": 0.0,
                 "args": {"upper": ["bath", "powder", "wc", "laundry"], "lower": _DAY + ["bed", "guest"], "tol": 12.0},
                 "why": "no wet room over a living room or a bedroom on the floor below (noise and leaks); pairs of " +
                        "kinds, e.g. a media room off the kids' rooms",
                 "exploit": "Bath 2 sitting over the dining room"},
    "bed_width": {"class": "size", "hard": False, "weight": 5.0, "args": {"wmin": 8.5},
                  "why": "bedrooms at least 8'-6\" across, for a full bed with its clearances",
                  "exploit": "a 7'-5\" x 14'-10\" bedroom that cannot take a full bed"},
}

_STANDARD = ["garage_size", "hall_width", "storage_cap", "well_band", "pantry_by_kitchen", "stack_day"]
_ARCHITECT = ["garage_door", "no_garage_only", "wet_off_hall", "bed_off_hall", "egress_facade", "bed_width"]

def menu():
    """Every rule the menu offers, by name: {"name", "family", "class", "hard", "weight", "args", "why", "exploit"}."""
    out = {}
    for k, v in _MENU.items():
        r = dict(v)
        r["name"] = k
        r["family"] = k
        r["args"] = dict(v["args"])
        out[k] = r
    return out

def rule(name, hard = None, weight = None, **args):
    """A rule from the menu by name, with its arguments overridden by `args` and its hardness by `hard=` (True: a
    rule the plan must keep; False: a price, `weight` per unit of shortfall)."""
    m = menu()
    if name not in m:
        fail("plan_rules.rule: no rule %r (have: %s)" % (name, ", ".join(sorted(m.keys()))))
    r = m[name]
    r["args"].update(args)
    if weight != None:
        r["weight"] = weight
        if hard == None:
            r["hard"] = False
    if hard != None:
        r["hard"] = hard
    return r

def standard(**overrides):
    """The standing set: the rules every generated plan gets (garage_size, hall_width, storage_cap, well_band,
    pantry_by_kitchen, stack_day). `overrides` maps a rule name to its replacement (or None to leave it out)."""
    out = []
    for k in _STANDARD:
        if k in overrides:
            if overrides[k] != None:
                out.append(overrides[k])
        else:
            out.append(rule(k))
    return out

def architect():
    """An architect's door and room rules (garage_door, no_garage_only, wet_off_hall, bed_off_hall,
    egress_facade, bed_width)."""
    return [rule(k) for k in _ARCHITECT]

def rule_families(rules):
    """The families the rules' rows are named under, in order, once each."""
    out = []
    for r in rules:
        if r["family"] not in out:
            out.append(r["family"])
    return out

# ------------------------------------------------------------------ checks on any plan value

def _rect_wd(r):
    return r[1] - r[0], r[3] - r[2]

def _room_rect(room):
    return room["rects"][0]

def _rooms(plan, kinds = None, floor = None):
    out = []
    for r in plan.get("rooms", []):
        if kinds != None and r.get("kind") not in kinds:
            continue
        if floor != None and r.get("floor") != floor:
            continue
        out.append(r)
    return out

def _ftin(x):
    n = int(abs(x) * 12.0 + 0.5)
    return "%s%d'-%d\"" % ("-" if x < 0 else "", n // 12, n % 12)

def _sqft(a):
    return format(a, ",.0f") + " sq ft"

def _row(rule_, key, ok, detail, floor = None, room = None, rect = None, at = None):
    out = {"key": "%s:%s" % (rule_["name"], key), "name": rule_["why"], "ok": ok, "detail": detail,
           "rule": rule_["name"], "severity": "note" if ok else ("fail" if rule_["hard"] else "warn"),
           "floor": floor, "room": room}
    if rect != None:
        out["rect"] = rect
    if at != None:
        out["at"] = at
    return out

def _doors_between(plan, a, b):
    return [d for d in plan.get("doors", []) if (d.get("a") == a and d.get("b") == b) or (d.get("a") == b and d.get("b") == a)]

def _door_partners(plan, key):
    out = []
    for d in plan.get("doors", []):
        if d.get("a") == key:
            out.append(d.get("b"))
        elif d.get("b") == key:
            out.append(d.get("a"))
    return out

def _kind_of(plan):
    out = {}
    for r in plan.get("rooms", []):
        out[r["key"]] = r.get("kind")
    return out

def _plate_depth(plan, floor):
    for f in plan.get("floors", []):
        if f["key"] == floor and f.get("plate"):
            return min([p[2] for p in f["plate"]]), max([p[3] for p in f["plate"]])
    rs = _rooms(plan, floor = floor)
    if not rs:
        return None
    return min([r["rects"][0][2] for r in rs]), max([r["rects"][0][3] for r in rs])

def _reach_without(plan, floor, start, banned):
    """Rooms on `floor` reachable from `start` through doors, never entering a room in `banned`."""
    adj = {}
    for d in plan.get("doors", []):
        if d.get("floor") != floor:
            continue
        a, b = d.get("a"), d.get("b")
        adj.setdefault(a, []).append(b)
        adj.setdefault(b, []).append(a)
    seen = {start: True}
    q = [start]
    for _ in range(10000):
        if not q:
            break
        x = q.pop()
        for y in adj.get(x, []):
            if y in seen or y in banned:
                continue
            seen[y] = True
            q.append(y)
    return seen

def _check_one(plan, r):
    a = r["args"]
    n = r["name"]
    out = []
    kinds = _kind_of(plan)
    if n == "garage_size":
        for g in _rooms(plan, ["garage"]):
            if g.get("kept"):
                continue
            w, d = _rect_wd(_room_rect(g))
            nw, nd = min(w, d), max(w, d)
            ok = nw <= a["wmax"] + 0.05 and nd <= a["dmax"] + 0.05
            out.append(_row(r, g["key"], ok, "%s: %s x %s (the rule: within %s x %s)" %
                            (g.get("name", g["key"]), _ftin(nw), _ftin(nd), _ftin(a["wmax"]), _ftin(a["dmax"])),
                            g["floor"], g["key"], rect = _room_rect(g)))
    elif n == "hall_width":
        for h in _rooms(plan, ["hall"]):
            if h.get("kept") or h.get("furnish") or h.get("hall_room"):
                continue
            w, d = _rect_wd(_room_rect(h))
            ok = min(w, d) <= a["wmax"] + 0.05
            out.append(_row(r, h["key"], ok, "%s: %s x %s, its narrow side %s (at most %s unless it is a room)" %
                            (h.get("name", h["key"]), _ftin(w), _ftin(d), _ftin(min(w, d)), _ftin(a["wmax"])),
                            h["floor"], h["key"], rect = _room_rect(h)))
    elif n == "storage_cap":
        for s in _rooms(plan, list(a["caps"].keys())):
            if s.get("kept"):
                continue
            w, d = _rect_wd(_room_rect(s))
            cap = a["caps"][s["kind"]]
            out.append(_row(r, s["key"], w * d <= cap + 0.5, "%s: %s (cap %s)" % (s.get("name", s["key"]), _sqft(w * d),
                            _sqft(cap)), s["floor"], s["key"], rect = _room_rect(s)))
    elif n == "well_band":
        for c in plan.get("courts", []):
            if c.get("kind") != "well":
                continue
            lo_hi = _plate_depth(plan, c["base"])
            v0, v1 = c["rect"][2], c["rect"][3]
            mid = 0.5 * (v0 + v1)
            ok = True
            detail = "light well %s" % c["key"]
            if lo_hi != None:
                L0, L1 = lo_hi
                b0, b1 = L0 + a["band"][0] * (L1 - L0), L0 + a["band"][1] * (L1 - L0)
                ok = b0 - 0.05 <= mid and mid <= b1 + 0.05
                detail += ": centred %s back of the %s band (%s to %s)" % (_ftin(mid - L0), "middle", _ftin(b0 - L0), _ftin(b1 - L0))
            lit = [w for w in plan.get("windows", []) if w.get("source") == "well:" + c["key"] and
                   kinds.get(w.get("room")) in _DAY + ["bed", "guest"]]
            if not lit:
                ok = False
                detail += "; it lights no habitable room"
            out.append(_row(r, c["key"], ok, detail, c["base"], None, rect = c["rect"]))
    elif n == "pantry_by_kitchen":
        p_, k_ = a["pantry"], a["kitchen"]
        if p_ in kinds and k_ in kinds:
            ok = len(_doors_between(plan, p_, k_)) > 0
            out.append(_row(r, p_, ok, "the pantry %s the kitchen" % ("opens off" if ok else "has no door to"),
                            None, p_))
    elif n == "stack_day":
        for s in plan.get("stacks", []):
            x, y = s["at"][0], s["at"][1]
            for f in s.get("floors", []):
                for q in _rooms(plan, a["kinds"], f):
                    if q.get("kept"):
                        continue
                    for rc in q["rects"]:
                        inside = rc[0] + 0.6 < x and x < rc[1] - 0.6 and rc[2] + 0.6 < y and y < rc[3] - 0.6
                        if inside:
                            out.append(_row(r, "%s:%s" % (s["key"], q["key"]), False, "stack %s stands in the %s" %
                                            (s["key"], q.get("name", q["key"])), f, q["key"], at = (x, y)))
        if not out:
            out.append(_row(r, "all", True, "every stack runs in a wall or a chase", None, None))
    elif n == "garage_door":
        for g in _rooms(plan, ["garage"]):
            # into the house: through its doors, never outside, to a hall, the entry or the stair
            seen = _reach_without(plan, g["floor"], g["key"], {"outside": True, "outside:street": True,
                                                                 "outside:yard": True, "outside:deck": True})
            circ = [k for k in seen if kinds.get(k) in ("hall", "entry", "foyer", "stair", "landing", "lobby")]
            out.append(_row(r, g["key"], len(circ) > 0, "the garage %s" %
                            ("reaches the house's %s" % "/".join(sorted(circ)) if circ else
                             "has no door into the house (only into %s)" % (
                                 "/".join(sorted([k for k in seen if k != g["key"]])) or "the street")),
                            g["floor"], g["key"]))
    elif n == "no_garage_only":
        for g in _rooms(plan, ["garage"]):
            f = g["floor"]
            entries = [d.get("a") for d in plan.get("doors", []) if d.get("floor") == f and d.get("b") == "outside:street"
                       and kinds.get(d.get("a")) != "garage"]
            if not entries:
                continue
            seen = _reach_without(plan, f, entries[0], {g["key"]: True, "outside:street": True})
            for q in _rooms(plan, a["kinds"], f):
                if q["key"] not in seen and not q.get("kept"):
                    out.append(_row(r, q["key"], False, "%s is reached only through the garage" % q.get("name", q["key"]),
                                    f, q["key"]))
    elif n == "wet_off_hall":
        for q in _rooms(plan, a["kinds"]):
            bad = [b for b in _door_partners(plan, q["key"]) if kinds.get(b) in a["not_off"]]
            if bad:
                out.append(_row(r, q["key"], False, "%s opens into the %s" % (q.get("name", q["key"]), "/".join(bad)),
                                q["floor"], q["key"]))
    elif n == "bed_off_hall":
        for q in _rooms(plan, ["bed", "guest"]):
            bad = [b for b in _door_partners(plan, q["key"]) if kinds.get(b) in a["not_off"]]
            if bad:
                out.append(_row(r, q["key"], False, "%s opens off the %s" % (q.get("name", q["key"]), "/".join(bad)),
                                q["floor"], q["key"]))
    elif n == "egress_facade":
        for q in _rooms(plan, ["bed", "guest"]):
            ws = [w for w in plan.get("windows", []) if w.get("room") == q["key"] and w.get("source") in a["sources"]]
            out.append(_row(r, q["key"], len(ws) > 0, "%s %s" % (q.get("name", q["key"]),
                            "has a window on a facade" if ws else "has no window on a facade (a well does not serve R310)"),
                            q["floor"], q["key"]))
    elif n == "not_over":
        floors = sorted([f for f in plan.get("floors", [])], key = lambda f: f.get("z", 0.0))
        for i in range(1, len(floors)):
            up, lo = floors[i]["key"], floors[i - 1]["key"]
            for a_ in _rooms(plan, a["upper"], up):
                for b_ in _rooms(plan, a["lower"], lo):
                    ra, rb = _room_rect(a_), _room_rect(b_)
                    ou = min(ra[1], rb[1]) - max(ra[0], rb[0])
                    ov = min(ra[3], rb[3]) - max(ra[2], rb[2])
                    if ou > 1.0 and ov > 1.0 and ou * ov > a["tol"]:
                        out.append(_row(r, "%s:%s" % (a_["key"], b_["key"]), False, "%s sits over the %s (%s)" %
                                        (a_.get("name", a_["key"]), b_.get("name", b_["key"]), _sqft(ou * ov)), up, a_["key"],
                                        rect = (max(ra[0], rb[0]), min(ra[1], rb[1]), max(ra[2], rb[2]), min(ra[3], rb[3]))))
        if not out:
            out.append(_row(r, "all", True, "no room of %s over a room of %s" % ("/".join(a["upper"]), "/".join(a["lower"])),
                            None, None))
    elif n == "bed_width":
        for q in _rooms(plan, ["bed", "guest"]):
            w, d = _rect_wd(_room_rect(q))
            out.append(_row(r, q["key"], min(w, d) >= a["wmin"] - 0.05, "%s: %s x %s (at least %s across)" %
                            (q.get("name", q["key"]), _ftin(w), _ftin(d), _ftin(a["wmin"])), q["floor"], q["key"],
                            rect = _room_rect(q)))
    return out

def check_rules(plan, rules):
    """Check rows (key, name, ok, detail, rule, severity, floor, room, at | rect) for `plan` against `rules`: a failed
    hard rule is a "fail", a failed weighted rule a "warn"; rows that hold are "note"s."""
    out = []
    for r in rules:
        out.extend(_check_one(plan, r))
    return out
