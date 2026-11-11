# Four rooms laid out by the solver: a living room, a kitchen, a bedroom
# and a bath on a plate 24 ft wide whose depth the lot's floor-area cap
# decides, each at least its minimum size, the living room and the bedroom
# no deeper than 1.6 times their width, no two overlapping, the kitchen
# sharing at least 4 ft of wall with the living room for a doorway. The
# solver picks every size and position; the script only states the rules.
#
#   cadpole eval examples/room_layout_opt.star
#   cadpole test examples/room_layout_opt.star
#   cadpole render examples/room_layout_opt.star --view top -o /tmp/rooms.png
#
# Solving needs native Cadpole (the CLI, `cadpole serve`, `cadpole mcp`); the browser's
# engine builds the problem and stops at solve().
#
# The model is in feet, plain numbers: `opt` knows nothing about units, and a
# MILP's tolerances are absolute, so square feet are the right size of number
# and square millimetres are not. ft() turns the answer into millimetres for
# drawing. The lot is fictional.

load("@cadpole/building.star", "floor_area_cap", "ft", "inch")

units(units.ft_in)

WIDTH = 24.0        # ft, the plate's width
MAX_DEPTH = 36.0    # ft, as deep as the plate may go
FLOORS = 2
M = 60.0            # ft: longer than any distance on the plate, for the either/or rules
DOOR = 4.0          # ft of wall the kitchen and the living room must share

# A 25 x 80 ft lot, 15 ft front setback, 25 ft rear yard: a 1,000 sq ft plate,
# 2,000 over two floors, less the 650 sq ft deduction, is a 1,350 sq ft cap.
SQ_FT = ft(1) * ft(1)
CAP = floor_area_cap(ft(25), ft(80), front_setback=ft(15), rear_yard=ft(25), floors=FLOORS)["cap"] / SQ_FT

p = opt.problem(name="rooms")
depth = p.var("plate depth", min=20, max=MAX_DEPTH)
p.constrain(FLOORS * WIDTH * depth, max=CAP, name="floor-area cap")

# name, minimum width, minimum depth (ft), what a foot of the room is worth
ROOMS = [
    ("living", 13, 14, 4),
    ("kitchen", 9, 10, 1),
    ("bedroom", 10, 11, 2),
    ("bath", 5, 8, 0.5),
]
R = {}
for name, wmin, dmin, _ in ROOMS:
    x = p.var(name + ".x", min=0)
    y = p.var(name + ".y", min=0)
    w = p.var(name + ".w", min=wmin)
    d = p.var(name + ".d", min=dmin)
    p.constrain(x + w, max=WIDTH, name=name + " within the width")
    p.constrain(y + d, max=depth, name=name + " within the depth")
    R[name] = (x, y, w, d)

def apart(a, b):
    """a and b do not overlap: a is west, east, south or north of b."""
    xa, ya, wa, da = R[a]
    xb, yb, wb, db = R[b]
    side = [p.binary("%s %s of %s" % (a, s, b)) for s in ("west", "east", "south", "north")]
    # Each rule holds when its binary is 1 and is slack by M when it is 0.
    p.constrain(xa + wa - xb + M * side[0], max=M, name="%s west of %s" % (a, b))
    p.constrain(xb + wb - xa + M * side[1], max=M, name="%s east of %s" % (a, b))
    p.constrain(ya + da - yb + M * side[2], max=M, name="%s south of %s" % (a, b))
    p.constrain(yb + db - ya + M * side[3], max=M, name="%s north of %s" % (a, b))
    p.constrain(opt.sum(side), min=1, name="%s clear of %s" % (a, b))

apart("living", "kitchen")
apart("living", "bedroom")
apart("kitchen", "bedroom")
apart("bath", "living")
apart("bath", "kitchen")
apart("bath", "bedroom")
for nm in ["living", "bedroom"]:
    p.constrain(R[nm][3] - 1.6 * R[nm][2], max=0, name=nm + " no deeper than 1.6 times its width")

def shares_wall(a, b, length):
    """a touches b on one side, with at least `length` of wall in common."""
    xa, ya, wa, da = R[a]
    xb, yb, wb, db = R[b]
    # For each side a may be on: the gap between the two faces, and the two
    # spans (start, length) that must overlap along that face.
    SIDES = [
        ("west", xb - xa - wa, (ya, da), (yb, db)),
        ("east", xa - xb - wb, (ya, da), (yb, db)),
        ("south", yb - ya - da, (xa, wa), (xb, wb)),
        ("north", ya - yb - db, (xa, wa), (xb, wb)),
    ]
    picks = []
    for side, gap, (sa, la), (sb, lb) in SIDES:
        s = p.binary("%s against %s, %s" % (a, b, side))
        picks.append(s)
        rule = "%s against %s, %s" % (a, b, side)
        # With s = 1 the gap is 0 and the spans overlap by `length`; with
        # s = 0 every one of these is slack by M.
        p.constrain(gap - M * s, min=-M, name=rule + ": no gap")
        p.constrain(gap + M * s, max=M, name=rule + ": no overlap")
        p.constrain(sa - sb - lb + M * s, max=M - length, name=rule + ": shared wall starts")
        p.constrain(sb - sa - la + M * s, max=M - length, name=rule + ": shared wall ends")
    p.constrain(opt.sum(picks), equals=1, name="%s shares a wall with %s" % (a, b))

