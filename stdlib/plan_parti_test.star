# The tests of @cadpole/plan_parti.star: the parti kernel on a 25 x 100-ft benchmark lot,
# synthetic geometry only. `cadpole test stdlib/plan_parti_test.star` runs it; each fit is one MILP over every floor.
# The explanations, the grafts and options are stdlib/plan_parti_explain_test.star.
#
# The side hall's proven optimum: objective 1091.93, kernel score 1031.93. The other three partis take 10 to 60 s
# each, so they are examples (examples/plan_parti_*.star), one per run, under the CLI's 120-s budget.

load("@cadpole/plan.star", "get_room", "validate")
load("@cadpole/plan_parti.star", "FAMILIES", "compare", "fit", "ftin", "grid", "precheck", "resolve")
load("@cadpole/plan_rules.star", "rule")
load("@cadpole/plan_templates.star", "PROGRAM", "SITE", "copy_template", "court", "knob", "library", "mirror", "set_room",
     "short_columns", "side_hall", "single_floor", "split_column", "with_light_well", "without_court")

def the_grid_is_the_template_lines():
    g = grid([3.7, 7.6, 18.2])
    assert_eq(g["C"], 4)
    assert_close(g["cols"][0][0], 0.0, tol = 1e-12)
    assert_close(g["cols"][0][1], 3.5, tol = 1e-12)
    assert_close(g["cols"][2][1] - g["cols"][2][0], 10.2, tol = 1e-9)  # the side hall's 10'-2" room column
    assert_close(g["cols"][3][1], 24.0, tol = 1e-12)
    assert_eq(ftin(10.2), "10'-2\"")
    return g["C"]

test("the column grid comes from the template's partition lines", the_grid_is_the_template_lines)

def the_side_hall_reproduces_its_proven_optimum():
    plan = fit(side_hall(), SITE, PROGRAM, name = "plan_parti_side_hall")
    c = plan["meta"]["plan_parti"]
    assert_eq(c["status"], "optimal")
    # the optimum: 1091.926235630252, kernel 1031.926
    assert_close(c["objective"], 1091.926235630252, tol = 1e-3)
    assert_close(c["kernel"], 1031.926235630252, tol = 1e-3)
    assert_eq(c["pattern_terms"][0][0], "light on two sides (159)")
    assert_close(c["pattern_terms"][0][2], 2.0, tol = 1e-6)
    # the floors stand on each other: the deck at least 8 ft, the upper floor within 2 ft of the main
    d = c["depths"]
    assert_true(d["G"] - d["M"] >= 8.0 - 1e-6, msg = "the main floor stops 8 ft short of the ground floor")
    assert_true(d["U"] - d["M"] <= 2.0 + 1e-6, msg = "the upper floor cantilevers at most 2 ft")
    assert_true(c["gross"] <= 3600.0 + 1e-6, msg = "the floor-area cap")
    # the plan value is well formed in the @cadpole/plan format
    probs = validate(plan)
    assert_eq(probs, [])
    living = get_room(plan, "living")
    assert_eq(living["floor"], "M")
    assert_close(living["rects"][0][2], 5.5, tol = 1e-6)  # on the street facade: v = 0 is the front wall's inside face
    street = [d for d in plan["doors"] if d["b"] == "outside:street"]
    assert_eq(sorted([d["a"] for d in street]), ["garage", "vestibule"])
    assert_true(len(plan["stacks"]) >= 1, msg = "the wet rooms stand on a stack")
    assert_eq([x["key"] for x in plan["courts"]], ["well"])
    # every program room has a character to compare options by
    assert_eq(len(c["character"]), 16)
    # the light measure: the 5'-7" rear arm lights the main-floor dining room 0 ft deep (view_cone's sky
    # table), not the assumed 12 ft, so the house is darker than the assumed figure says
    assert_close(c["light"]["court_lit"]["w_dining_wellM"], 0.0, tol = 1e-6)
    assert_true(c["light"]["dark_sqft"] > c["metrics"]["dark_sqft_assumed"] + 50.0,
                msg = "%s vs %s" % (c["light"]["dark_sqft"], c["metrics"]["dark_sqft_assumed"]))
    return c["objective"]

test("the side hall reproduces its proven optimum", the_side_hall_reproduces_its_proven_optimum)

def the_four_partis_pass_the_precheck():
    out = []
    for t in library():
        rows = [r for r in precheck(t) if not r["ok"]]
        assert_eq(rows, [], msg = t["key"])
        out.append(t["key"])
    return len(out)

