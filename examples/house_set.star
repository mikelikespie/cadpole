# A small house on a slope, drawn as a four-sheet concept set with one
# export: a cover with the project data and a table of the house at
# three front setbacks, a site plan with the lot, 1 ft contours, the
# setback and a north arrow, the two floor plans dimensioned in feet and
# inches, and a section down the slope with the grade and the 30 ft height
# line, beside the street elevation with its hidden lines removed.
#
#   cadpole export examples/house_set.star --run-exports --out-dir /tmp   # /tmp/house_set.pdf, four ARCH D pages
#   cadpole op Drawing --doc examples/house_set.star --view sheet:A2.0 --format svg > /tmp/plans.json   # one sheet, previewed
#   cadpole test examples/house_set.star
#
# Everything on the sheets is computed here: the views are the bodies cut
# and projected with their hidden lines removed, the dimensions are the
# model's lengths written in feet and inches, and the cover's table is the
# document evaluated at three setbacks by the host before it writes the
# pages -- `report()` names a value, `sweep_table()` asks for it at other
# parameter values. The lot, the survey and the house are fictional.
#
# Frame: x across the lot along the street, y from the front property line
# toward the rear (the lot falls that way, and that way is north), z up
# from the sidewalk at the front line. The model is in millimetres; `ft()`
# turns feet into them.

load("@cadpole/site.star", "ft", "to_ft")
load("@cadpole/plan.star", "ftin")

# Feet and inches wherever the document shows a length -- the inspector,
# the viewer, and every sheet of the set, which takes the document's units
# when it names none.
units(units.ft_in)

# ---- The design, in feet.
front_setback = param("front_setback", 15.0, unit="ft", min=5, max=30, step=1, doc="The house's front wall behind the front property line")
width = param("width", 30.0, unit="ft", min=20, max=40, step=1, doc="The house across the lot")
depth = param("depth", 32.0, unit="ft", min=24, max=44, step=1, doc="The house down the lot")
level_height = param("level_height", 10.0, unit="ft", min=8, max=12, doc="Floor to floor; the lower level walks out at the rear")

LOT_W = 50.0
LOT_D = 100.0
SIDE = (LOT_W - width) / 2.0
HEIGHT_LIMIT = 30.0
WALL = 8.0 / 12.0
PITCH = 6.0 / 12.0
EAVE = 1.5

# ---- The survey: shots on a 10 ft grid, the ground falling toward the
# rear at about a fifth, steepening, and rising a little to the east.
def ground_z(x, y):
    return -0.16 * y - 0.0009 * y * y + 0.04 * x + 0.8 * math.sin(x * 9.0) * math.cos(y * 5.0)

SHOTS = [(ft(x), ft(y), ft(ground_z(x, y))) for y in range(-10, 111, 10) for x in range(-10, 61, 10)]
grade = terrain(SHOTS, method=terrain.tin, extent=((ft(-10), ft(-10)), (ft(60), ft(110))), name="grade")
contours = grade.contour_sketch(ft(1), name="contours")
index_contours = grade.contour_sketch(ft(5), name="index_contours")

# ---- The house: a walk-out level and a street level, each a ring of
# 8 in walls on a slab with 4 in partitions inside, a straight stair
# between them, glass in every window, and a gable roof -- a 10 in slab
# over gable-end walls and a ceiling -- whose ridge runs across the lot.
# The street level's floor is half a foot above the sidewalk and the lower
# level's one level below it.
X0 = SIDE
X1 = SIDE + width
Y0 = front_setback
Y1 = front_setback + depth
Z_UP = 0.5
Z_LOW = Z_UP - level_height
CX = (X0 + X1) / 2.0
CY = (Y0 + Y1) / 2.0

def ring(z0, h, name):
    s = sketch(XY.offset(ft(z0)), name=name + "_plan").rect(ft(width), ft(depth), center=(ft(CX), ft(CY)))
    s = s.rect(ft(width - 2 * WALL), ft(depth - 2 * WALL), center=(ft(CX), ft(CY)))
    return extrude(s, ft(h), name=name + "_ring")

