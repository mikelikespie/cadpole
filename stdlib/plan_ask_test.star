# The tests of @cadpole/plan_ask.star: a drawn plan pinned, checked, costed, edited and priced, on a synthetic
# floor with round-number geometry (the 30 x 30 ft floor of plan_solve_test.star, four rooms).
#
#   cadpole test stdlib/plan_ask_test.star

load("@cadpole/plan.star", plan_door = "door", plan_room = "room", "plan", "validate")
load("@cadpole/plan_ask.star", "add_room", "apply", "check", "cost", "describe", "drop_family", "duals", "edit", "explain", "move_zone",
     "pin", "set_floor", "set_room")
load("@cadpole/plan_solve.star", "build", "floor", "rect", "room", "problem", "rule", "run", "solve", "wall")

def floor_a():
    return floor("A", zones = {"A": rect(0.5, 30.5, 0.5, 30.5)}, obstacles = {"stair": rect(0.5, 10.5, 0.5, 7.0)},
                 fixed = {"lobby": rect(10.5, 14.5, 0.5, 7.0)}, roots = ["lobby"],
                 egress = [wall("W", 0.5, (10.0, 30.5)), wall("N", 30.5, (0.5, 30.5))])

def rooms_a():
    return [
        room("hall", floor = "A", kind = "hall", value = 0.2, w = (3.0, 30.0), d = (3.0, 30.0), opens_to = ["lobby"]),
        room("bed0", floor = "A", kind = "bed", area = (100.0, 150.0), w = (9.5, 15.0), d = (9.5, 15.0), aspect = 1.4, opens_to = ["hall", "lobby"]),
        room("bath1", floor = "A", kind = "bath", area = (40.0, 60.0), w = (5.0, 9.0), d = (5.0, 9.0), orient = (5.0, 7.5), wet = False,
             opens_to = ["hall", "lobby"]),
        room("bed2", floor = "A", kind = "bed", area = (100.0, 150.0), w = (9.5, 15.0), d = (9.5, 15.0), aspect = 1.4, opens_to = ["hall", "lobby"]),
    ]

def problem_a(name):
    return problem(name, floors = [floor_a()], rooms = rooms_a(), sym = [("A", "bed0", "bed2")])

def drawn(hall_w):
    """A drawn plan: a hall along the west bedrooms' doors, the bath off its east side."""
    hx1 = 11.9 + hall_w
    rs = [plan_room("hall", floor = "A", kind = "hall", rect = (11.9, hx1, 7.4, 30.5)),
          plan_room("bed0", floor = "A", kind = "bed", rect = (0.5, 11.5, 19.0, 30.5)),
          plan_room("bed2", floor = "A", kind = "bed", rect = (0.5, 11.5, 7.4, 18.6)),
          plan_room("bath1", floor = "A", kind = "bath", rect = (hx1 + 0.4, hx1 + 7.9, 7.4, 14.4))]
    ds = [plan_door("d1", a = "hall", b = "lobby", wall = "S", floor = "A", at = (13.0, 7.4)),
          plan_door("d2", a = "bed0", b = "hall", wall = "E", floor = "A", at = (11.5, 25.0)),
          plan_door("d3", a = "bed2", b = "hall", wall = "E", floor = "A", at = (11.5, 12.0)),
          plan_door("d4", a = "bath1", b = "hall", wall = "W", floor = "A", at = (hx1 + 0.4, 10.0))]
    return plan("drawn", rooms = rs, doors = ds)

def ask(pl):
    return pl["meta"]["plan_ask"]

def rect_of(pl, key):
    for r in pl["rooms"]:
        if r["key"] == key:
            return r["rects"][0]
    fail("no room " + key)

def w(q):
    return q[1] - q[0]

def a_drawing_checked_under_the_rules():
    c = check(problem_a("chk"), drawn(3.5))
    assert_eq(c["meta"]["plan_solve"]["status"], "optimal")
    assert_close(ask(c)["drawn_score"], 313.12, tol = 1e-9)  # 0.2 x 80.85 + 126.5 + 123.2 + 0.9 x 52.5
    assert_eq(ask(c)["broken"], [])
    assert_eq(ask(c)["changed"], [])  # held at its sizes
    assert_eq(len(c["doors"]), 4)
    assert_eq(validate(c), [])
    return ask(c)["drawn_score"]

test("check: a drawing under the rules, held at its sizes", a_drawing_checked_under_the_rules)

def check_names_the_rule_a_drawing_breaks():
    c = check(problem_a("chk_narrow"), drawn(2.5))
    assert_eq(ask(c)["broken"], ["R311:A:hall:width >= 3'-0\" (R311.6)"])
    return len(ask(c)["broken"])

