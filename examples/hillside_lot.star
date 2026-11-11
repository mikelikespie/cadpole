# A hillside lot: a 40 x 80 ft parcel on a 25 % slope, modelled from the
# shapes site data comes in -- the parcel's corners as latitude and
# longitude, a surveyed profile down the lot, LiDAR ground samples either
# side, the neighbouring footprints with a ground elevation and a
# height -- with the zoning envelope "30 ft above grade at every point,
# never above 40 ft" as an exact body, the earthwork for the walk-out
# level's pad, a garden studio at the rear, the two neighbours, and a
# two-and-a-half-level house at the front that has to fit the envelope
# (`@cadpole/site.star`). The lot, the ground, the buildings and the
# rules are all invented.
#
# Frame: x along the street frontage, y from the front property line
# toward the rear (the lot falls that way), z up; the sidewalk at the
# front line is z = 0. `local_frame` turns that frame so the lot stands
# upright whatever the street's bearing, and `ft()` turns feet into the
# kernel's millimetres.
#
# What is built, in order:
#
#   * `grade`, the TIN of the shots (`terrain(method=terrain.tin)`) --
#     through every one; the site body, the 1 ft contours for the plan and
#     the earthwork come from it -- and `grade_fit`, the bicubic B-spline
#     through the same shots (`terrain.fit`): smooth, an exact B-rep, the
#     surface the envelope is raised from.
#   * The envelope: the fit raised 30 ft (`Terrain.offset_up`, exact), cut
#     to the plan area in front of the 52.5 ft rear-yard line by knot
#     insertion (`Terrain.clip`; no boolean), capped at 40 ft only where
#     the cap bites, which here is nowhere (`envelope_above_grade`). It is
#     guide geometry, `shadow=False`: seen in every render, never in the
#     light, so the ground and the house under it are lit as if it were
#     not there.
#   * The house: the walk-out level and the street level as one prism from
#     10 ft below the sidewalk, and an upper level whose rear wall steps
#     back from the rear-yard line by `upper_setback`; the rule bites
#     there, where the ground has fallen away. `first_passing_setback()`
#     reads the smallest whole-foot setback that clears the rule off the
#     grade surface, and the tests prove it with the boolean.
#   * The pad: `cut_fill` on the TIN for the walk-out floor -- the earth
#     above it under the house is the cut, the space below it at the rear
#     is the fill, both bodies with their volumes.
#   * The studio at the rear (walls and a gable), the two neighbours as
#     masses on their own ground, the site plan with contours and, behind
#     `sheets`, a plan and a section sheet.

load(
    "@cadpole/site.star",
    "cut_fill",
    "envelope_above_grade",
    "footprint_mass",
    "ft",
    "grade_profile",
    "inch",
    "local_frame",
    "local_to_latlon",
    "parcel",
    "profile_points",
    "rear_yard",
    "setbacks",
    "to_ft",
    "to_mm",
)
load("@cadpole/building.star", "height_check", "height_envelope", "roof_profile")

# Every length is a millimetre, as everywhere in Cadpole; this document reads
# them back in feet and inches -- `cadpole inspect`, the browser, the sheets --
# by the display convention it declares once. Nothing converts.
units(units.ft_in)

# ---- The rules and the design, in feet.
height_above_grade = param("height_above_grade", 30.0, unit="ft", min=20, max=40, doc="Height limit above grade at every point")
height_cap = param("height_cap", 40.0, unit="ft", min=25, max=60, doc="Absolute height limit, above the front sidewalk")
rear_yard_line = param("rear_yard_line", 52.5, unit="ft", min=30, max=70, doc="How far from the front line a building may extend")
upper_setback = param("upper_setback", 15.0, unit="ft", min=0, max=30, step=1, doc="The upper level's rear wall, stepped back from the rear-yard line")
front_setback = param("front_setback", 2.0, unit="ft", min=0, max=10, doc="The house's front wall behind the front line")
level_height = param("level_height", 10.0, unit="ft", min=8, max=12, doc="Floor to floor; the walk-out floor is one level below the sidewalk")
envelope_method = param("envelope_method", "fit", choices=["fit", "tin"], doc="The grade the envelope is raised from: the fitted B-spline (an exact body) or the TIN (a mesh)")
contour_interval = param("contour_interval", 1.0, unit="ft", min=0.5, max=5, doc="Contour interval on the site plan")
sheets = param("sheets", False, doc="Draw the plan and the section sheets (a projection of every body; slow)")