def opening(x, y, w, d, z0, h):
    """A box through a wall: `w` along x, `d` along y, feet."""
    return box(ft(w), ft(d), ft(h), at=(ft(x), ft(y), ft(z0)))

front = Y0 - 0.5
rear = Y1 - WALL - 0.5
XI0, XI1, YI0, YI1 = X0 + WALL, X1 - WALL, Y0 + WALL, Y1 - WALL  # inside faces
P = 4.0 / 12.0  # a partition

# Openings in the outside walls: (x, y, along x, along y, sill above the floor, height, glazed).
LOWER_OPENINGS = [
    (CX - 6.0, rear, 12.0, WALL + 1.0, 0.0, 7.0, True),          # the walk-out's sliding door
    (X0 - 0.5, CY + 2.0, WALL + 1.0, 4.0, 3.0, 4.0, True),        # a west window in the family room
]
UPPER_OPENINGS = [
    (CX - 1.5, front, 3.0, WALL + 1.0, 0.0, 7.0, False),          # the front door
    (X0 + 3.0, front, 5.0, WALL + 1.0, 3.0, 4.0, True),
    (X1 - 8.0, front, 5.0, WALL + 1.0, 3.0, 4.0, True),
    (X0 + 3.0, rear, 8.0, WALL + 1.0, 2.0, 5.0, True),
    (X1 - 11.0, rear, 8.0, WALL + 1.0, 2.0, 5.0, True),
]
lower = subtract(
    ring(Z_LOW, level_height - 1.0, "lower"),
    *[opening(x, y, w, d, Z_LOW + z, h) for x, y, w, d, z, h, _ in LOWER_OPENINGS],
    **{"name": "lower_walls"}
)
upper = subtract(
    ring(Z_UP, level_height - 1.0, "upper"),
    *[opening(x, y, w, d, Z_UP + z, h) for x, y, w, d, z, h, _ in UPPER_OPENINGS],
    **{"name": "upper_walls"}
)

# Glass: a 1 in pane in the middle of each glazed opening's wall.
panes = []
for z0, ops in [(Z_LOW, LOWER_OPENINGS), (Z_UP, UPPER_OPENINGS)]:
    for i, (x, y, w, d, z, h, glazed) in enumerate(ops):
        if not glazed:
            continue
        if d > w:  # in an east or west wall
            g = box(ft(1.0 / 12), ft(d - 1.0), ft(h), at=(ft(x + 0.5 + WALL / 2 - 1.0 / 24), ft(y + 0.5), ft(z0 + z)), name="glass_%d_%d" % (int(z0), i))
        else:
            g = box(ft(w), ft(1.0 / 12), ft(h), at=(ft(x), ft(y + 0.5 + WALL / 2 - 1.0 / 24), ft(z0 + z)), name="glass_%d_%d" % (int(z0), i))
        g.material(color=(0.62, 0.74, 0.8), roughness=0.1, opacity=0.45)
        panes.append(g)

# Partitions: (level floor, x0, y0, x1, y1, door gaps along the wall as (start, end) in feet).
def partition(z0, r, gaps, name):
    x0_, y0_, x1_, y1_ = r
    b = box(ft(x1_ - x0_), ft(y1_ - y0_), ft(level_height - 1.0), at=(ft(x0_), ft(y0_), ft(z0)), name=name + "_mass")
    holes = []
    for a0, a1 in gaps:
        if x1_ - x0_ > y1_ - y0_:
            holes.append(box(ft(a1 - a0), ft(y1_ - y0_) + 2, ft(7.0), at=(ft(a0), ft(y0_) - 1, ft(z0) - 1)))
        else:
            holes.append(box(ft(x1_ - x0_) + 2, ft(a1 - a0), ft(7.0), at=(ft(x0_) - 1, ft(a0), ft(z0) - 1)))
    return subtract(b, *holes, **{"name": name}) if holes else b