test("check names the rule a drawing breaks: a 2'-6\" hall against R311.6", check_names_the_rule_a_drawing_breaks)

def drawn_on_the_lobby():
    """The drawing with bed 2 flush on the lobby (no wall) and the hall drawn over the lobby (a second rect, the
    lobby's own): the lobby is drawn to its inside faces, so bed 2 breaks its wall, where the hall is open to it."""
    rs = [plan_room("hall", floor = "A", kind = "hall", rects = [(11.9, 15.4, 7.0, 30.5), (10.5, 14.5, 0.5, 7.0)]),
          plan_room("bed0", floor = "A", kind = "bed", rect = (0.5, 11.5, 19.0, 30.5)),
          plan_room("bed2", floor = "A", kind = "bed", rect = (0.5, 11.5, 7.0, 18.6)),
          plan_room("bath1", floor = "A", kind = "bath", rect = (15.8, 23.3, 7.4, 14.4))]
    ds = [plan_door("d1", a = "hall", b = "lobby", wall = "S", floor = "A", at = (13.0, 7.0)),
          plan_door("d2", a = "bed0", b = "hall", wall = "E", floor = "A", at = (11.5, 25.0)),
          plan_door("d3", a = "bed2", b = "hall", wall = "E", floor = "A", at = (11.5, 12.0)),
          plan_door("d4", a = "bath1", b = "hall", wall = "W", floor = "A", at = (15.8, 10.0))]
    return plan("drawn_on_the_lobby", rooms = rs, doors = ds)

def a_drawing_keeps_its_walls_off_the_lobby():
    D = drawn_on_the_lobby()
    c = check(problem_a("chk_lobby"), D)
    assert_eq(c["meta"]["plan_solve"]["status"], "optimal")
    assert_eq(ask(c)["broken"], ["pair:A:bed2|@lobby:apart (overlap or closer than 0'-5\")"])
    assert_eq("hall on A is drawn over the floor's lobby: open to it, no wall between" in ask(c)["notes"], True)
    # pinned and sized: bed 2 keeps its partition off the lobby; the hall, open to it, need not move off it
    p = pin(problem_a("pin_lobby"), D)
    assert_eq(p["meta"]["plan_solve"]["status"], "optimal")
    assert_eq("bed2 and lobby on A are closer than their wall (0'-5\") or overlap" in ask(p)["notes"], True)
    b2 = rect_of(p, "bed2")
    lob = (10.5, 14.5, 0.5, 7.0)
    assert_eq(b2[1] + 0.4 <= lob[0] + 1e-6 or b2[2] >= lob[3] + 0.4 - 1e-6, True)
    assert_eq("bed2" in ask(p)["changed"], True)
    return len(ask(c)["broken"])

test("a drawing keeps its walls off the lobby: a room drawn on its face breaks one, a hall drawn over it is open to it", a_drawing_keeps_its_walls_off_the_lobby)

def pin_sizes_a_drawing_in_its_arrangement():
    p = pin(problem_a("pin_narrow"), drawn(2.5))
    assert_eq(p["meta"]["plan_solve"]["status"], "optimal")
    h = rect_of(p, "hall")
    b0 = rect_of(p, "bed0")
    b2 = rect_of(p, "bed2")
    ba = rect_of(p, "bath1")
    assert_eq(w(h) >= 3.0 - 1e-6, True)
    # the arrangement held: both bedrooms west of the hall, the bath east of it, bed 2 south of bed 0
    assert_eq(b0[1] + 0.4 <= h[0] + 1e-6 and b2[1] + 0.4 <= h[0] + 1e-6, True)
    assert_eq(h[1] + 0.4 <= ba[0] + 1e-6, True)
    assert_eq(b2[3] + 0.4 <= b0[2] + 1e-6, True)
    assert_eq(ask(p)["score"] >= ask(p)["drawn_score"], True)
    assert_eq("hall" in ask(p)["changed"], True)
    return ask(p)["score"]

test("pin sizes a drawing within every rule and keeps its arrangement", pin_sizes_a_drawing_in_its_arrangement)

def cost_answers_in_score_and_sq_ft():
    k = cost(problem_a("cost_bed"), set_room("A", "bed0", area = (100.0, 120.0)), gap = 1e-6)
    assert_eq(k["score"] < -1.0, True)
    assert_eq(k["sq_ft"] < 0.0, True)
    assert_eq("bed0" in k["moved"], True)
    assert_eq(k["answer"].startswith(format(k["score"], "+.1f") + " of score"), True)
    # the same change on a plan whose bedroom is already smaller: nothing to pay
    z = cost(problem_a("cost_none"), set_room("A", "bed0", area = (100.0, 150.0)), gap = 1e-6)
    assert_close(z["score"], 0.0, tol = 1e-6)
    return k["score"]