# ---- The frame and the lot. A parcel file gives the corners as latitude
# and longitude; this invented lot's are derived from its 40 x 80 ft plan
# so the numbers below are exact, and `parcel` takes them the way it would
# take a file's. The lot's depth runs at bearing 345 degrees, so the frame
# is turned 15 degrees and the lot stands upright in it.
LOT_W = 40.0
LOT_D = 80.0
frame = local_frame(38.0, -121.0, rotation_deg=15, units="ft")
lot = parcel(
    frame,
    [local_to_latlon(frame, x, y) for x, y in [(0, 0), (LOT_W, 0), (LOT_W, LOT_D), (0, LOT_D)]],
    front=0,
    name="lot",
)

# ---- The survey. A profile down the lot: (depth from the front line,
# elevation) in feet, the sidewalk at 0 -- a gentle start behind the
# sidewalk, a steeper run, a bench two-fifths of the way down, and the
# long fall to the rear line, 20 ft over 80, a 25 % average. LiDAR gives
# ground samples either side, (x, y, z) in the frame's feet, the ground
# rising a little toward the east. Every elevation is on one datum (the
# survey's; LiDAR heights on another datum are shifted by the local
# difference before they go in).
PROFILE = [
    (-8, 0.3), (0, 0.0), (5, -0.6), (10, -1.8), (16, -3.9), (22, -5.6), (28, -6.8), (35, -8.1),
    (42, -9.9), (50, -12.3), (58, -14.6), (66, -16.8), (73, -18.5), (80, -20.0), (88, -21.9), (95, -23.6),
]
LIDAR = [
    (-20, 3, -0.9), (-20, 27, -7.1), (-20, 52, -13.4), (-20, 84, -21.6), (-9, 64, -16.8),
    (52, 5, -0.2), (52, 33, -7.2), (52, 61, -14.9), (52, 90, -21.7), (64, 17, -3.5), (64, 44, -9.6), (64, 76, -18.2),
]

# `profile_points` puts the profile on both side lines and the centreline;
# with the LiDAR beyond the sides that is the site's ground.
points = profile_points(lot, to_mm(frame, PROFILE)) + to_mm(frame, LIDAR)
EXTENT = ((-ft(24), -ft(10)), (ft(70), ft(98)))
grade = terrain(points, method=terrain.tin, extent=EXTENT, name="grade")
grade_fit = terrain(points, method=terrain.fit, extent=EXTENT, name="grade_fit")
rule_grade = grade_fit if envelope_method == "fit" else grade

# The ground as it is seen: a 3 ft skin under the TIN rather than a block
# down to a datum, so its edges are thin and the slope reads as a surface.
# What is drawn stops at the skin's underside (`seen`), so nothing hangs
# below it.
SKIN = ft(3)

def under(name):
    return grade.offset_up(-SKIN).body(base=-ft(45), name=name)

def seen(b, name):
    return subtract(b, under(name + "_under"), name=name, kernel=kernel.mesh)

ground = subtract(grade.body(base=-ft(35), name="ground_block"), under("ground_under"), name="ground", kernel=kernel.mesh)
ground.material(color=(0.43, 0.5, 0.36), roughness=0.95)
# Drawn in 5 ft bands of elevation, two greens a shade apart, so every
# boundary is a contour and the fall of the lot reads at a glance.
BAND = 5.0
(ex0, ey0), (ex1, ey1) = EXTENT
bands = []
glo, ghi = ground.bbox()
for k in range(int(math.floor((glo[2] + 1) / ft(BAND))), int(math.floor((ghi[2] - 1) / ft(BAND))) + 1):
    slab = box(ex1 - ex0 + 2, ey1 - ey0 + 2, ft(BAND), at=(ex0 - 1, ey0 - 1, k * ft(BAND)), name="band%d_slab" % k)
    part = intersect(copy(ground, name="ground_copy%d" % k), slab, name="ground_band%d" % k, kernel=kernel.mesh)
    part.material(color=(0.3, 0.4, 0.27) if k % 2 == 0 else (0.42, 0.52, 0.36), roughness=0.95)
    bands.append(part)

