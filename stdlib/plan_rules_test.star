# The tests of @cadpole/plan_rules.star: the vetted rule menu, and a regression test for each rule of the standing
# set -- the exploit a solver takes when the rule is left out, shown on a fit without the rule (the check flags
# it), and closed on a fit with it (compiled into the MILP, the check passes) -- and for the architect's rules.
#
# Every fit here is one floor of a 25-ft benchmark parti on its own (plan_templates.single_floor), which solves in
# well under a second; synthetic geometry only.

load("@cadpole/plan_parti.star", "explain", "fit")
load("@cadpole/plan_rules.star", "architect", "check_rules", "menu", "rule", "rule_families", "standard")
load("@cadpole/plan_templates.star", "copy_template", "court", "floors_only", "set_room", "short_columns", "side_hall",
     "single_floor", "split_column")

def _red(plan, rules):
    return [x for x in check_rules(plan, rules) if not x["ok"]]

def _keys(rows):
    return [x["key"] for x in rows]

def _card(plan):
    return plan["meta"]["plan_parti"]

def the_menu():
    m = menu()
    assert_eq([r["name"] for r in standard()], ["garage_size", "hall_width", "storage_cap", "well_band",
                                                "pantry_by_kitchen", "stack_day"])
    assert_eq(len(architect()), 6)
    for r in m.values():
        # typology rules are hard; size rules and door etiquette are weighted
        assert_eq(r["hard"], r["class"] == "typology", msg = r["name"])
    g = rule("garage_size", wmax = 11.0, hard = True)
    assert_eq(g["args"]["wmax"], 11.0)
    assert_eq(g["hard"], True)
    assert_eq(rule("storage_cap", weight = 3.0)["hard"], False)
    assert_eq(rule_families(standard() + standard()), [r["name"] for r in standard()])
    return len(m)

test("the menu: typology rules hard, size rules weighted", the_menu)

def garage_size():
    # the middle court's grid gives the garage only its two east columns: 15'-8" wide, 314 sq ft
    cg = single_floor(court(), "G")
    base = fit(cg, name = "plan_rules_t_g0")
    assert_eq(_keys(_red(base, [rule("garage_size")])), ["garage_size:garage"])
    # weighted: the scorecard prices the 61 sq ft past 11'-6" x 22'-0"
    w = _card(fit(cg, rules = [rule("garage_size")], name = "plan_rules_t_g1"))
    assert_eq(w["status"], "optimal")
    assert_close(w["rule_terms"][0][2], 314.0 - 11.5 * 22.0, tol = 0.01)
    # hard: no fit, and the deletion filter names the rule
    x = explain(cg, rules = [rule("garage_size", hard = True)], name = "plan_rules_t_g2", families = ["garage_size"],
                cheapest = False)
    assert_eq(x["families"], ["garage_size"])
    # the template's knob: a column split at 12'-6" gives the garage 11'-4" and the rule holds
    sp = set_room(split_column(cg, 2, 12.5), "garage", spans = [(3, 4), (2, 4)])
    fixed = fit(sp, rules = [rule("garage_size", hard = True)], name = "plan_rules_t_g3")
    assert_eq(_card(fixed)["status"], "optimal")
    assert_eq(_red(fixed, [rule("garage_size")]), [])
    return w["rule_terms"][0][2]

test("garage_size: a one-car garage within about 11'-6\" x 22'-0\"", garage_size)

def hall_width():
    # the court's ground floor held to its three-floor depth: a cross hall soaks up the spare 4'-10"
    cg = copy_template(single_floor(court(), "G"))
    cg["fix_depth"] = {"G": 50.92}
    base = fit(cg, name = "plan_rules_t_h0")
    assert_eq(_keys(_red(base, [rule("hall_width")])), ["hall_width:hallG2"])
    ruled = fit(cg, rules = [rule("hall_width")], name = "plan_rules_t_h1")
    assert_eq(_red(ruled, [rule("hall_width")]), [])
    return _card(base)["objective"] - _card(ruled)["objective"]

