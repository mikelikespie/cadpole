# The tests of @cadpole/plan_light.star: lit depth, the street window's sky, reference court measurements
# (a middle court's dining 0.039, its bedroom a floor up 0.182, a side
# hall's dining 0.009, against a street window's 0.456), the sky table and its lookup, and the light rows on a small
# synthetic plan. `cadpole test stdlib/plan_light_test.star`. Every plan and lot here is made up.

load("@cadpole/plan.star", "court", "door", "floor", "plan", "room", "site", "window")
load(
    "@cadpole/plan_light.star",
    "SKY_TABLE",
    "sky_table_text",
    "court_sky",
    "glass_doors",
    "light_rows",
    "lit_depth",
    "lit_pieces",
    "lit_share",
    "sky_row",
    "sky_table",
    "street_sky",
    "window_sky",
)

def _f3(x):
    # rounded to a thousandth, halves up
    return int(x * 1000 + 0.5) / 1000.0

def lit_depths():
    assert_eq(lit_depth(8.0), 16.0)
    assert_eq(lit_depth(7.0), 14.0)
    # a 10 x 24 ft room, its 4-ft street window centred in the south wall, head 8 ft: lit to 16 ft from the glass
    p = plan("lit", site = site((0.0, 30.0, 0.0, 60.0), street = "S"), floors = [floor("F1", z = 0.0)],
             rooms = [room("r", "Room", floor = "F1", kind = "living", rect = (0.0, 10.0, 0.0, 24.0))],
             windows = [window("w", floor = "F1", room = "r", wall = "S", at = (5.0, 0.0), width = 4.0, sill = 2.0, head = 8.0, source = "street")])
    s = lit_share(p, "F1")
    assert_close(s["total"], 240.0, tol = 1e-9)
    # on the 1-ft grid every cell centre within 16 ft of the glass segment: rows v 0.5 .. 15.5 all lit (16 rows of 10)
    assert_close(s["lit"], 160.0, tol = 1e-9)
    # a level-wide measure, 15 ft from any glass on the floor
    assert_close(lit_share(p, "F1", depth = 15.0)["lit"], 150.0, tol = 1e-9)
    # a sliding glass door onto a deck lights like a window (8-ft head: 16 ft): the far 8 ft of the room
    q = plan("lit2", site = site((0.0, 30.0, 0.0, 60.0), street = "S"), floors = [floor("F1", z = 0.0)],
             rooms = [room("r", "Room", floor = "F1", kind = "living", rect = (0.0, 10.0, 0.0, 24.0)), room("dk", "Deck", floor = "F1", kind = "deck", rect = (0.0, 10.0, 24.4, 34.0))],
             doors = [door("ds", floor = "F1", a = "r", b = "dk", wall = "N", at = (5.0, 24.0), width = 6.0, swing = "slide")])
    assert_close(lit_share(q, "F1")["lit"], 160.0, tol = 1e-9)
    assert_eq([g["source"] for g in glass_doors(q)], ["deck"])
    return s["share"]

test("Lit depth: twice the head height, sampled on a 1-ft grid", lit_depths)

def the_street_reference():
    # the reference: a main-floor street window 74 ft from a 32-ft row, eye 2 ft and 9 ft inside
    fr = street_sky(depths = (2.0, 9.0))
    assert_eq(_f3(fr[2.0]), 0.456)
    assert_eq(_f3(fr[9.0]), 0.099)
    return fr[2.0]

test("The street window's sky: 0.456 two feet in, 0.099 at nine", the_street_reference)