# ---- The envelope over the plan area in front of the rear-yard line: the
# raised surface cut to that rectangle, exact from the fit.
yard = rear_yard(lot, ft(rear_yard_line), name="rear_yard")
envelope = envelope_above_grade(rule_grade, yard, ft(height_above_grade), cap=ft(height_cap), base=-ft(30), name="envelope")
# Guide geometry: the envelope is a rule, not a thing, so the camera
# sees it and light does not -- it shadows nothing under it in a render.
envelope.material(color=(0.45, 0.72, 0.86), opacity=0.22, roughness=0.6, shadow=False)
# What is drawn of it: only the part above the ground, the exact body kept
# for the tests on a hidden layer.
checks = layer("rule_check", doc="The bodies the envelope tests use: the exact envelope, the house as one body and the ground in one piece", visible=False)
envelope_seen = subtract(copy(envelope, name="envelope_copy"), grade.body(base=-ft(45), name="envelope_ground"), name="envelope_seen", kernel=kernel.mesh)
envelope_seen.material(color=(0.45, 0.72, 0.86), opacity=0.22, roughness=0.6, shadow=False)
checks.add(envelope)

# ---- The house: the walk-out level and the street level as one prism
# from a level below the sidewalk, half a foot inside the side lines and
# half a foot short of the rear-yard line so nothing is coplanar with the
# envelope; the upper level a foot further in on every side and stepped
# back from the rear-yard line by `upper_setback`, overlapping the prism
# below by a millimetre so the union is a crossing rather than a touch.
H = ft(level_height)
street = setbacks(lot, front=ft(front_setback), rear=lot["depth"] - ft(rear_yard_line) + ft(0.5), sides=ft(0.5), name="street_plan")
upper = setbacks(lot, front=ft(front_setback) + ft(1), rear=lot["depth"] - ft(rear_yard_line) + ft(0.5) + ft(upper_setback), sides=ft(1.5), name="upper_plan")
lower_levels = translate(extrude(street["sketch"], 2 * H, name="lower_levels"), (0, 0, -H))
upper_level = translate(extrude(upper["sketch"], H + 1, name="upper_level"), (0, 0, H - 1))
house = union(lower_levels, upper_level, name="house")
# The house the rules are checked on is one body; what is seen is its three
# levels -- the walk-out floor a level below the sidewalk, the street level,
# the stepped-back upper level -- each a shade apart.
checks.add(house, ground)
house.material(color=(0.9, 0.88, 0.83), roughness=0.7)
LEVELS = [
    seen(translate(extrude(street["sketch"], H, name="walkout_prism"), (0, 0, -H)), "walkout_level"),
    extrude(street["sketch"], H, name="street_level"),
    translate(extrude(upper["sketch"], H, name="upper_prism"), (0, 0, H), name="upper_level_seen"),
]
for b, c in zip(LEVELS, [(0.78, 0.76, 0.71), (0.9, 0.88, 0.83), (0.95, 0.94, 0.9)]):
    b.material(color=c, roughness=0.7)
# Glass across the downhill face of each level, an inch proud of the wall,
# so the walk-out reads as one.
REAR = ft(rear_yard_line) - ft(0.5)
glazing = []
for i, (z0, back, inset_x) in enumerate([(-H, REAR, ft(0.5)), (0, REAR, ft(0.5)), (H, REAR - ft(upper_setback), ft(1.5))]):
    g = box(ft(LOT_W) - 2 * inset_x - ft(6), inch(1), ft(6), at=(inset_x + ft(3), back, z0 + ft(2)), name="glazing%d" % i)
    g.material(color=(0.55, 0.68, 0.76), roughness=0.15)
    glazing.append(g)

# ---- The earthwork for the walk-out floor: the pad at -H over the house's
# plan, cut and fill from the TIN (through every shot).
pad = cut_fill(grade, street, -H, base=-ft(35), name="pad")
# The cut is inside the walk-out level, so it is drawn only on request; the
# fill is the terrace the rear of the house stands on.
earthwork = layer("earthwork", doc="The earth dug out for the walk-out level", visible=False)
if pad["cut"] != None:
    pad["cut"].material(color=(0.55, 0.42, 0.3), roughness=0.9)
    earthwork.add(pad["cut"])
if pad["fill"] != None:
    pad["fill"].material(color=(0.66, 0.6, 0.5), roughness=0.9)