shares_wall("kitchen", "living", DOOR)

# Bigger rooms are better, the living room most. The hundredths keep the plan
# from being a mirror image of itself: the living room as far west and the
# kitchen as far south as they go, where nothing else decides.
p.maximize(opt.sum([k * (R[n][2] + R[n][3]) for n, _, _, k in ROOMS]) - 0.01 * (R["living"][0] + R["kitchen"][1]))
sol = p.solve()

def feet(v):
    return sol.value(v)

PLAN = {}
for name, _, _, _ in ROOMS:
    x, y, w, d = R[name]
    PLAN[name] = (feet(x), feet(y), feet(w), feet(d))
    print("%s: %s x %s ft at (%s, %s)" % tuple([name] + [format(v, ".2f") for v in (PLAN[name][2], PLAN[name][3], PLAN[name][0], PLAN[name][1])]))
print("plate %s ft deep, %s of the %s sq ft cap left" % (format(feet(depth), ".3f"), format(sol.slack("floor-area cap"), ".1f"), format(CAP, ",.0f")))

# ---- The drawing, as a plan seen from above: the plate as a slab, each
# room a coloured floor with its name and size on it, walls cut at 2 ft so
# the rooms show, a 4 ft opening where the kitchen meets the living room,
# and the second floor the cap counts drawn as the same plate's outline.
COLORS = {"living": "#c9b48f", "kitchen": "#9fb7a0", "bedroom": "#a9b9cf", "bath": "#b9c9cc"}
LABEL = {"living": "Living", "kitchen": "Kitchen", "bedroom": "Bedroom", "bath": "Bath"}
T = inch(4)            # wall thickness, centred on the rooms' edges
WALL_H = ft(2)         # walls cut low, so the rooms show from any side
STOREY = ft(10)        # floor to floor
D = feet(depth)

def ftin(v):
    """Feet and inches to the half inch: 28.125 -> 28'-1½\"."""
    h = int(v * 24 + 0.5)
    whole, inches = h // 24, (h % 24) / 2.0
    if inches == 0:
        return "%d'-0\"" % whole
    if inches == int(inches):
        return "%d'-%d\"" % (whole, int(inches))
    return "%d'-%d½\"" % (whole, int(inches))

plate = box(ft(WIDTH), ft(D), inch(4), at=(0, 0, -inch(4)), name="plate")
plate.material(color="#8d918b", roughness=0.9)

def shared_span(a, b):
    """Where rooms a and b share more than half a foot of wall: ("x", at, lo, hi) for a wall along y at x = at,
    or ("y", at, lo, hi); None if they do not (two rooms that only meet at a corner)."""
    ax, ay, aw, ad = PLAN[a]
    bx, by, bw, bd = PLAN[b]
    for at in (ax, ax + aw):
        if abs(at - bx) < 1e-6 or abs(at - (bx + bw)) < 1e-6:
            lo, hi = max(ay, by), min(ay + ad, by + bd)
            if hi > lo + 0.5:
                return ("x", at, lo, hi)
    for at in (ay, ay + ad):
        if abs(at - by) < 1e-6 or abs(at - (by + bd)) < 1e-6:
            lo, hi = max(ax, bx), min(ax + aw, bx + bw)
            if hi > lo + 0.5:
                return ("y", at, lo, hi)
    return None

DOORWAY = shared_span("kitchen", "living")
mid = (DOORWAY[2] + DOORWAY[3]) / 2.0
if DOORWAY[0] == "x":
    gap = box(T + 2, ft(DOOR), WALL_H + 2, at=(ft(DOORWAY[1]) - T / 2 - 1, ft(mid - DOOR / 2), -1), name="doorway")
else:
    gap = box(ft(DOOR), T + 2, WALL_H + 2, at=(ft(mid - DOOR / 2), ft(DOORWAY[1]) - T / 2 - 1, -1), name="doorway")

ROOM_BODIES = []
rings = []
for name, _, _, _ in ROOMS:
    x, y, w, d = PLAN[name]
    fl = box(ft(w), ft(d), inch(1), at=(ft(x), ft(y), 0), name=name)
    fl.material(color=COLORS[name], roughness=0.85)
    ROOM_BODIES.append(fl)
    rings.append(subtract(box(ft(w) + T, ft(d) + T, WALL_H, at=(ft(x) - T / 2, ft(y) - T / 2, 0), name=name + "_outer"),
                          box(ft(w) - T, ft(d) - T, WALL_H + 2, at=(ft(x) + T / 2, ft(y) + T / 2, -1), name=name + "_inner"), name=name + "_ring"))
    words = text("%s\n%s x %s" % (LABEL[name], ftin(w), ftin(d)), size=ft(min(0.8, 0.85 * w / 9.5)), weight=600, align="center", baseline="middle", line_height=1.3,
                 plane=XY.offset(inch(1)), at=(ft(x + w / 2), ft(y + d / 2)), name=name + "_label")
    for g in extrude(words, inch(0.5), name=name + "_letters"):
        g.material(color="#2f3a37", roughness=0.7)