# A straight stair along the east wall: 15 risers between the floors, 10 in
# treads, rising toward the street; the top lands at the street level's
# floor 3 ft inside the front wall, through a hole in that floor.
RISERS = 15
TREAD = 10.0 / 12.0
STAIR_W = 3.5
SX0, SX1 = XI1 - STAIR_W, XI1
SY0 = YI0 + 3.0                     # the top riser
SY1 = SY0 + (RISERS - 1) * TREAD    # the foot of the stair
steps = []
for i in range(RISERS - 1):
    top = Z_LOW + (i + 1) * level_height / RISERS
    ya = SY1 - (i + 1) * TREAD
    steps.append(box(ft(STAIR_W), ft(TREAD) + (1 if i < RISERS - 2 else 0), ft(top - Z_LOW), at=(ft(SX0), ft(ya), ft(Z_LOW)), name="tread%d" % i))
stair = union(*steps, **{"name": "stair", "kernel": kernel.mesh})

PARTS = [
    # lower level: a storage room and a bath in front, under the street; the family room behind, at the walk-out
    partition(Z_LOW, (XI0, CY - 2.0, SX0, CY - 2.0 + P), [(XI0 + 4.0, XI0 + 7.0), (XI0 + 16.0, XI0 + 18.67)], "lower_cross"),
    partition(Z_LOW, (XI0 + 13.0, YI0, XI0 + 13.0 + P, CY - 2.0), [], "lower_split"),
    partition(Z_LOW, (SX0 - P, YI0, SX0, CY - 2.0), [], "stair_wall"),
    # street level: the living room in front; the kitchen and a bedroom behind
    partition(Z_UP, (XI0, CY + 2.0, XI1, CY + 2.0 + P), [(XI0 + 4.0, XI0 + 10.0), (XI1 - 7.0, XI1 - 4.0)], "upper_cross"),
    partition(Z_UP, (CX + 1.0, CY + 2.0 + P, CX + 1.0 + P, YI1), [], "upper_split"),
]
ROOMS = [  # (level floor, name, centre x, centre y)
    (Z_LOW, "STORAGE", XI0 + 6.5, (YI0 + CY - 2.0) / 2.0), (Z_LOW, "BATH", (XI0 + 13.0 + SX0) / 2.0, (YI0 + CY - 2.0) / 2.0),
    (Z_LOW, "FAMILY", CX, (CY + YI1) / 2.0),
    (Z_UP, "LIVING", (XI0 + SX0) / 2.0, (YI0 + CY + 2.0) / 2.0), (Z_UP, "KITCHEN", (XI0 + CX + 1.0) / 2.0, (CY + 2.0 + YI1) / 2.0),
    (Z_UP, "BEDROOM", (CX + 1.0 + XI1) / 2.0, (CY + 2.0 + YI1) / 2.0),
]
# Doors that swing, for the plans: (level floor, hinge x, hinge y, leaf ft, angle of the leaf closed (degrees), +1 or -1 the way it opens)
SWINGS = [
    (Z_UP, CX - 1.5, YI0, 3.0, 0.0, 1),
    (Z_LOW, XI0 + 4.0, CY - 2.0, 3.0, 0.0, -1),
    (Z_LOW, XI0 + 16.0, CY - 2.0, 2.67, 0.0, -1),
    (Z_UP, XI1 - 7.0, CY + 2.0 + P, 3.0, 0.0, 1),
]

lower_slab = box(ft(width), ft(depth), ft(0.5), at=(ft(X0), ft(Y0), ft(Z_LOW - 0.5)), name="lower_slab")
upper_floor = subtract(box(ft(width), ft(depth), ft(1.0), at=(ft(X0), ft(Y0), ft(Z_UP - 1.0)), name="upper_floor_slab"),
                       box(ft(STAIR_W), ft(SY1 - SY0), ft(1.0) + 2, at=(ft(SX0), ft(SY0), ft(Z_UP - 1.0) - 1), name="stair_hole"), name="upper_floor")