test("hall_width: halls about 4 ft wide unless they are rooms", hall_width)

def storage_cap():
    # the side hall's ground floor: an 80-sq-ft mudroom against a 60-sq-ft cap
    g = single_floor(side_hall(), "G")
    base = fit(g, name = "plan_rules_t_s0")
    assert_eq(_keys(_red(base, [rule("storage_cap")])), ["storage_cap:mud"])
    capped = fit(g, rules = [rule("storage_cap", hard = True)], name = "plan_rules_t_s1")
    assert_eq(_red(capped, [rule("storage_cap")]), [])
    # weighted, the cap is a price per sq ft; with columns that may end short the stores need not fill the floor
    soft = _card(fit(short_columns(g), rules = [rule("storage_cap")], name = "plan_rules_t_s2"))
    assert_eq(soft["status"], "optimal")
    assert_true(len(soft["column_depths"]) == 4)
    return len(soft["rule_terms"])

test("storage_cap: caps on storage and workshop area", storage_cap)

def well_band():
    # the court's own court, on its main floor: with the rule it sits in the middle third and lights a living room
    cm = single_floor(court(), "M")
    ruled = fit(cm, rules = [rule("well_band")], name = "plan_rules_t_w0")
    assert_eq(_card(ruled)["status"], "optimal")
    assert_eq(_red(ruled, [rule("well_band")]), [])
    # the same court moved to the street front is flagged
    moved = dict(ruled)
    c = dict(ruled["courts"][0])
    r = c["rect"]
    front = ruled["floors"][0]["plate"][0][2]
    c["rect"] = (r[0], r[1], front, front + (r[3] - r[2]))
    moved["courts"] = [c]
    assert_eq(_keys(_red(moved, [rule("well_band")])), ["well_band:well"])
    return r[3] - r[2]

test("well_band: light wells only in the middle band, lighting a habitable room", well_band)

def pantry_by_kitchen():
    # the side hall's main floor: the pantry opens off the hall, a long walk from the kitchen
    m = single_floor(side_hall(), "M")
    base = fit(m, name = "plan_rules_t_p0")
    assert_eq(_keys(_red(base, [rule("pantry_by_kitchen")])), ["pantry_by_kitchen:pantry"])
    ruled = fit(m, rules = [rule("pantry_by_kitchen")], name = "plan_rules_t_p1")
    assert_eq(_red(ruled, [rule("pantry_by_kitchen")]), [])
    cost = _card(base)["objective"] - _card(ruled)["objective"]
    assert_close(cost, 31.0, tol = 0.1)  # what the rule costs this floor, in score
    return cost

test("pantry_by_kitchen: the pantry opens off the kitchen", pantry_by_kitchen)

def stack_day():
    # the kernel's stacks run on column lines and clear every room they would cross: a fit never stands one in a day
    # room; a plan that does is flagged
    m = fit(single_floor(side_hall(), "M"), name = "plan_rules_t_k0")
    assert_eq(_red(m, [rule("stack_day")]), [])
    living = [r for r in m["rooms"] if r["key"] == "living"][0]["rects"][0]
    bad = dict(m, stacks = m["stacks"] + [{"key": "s9", "at": ((living[0] + living[1]) / 2.0, (living[2] + living[3]) / 2.0),
                                           "floors": ["M"], "kind": "dwv"}])
    assert_eq(_keys(_red(bad, [rule("stack_day")])), ["stack_day:s9:living"])
    return len(m["stacks"])

test("stack_day: no plumbing stack through a day room", stack_day)