test("the four partis pass the precheck", the_four_partis_pass_the_precheck)

def the_precheck_names_what_cannot_fit():
    # a side hall whose upper hall is gone: the bedrooms have nothing to open off
    t = copy_template(side_hall())
    t["rooms"] = [r for r in t["rooms"] if r["key"] != "hallU"]
    t = set_room(t, "bed2", parents = ["hallU"])
    rows = [r for r in precheck(t) if not r["ok"]]
    keys = [r["key"] for r in rows]
    assert_true("door:bed2" in keys and "door:primary" in keys, msg = "nothing to open off: %s" % keys)
    # a laundry moved off every stack line
    t2 = set_room(copy_template(side_hall()), "laundry", spans = [(0, 0)])
    keys2 = [r["key"] for r in precheck(t2) if not r["ok"]]
    assert_true("stack:laundry" in keys2, msg = str(keys2))
    # a program room the template has no place for
    keys3 = [r["key"] for r in precheck(side_hall(), program = dict(PROGRAM, sauna = PROGRAM["bath2"])) if not r["ok"]]
    assert_eq(keys3, ["place:sauna"])
    return len(keys) + len(keys2) + len(keys3)

test("the precheck names what cannot fit, before any MILP", the_precheck_names_what_cannot_fit)

def a_program_shapes_the_template():
    # without a den the ground floor's den placement goes, and nothing still opens off it
    prog = {k: v for k, v in PROGRAM.items() if k != "den"}
    t = resolve(side_hall(), SITE, prog)
    assert_eq([r["key"] for r in t["rooms"] if r["key"] == "den"], [])
    assert_eq([o for o in t["orders"] if "den" in o], [])
    # with a fourth bedroom, the template's bed4 placement is used
    t4 = resolve(side_hall(), SITE, dict(PROGRAM, bed4 = PROGRAM["bed3"]))
    assert_eq([r["key"] for r in t4["rooms"] if r["key"] == "bed4"], ["bed4"])
    return len(t["rooms"])

test("the program decides which of a template's rooms are built", a_program_shapes_the_template)

def the_parameters_are_template_transforms():
    t = side_hall()
    m = mirror(t)
    assert_eq(m["lines"], [24.0 - 18.2, 24.0 - 7.6, 24.0 - 3.7])
    assert_eq(m["stair"]["col"], 3)
    assert_eq(m["wells"][0]["side"], "W")
    s = split_column(t, 2, 12.0)
    assert_eq(s["lines"], [3.7, 7.6, 12.0, 18.2])
    assert_eq(s["bearing"], [2])
    lw = with_light_well(t, side = "W", y = 18.0, d = 12.0)
    assert_eq(lw["fixed"]["lwM"], {"span": (0, 0), "y": 18.0, "d": 12.0})
    nc = without_court(t)
    assert_eq(nc["wells"], [])
    assert_eq([r["key"] for r in nc["rooms"] if r["key"].startswith("well")], [])
    four = side_hall(floors = 4)
    assert_eq(four["floors"], ["G", "M", "U", "T"])
    return len(FAMILIES)

test("mirror, column splits, a neighbour's well and the court are template transforms", the_parameters_are_template_transforms)

# ---- the door graph, short columns, explanations and options, on one floor at a time (each solves in a second)

def _doors_of(plan, k):
    return [(d["room"], d["parent"]) for d in plan["meta"]["plan_parti"]["doors_kernel"] if d["room"] == k]

def parents_need_no_order():
    # the side hall's main floor with its hall listed after the rooms that open off it: depth rows keep the access
    # doors a tree, and the optimum is the ordered template's
    m = single_floor(side_hall(), "M")
    base = fit(m, name = "plan_parti_t_order_a")["meta"]["plan_parti"]["objective"]
    m2 = copy_template(m)
    hall = [r for r in m2["rooms"] if r["key"] == "hallM"][0]
    m2["rooms"] = [r for r in m2["rooms"] if r["key"] != "hallM"] + [hall]
    got = fit(m2, name = "plan_parti_t_order_b")["meta"]["plan_parti"]
    assert_eq(got["status"], "optimal")
    assert_close(got["objective"], base, tol = 1e-6)
    return got["objective"]

test("a room's parents may come in any order", parents_need_no_order)

