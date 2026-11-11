# The tests of @cadpole/plan_parti.star's explanations, grafts and options: the deletion filter by rule family and
# floor, author() and the precheck, the room stage's fit rows and the measured light in the kernel, and
# options by knob. Synthetic geometry: one floor of a 25-ft benchmark parti at a time, each fit under a second.

load("@cadpole/plan_parti.star", "author", "explain", "fit", "fit_table", "library", "options", "precheck")
load("@cadpole/plan_rules.star", "rule")
load("@cadpole/plan_templates.star", "copy_template", "court", "side_hall", "single_floor", "without_court")

def explain_names_the_rule_family():
    # the middle court's grid gives the garage 15'-8": a hard one-car rule cannot hold, and the filter says so
    cg = single_floor(court(), "G")
    x = explain(cg, rules = [rule("garage_size", hard = True)], name = "plan_parti_t_x",
                families = ["garage_size", "program", "facade", "access", "light"])
    assert_eq(x["feasible"], "infeasible")
    assert_eq(x["families"], ["garage_size"])
    assert_eq(x["groups"], ["garage_size:G"])
    assert_eq(x["undecided"], 0)  # every probe answered: the set is irreducible
    assert_true(x["cheapest"] != None and x["cheapest"]["group"] == "garage_size:G")
    # the solver's own filter (sol.conflict()) agrees: by single rules, and stopped after the families
    r = explain(cg, rules = [rule("garage_size", hard = True)], name = "plan_parti_t_xr", engine = "rust", cheapest = False)
    assert_eq((r["families"], r["groups"], len(r["rules"])), (["garage_size"], ["garage_size:G"], 1))
    r = explain(cg, rules = [rule("garage_size", hard = True)], name = "plan_parti_t_xf", engine = "rust",
                by_floor = False, cheapest = False)
    assert_eq((r["families"], r["groups"]), (["garage_size"], []))
    a = author(cg, rules = [rule("garage_size", hard = True)], name = "plan_parti_t_a")
    assert_eq(a["feasible"], "infeasible")
    assert_eq(a["explain"]["families"], ["garage_size"])
    ok = author(cg, name = "plan_parti_t_a2")
    assert_eq(ok["feasible"], "optimal")
    return x["solves"]

test("explain() names the clashing rule family, floor by floor", explain_names_the_rule_family)

def fit_rows_rule_out_the_slivers():
    # the worst of a set's door classes at each width
    tub = fit_table("tub", ["side", "wide"])
    rows = {w: d for (w, d) in tub["rows"]}
    assert_eq(rows[4.6], None)
    assert_close(rows[5.5], 12.166667, tol = 1e-6)  # the 6-in grid: the 72 x 36 tub along a 5'-6" room
    assert_close(rows[6.2], 10.333334, tol = 1e-6)
    # the side hall's upper floor with fit rows: bedroom 3's 7'-5" span takes no full bed, so the floor cannot hold its
    # program -- and the filter says which rules clash. The 5'-7" column (3, 3) keeps both baths: it reads the tub's
    # 5'-6" row, 12'-2" deep with a side door (9'-3" with the door in the end wall).
    u = single_floor(side_hall(), "U")
    plan = fit(u, name = "plan_parti_t_f0", fit_rows = True)
    c = plan["meta"]["plan_parti"]
    assert_eq(c["status"], "infeasible")
    dropped = {k: v["dropped"] for k, v in c["grafts"]["fit_rows"].items() if v["dropped"]}
    assert_eq(dropped, {"bed3": [(0, 1)]})
    x = explain(u, name = "plan_parti_t_f1", fit_rows = True, cheapest = False,
                families = ["fit", "facade", "program", "access"])
    assert_true("fit" in x["families"], msg = str(x["families"]))
    return len(dropped)

test("the fit rows rule out the spans no furniture fits", fit_rows_rule_out_the_slivers)

def measured_light_rules_out_a_court_lit_room():
    # the side hall's main floor, its dining room lit only by the rear-arm court: measured, the court does not light
    # it deep enough, and the filter names the light rule against the template's front-to-back order
    m = single_floor(side_hall(), "M")
    c = fit(m, name = "plan_parti_t_l0", light = "measured")["meta"]["plan_parti"]
    assert_eq(c["status"], "infeasible")
    x = explain(m, name = "plan_parti_t_l1", light = "measured", cheapest = False,
                families = ["order", "light", "facade", "program"])
    assert_eq(x["families"], ["order", "light"])
    return len(x["groups"])

test("plan-audit's measured light rules out a court-lit room", measured_light_rules_out_a_court_lit_room)

def the_precheck_sees_rooms_walled_off_from_glass():
    # the middle court without its court: the dining room and bedroom 3 sit between rooms the template orders to the
    # street and the garden, so no glass can reach them -- said before any MILP
    rows = [r for r in precheck(without_court(court())) if not r["ok"]]
    assert_eq(sorted([r["key"] for r in rows]), ["light:bed3", "light:dining"])
    # every template, mirrored and on four floors, passes
    for t in library(floors = 4) + library(mirror = True):
        assert_eq([r["key"] for r in precheck(t) if not r["ok"]], [], msg = t["key"])
    return len(rows)

test("the precheck sees rooms walled off from any glass", the_precheck_sees_rooms_walled_off_from_glass)

def explain_and_author_stop_at_the_precheck():
    # the side hall with its room band's partition moved to 17'-7": the garage's column falls under its 10-ft least
    # width, and the mudroom, which opens only off the garage, has nothing to open off: the precheck says so before any
    # MILP is built
    t = copy_template(side_hall())
    t["lines"] = [3.7, 7.6, 17.6]
    x = explain(t, name = "plan_parti_t_xp")
    assert_eq(x["feasible"], "precheck")
    assert_eq([r["key"] for r in x["precheck"]], ["span:garage", "door:mud"])
    assert_eq(author(t, name = "plan_parti_t_ap")["feasible"], "precheck")
    return len(x["precheck"])

test("explain() and author() stop at the precheck, saying why", explain_and_author_stop_at_the_precheck)

def options_fit_a_template_and_its_knobs():
    o = options(single_floor(side_hall(), "U"), knobs = ["primary_at_street"], name = "plan_parti_t_op")
    assert_eq(len(o["fits"]), 2)
    pair = o["compare"]["pairs"][0]
    assert_true("primary" in pair["rooms"], msg = str(pair["rooms"]))
    return pair["differ"]

test("options() fits a template and each of its knobs", options_fit_a_template_and_its_knobs)