walls = subtract(union(*rings, **{"name": "wall_rings", "kernel": kernel.mesh}), gap, name="walls", kernel=kernel.mesh)
walls.material(color="#ece8df", roughness=0.85)

# The second floor: the cap is for two, so the same plate again, one storey up, as its outline.
upper = layer("second_floor", doc="The second floor the floor-area cap counts: the same plate, one storey up")
E = inch(2)
frame = [
    box(ft(WIDTH), E, E, at=(0, 0, STOREY), name="upper_s"),
    box(ft(WIDTH), E, E, at=(0, ft(D) - E, STOREY), name="upper_n"),
    box(E, ft(D) - 2 * E, E, at=(0, E, STOREY), name="upper_w"),
    box(E, ft(D) - 2 * E, E, at=(ft(WIDTH) - E, E, STOREY), name="upper_e"),
] + [box(E, E, STOREY, at=(cx, cy, 0), name="post%d" % i) for i, (cx, cy) in enumerate([(0, 0), (ft(WIDTH) - E, 0), (0, ft(D) - E), (ft(WIDTH) - E, ft(D) - E)])]
for b in frame:
    b.material(color="#9fc4d6", roughness=0.5, opacity=0.55)
    upper.add(b)

# ---- Tests: the answer, checked against the rules directly rather than
# against the solver's account of itself. Ties are the one thing a plan can
# change across platforms; these assert what the rules decide.
def overlap(a0, a1, b0, b1):
    return min(a1, b1) - max(a0, b0)

def the_plan_is_optimal_and_the_cap_binds():
    return sol.ok and "floor-area cap" in sol.binding() and abs(feet(depth) - CAP / (FLOORS * WIDTH)) < 1e-6

def every_room_fits_and_none_overlaps():
    names = [n for n, _, _, _ in ROOMS]
    for i, (n, wmin, dmin, _) in enumerate(ROOMS):
        x, y, w, d = PLAN[n]
        if w < wmin - 1e-6 or d < dmin - 1e-6 or x < -1e-6 or y < -1e-6:
            return False
        if x + w > WIDTH + 1e-6 or y + d > feet(depth) + 1e-6:
            return False
        for m in names[i + 1:]:
            u = PLAN[m]
            if overlap(x, x + w, u[0], u[0] + u[2]) > 1e-6 and overlap(y, y + d, u[1], u[1] + u[3]) > 1e-6:
                return False
    return True

def the_kitchen_shares_a_doorway_of_wall_with_the_living_room():
    k = PLAN["kitchen"]
    l = PLAN["living"]
    touch_x = abs(k[0] + k[2] - l[0]) < 1e-6 or abs(l[0] + l[2] - k[0]) < 1e-6
    touch_y = abs(k[1] + k[3] - l[1]) < 1e-6 or abs(l[1] + l[3] - k[1]) < 1e-6
    along_y = overlap(k[1], k[1] + k[3], l[1], l[1] + l[3])
    along_x = overlap(k[0], k[0] + k[2], l[0], l[0] + l[2])
    return (touch_x and along_y >= DOOR - 1e-6) or (touch_y and along_x >= DOOR - 1e-6)

def the_rooms_are_as_big_as_the_rules_allow():
    # Hand-checkable: the kitchen, the bath and the bedroom stand in a row
    # along the front at their minimum widths (9 + 5 + 10 = the plate's 24 ft)
    # and the bedroom's minimum depth, 11 ft; the living room takes the
    # whole width behind them, 24 ft by the 17 1/8 ft left of the plate.
    assert_close(sol.objective, 4 * (24 + 17.125) + (9 + 11) + 2 * (10 + 11) + 0.5 * (5 + 11), tol=1e-6)
    return sol.objective

test("the plan is optimal and the floor-area cap binds", the_plan_is_optimal_and_the_cap_binds)
test("every room fits the plate and none overlaps another", every_room_fits_and_none_overlaps)
test("the kitchen shares a doorway of wall with the living room", the_kitchen_shares_a_doorway_of_wall_with_the_living_room)
test("the rooms are as big as the rules allow", the_rooms_are_as_big_as_the_rules_allow)

def the_model_shows_the_doorway():
    """The opening in the model is in the wall the two rooms share, DOOR long and inside the shared stretch."""
    assert_true(DOORWAY != None, msg="the kitchen and the living room do not touch")
    assert_true(DOORWAY[3] - DOORWAY[2] >= DOOR - 1e-6)
    lo, hi = gap.bbox()
    along = (hi[1] - lo[1]) if DOORWAY[0] == "x" else (hi[0] - lo[0])
    assert_close(along, ft(DOOR), tol=0.01)
    assert_solid(walls)
    return DOORWAY[3] - DOORWAY[2]

test("the model's doorway is in the shared wall", the_model_shows_the_doorway)