Z_EAVE = Z_UP + level_height - 1.0
# The roof: a 10 in slab sloping both ways, its underside meeting the
# outside face of the front and rear walls at the eave, overhanging them by
# EAVE, with a square-cut fascia; gable-end walls close the attic.
T_ROOF = 10.0 / 12.0
Z_TIP = Z_EAVE + T_ROOF - EAVE * PITCH       # the top of the fascia
RIDGE = Z_EAVE + T_ROOF + depth / 2.0 * PITCH
gable = sketch(plane(origin=(ft(X0 - EAVE), 0, 0), normal=(1, 0, 0), x_dir=(0, 1, 0)), name="gable").polygon([
    (ft(Y0 - EAVE), ft(Z_TIP - T_ROOF)),
    (ft(CY), ft(RIDGE - T_ROOF)),
    (ft(Y1 + EAVE), ft(Z_TIP - T_ROOF)),
    (ft(Y1 + EAVE), ft(Z_TIP)),
    (ft(CY), ft(RIDGE)),
    (ft(Y0 - EAVE), ft(Z_TIP)),
])
roof = extrude(gable, ft(width + 2 * EAVE), name="roof")
gables = []
for gx, nm in [(X0, "west"), (X1 - WALL, "east")]:
    tri = sketch(plane(origin=(ft(gx), 0, 0), normal=(1, 0, 0), x_dir=(0, 1, 0)), name="gable_%s_profile" % nm).polygon([
        (ft(Y0), ft(Z_EAVE)), (ft(Y1), ft(Z_EAVE)), (ft(CY), ft(Z_EAVE + depth / 2.0 * PITCH)),
    ])
    gables.append(extrude(tri, ft(WALL), name="gable_" + nm))
ceiling = box(ft(width - 2 * WALL), ft(depth - 2 * WALL), ft(0.5), at=(ft(XI0), ft(YI0), ft(Z_EAVE - 0.5)), name="ceiling")
HOUSE = [lower_slab, lower, upper_floor, upper, roof, ceiling, stair] + gables + PARTS + panes
for b in [lower, upper] + gables + PARTS:
    b.material(color=(0.93, 0.92, 0.88), roughness=0.8)
for b in [lower_slab, upper_floor, ceiling]:
    b.material(color=(0.78, 0.75, 0.7), roughness=0.85)
stair.material(color=(0.66, 0.53, 0.38), roughness=0.8)
roof.material(color=(0.22, 0.23, 0.26), roughness=0.7)

def signed(z):
    """A level in feet and inches with its sign: +18'-3\"."""
    return ("+" if z >= 0 else "") + ftin(z)

# ---- What the cover reports, here and at the other setbacks.
corners = [(X0, Y0), (X1, Y0), (X1, Y1), (X0, Y1)]
lowest = min([to_ft(grade.z_at(ft(x), ft(y))) for x, y in corners])
height = RIDGE - lowest
floor_area = 2 * width * depth
report("front_setback", front_setback)
report("floor_area", floor_area)
report("rear_grade", to_ft(grade.z_at(ft(CX), ft(Y1))))
report("height", height)
report("fits", height <= HEIGHT_LIMIT)

# ---- The set: ARCH D, the document's feet and inches, the title column on
# every sheet.
set = sheet_set(
    "house_set",
    size=sheet.arch_d,
    north=-12.0,
    title_block=title_block(
        project="A SMALL HOUSE ON A SLOPE",
        address="LOT 7, EXAMPLE HILL ROAD",
        block_lot="A FICTIONAL SITE",
        stamp=["CONCEPT STUDY", "NOT FOR CONSTRUCTION"],
        notes=["Every number on these sheets is computed from examples/house_set.star; the survey is fictional."],
        issue="CONCEPT",
        date="2026-09-27",
        drawn="CADPOLE",
        checked="-",
    ),
)
x0, y0, x1, y1 = set.area()

# ---- G0.0: the cover.
cover = set.sheet("G0.0", "COVER SHEET")
cover.text("A SMALL HOUSE ON A SLOPE", at=(x0 + 10, y1 - 22), size=9, bold=True)
cover.text("CONCEPT DRAWINGS", at=(x0 + 10, y1 - 34), size=4.5)