# ---- The studio at the rear: walls from its floor 15 ft below the street
# to the eave, under a gable whose ridge runs along the depth and whose
# eaves overhang the walls by a foot all round, so the roof crosses the
# walls rather than sitting flush on them.
STUDIO = [(6.0, 58.0), (24.0, 58.0), (24.0, 78.0), (6.0, 78.0)]
studio_floor = -ft(15)
studio_walls = translate(extrude(sketch(XY, name="studio_plan").polygon(to_mm(frame, STUDIO)), ft(8), name="studio_walls"), (0, 0, studio_floor))
# The plane's second axis points down (normal x x_dir), so heights go in negated.
gable_plane = plane(origin=(0, ft(57), 0), normal=(0, 1, 0), x_dir=(1, 0, 0))
gable = sketch(gable_plane, name="gable_profile").polygon([
    (ft(5), -(studio_floor + ft(7.5))),
    (ft(25), -(studio_floor + ft(7.5))),
    (ft(15), -(studio_floor + ft(13))),
])
studio_roof = extrude(gable, ft(22), name="studio_roof")
studio = union(studio_walls, studio_roof, name="studio")
studio.material(color=(0.78, 0.66, 0.55), roughness=0.85)
# Its own pad: the ground under it cut and filled level with its floor.
studio_pad = cut_fill(grade, to_mm(frame, STUDIO), studio_floor, base=-ft(35), name="studio_pad")
if studio_pad["cut"] != None:
    earthwork.add(studio_pad["cut"])
if studio_pad["fill"] != None:
    studio_pad["fill"].material(color=(0.66, 0.6, 0.5), roughness=0.9)

# ---- The neighbours: public LiDAR data gives a footprint, a ground
# elevation and a height; here the ground is read off the grade at each
# footprint's front.
EAST = [(43.0, 6.0), (68.0, 6.0), (68.0, 48.0), (43.0, 48.0)]
WEST = [(-22.0, 12.0), (-3.0, 12.0), (-3.0, 38.0), (-22.0, 38.0)]
# The mass stands from a foot below the lowest ground under its footprint,
# so it meets the slope everywhere, to its height above the ground at its front.
def neighbour(poly, front, height, name):
    lo, _ = grade.z_range(to_mm(frame, poly))
    top = grade.z_at(ft(front[0]), ft(front[1])) + height
    return seen(footprint_mass(frame, poly, lo - ft(1), top - lo + ft(1), latlon=False, name=name + "_mass"), name)

east = neighbour(EAST, (55, 6), ft(27), "east_neighbour")
west = neighbour(WEST, (-12, 12), ft(17), "west_neighbour")
east.material(color=(0.32, 0.35, 0.36), roughness=0.95)
west.material(color=(0.36, 0.34, 0.31), roughness=0.95)

# ---- Where the rule bites: the upper level's roof against the envelope
# along its rear wall, read off a grade surface at ten points, no
# boolean. The smallest whole-foot setback that clears it -- on the
# document's rule surface, and on the TIN, which the boolean proofs below
# use; the two differ by the fit's residual, a foot of setback or so.
def clears(setback, surface):
    roof = 2 * H
    rear_v = ft(rear_yard_line) - ft(0.5) - ft(setback)
    for i in range(10):
        x = ft(1.5) + (ft(LOT_W) - ft(3)) * i / 9.0
        limit = min(surface.z_at(x, rear_v) + ft(height_above_grade), ft(height_cap))
        if roof > limit:
            return False
    return True

def first_passing_setback(surface):
    for s in range(0, 31):
        if clears(s, surface):
            return s
    return None

first_passing = first_passing_setback(rule_grade)
first_passing_tin = first_passing_setback(grade)

# When the setback is too small, the part of the house above the envelope,
# in red: the house less the TIN's envelope (a mesh), shown when the
# sampled rule on the TIN says something is above it.
over_envelope = None
if not clears(upper_setback, grade):
    over_envelope = subtract(copy(house, name="house_over"), envelope_above_grade(grade, yard, ft(height_above_grade), cap=ft(height_cap), base=-ft(30), name="envelope_tin"),
                             name="over_envelope", kernel=kernel.mesh)
    over_envelope.material(color=(0.86, 0.22, 0.16), roughness=0.6)

# ---- The ground down the centreline, (depth, z): what a section plots
# and what a building library's height check takes as its grade.
section_grade = grade_profile(grade, lot, step=ft(1))

