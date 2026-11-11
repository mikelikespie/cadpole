# The tests of @cadpole/plan_furnish.star, the room stage: fit-table entries reproduced, plan rooms
# furnished and re-walked, the rules a layout must keep (door swings, windows, the stack wall), the re-walk's verdicts,
# and fit_row's step function exact at every width. `cadpole test stdlib/plan_furnish_test.star`. Synthetic rooms only:
# @cadpole/plan.star's toy_house() and hand-made room_local() records; the fit entries are a 25-ft benchmark plate.
# The solves here are the cheap ones (each well under a second in an optimised build).

load("@cadpole/plan.star", "toy_house", "room_local")
load("@cadpole/plan_furnish.star", "furnish", "least_depth", "least_depth_record", "holds", "fit_row", "fit_lookup", "walk_check", "furnish_probe")
load("@cadpole/plan_fit_tables.star", "FIT_ENTRIES", "FIT_TABLES")

def _room(w, d, doors = [], windows = [], stacks = [], kind = "bed", key = "r"):
    return {"key": key, "kind": kind, "furnish": None, "origin": (100.0, 50.0), "w": w, "d": d, "rects": [(0.0, w, 0.0, d)],
            "doors": doors, "windows": windows, "fixtures": [], "stacks": stacks, "floor": "L1"}

def _door(wall, off, width = 2.67, swing = "in"):
    return {"key": "d", "wall": wall, "offset": off, "width": width, "swing": swing, "hinge": "lo", "to": "hall"}

def _win(wall, off, width = 3.0, sill = 2.0):
    return {"key": "w", "wall": wall, "offset": off, "width": width, "sill": sill, "head": 7.0, "egress": True, "source": "street"}

def _overlap(a, b):
    return min(a[1], b[1]) - max(a[0], b[0]) > 1e-6 and min(a[3], b[3]) - max(a[2], b[2]) > 1e-6

def _inside(r, box):
    return r[0] >= box[0] - 1e-6 and r[1] <= box[1] + 1e-6 and r[2] >= box[2] - 1e-6 and r[3] <= box[3] + 1e-6

def fit_entries_reproduce():
    # The tables hold one solve per entry; here a handful of the cheap ones, two of them needing a re-walk cut.
    picks = [("dining", "rear:WN", 201.6), ("living", "front:WN", 243.6), ("car", "garage", 122.4), ("powder", "side", 55.2),
             ("entry", "entry:a", 116.4), ("tub", "wide", 55.2), ("shower", "core", 81.6), ("tub", "wide", 78.0)]
    n = 0
    for st, door, w in picks:
        want = [e for e in FIT_ENTRIES if e[0] == st and e[1] == door and e[8] == w][0]
        got = least_depth_record(st, w, door, name = "t_dl_%d" % n)
        if want[9] == None:
            assert_eq(got["D"], None)
            assert_eq(got["status"], "infeasible")
        else:
            assert_close(got["D"], want[9], tol = 1e-4)
        assert_eq(got["cuts"], want[7])
        n += 1
    return n

test("the room stage reproduces the fit tables' least depths, walk cuts included", fit_entries_reproduce)

def the_table_has_its_counts():
    feasible = [e for e in FIT_ENTRIES if e[3] != None]
    assert_eq(len(FIT_ENTRIES), 170)
    assert_eq(len(feasible), 154)
    # a depth in feet never undercuts the solved inches
    for e in feasible:
        assert_true(e[3] * 12.0 >= e[9] - 1e-9, msg = str(e))
    # every table's least depth falls (or holds) as the room widens
    for k, t in FIT_TABLES.items():
        prev = None
        for (w, d) in t["rows"]:
            if d != None and prev != None:
                assert_true(fit_lookup(t, w) <= prev + 1e-9, msg = k)
            if d != None:
                prev = fit_lookup(t, w)
    return len(feasible)

test("the fit tables: 154 of the 170 entries hold their set", the_table_has_its_counts)

def toy_rooms_are_furnished():
    H = toy_house()
    n = 0
    for key in ("bed1", "primary"):
        loc = room_local(H, key)
        r = furnish(loc, None)
        assert_true(r["ok"], msg = "%s: %s" % (key, r["reason"]))
        assert_eq(r["set"], loc["furnish"])
        box = (loc["origin"][0], loc["origin"][0] + loc["w"], loc["origin"][1], loc["origin"][1] + loc["d"])
        solids = [f["rect"] for f in r["furniture"] + r["fixtures"]]
        assert_true(len(solids) >= 3, msg = key)
        for i in range(len(solids)):
            assert_true(_inside(solids[i], box), msg = "%s: %s outside" % (key, solids[i]))
            for j in range(i + 1, len(solids)):
                assert_true(not _overlap(solids[i], solids[j]), msg = "%s: %s and %s overlap" % (key, solids[i], solids[j]))
        for f in r["furniture"]:
            assert_eq(f["room"], key)
            assert_true(f["front"] in ("N", "S", "E", "W"), msg = str(f))
        n += len(solids)
    return n

