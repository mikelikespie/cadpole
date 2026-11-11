# The tests of @cadpole/plan_kitchen.star: one kitchen run against NKBA's landings. `cadpole test
# stdlib/plan_kitchen_test.star`. The appliances are SIZES (a 30-in range, a 24-in sink base, a 24-in fridge, an 18-in
# dishwasher); the runs are synthetic.

load("@cadpole/plan_kitchen.star", "kitchen_run", "check_run", "NKBA", "SIZES", "kitchen_probe")

def the_shortest_compliant_run():
    # 78 in of appliances and 78 in of landings: 24 + 18 at the sink, 12 + 15 at the range, 15 at the fridge, and the
    # two shared counters each the longer plus 12 -- 13'-0" whatever the order
    r = kitchen_run(name = "t_kitchen_shortest")
    assert_true(r["ok"])
    assert_close(r["length"], 156.0, tol = 1e-6)
    assert_eq(len(r["order"]), 3)
    assert_eq(kitchen_probe(), 78.0)
    return r["length"]

test("the shortest run that meets every NKBA landing is 13'-0\"", the_shortest_compliant_run)

def a_short_run_misses_by_the_least():
    # at 10'-6" (126 in) the best any order and any counters do is miss by 30 in in all
    best = kitchen_run(length = 126.0 / 12.0, name = "t_kitchen_126")
    assert_close(best["shortfall"], 30.0, tol = 1e-6)
    # a run drawn 12 | range | 18 | sink | 18 | fridge misses by the same 30 in: already the best
    drawn = check_run(["range", "sink", "fridge"], [12.0, 18.0, 18.0, 0.0], name = "t_kitchen_drawn")
    assert_close(drawn["shortfall"], 30.0, tol = 1e-6)
    assert_close(drawn["length"], 126.0, tol = 1e-6)
    # the sink first misses by 33
    other = check_run(["sink", "range", "fridge"], [24.0, 12.0, 12.0, 0.0], name = "t_kitchen_other")
    assert_close(other["shortfall"], 33.0, tol = 1e-6)
    # and a long enough run misses by nothing
    ok = kitchen_run(length = 160.0 / 12.0, name = "t_kitchen_160")
    assert_close(ok["shortfall"], 0.0, tol = 1e-6)
    return best["shortfall"]

test("a short run: the least total shortfall, and drawn runs checked against it", a_short_run_misses_by_the_least)