# ---- Drawings: the site plan with contours (cheap: sketches only), and
# behind `sheets` a plan and a section of every body.
contours = grade.contour_sketch(ft(contour_interval), name="contours")
export_svg([lot["sketch"], yard["sketch"], contours], "hillside_lot_contours.svg")
if sheets:
    site_plan = drawing(view="plan", sheet="ARCH_C", units="ft-in", title="Hillside lot, site plan", name="site_plan")
    export_svg(site_plan, "hillside_lot_plan.svg")
    section = drawing(view="section:x=%s" % ft(LOT_W / 2), sheet="ARCH_C", units="ft-in", title="Hillside lot, section along the slope", name="section")
    export_svg(section, "hillside_lot_section.svg")
export([ground] + LEVELS + [studio, east, west], "hillside_lot.3mf")

# ---- Tests: `cadpole test examples/hillside_lot.star`.

def overflow():
    """How much of the house stands above the envelope, mm³, proved with
    the boolean on the TIN's envelope (a mesh of a hundred triangles; the
    exact one is checked by sampling and by volume): the house less its
    intersection with the envelope (a subtract would refuse the empty body
    a fitting house leaves)."""
    mesh_envelope = envelope_above_grade(grade, yard, ft(height_above_grade), cap=ft(height_cap), base=-ft(30))
    inside = intersect(house, mesh_envelope)
    return house.volume() - inside.volume()

def the_house_fits():
    """The default design clears the rule on the document's surface by
    sampling and on the TIN by the boolean."""
    assert_solid(house)
    assert_solid(envelope)
    assert_true(clears(upper_setback, rule_grade), msg="the roof line is above the rule at the rear wall")
    return fits_the_tin()

def fits_the_tin():
    over = overflow()
    assert_true(over < 0.002 * house.volume(), msg="%d cu ft of the house is above the envelope" % int(over / ft(1) / ft(1) / ft(1)))
    return over

test("the house fits the envelope", the_house_fits)
test("the first passing upper setback on the TIN, %s ft, passes the boolean too" % first_passing_tin, fits_the_tin, sweep={"upper_setback": [first_passing_tin if first_passing_tin != None else 30]})

def the_house_breaks_the_rule():
    assert_true(not clears(upper_setback, grade), msg="the sampled rule passes at %d ft" % upper_setback)
    over = overflow()
    assert_true(over > 0, msg="nothing above the envelope at %d ft" % upper_setback)

test("one foot less setback than the TIN's first passing one breaks the rule", the_house_breaks_the_rule, sweep={"upper_setback": [first_passing_tin - 1 if first_passing_tin else 0]})
test("more setback, less of the house above the envelope", overflow, sweep={"upper_setback": [0, 3, 6]}, monotone=monotone.decreasing)

def cut_volume():
    return pad["cut_volume"]

test("a deeper walk-out level is more cut", cut_volume, sweep={"level_height": [8, 10, 12]}, monotone=monotone.increasing)