cover.view_title("1", "PROJECT DATA", at=(x0 + 10, y1 - 62), scale="", width=230)
data = [
    ["ITEM", "VALUE"],
    ["LOT", "%s FT x %s FT, %s SQ FT" % (format(LOT_W, ".0f"), format(LOT_D, ".0f"), format(LOT_W * LOT_D, ",.0f"))],
    ["HOUSE", "%s FT x %s FT, TWO LEVELS" % (format(width, ".0f"), format(depth, ".0f"))],
    ["FRONT SETBACK", "%s FT" % format(front_setback, ".1f")],
    ["GROSS FLOOR AREA", "%s SQ FT" % format(floor_area, ",.0f")],
    ["LOT COVERAGE", format(width * depth / (LOT_W * LOT_D), ".1%")],
    ["RIDGE ABOVE LOWEST GRADE", "%s (LIMIT %s FT)" % (ftin(height), format(HEIGHT_LIMIT, ".0f"))],
    ["FALL ACROSS THE LOT", ftin(to_ft(grade.z_at(ft(25), 0) - grade.z_at(ft(25), ft(LOT_D))))],
]
bottom = cover.table(data, at=(x0 + 10, y1 - 72), col_widths=[70, 160], size=2.6)

# The same house at three setbacks: each row is the document evaluated at
# that setback by the host, before this evaluation, never inside it.
setbacks = [10.0, 20.0, 30.0]
rows = sweep_table([{"front_setback": s} for s in setbacks], labels=["%s FT" % format(s, ".0f") for s in setbacks])

def cell(row, key, spec):
    v = row["values"].get(key)
    if v == None:
        return "PENDING" if row["pending"] else "FAILED"
    if spec == "yes":
        return "YES" if v else "NO"
    if spec == "ftin":
        return signed(v)
    return format(v, spec)

cover.view_title("2", "THE HOUSE AT THREE FRONT SETBACKS", at=(x0 + 10, bottom - 22), scale="", width=230)
study = [["FRONT SETBACK", "GRADE AT REAR", "RIDGE ABOVE GRADE", "UNDER 30 FT"]]
for r in rows:
    study.append([r["label"], cell(r, "rear_grade", "ftin"), cell(r, "height", "ftin"), cell(r, "fits", "yes")])
bottom = cover.table(study, at=(x0 + 10, bottom - 32), col_widths=[45, 55, 65, 45], size=2.6, align=["left", "right", "right", "center"])
cover.note(
    "Each row is this document evaluated at that front setback: the host runs the three configurations before " +
    "it writes the sheets, and every value is the model's own report(). The further down the slope the house " +
    "sits, the lower the grade at its rear and the taller it stands over it: at 30 ft back it breaks the 30 ft limit.",
    at=(x0 + 10, bottom - 8),
    width=210,
    size=2.2,
)

cover.view_title("3", "GENERAL NOTES", at=(x0 + 10, bottom - 40), scale="", width=230)
cover.note(
    "1. A concept study of massing and height only; not for construction.\n" +
    "2. Grades are from a fictional survey on a 10 ft grid; contours are drawn from its triangulation.\n" +
    "3. Heights are measured from the lowest grade under the house to the ridge.\n" +
    "4. Dimensions are to the outside face of the walls.",
    at=(x0 + 10, bottom - 52),
    size=2.4,
    line_height=4.2,
)
axon = cover.axon_view(HOUSE, az=35, el=28, box=(x0 + 290, y0 + 120, x1 - 20, y1 - 60))
cover.view_title("4", "AXONOMETRIC", at=(x0 + 290, y0 + 100), scale="NOT TO SCALE", width=200)

