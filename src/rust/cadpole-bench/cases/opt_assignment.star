# cadpole: bench case -- an `opt` MILP the size of a floor plan's.
#
# 5,000 binaries and 20,000 rules by default: 100 items into 50 slots, x[i][s]
# = 1 when item i goes in slot s; each item in exactly one slot (100 rows),
# each slot under its capacity (50), and 397 pairs of items that may not share
# a slot, one two-term row per pair per slot (19,850). That is the shape of a
# floor plan's either/or rules -- many, sparse, mostly two terms -- at the size
# a whole house reaches. `solve=False` measures the script building the
# problem alone, which is what has to stay cheap; with it on, the solve too.
#
#   cadpole eval src/rust/cadpole-bench/cases/opt_assignment.star -p solve=False
#   cadpole eval src/rust/cadpole-bench/cases/opt_assignment.star
#
# Everything is drawn from a fixed linear congruential sequence, so every run
# builds the same problem.

items = param("items", 100, min=2, max=400, doc="How many items to place")
slots = param("slots", 50, min=2, max=200, doc="How many slots there are")
pairs = param("pairs", 397, min=0, max=20000, doc="Pairs of items that may not share a slot: one rule per pair per slot")
solve = param("solve", True, doc="Solve it (False measures building it only)")
seconds = param("time_limit", 120.0, min=1, max=3600, doc="Seconds the solver may take")

state = [20261024]

def draw(n):
    """The next number in 0..n-1 of a fixed sequence (an LCG mod 2^31)."""
    state[0] = (state[0] * 1103515245 + 12345) % 2147483648
    return (state[0] // 65536) % n

p = opt.problem(name="assignment")
x = [[p.binary("x[%d][%d]" % (i, s)) for s in range(slots)] for i in range(items)]
for i in range(items):
    p.constrain(opt.sum(x[i]), equals=1, name="item %d placed" % i)

weight = [1 + draw(10) for _ in range(items)]
total = 0
for w in weight:
    total += w
capacity = 1.5 * total / slots
for s in range(slots):
    p.constrain(opt.dot(weight, [x[i][s] for i in range(items)]), max=capacity, name="slot %d capacity" % s)

apart = {}
for _ in range(pairs * 20):
    if len(apart) == pairs:
        break
    i = draw(items)
    j = draw(items)
    if i == j:
        continue
    key = (min(i, j), max(i, j))
    apart[key] = True
for i, j in apart.keys():
    for s in range(slots):
        p.constrain(x[i][s] + x[j][s], max=1, name="%d and %d apart in %d" % (i, j, s))

value = [[draw(100) for _ in range(slots)] for _ in range(items)]
p.maximize(opt.sum([opt.dot(value[i], x[i]) for i in range(items)]))

if solve:
    sol = p.solve(time_limit=seconds)
    print("status %s, objective %s, %d nodes, gap %s" % (sol.status, sol.objective, sol.nodes, sol.gap))

# Something to tessellate and render, so the harness has a body to measure.
box(10, 10, 10, name="marker")