test("furnish() holds toy_house()'s bedrooms, every piece inside and none overlapping", toy_rooms_are_furnished)

def nothing_stands_in_a_door_swing():
    # a queen room with its door on the east wall: the swing (the leaf, 18 in to step past) stays clear of solids
    loc = _room(11.0, 12.0, [_door("E", 10.0)])
    r = furnish(loc, "queen")
    assert_true(r["ok"], msg = r["reason"])
    op = 2.67 + 2.0 / 12.0
    swing = (100.0 + 11.0 - (2.67 + 1.5), 100.0 + 11.0, 50.0 + 10.0 - op / 2.0, 50.0 + 10.0 + op / 2.0)
    for f in r["furniture"]:
        assert_true(not _overlap(f["rect"], swing), msg = "%s in the swing" % f["kind"])
    return len(r["furniture"])

test("no solid stands in a door's swing", nothing_stands_in_a_door_swing)

def only_low_pieces_under_a_window():
    # a 2-ft sill on the north wall: the bookshelf (7 ft) and the desk (2'-6") stay out of the 2-ft zone in front of it
    loc = _room(10.0, 9.0, [_door("S", 1.6)], [_win("N", 5.0, 4.0, 2.0)], kind = "office", key = "of")
    r = furnish(loc, "office")
    assert_true(r["ok"], msg = r["reason"])
    zone = (100.0 + 3.0, 100.0 + 7.0, 50.0 + 9.0 - 2.0, 50.0 + 9.0)
    for f in r["furniture"]:
        if f["h"] > 2.0:
            assert_true(not _overlap(f["rect"], zone), msg = "%s (%s ft tall) under a 2-ft sill" % (f["kind"], f["h"]))
    return len(r["furniture"])

test("only pieces below the sill stand under a window", only_low_pieces_under_a_window)

def wet_fixtures_go_on_the_stack_wall():
    # toy_house()'s bath has its stack beyond the east wall; a 48-in shower, a WC and a vanity do not all fit on that
    # 7'-6" wall, so the room stage says so and gives the layout with the fewest wet walls
    loc = room_local(toy_house(), "bath")
    r = furnish(loc, "shower")
    assert_true(not r["ok"])
    assert_true("stack wall (E)" in r["reason"], msg = r["reason"])
    assert_true(r["relaxed"]["ok"])
    assert_eq(len(r["relaxed"]["wet_walls"]), 1)
    # on a stack wall long enough, every wet fixture backs onto it
    loc2 = _room(6.0, 10.0, [_door("W", 1.6, 2.33)], [], [(6.3, 5.0)], kind = "bath", key = "b")
    r2 = furnish(loc2, "shower")
    assert_true(r2["ok"], msg = r2["reason"])
    assert_eq(r2["wet_walls"], ["E"])
    for f in r2["fixtures"]:
        assert_eq(f["front"], "W")
    return len(r2["fixtures"])

test("wet fixtures back onto the stack wall, or the reason says how many wet walls they need", wet_fixtures_go_on_the_stack_wall)

def a_room_too_small_says_why():
    r = furnish(_room(9.0, 9.0, [_door("E", 7.5)]), "queen")
    assert_true(not r["ok"])
    assert_true("does not fit 9'-0\" x 9'-0\"" in r["reason"], msg = r["reason"])
    assert_true("at any depth up to 40 ft" in r["reason"], msg = r["reason"])  # 9 ft is too narrow for the bed's 10 ft
    r2 = furnish(_room(5.0, 8.0, [_door("S", 1.5, 2.33)], kind = "bath", key = "b"), "tub", wet = "fewest")
    assert_true(not r2["ok"])
    assert_true("needs 11'-4\" deep" in r2["reason"], msg = r2["reason"])
    return r2["need_depth"]

test("a room too small says why, and how deep it would have to be", a_room_too_small_says_why)

def an_l_shaped_room_is_furnished_whole():
    # toy_house()'s kitchen is two rects: the stage packs it in their bounding box, the box's other corner kept clear of
    # every piece and use space, and walks it from its doors (an office's three pieces: the rule, not the kitchen, is
    # what is tested)
    loc = room_local(toy_house(), "kitchen")
    assert_eq(len(loc["rects"]), 2)
    r = furnish(loc, "office", wet = "any", name = "t_kitchen_l")
    assert_true(r["ok"], msg = r["reason"])
    ou, ov = loc["origin"]
    for f in r["furniture"] + r["clears"]:
        x0, x1, y0, y1 = f["rect"]
        for (x, y) in ((x0 + 0.01, y0 + 0.01), (x1 - 0.01, y0 + 0.01), (x0 + 0.01, y1 - 0.01), (x1 - 0.01, y1 - 0.01)):
            assert_true(any([rc[0] + ou - 1e-6 <= x and x <= rc[1] + ou + 1e-6 and rc[2] + ov - 1e-6 <= y and y <= rc[3] + ov + 1e-6 for rc in loc["rects"]]),
                        msg = "%s at %s is outside the room" % (f.get("kind", f.get("item")), f["rect"]))
    return len(r["furniture"])