# ---- A1.0: the site plan, looking down on the roof.
site = set.sheet("A1.0", "SITE PLAN", scale="1/8\" = 1'-0\"")
plan = site.plan_view(HOUSE, cut=ft(40), at=(x0 + 120, y0 + 70), hatch="none", name="site")
# Under the roof's line work: the contours, then the roof's outline in
# white, which knocks them out where the house stands over them.
plan.sketch(contours, weight=0.13, color="grey", under=True)
plan.sketch(index_contours, weight=0.3, under=True)
plan.fill([(ft(X0 - EAVE), ft(Y0 - EAVE)), (ft(X1 + EAVE), ft(Y0 - EAVE)), (ft(X1 + EAVE), ft(Y1 + EAVE)), (ft(X0 - EAVE), ft(Y1 + EAVE))], color="white", under=True)
for c in grade.contours(ft(5)):
    x, y, z = c.end
    plan.text(format(to_ft(z), ".0f"), at=(x, y), size=1.8, align="center", valign="middle")
lot_corners = [(0, 0), (ft(LOT_W), 0), (ft(LOT_W), ft(LOT_D)), (0, ft(LOT_D))]
plan.polyline(lot_corners + [lot_corners[0]], weight=0.5, dash="center")
plan.polyline([(-ft(5), ft(front_setback)), (ft(LOT_W + 5), ft(front_setback))], weight=0.25, dash="dashed")
plan.label("FRONT SETBACK %s" % format(front_setback, ".0f") + "'", at=(ft(LOT_W + 8), ft(front_setback) + ft(4)), to=(ft(LOT_W), ft(front_setback)))
plan.dimension((0, 0), (ft(LOT_W), 0), offset=-12)
plan.dimension((ft(LOT_W), 0), (ft(LOT_W), ft(LOT_D)), offset=-14)
plan.dimension((ft(X0), ft(Y1)), (ft(X1), ft(Y1)), offset=8)
# The walls under the roof, dashed, so the house's own dimension has edges.
plan.polyline([(ft(X0), ft(Y0)), (ft(X1), ft(Y0)), (ft(X1), ft(Y1)), (ft(X0), ft(Y1)), (ft(X0), ft(Y0))], weight=0.25, dash="hidden")
# Spot elevations: a cross at the point, the level beside it.
for x, y in [(0, 0), (LOT_W, 0), (LOT_W, LOT_D), (0, LOT_D), (CX, Y1 + 10)]:
    z = to_ft(grade.z_at(ft(x), ft(y)))
    plan.polyline([(ft(x - 1), ft(y - 1)), (ft(x + 1), ft(y + 1))], weight=0.25)
    plan.polyline([(ft(x - 1), ft(y + 1)), (ft(x + 1), ft(y - 1))], weight=0.25)
    plan.text(signed(z), at=(ft(x + 1.8), ft(y + 1.2)), size=1.8)
plan.text("STREET", at=(ft(LOT_W / 2), -ft(12)), size=3.5, bold=True, align="center")
plan.text("HOUSE, FIN. FLOOR %s" % signed(Z_UP), at=(ft(CX), ft(CY + 6)), size=2.6, bold=True, align="center", valign="middle")
site.view_title("1", "SITE PLAN", at=(x0 + 20, y0 + 25), width=260)
site.north_arrow(at=(x0 + 330, y0 + 380), r=14)
site.scale_bar(at=(x0 + 20, y0 + 10), length=90)
site.legend([("center", "PROPERTY LINE"), ("dashed", "FRONT SETBACK"), ("line", "1 FT CONTOUR"), ("heavy", "5 FT CONTOUR")], at=(x0 + 330, y0 + 340), title="LEGEND")
site.note(
    "SITE NOTES\n" +
    "1. Contours at 1 ft from a fictional survey on a 10 ft grid, every fifth labelled in feet from the sidewalk.\n" +
    "2. Spot elevations (+) at the lot corners and behind the house, from the sidewalk at the front line.\n" +
    "3. The front setback is %s ft; the house is %s ft by %s ft under a %s in 12 gable." % (format(front_setback, ".0f"), format(width, ".0f"), format(depth, ".0f"), format(PITCH * 12, ".0f")),
    at=(x0 + 330, y0 + 250),
    width=220,
    size=2.3,
)