def the_site_reads_right():
    assert_true(first_passing != None, msg="no setback clears the rule")
    assert_true(first_passing <= upper_setback, msg="the default setback of %d ft does not clear the rule (%d ft does)" % (upper_setback, first_passing))
    # The fit and the TIN put the line within a couple of feet of each
    # other: the residual is inches, the ground falls 0.3 ft per foot there.
    assert_true(first_passing_tin != None and abs(first_passing - first_passing_tin) <= 2, msg="first passing: fit %s ft, TIN %s ft" % (first_passing, first_passing_tin))
    g = grade.average_grade(lot["corners"])
    assert_true(g > 0.22 and g < 0.28, msg="average grade %s" % g)
    # The TIN reproduces a shot exactly, the fit within its residual, which
    # is well under a foot.
    assert_true(grade_fit.residual < ft(0.5), msg="rms residual %d mm" % int(grade_fit.residual))
    u, v, z = LIDAR[6]
    assert_close(grade.z_at(ft(u), ft(v)), ft(z), tol=1e-6)
    assert_close(grade_fit.z_at(ft(u), ft(v)), ft(z), tol=4 * grade_fit.max_residual)
    # The envelope: exact from the fit, over exactly the plan area, its top
    # the rule at the front and the cap nowhere.
    if envelope_method == "fit":
        assert_eq(envelope.kind, "brep")
        assert_eq(sorted([f.name for f in envelope.faces()]), ["envelope/bottom", "envelope/side[0]", "envelope/side[1]", "envelope/side[2]", "envelope/side[3]", "envelope/top"])
    lo, hi = envelope.bbox()
    assert_close(lo[0], 0.0, tol=1.0)
    assert_close(hi[0], ft(LOT_W), tol=1.0)
    assert_close(hi[1], ft(rear_yard_line), tol=1.0)
    assert_close(hi[2], rule_grade.z_at(ft(LOT_W / 2), 1.0) + ft(height_above_grade), tol=ft(0.5))
    assert_true(hi[2] < ft(height_cap), msg="the cap bites")
    # The ground falls all the way down the lot.
    assert_eq(len(section_grade), 81)
    for i in range(1, len(section_grade)):
        assert_true(section_grade[i][1] < section_grade[i - 1][1], msg="the profile rises at %s ft" % to_ft(section_grade[i][0]))
    assert_close(section_grade[0][1], 0.0, tol=1e-6)
    # The pad: both a cut (the bank under the front of the house) and a
    # fill (the rear, where the ground is below the walk-out floor).
    assert_true(pad["cut"] != None and pad["fill"] != None, msg="a cut and a fill")
    assert_solid(pad["cut"])
    assert_solid(pad["fill"])
    assert_true(pad["cut_volume"] > pad["fill_volume"], msg="more cut than fill on a bank")
    assert_true(len(grade.contours(ft(1))) >= 20, msg="contours")
    assert_eq(lot["edge_names"], ["front", "side[0]", "rear", "side[1]"])
    assert_close(yard["line"][0][1], ft(rear_yard_line), tol=1e-6)
    assert_true(contours.paths >= 20)
    assert_solid(studio)
    assert_solid(east)
    assert_solid(west)
    assert_solid(ground)

test("the site reads right", the_site_reads_right)
test("the TIN envelope reads the same", the_site_reads_right, sweep={"envelope_method": ["tin"]})


# ---- The site library's grade against the building library's rule:
# `grade_profile` is what `height_envelope` takes as its grade, and the
# house's own profile down the centreline is the roof.

def the_grade_profile_feeds_the_height_envelope():
    """The averaged measure on the section: the house against `grade + 30 ft`
    down the centreline, with averaging -- it clears the line everywhere,
    worst at the upper level's top rear corner, where the ground has fallen
    away -- then a flat 22 ft roof over the same plan, which only the
    averaging lets through, and the rearmost 8 ft over 32 ft does not."""
    roof = roof_profile(house, axis=(0, 1), through=(ft(LOT_W / 2), 0))
    env = height_envelope(section_grade, roof, limit=ft(height_above_grade), cap=ft(height_cap))
    assert_true(env["passes"], msg="the house is %s ft over the line %s ft in" % (to_ft(env["max_over"]), to_ft(env["max_over_at"])))
    assert_eq(env["over_area"], 0.0)
    assert_true(env["max_over"] < 0, msg="max over %s ft" % to_ft(env["max_over"]))
    assert_close(env["max_over_at"], ft(rear_yard_line) - ft(0.5) - ft(upper_setback), tol=1.0)
    # The limit line is the grade profile lifted by the limit, to the rear wall.
    assert_close(env["limit_line"][-1][1], section_grade[52][1] + ft(height_above_grade), tol=1.0)
    flat = [(ft(front_setback), ft(22)), (ft(rear_yard_line) - ft(0.5), ft(22))]
    plain = height_check(section_grade, flat, limit=ft(height_above_grade))
    assert_true(not plain["passes"] and plain["max_over"] > 0, msg="a flat 22 ft roof clears the plain check")
    avg = height_envelope(section_grade, flat, limit=ft(height_above_grade), cap=ft(height_cap))
    assert_true(avg["over_area"] > 0 and avg["over_area"] < avg["under_area"], msg="over %s, under %s" % (avg["over_area"], avg["under_area"]))
    assert_true(avg["passes"], msg="the averaging does not pass the flat roof")
    strip = height_envelope(section_grade, flat, limit=ft(height_above_grade), cap=ft(height_cap), rear=(ft(8), ft(32)))
    assert_true(not strip["rear_ok"] and not strip["passes"], msg="the rear strip passes")
    return plain["max_over"]

test("the site's grade profile feeds the building library's height envelope", the_grade_profile_feeds_the_height_envelope)