test("an L-shaped room is furnished in its bounding box, nothing outside its own rects", an_l_shaped_room_is_furnished_whole)

def an_unknown_set_is_a_finding_not_an_error():
    r = furnish(_room(10.0, 10.0, [_door("S", 2.0)], kind = "office", key = "of2"), "drafting table")
    assert_true(not r["ok"])
    assert_true("no furniture set named" in r["reason"], msg = r["reason"])
    d = furnish(_room(10.0, 10.0, [_door("S", 2.0)], kind = "office", key = "of3"), "desk")
    assert_true(d["ok"], msg = d["reason"])
    return len(d["furniture"])

test("a set the catalog does not have is a finding, not an error; a plan's \"desk\" is a set", an_unknown_set_is_a_finding_not_an_error)

def the_rewalk_finds_a_pinched_point():
    # a 5 x 5-ft room cut across by a solid: from the south door the far side cannot be reached
    lay = {"W": 60.0, "D": 60.0, "rects": [{"kind": "solid", "x0": 0.0, "y0": 20.0, "x1": 60.0, "y1": 30.0}]}
    assert_eq(walk_check(lay, [(30.0, 1.0)], {"far": (30.0, 50.0), "near": (30.0, 5.0)}), ["far"])
    # a 2-ft gap at its east end: a person gets by (the room's own walls are not grown); a 10-in gap: pinched
    lay["rects"] = [{"kind": "solid", "x0": 0.0, "y0": 20.0, "x1": 36.0, "y1": 30.0}]
    assert_eq(walk_check(lay, [(30.0, 1.0)], {"far": (30.0, 50.0)}), [])
    lay["rects"] = [{"kind": "solid", "x0": 0.0, "y0": 20.0, "x1": 50.0, "y1": 30.0}]
    assert_eq(walk_check(lay, [(30.0, 1.0)], {"far": (30.0, 50.0)}), ["far"])
    assert_eq(furnish_probe(), [])
    return True

test("the re-walk finds a use point a person cannot reach", the_rewalk_finds_a_pinched_point)

_T = {"set": "test", "door": "x", "rows": [(8.0, None), (9.0, 12.0), (10.0, 11.0), (11.0, 11.0), (12.0, 9.5)]}

def fit_row_is_the_step_function():
    # minimise the depth at each width: the rows give exactly the table's least depth there, and refuse a room
    # narrower than the narrowest width that holds the set
    n = 0
    for w in (8.5, 8.99, 9.0, 9.5, 10.0, 10.99, 11.0, 11.5, 12.0, 15.0):
        p = opt.problem(name = "t_fit_row_%d" % n)
        wv = p.var("w", min = w, max = w)
        d = p.var("d", min = 0, max = 40)
        fit_row(p, wv, d, _T, name = "fit:L1:r:depth")
        p.minimize(d)
        sol = p.solve()
        want = fit_lookup(_T, w)
        if want == None:
            assert_eq(sol.status, "infeasible")
        else:
            assert_close(sol.value(d), want, tol = 1e-9)
        n += 1
    # the one-hot form: one row
    p = opt.problem(name = "t_fit_row_onehot")
    z = [p.binary("z%d" % k) for k in range(3)]
    p.constrain(opt.sum(z), equals = 1, name = "one width")
    d = p.var("d", min = 0, max = 40)
    rows = fit_row(p, [(z[0], 8.5), (z[1], 10.5), (z[2], 12.0)], d, _T, name = "fit:L1:r:depth")
    assert_eq(len(rows), 2)  # the 8'-6" width holds nothing: its binary is held at 0, and the one depth row
    p.minimize(d + 0.01 * z[2])
    sol = p.solve()
    assert_eq(sol.value(z[0]), 0)
    assert_close(sol.value(d), 9.5, tol = 1e-9)
    return n

test("fit_row is the table's step function, exactly, in both forms", fit_row_is_the_step_function)

def deeper_rooms_still_hold_their_sets():
    # the stretch test, sampled: 0, 6 in, 1, 2, 4 and 8 ft deeper
    n = 0
    for st, door, w in (("powder", "side", 55.2), ("tub", "wide", 78.0), ("dining", "rear:WN", 201.6)):
        e = [x for x in FIT_ENTRIES if x[0] == st and x[1] == door and x[8] == w][0]
        for extra in (0.0, 0.5, 1.0, 2.0, 4.0, 8.0):
            r = holds(st, 0, 0, door, width_in = w, depth_in = e[9] + 12.0 * extra, name = "t_stretch_%d" % n)
            assert_true(r["ok"], msg = "%s %s %s +%s ft: %s %s" % (st, door, w, extra, r["status"], r["unreachable"]))
            n += 1
    return n

test("a deeper room still holds its set (the stretch test, sampled)", deeper_rooms_still_hold_their_sets)