# ---- A2.0: the floor plans, cut 4 ft above each floor.
plans = set.sheet("A2.0", "FLOOR PLANS", scale="1/4\" = 1'-0\"")
LEVELS = [("LOWER LEVEL", Z_LOW, x0 + 60), ("STREET LEVEL", Z_UP, x0 + 380)]
for i, (title, z, px) in enumerate(LEVELS):
    v = plans.plan_view(HOUSE, cut=ft(z + 4.0), at=(px, y0 + 220), origin=(ft(X0), ft(Y0)), hatch="solid", name=title)
    v.dimension((ft(X0), ft(Y0)), (ft(X1), ft(Y0)), offset=-14)
    v.dimension((ft(X1), ft(Y0)), (ft(X1), ft(Y1)), offset=-14)
    v.dimension((ft(X0), ft(Y1)), (ft(CX), ft(Y1)), offset=10)
    for rz, name, rx, ry in ROOMS:
        if rz == z:
            v.text(name, at=(ft(rx), ft(ry)), size=2.6, bold=True, align="center", valign="middle")
    for sz, hx, hy, leaf, a0, turn in SWINGS:
        if sz != z:
            continue
        # The leaf drawn open at 90 degrees and the arc its edge sweeps (math.cos takes degrees).
        a1 = a0 + turn * 90.0
        pts = [(ft(hx + leaf * math.cos(a0 + (a1 - a0) * k / 12.0)), ft(hy + leaf * math.sin(a0 + (a1 - a0) * k / 12.0))) for k in range(13)]
        v.polyline(pts, weight=0.18)
        v.polyline([(ft(hx), ft(hy)), pts[-1]], weight=0.35)
    if z == Z_LOW:
        v.text("UP", at=(ft((SX0 + SX1) / 2.0), ft(SY1 - 1.5)), size=2.0, align="center", valign="middle")
    else:
        v.text("DN", at=(ft((SX0 + SX1) / 2.0), ft(SY0 + 3.0)), size=2.0, align="center", valign="middle")
    plans.view_title(str(i + 1), "%s PLAN, FIN. FLOOR %s" % (title, signed(z)), at=(px, y0 + 170), width=200)
plans.north_arrow(at=(x0 + 690, y0 + 330), r=12)
plans.scale_bar(at=(x0 + 60, y0 + 130), length=60)

# ---- A3.0: a section down the slope through the middle of the house, and
# the street elevation under it.
sections = set.sheet("A3.0", "SECTION AND ELEVATION", scale="3/16\" = 1'-0\"")
cut = sections.section_view(HOUSE, plane="x=%s" % ft(CX), at=(x0 + 90, y0 + 420), hatch="lines", name="section")
BOTTOM = -30.0
front_grade = [(ft(y), grade.z_at(ft(CX), ft(y))) for y in range(-10, int(Y0) + 1)]
rear_grade = [(ft(y), grade.z_at(ft(CX), ft(y))) for y in range(int(Y1) + 1, 101)]
front_grade.append((ft(Y0), grade.z_at(ft(CX), ft(Y0))))
rear_grade.insert(0, (ft(Y1), grade.z_at(ft(CX), ft(Y1))))
# Earth in front of the house, behind it and under its slab; the grade
# line stops at the walls, as it does on a drawn section.
for region in [
    front_grade + [(ft(Y0), ft(BOTTOM)), (ft(-10), ft(BOTTOM))],
    rear_grade + [(ft(100), ft(BOTTOM)), (ft(Y1), ft(BOTTOM))],
    [(ft(Y0), ft(Z_LOW - 0.5)), (ft(Y1), ft(Z_LOW - 0.5)), (ft(Y1), ft(BOTTOM)), (ft(Y0), ft(BOTTOM))],
]:
    cut.hatch(region, pattern="earth", spacing=2.5)
cut.polyline(front_grade, weight=0.7)
cut.polyline(rear_grade, weight=0.7)
limit = [(p[0], p[1] + ft(HEIGHT_LIMIT)) for p in front_grade + rear_grade]
cut.polyline(limit, weight=0.35, dash="dashed")
cut.label("30 FT ABOVE GRADE", at=(ft(80), limit[-12][1] + ft(8)), to=(ft(86), limit[-6][1]))
cut.label("GRADE", at=(ft(96), rear_grade[-6][1] - ft(9)), to=(ft(94), rear_grade[-8][1]))
for name, z in [("RIDGE", RIDGE), ("STREET LEVEL", Z_UP), ("LOWER LEVEL", Z_LOW)]:
    cut.polyline([(ft(Y1 + 3), ft(z)), (ft(Y1 + 10), ft(z))], weight=0.18)
    cut.text("%s %s" % (name, signed(z)), at=(ft(Y1 + 11), ft(z)), size=2.0, valign="middle")