test("cost: paired solves, in score and sq ft, with the rooms that move", cost_answers_in_score_and_sq_ft)

def edit_moves_the_fewest_feet_of_wall():
    D = drawn(3.5)
    e = edit(problem_a("edit_bath"), D, set_room("A", "bath1", area = (58.0, 60.0)))
    a = ask(e)
    assert_eq(a["tried"], ["pinned: optimal"])
    assert_eq(a["changed"], ["bath1"])  # only the bath moves
    ba = rect_of(e, "bath1")
    assert_eq(w(ba) * (ba[3] - ba[2]) >= 58.0 - 1e-6, True)
    assert_eq(a["walls_moved_ft"] < 1.0, True)  # 52.5 -> 58 sq ft by under a foot of its faces
    # the best plan with the change, freely: far more of the plan moves
    f = edit(problem_a("edit_bath_best"), D, set_room("A", "bath1", area = (58.0, 60.0)), minimal = False, arrangement = "free")
    assert_eq(ask(f)["walls_moved_ft"] > a["walls_moved_ft"] + 5.0, True)
    return a["walls_moved_ft"]

test("edit: the change with the fewest feet of wall moved", edit_moves_the_fewest_feet_of_wall)

def duals_on_a_pinned_plan_are_local_and_checked():
    d = duals(problem_a("duals"), drawn(3.5), resolve = 1, arrangement = "pinned")
    assert_eq(d["local"], True)  # the pinned problem had integers (the widths' bits): its LP duals are local
    assert_eq(d["lp_status"], "optimal")
    top = d["rows"][0]
    assert_eq(top["rule"], "zone:A:bed0:in A (N)")  # one more foot of the floor's north wall: bed 0 grows
    assert_close(top["dual"], top["resolved"], tol = 0.05)  # here the arrangement does not change, so they agree
    assert_eq(top["started"] > 0, True)  # the re-solve began at the base's point (the rule relaxed, it still holds)
    return top["dual"]

test("duals: each binding rule's worth on a pinned plan, local, and its re-solve", duals_on_a_pinned_plan_are_local_and_checked)

def apply_changes_a_copy():
    P = problem_a("apply")
    Q = apply(P, [set_room("A", "bath1", area = (50.0, 60.0)), move_zone("A", "A", "N", -2.0), drop_family("R310:A")])
    assert_eq([r["area"] for r in P["rooms"] if r["key"] == "bath1"], [(40.0, 60.0)])
    assert_eq([r["area"] for r in Q["rooms"] if r["key"] == "bath1"], [(50.0, 60.0)])
    assert_eq(Q["floors"][0]["zones"]["A"], (0.5, 30.5, 0.5, 28.5))
    assert_eq([e["at"] for e in Q["floors"][0]["egress"]], [0.5, 28.5])  # the north escape wall came with the face
    assert_eq(Q["floors"][0]["egress"][0]["span"], (10.0, 28.5))  # the west one now stops at it
    assert_eq(Q["drop"], ["R310:A"])
    assert_eq(describe(set_room("A", "bath1", area = (50.0, 60.0))), "bath1 on A: area (50.0, 60.0)")
    # a floor's fixed rooms given as rects, named as floor() names them (the model reads each as a dict)
    L = apply(P, set_floor("A", fixed = {"lobby": rect(10.5, 14.5, 0.5, 7.0)}))
    assert_eq(L["floors"][0]["fixed"], P["floors"][0]["fixed"])
    return len(Q["rooms"])

test("apply changes a copy: a room's area, a moved face with its escape walls, a family left out", apply_changes_a_copy)

# ----------------------------------------------------------------------------------------------------- explain

def three_bedrooms_two_windows():
    """A 30 x 24 ft floor whose escape walls -- the west wall north of v 8 (16'-6") and the north wall west of u 9
    (8'-6") -- hold two escape windows, not three."""
    F = floor("K", zones = {"K": rect(0.5, 30.5, 0.5, 24.5)}, obstacles = {"stair": rect(20.5, 30.5, 0.5, 7.0)},
              fixed = {"lobby": rect(16.5, 20.5, 0.5, 7.0)}, roots = ["lobby"],
              egress = [wall("W", 0.5, (8.0, 24.5)), wall("N", 24.5, (0.5, 9.0))])
    def bed(k):
        return room(k, floor = "K", kind = "bed", area = (100.0, 150.0), w = (9.5, 14.0), d = (9.5, 14.0), aspect = 1.4, opens_to = ["hall"])
    R = [room("hall", floor = "K", kind = "hall", value = 0.2, w = (3.0, 30.0), d = (3.0, 24.0), opens_to = ["lobby"]), bed("kid1"), bed("kid2"),
         room("bath", floor = "K", kind = "bath", area = (40.0, 60.0), w = (5.0, 9.0), d = (5.0, 9.0), orient = (5.0, 7.5), opens_to = ["hall"])]
    return problem("kids", floors = [F], rooms = R, sym = [("K", "kid1", "kid2")]), bed("kid3")