def doors_are_a_graph_with_wishes_and_bans():
    m = single_floor(side_hall(), "M")
    base = fit(m, name = "plan_parti_t_dg0")
    assert_eq(_doors_of(base, "pantry"), [("pantry", "hallM")])  # the pantry: off the hall
    banned = copy_template(m)
    banned["doors"]["ban"] = [("pantry", "hallM")]
    b = fit(banned, name = "plan_parti_t_dg1")
    assert_true(("pantry", "hallM") not in _doors_of(b, "pantry"))
    # a wish worth more than its price is taken, one worth less is not
    worth = copy_template(m)
    worth["doors"]["wish"] = [("pantry", "kitchen", 40.0)]
    w = fit(worth, name = "plan_parti_t_dg2")["meta"]["plan_parti"]
    assert_close(w["wish_terms"][0][2], 1.0, tol = 1e-6)
    cheap = copy_template(m)
    cheap["doors"]["wish"] = [("pantry", "kitchen", 10.0)]
    c = fit(cheap, name = "plan_parti_t_dg3")["meta"]["plan_parti"]
    assert_close(c["wish_terms"][0][2], 0.0, tol = 1e-6)
    assert_close(c["objective"], base["meta"]["plan_parti"]["objective"], tol = 1e-6)
    # the price of the door the wish asks for: 31 points on this floor, so 40 buys it (and 10 does not)
    assert_close(w["objective"] - 40.0, 547.04, tol = 0.05)
    # a must: the garage opens into the house as well as onto the street, here for nothing
    g = copy_template(single_floor(side_hall(), "G"))
    g["doors"]["must"] = [("garage", ["hallG", "vestibule"])]
    gp = fit(g, name = "plan_parti_t_dg4")
    extra = [d for d in gp["meta"]["plan_parti"]["doors_kernel"] if d["room"] == "garage" and d.get("extra")]
    assert_eq(len(extra), 1)
    return w["objective"]

test("doors are a graph: access doors, musts, weighted wishes and bans", doors_are_a_graph_with_wishes_and_bans)

def columns_may_end_short():
    # the side hall's ground floor with stores capped: every column filled to the rear wall, or columns that may end
    # short; the closets shrink to their targets and the plate steps
    g = single_floor(side_hall(), "G")
    caps = [rule("storage_cap", hard = True)]
    full = fit(g, rules = caps, name = "plan_parti_t_sc0")
    short = fit(short_columns(g), rules = caps, name = "plan_parti_t_sc1")
    cf, cs = full["meta"]["plan_parti"], short["meta"]["plan_parti"]
    assert_eq(cs["status"], "optimal")
    assert_true(cs["objective"] > cf["objective"] + 5.0, msg = "%s vs %s" % (cs["objective"], cf["objective"]))
    closets = lambda c: sorted([r["area"] for r in c["rooms"] if r["key"].startswith("cl")])
    assert_true(closets(cs)[-1] < closets(cf)[-1] - 10.0, msg = "%s vs %s" % (closets(cs), closets(cf)))
    depths = [v for v in cs["column_depths"].values()]
    assert_true(max(depths) - min(depths) > 3.0, msg = "the plate steps: %s" % depths)
    assert_eq(validate(short), [])
    assert_true(len(short["floors"][0]["plate"]) > 1)
    return cs["objective"] - cf["objective"]

test("columns may end short instead of filling up with stores", columns_may_end_short)

def mirror_images_are_one_plan_and_knobs_make_options():
    m = single_floor(side_hall(), "M")
    a = fit(m, name = "plan_parti_t_m0")
    b = fit(mirror(m), name = "plan_parti_t_m1")
    assert_close(a["meta"]["plan_parti"]["objective"], b["meta"]["plan_parti"]["objective"], tol = 1e-6)
    cmp = compare([a, b])
    assert_eq(cmp["pairs"][0]["differ"], 0)
    assert_eq(cmp["pairs"][0]["mirrored"], True)
    # a knob is a template edit: the primary bedroom at the street, on the upper floor
    t = side_hall()
    ka = fit(single_floor(t, "U"), name = "plan_parti_t_k0")
    kb = fit(single_floor(knob(t, "primary_at_street"), "U"), name = "plan_parti_t_k1")
    pair = compare([ka, kb])["pairs"][0]
    assert_true("primary" in pair["rooms"] and "bed2" in pair["rooms"], msg = str(pair["rooms"]))
    return pair["differ"]

test("mirror images count as one plan; options come by knob", mirror_images_are_one_plan_and_knobs_make_options)