# The height at the rear, clear of the setback; the setback from the
# property line, marked at the sidewalk, to the front wall.
cut.dimension((ft(Y1), ft(Z_LOW)), (ft(Y1), ft(RIDGE)), offset=-6)
cut.dimension((ft(Y0), ft(Z_LOW - 0.5)), (ft(Y1), ft(Z_LOW - 0.5)), offset=-10)
z_pl = grade.z_at(ft(CX), 0)
cut.polyline([(0, z_pl), (0, z_pl + ft(4))], weight=0.35)
cut.text("PL", at=(0, z_pl + ft(5)), size=2.0, align="center")
cut.dimension((0, z_pl + ft(3)), (ft(front_setback), z_pl + ft(3)), offset=4)
sections.view_title("1", "SECTION A-A, LOOKING WEST", at=(x0 + 20, y0 + 255), width=300)
elev = sections.elevation_view(HOUSE, direction="south", at=(x0 + 120, y0 + 70), origin=(ft(X0), ft(Z_LOW)), name="elevation")
# The ground along the front wall, and white under it: the lower level is
# below grade on the street side, so none of it shows.
street_grade = [(ft(x), grade.z_at(ft(x), ft(Y0))) for x in range(int(X0) - 8, int(X1) + 9)]
elev.fill(street_grade + [(street_grade[-1][0], ft(Z_LOW - 2)), (street_grade[0][0], ft(Z_LOW - 2))], color="white")
elev.polyline(street_grade, weight=0.7)
elev.dimension((ft(X0), ft(RIDGE)), (ft(X1), ft(RIDGE)), offset=10)
sections.view_title("2", "STREET ELEVATION", at=(x0 + 20, y0 + 25), width=260)

export(set, "house_set.pdf")

# ---- Tests: `cadpole test examples/house_set.star`.

def the_set_is_four_arch_d_pages():
    assert_eq(set.sheets(), ["G0.0", "A1.0", "A2.0", "A3.0"])
    assert_eq(set.size(), (914.4, 609.6))
    assert_eq(sections.scale(), "3/16\" = 1'-0\"")
    for s in [cover, site, plans, sections]:
        assert_true(s.items() > 100, msg="%s holds %d items" % (s.number(), s.items()))

def the_house_reads_right():
    for b in HOUSE:
        assert_solid(b)
    assert_true(height < HEIGHT_LIMIT, msg="the ridge is %s ft over grade" % format(height, ".1f"))
    assert_close(RIDGE - Z_EAVE, T_ROOF + depth / 2.0 * PITCH, tol=1e-9)
    # The stair climbs exactly one level, and lands in its hole in the floor.
    assert_close(stair.bbox()[1][2], ft(Z_UP - level_height / RISERS), tol=1e-6)
    assert_true(SY1 - SY0 >= (RISERS - 1) * TREAD - 1e-9 and SX0 > CX + 1.0 + P, msg="the stair is clear of the street level's partition")
    # A plan is the model's own frame at the sheet's scale: the house's
    # 30 ft is 7 1/2 in of paper at a quarter inch to the foot.
    assert_true(axon.scale > 0 and plan.scale == 1.0 / 96)
    assert_eq(format(width * 12 * 25.4 / 48, ".1f"), "190.5")

def a_house_further_down_the_hill_stands_taller():
    return height

test("four ARCH D sheets", the_set_is_four_arch_d_pages)
test("the house", the_house_reads_right)
test("height grows down the slope", a_house_further_down_the_hill_stands_taller, sweep={"front_setback": [10, 15, 25]}, monotone=monotone.increasing)