def two_narrow_courts():
    ref = street_sky(depths = (2.0,), name = "t_ref")[2.0]
    # a middle court: 8'-2" clear to the wall across it, 18'-6" long, 12'-11" of building before it
    # and 12 ft after it, the walls to 32 ft; the dining on the main floor at 10 ft, its bedroom 3 a floor up
    dining = court_sky(8.2, 32.0, floor_z = 10.0, length = 18.5, along = 9.25, front = 12.9, rear = 12.0, back = 16.8,
                       opp = (-12.9, 52.1), name = "t_court_dining")[2.0]
    bed3 = court_sky(8.2, 32.0, floor_z = 20.0, length = 18.5, along = 6.6, front = 12.9, rear = 12.0, back = 16.8,
                     opp = (-12.9, 52.1), name = "t_court_bed3")[2.0]
    # a side hall: a rear-arm court 6'-4" clear, the dining's 5'-5" opening at its front end, open to the yard behind
    side = court_sky(6.3, 32.0, floor_z = 10.0, span = (0.0, 5.4), length = 22.2, along = 2.5, front = 21.8, rear = 0.0,
                     back = 18.7, opp = (-21.8, 43.2), name = "t_side_dining")[2.0]
    assert_eq(_f3(dining), 0.039)
    assert_eq(_f3(bed3), 0.182)
    assert_eq(_f3(side), 0.009)
    # 9 %, 2 % and 40 % of a street window's sky: 0.039 / 0.456 is 8.55 %, the unrounded ratio 8.46 %
    assert_close(dining / ref, 0.0846, tol = 0.0005)
    assert_close(side / ref, 0.0193, tol = 0.0005)
    assert_close(bed3 / ref, 0.3988, tol = 0.0005)
    return dining / ref

test("Two narrow courts: 9% and 2% of a street window's sky, 40% a floor up", two_narrow_courts)

def a_small_sky_table():
    T = sky_table(widths = (6.0, 12.0), heights = (20.0,), floors = (0, 1), name = "t_table")
    assert_eq(len(T), 4)
    by = {(r["w"], r["floor"]): r for r in T}
    # a wider court and a higher floor both see more sky, and light deeper
    assert_true(by[(12.0, 0)]["sky"] > by[(6.0, 0)]["sky"], msg = str(T))
    assert_true(by[(6.0, 1)]["sky"] > by[(6.0, 0)]["sky"], msg = str(T))
    assert_true(by[(12.0, 0)]["lit"] >= by[(6.0, 0)]["lit"], msg = str(T))
    for r in T:
        assert_true(r["rel"] > 0.0 and r["rel"] < 1.5 and r["lit"] >= 0.0 and r["lit"] <= 20.0, msg = str(r))
    # the lookup between the table's widths is a straight line, clamped at its ends
    mid = sky_row(9.0, 20.0, 0, T)
    assert_close(mid["sky"], 0.5 * (by[(6.0, 0)]["sky"] + by[(12.0, 0)]["sky"]), tol = 1e-12)
    assert_close(sky_row(30.0, 20.0, 0, T)["sky"], by[(12.0, 0)]["sky"], tol = 1e-12)
    assert_eq(sky_row(9.0, 20.0, 3, T), None)
    assert_eq([w for w, _ in lit_pieces(20.0, 0, T)], [6.0, 12.0])
    return len(T)

test("sky_table: court width x wall height x floor, and sky_row between its rows", a_small_sky_table)

def the_committed_table():
    # SKY_TABLE is sky_table()'s output with its defaults; regenerate it after any change to the geometry
    assert_true(len(SKY_TABLE) >= 60, msg = "%d rows" % len(SKY_TABLE))
    r = SKY_TABLE[0]
    fresh = sky_table(widths = (r["w"],), heights = (r["h"],), floors = (r["floor"],), name = "t_fresh")[0]
    assert_close(fresh["sky"], r["sky"], tol = 1e-8)  # the table keeps 9 decimals of sky and 6 of lit depth
    assert_close(fresh["lit"], r["lit"], tol = 1e-5)

test("SKY_TABLE matches a fresh run of its generator", the_committed_table)

