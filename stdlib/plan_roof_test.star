# The tests of @cadpole/plan_roof.star: PV rows on a roof plane, around a skylight and inside the fire code's
# pathway and setbacks. `cadpole test stdlib/plan_roof_test.star`. Synthetic roofs: a 24 x 16-ft plane at a 3-in-4 pitch
# (cos 0.8 exactly, so 20 ft up the slope), 400-W modules 5'-8" x 3'-9" (5.65 x 3.72 ft), 1.5 ft off every edge -- a
# clear 21 x 17 ft -- so every count can be checked by hand.

load("@cadpole/plan_roof.star", "pv_rows", "fire_setbacks", "LOW_SLOPE", "roof_probe")

_ROOF = {"rect": (0.0, 24.0, 0.0, 16.0), "slope": 0.75, "up": "N"}
_SKY = [{"rect": (2.0, 7.0, 6.0, 9.2), "clear": 0.5}]

def rows_by_hand():
    # landscape: 3 across (21 / 5.65), 4 rows up (17 / 3.72) = 12; portrait: 5 across (21 / 3.72), 3 rows (17 / 5.65) = 15
    lnd = pv_rows(_ROOF, orient = ("L",), max_rows = 5, name = "t_roof_l")
    por = pv_rows(_ROOF, orient = ("P",), max_rows = 5, name = "t_roof_p")
    free = pv_rows(_ROOF, max_rows = 5, name = "t_roof_free")
    assert_eq(lnd["count"], 12)
    assert_eq(por["count"], 15)
    assert_eq(free["count"], 15)
    for m in free["modules"]:
        assert_true(m[0] >= 1.5 - 1e-6 and m[1] <= 22.5 + 1e-6, msg = str(m))
        assert_true(m[2] >= 1.5 * 0.8 - 1e-6 and m[3] <= 16.0 - 1.5 * 0.8 + 1e-6, msg = str(m))  # 1.5 ft up the slope is 1.2 ft in plan
    assert_eq(roof_probe()["ridge"], 3.0)
    return free["count"]

test("rows of modules on a sloped plane: 12 landscape, 15 portrait, as counted by hand", rows_by_hand)

def round_a_skylight():
    # the skylight and its 6 in take u 1.5..7.5 out of the rows that cross it: 2 landscape (15 / 5.65) or 4 portrait
    # (15 / 3.72) there
    lnd = pv_rows(_ROOF, orient = ("L",), obstacles = _SKY, max_rows = 5, name = "t_roof_sky_l")
    free = pv_rows(_ROOF, obstacles = _SKY, max_rows = 5, name = "t_roof_sky_free")
    assert_eq(lnd["count"], 10)
    assert_eq(free["count"], 13)
    s = _SKY[0]["rect"]
    for m in free["modules"]:
        assert_true(m[1] <= s[0] - 0.5 + 1e-6 or m[0] >= s[1] + 0.5 - 1e-6 or m[3] <= s[2] - 0.5 + 1e-6 or m[2] >= s[3] + 0.5 - 1e-6, msg = str(m))
    return free["count"]

test("modules keep 6 in off a skylight, and the rows that cross it lose its width", round_a_skylight)

def the_fire_codes_rules():
    assert_eq(fire_setbacks(_ROOF, 0.30, False)["ridge"], 1.5)
    assert_eq(fire_setbacks(_ROOF, 0.50, False)["ridge"], 3.0)
    assert_eq(fire_setbacks(_ROOF, 0.50, True)["ridge"], 1.5)   # sprinklered: 18 in up to 66 percent
    assert_eq(fire_setbacks(_ROOF, 0.70, True)["ridge"], 3.0)
    low = dict(_ROOF, slope = LOW_SLOPE)
    assert_eq(fire_setbacks(low, 0.5, False)["applies"], False)  # 2 in 12 or less: R324.6 does not apply
    # a 36-in pathway, eave to ridge, may not cross the skylight: it goes east, and the rows keep 3 portrait where they
    # cross the skylight (u 7.5 to the pathway) and 5 elsewhere: 11; 36 in at the ridge drops a row's worth more
    p18 = pv_rows(_ROOF, obstacles = _SKY, pathway = True, ridge = 1.5, uniform = True, max_rows = 4, name = "t_roof_path18")
    p36 = pv_rows(_ROOF, obstacles = _SKY, pathway = True, ridge = 3.0, uniform = True, max_rows = 4, name = "t_roof_path36")
    assert_eq(p18["count"], 11)
    assert_eq(p36["count"], 10)
    x0, x1 = p18["pathway"]
    assert_true(x0 >= 7.0 - 1e-6, msg = "the pathway crosses the skylight: %s" % (p18["pathway"],))
    for m in p18["modules"]:
        assert_true(m[1] <= x0 + 1e-6 or m[0] >= x1 - 1e-6, msg = "a module in the pathway: %s" % (m,))
    # with no skylight a pathway along an edge costs a landscape array nothing (19.5 ft still takes 3 across)
    pl = pv_rows(_ROOF, pathway = True, ridge = 1.5, orient = ("L",), max_rows = 4, name = "t_roof_path_l")
    assert_eq(pl["count"], 12)
    return p18["count"]

test("the fire code's pathway and ridge setbacks, and the low-slope exemption", the_fire_codes_rules)