def garage_door_and_no_garage_only():
    # the side hall's ground floor with columns that may end short: the garage opens only into a mudroom and a store
    g = short_columns(single_floor(side_hall(), "G"))
    base = fit(g, name = "plan_rules_t_a0")
    assert_eq(_keys(_red(base, [rule("garage_door")])), ["garage_door:garage"])
    ruled = fit(g, rules = [rule("garage_door")], name = "plan_rules_t_a1")
    assert_eq(_red(ruled, [rule("garage_door")]), [])
    # with the stores capped, the bath's only door is the garage's
    caps = [rule("storage_cap", hard = True)]
    b2 = fit(g, rules = caps, name = "plan_rules_t_a2")
    assert_eq(_keys(_red(b2, [rule("no_garage_only")])), ["no_garage_only:bathG"])
    r2 = fit(g, rules = caps + [rule("no_garage_only")], name = "plan_rules_t_a3")
    assert_eq(_red(r2, [rule("no_garage_only")]), [])
    return 2

test("garage_door and no_garage_only: the garage opens into the house, and no bath is reached through it",
     garage_door_and_no_garage_only)

def wet_off_hall():
    # the side hall's main floor: the powder room opens into the living room; weighted, the rule is a price
    m = single_floor(side_hall(), "M")
    pb = fit(m, name = "plan_rules_t_a4")
    assert_eq(_keys(_red(pb, [rule("wet_off_hall")])), ["wet_off_hall:powder"])
    pw = _card(fit(m, rules = [rule("wet_off_hall")], name = "plan_rules_t_a5"))
    assert_close(pw["rule_terms"][0][2], 1.0, tol = 1e-6)
    ph = fit(m, rules = [rule("wet_off_hall", hard = True)], name = "plan_rules_t_a6")
    assert_eq(_red(ph, [rule("wet_off_hall")]), [])
    return pw["rule_terms"][0][1]

test("wet_off_hall: powder rooms and baths off a hall (door etiquette, weighted)", wet_off_hall)

def bed_width():
    # the side hall's upper floor: the 7'-5" x 14'-10" third bedroom, which takes no full bed
    u = single_floor(side_hall(), "U")
    ub = fit(u, name = "plan_rules_t_a7")
    assert_eq(_keys(_red(ub, [rule("bed_width")])), ["bed_width:bed3"])
    x = explain(u, rules = [rule("bed_width", hard = True)], name = "plan_rules_t_a8", cheapest = False,
                families = ["bed_width"])
    assert_true("bed_width" in x["families"], msg = str(x["families"]))
    return len(x["families"])

test("bed_width: bedrooms wide enough for a full bed", bed_width)

def egress_facade():
    # the court's upper floor: the middle bedroom's only window is on the court (R310.1, unverified with the building
    # department); the clash is the template's own front-to-back order of the bedrooms, and dropping it is free
    cu = single_floor(court(), "U")
    cb = fit(cu, name = "plan_rules_t_a9")
    assert_eq(_keys(_red(cb, [rule("egress_facade")])), ["egress_facade:bed3"])
    xe = explain(cu, rules = [rule("egress_facade")], name = "plan_rules_t_a10",
                 families = ["order", "facade", "egress_facade"])
    assert_eq(xe["families"], ["order", "egress_facade"])
    assert_eq(xe["cheapest"]["group"], "order:U")
    assert_close(xe["cheapest"]["objective"], _card(cb)["objective"], tol = 1e-6)
    return len(xe["groups"])

test("egress_facade: an escape window on a facade, not on an enclosed court", egress_facade)

def not_over():
    # the side hall's main and upper floors: its optimum puts Bath 2 and the primary bath over the living room and the
    # laundry over the dining room; with the rule they cannot fit at all -- the main floor's one room band is all day
    # rooms -- and the filter says which rules clash
    t = floors_only(side_hall(), ["M", "U"])
    base = fit(t, name = "plan_rules_t_n0")
    assert_eq(sorted(_keys(_red(base, [rule("not_over")]))),
              ["not_over:bath2:living", "not_over:laundry:dining", "not_over:pbath:living"])
    x = explain(t, rules = [rule("not_over")], name = "plan_rules_t_n1", cheapest = False, families = ["not_over"])
    assert_eq(x["feasible"], "infeasible")
    assert_true("not_over" in x["families"], msg = str(x["families"]))
    return len(x["families"])

test("not_over: no wet room over a living room or a bedroom", not_over)