def _court_house(court_w):
    """Two floors of a 25-ft-wide house; a court court_w wide on its east side from the first floor up, a living room
    on the court and a dining room on the street."""
    cw = court_w
    return plan("courts", site = site((0.0, 25.0, 0.0, 80.0), street = "S",
                                      neighbors = [{"rect": (25.0, 60.0, 5.0, 70.0), "height": 22.0}, {"rect": (-35.0, 0.0, 5.0, 70.0), "height": 22.0},
                                                   {"rect": (-100.0, 125.0, -120.0, -69.0), "height": 32.0}]),
                floors = [floor("F1", z = 0.0, ceiling = 9.0), floor("F2", z = 10.0, ceiling = 9.0)],
                rooms = [
                    room("dining", "Dining", floor = "F1", kind = "dining", rect = (0.5, 24.5, 5.5, 18.0)),
                    room("living", "Living", floor = "F1", kind = "living", rect = (0.5, 24.5 - cw - 0.4, 18.4, 38.0)),
                    room("bed", "Bedroom", floor = "F2", kind = "bed", rect = (0.5, 24.5 - cw - 0.4, 18.4, 38.0)),
                    room("front", "Front room", floor = "F2", kind = "study", rect = (0.5, 24.5, 5.5, 18.0)),
                ],
                courts = [court("c", kind = "court", rect = (24.5 - cw, 24.5, 18.4, 38.0), base = "F1", top = 22.0)],
                windows = [
                    window("w_din", floor = "F1", room = "dining", wall = "S", at = (12.5, 5.5), width = 8.0, sill = 2.0, head = 8.0, source = "street"),
                    window("w_liv", floor = "F1", room = "living", wall = "E", at = (24.5 - cw - 0.4, 28.0), width = 6.0, sill = 2.0, head = 8.0, source = "court:c"),
                    window("w_bed", floor = "F2", room = "bed", wall = "E", at = (24.5 - cw - 0.4, 28.0), width = 6.0, sill = 2.0, head = 8.0, source = "court:c"),
                ])

def light_rows_on_a_plan():
    rows = light_rows(_court_house(4.0), name = "t_narrow")
    by = {r["key"]: r for r in rows}
    liv = by["light:sky-w_liv:F1:living"]
    bed = by["light:sky-w_bed:F2:bed"]
    for r in (liv, bed):
        assert_true("of a street window's sky" in r["detail"] and "uncalibrated" in r["detail"], msg = r["detail"])
    # a 4-ft court two floors deep: the living room under it is dark; the bedroom a floor up sees more
    assert_eq(liv["ok"], False)
    assert_eq(liv["severity"], "warn")
    s_liv = window_sky(_court_house(4.0), _court_house(4.0)["windows"][1], name = "t_ws1")[2.0]
    s_bed = window_sky(_court_house(4.0), _court_house(4.0)["windows"][2], name = "t_ws2")[2.0]
    assert_true(s_bed > s_liv, msg = "%s %s" % (s_liv, s_bed))
    # a wider court is lighter
    s_wide = window_sky(_court_house(10.0), _court_house(10.0)["windows"][1], name = "t_ws3")[2.0]
    assert_true(s_wide > s_liv, msg = "%s %s" % (s_liv, s_wide))
    # the floors' shares are notes; a room with no glass at all is dark; the living room's mark is its court's sky,
    # not a second one for being dark
    assert_eq(by["light:share:F1:-"]["severity"], "note")
    assert_true("light:dark:F2:front" in by, msg = ", ".join(sorted(by.keys())))
    assert_true("light:dark:F1:living" not in by, msg = ", ".join(sorted(by.keys())))
    # measure=False estimates the same rows from SKY_TABLE
    est = {r["key"]: r for r in light_rows(_court_house(4.0), measure = False)}
    assert_true("estimated from SKY_TABLE" in est["light:sky-w_liv:F1:living"]["detail"], msg = est["light:sky-w_liv:F1:living"]["detail"])
    return s_liv

test("light_rows: court windows measured on the plan's own massing, floors' shares, dark rooms", light_rows_on_a_plan)