def explain_names_the_families_and_rooms_that_clash():
    P, kid3 = three_bedrooms_two_windows()
    assert_eq(ps_status(solve(P, name = "kids_two")), "optimal")  # two bedrooms fit
    e = explain(apply(P, add_room(kid3)), name = "kids_three")
    assert_eq(e["feasible"], False)
    assert_eq(e["complete"], True)
    # the floor's zones and the walls between rooms held throughout (the background): what clashes is the escape
    # windows and the bedrooms' sizes, for all three
    assert_eq(sorted(e["families"]), ["R310:K", "size:K"])
    assert_eq(e["rooms"]["R310:K"], ["kid1", "kid2", "kid3"])
    assert_eq(e["rooms"]["size:K"], ["kid1", "kid2", "kid3"])
    assert_eq("pair:K" in e["background"], True)
    assert_eq(e["answer"].startswith("these cannot all hold: an escape window for every sleeping room (R310) on K"), True)
    # with the walls among the candidates too, they are named
    w = explain(apply(P, add_room(kid3)), background = ["zone", "sym", "value", "annex", "def"], name = "kids_three_walls")
    assert_eq(sorted(w["families"]), ["R310:K", "pair:K", "size:K"])
    assert_eq(w["rooms"]["pair:K"], ["kid1|kid2", "kid1|kid3", "kid2|kid3"])
    return e["probes"]

def ps_status(pl):
    return pl["meta"]["plan_solve"]["status"]

test("explain: escape windows, sizes and walls clash, for the three bedrooms", explain_names_the_families_and_rooms_that_clash)

def explain_by_the_builders_conflict():
    P, kid3 = three_bedrooms_two_windows()
    # the background written untagged: one group, tried last, so it is held while the families are filtered and
    # its rows come back as held; a stop at max_probes is a count, and repeats
    e = explain(apply(P, add_room(kid3)), method = "conflict", max_probes = 20, room_probes = 10, name = "kids_conflict")
    assert_eq(e["feasible"], False)
    assert_eq("R310:K" in e["families"], True)
    assert_eq(e["rooms"]["R310:K"], ["kid1", "kid2", "kid3"])
    assert_eq("pair:K" in e["families"], False)
    assert_eq(len(e["held"]) > 0, True)
    assert_eq("R310:K:kid3:escape window (R310)" in e["rows"], True)
    nothing = explain(P, method = "conflict", name = "kids_fine")
    assert_eq(nothing["feasible"], True)
    return len(e["rows"])

test("explain by the builder's conflict(): the background held, the escape windows named; nothing to explain when it solves", explain_by_the_builders_conflict)

def a_study_its_faces_fix():
    """One room held by its faces at u 6.5..20.5: 14 ft wide exactly, the top of its 12-14 ft limit. A width grid
    from 0 to the zone's 20 ft (k x 20/127 ft) has no point at 14 ft; the problem's own (12 + k x 2/15 ft) does."""
    F = floor("W", zones = {"W": rect(0.5, 20.5, 0.5, 12.5)})
    R = [room("study", floor = "W", kind = "study", w = (12.0, 14.0), d = (8.0, 12.0))]
    return problem("faces", floors = [F], rooms = R,
                   rules = [rule("W", "study", "x0", equals = 6.5), rule("W", "study", "x1", equals = 20.5)])

def explain_finds_nothing_to_explain_where_solve_finds_a_plan():
    P = a_study_its_faces_fix()
    pl = solve(P, name = "faces_solve")
    assert_eq(ps_status(pl), "optimal")
    q = pl["meta"]["plan_solve"]["layout"]["W"]["study"]
    assert_close(q[1] - q[0], 14.0, tol = 1e-6)
    # explain asks the problem solve() solves first: nothing to explain
    e = explain(P, name = "faces_why")
    assert_eq(e["feasible"], True)
    # every model of the problem puts the width on the problem's own lattice: the limits as rules (bounds="loose"),
    # and with the room's size rules left out -- a true relaxation, as explain's filters need
    for b in ("tight", "loose"):
        assert_eq(run(build(P, objective = "none", bounds = b, name = "faces_" + b)).status, "optimal")
    assert_eq(run(build(P, objective = "none", drop = ["size"], name = "faces_nosize")).status, "optimal")
    return q[1] - q[0]

test("explain finds nothing to explain where solve() finds a plan: one width lattice in every model", explain_finds_nothing_to_explain_where_solve_finds_a_plan)
