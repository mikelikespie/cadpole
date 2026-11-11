# The tests of the `opt` namespace as a script writes it: an LP and
# what its duals say, a MILP and its distinct solutions, an infeasible problem
# and its conflict, and @cadpole/building.star's floor-area cap as a rule. A
# document, not a library: `cadpole test stdlib/opt_test.star` runs it, and
# `//src/rust/cadpole-script:cadpole_script_test` (`every_stdlib_test_passes`) runs
# it in CI against the CLI's solver. In the browser's engine every test here
# stops at solve(), which needs native Cadpole.
#
# Numbers are plain: these problems are in feet and square feet, and turn
# millimetres from the library into them on the way in.

load("@cadpole/building.star", "floor_area_cap", "ft")

units(units.ft_in)

SQ_FT = ft(1) * ft(1)

def an_lp_and_its_duals():
    # A two-variable LP whose corner is known by hand: (1.6, 1.2), 2.8.
    p = opt.problem(name="corner")
    x = p.var("x", min=0)
    y = p.var("y", min=0)
    p.constrain(x + 2 * y, max=4, name="a")
    p.constrain(3 * x + y, max=6, name="b")
    p.maximize(x + y)
    sol = p.solve()
    assert_eq(sol.status, "optimal")
    assert_close(sol.objective, 2.8, tol=1e-9)
    assert_close(sol.value(x), 1.6, tol=1e-9)
    assert_close(sol.value(y), 1.2, tol=1e-9)
    assert_eq(sol.binding(), ["a", "b"])
    # A dual is how fast the objective grows as the rule's limit does.
    assert_close(sol.dual("a"), 0.4, tol=1e-9)
    assert_close(sol.dual("b"), 0.2, tol=1e-9)
    return sol.objective

test("an LP finds its corner, and its duals price its rules", an_lp_and_its_duals)

def the_floor_area_cap_as_a_rule():
    # How deep can a 24 ft plate be when two floors of it must fit the cap of
    # a 25 x 80 ft lot? An LP: the cap binds, and its dual says one more
    # square foot of cap buys 1/48 ft of depth.
    cap = floor_area_cap(ft(25), ft(80), front_setback=ft(15), rear_yard=ft(25), floors=2)["cap"] / SQ_FT
    assert_close(cap, 1350.0, tol=1e-6)
    p = opt.problem(name="plate")
    depth = p.var("depth", min=10, max=36)
    p.constrain(2 * 24 * depth, max=cap, name="floor-area cap")
    p.maximize(depth)
    sol = p.solve()
    assert_close(sol.value(depth), 1350.0 / 48, tol=1e-9)
    assert_eq(sol.binding(), ["floor-area cap"])
    assert_close(sol.dual("floor-area cap"), 1.0 / 48, tol=1e-12)
    assert_eq(sol.slack("floor-area cap"), 0.0)
    return sol.value(depth)

test("the floor-area cap is a rule a plate's depth runs into", the_floor_area_cap_as_a_rule)

def a_milp_and_its_next_best():
    # Four items in a sack of seven: the best two, then the next best sets.
    p = opt.problem(name="sack")
    items = [("a", 10, 3), ("b", 13, 4), ("c", 7, 2), ("d", 8, 3)]
    take = [p.binary(n) for n, _, _ in items]
    p.constrain(opt.dot([w for _, _, w in items], take), max=7, name="weight")
    p.maximize(opt.dot([v for _, v, _ in items], take))
    best = p.solve(gap=0)
    assert_eq([best.value(t) for t in take], [1, 1, 0, 0])
    assert_eq(best.objective, 23.0)
    assert_eq(best.dual("weight"), None)
    runners = p.solve(count=4, gap=0)
    assert_eq([s.objective for s in runners], [23.0, 21.0, 20.0, 18.0])
    seen = [str([s.value(t) for t in take]) for s in runners]
    assert_eq(len(seen), len({k: 1 for k in seen}))
    return len(runners)

test("a MILP gives whole numbers, and count= the next best that differ", a_milp_and_its_next_best)

def an_infeasible_plan_names_what_conflicts():
    # A 12 ft room on a 10 ft lot, beside a rule that is fine: the conflict
    # is the two that cannot both hold, not the third.
    p = opt.problem(name="tight")
    w = p.var("w", min=0)
    h = p.var("h", min=0, max=30)
    p.constrain(w, min=12, name="room width")
    p.constrain(w, max=10, name="lot width")
    p.constrain(h, max=20, name="height")
    sol = p.solve()
    assert_eq(sol.status, "infeasible")
    assert_eq(sol.ok, False)
    assert_eq(sol.conflict(), ["room width", "lot width"])
    return len(sol.conflict())

test("an infeasible problem names the rules that conflict", an_infeasible_plan_names_what_conflicts)

def a_bound_is_an_expression():
    # a <= b + 1 and a + b == 2b + 1: comparisons cannot build rules in
    # Starlark, so a bound may be an expression instead.
    p = opt.problem(name="bounds")
    a = p.var("a", min=0, max=5)
    b = p.var("b", min=0, max=3)
    p.constrain(a, max=b + 1, name="a within b")
    p.constrain(a + b, equals=2 * b + 1, name="tie")
    p.maximize(a + b)
    sol = p.solve()
    assert_eq(sol.value(a), 4.0)
    assert_eq(sol.value(b), 3.0)
    assert_eq(sol.value(a - b + 10), 11.0)
    return sol.objective

test("a bound may be an expression", a_bound_is_an_expression)
