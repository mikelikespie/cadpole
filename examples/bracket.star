# A pivot bracket: a bent angle with a bored boss on its upright, two
# slotted M5 holes in its foot and a gusset at each end.
# Units are millimetres.

w = param("width", 60, unit="mm", min=52, max=120, doc="Width along the bend (x)")
d = param("depth", 40, unit="mm", min=36, max=80, doc="Foot depth (y)")
h = param("height", 50, unit="mm", min=50, max=90, doc="Upright height (z)")
t = param("thickness", 6, unit="mm", min=4, max=8, doc="Plate thickness")
bolt = param("bolt_d", 5.5, unit="mm", min=3, max=6.6, doc="Bolt hole diameter")
boss_r = param("boss_r", 9, unit="mm", min=8, max=12, doc="Boss radius")
bore = param("bore_d", 8, unit="mm", min=4, max=10, doc="Pivot bore diameter")

bend = t                    # inside bend radius: one thickness, as a press brake leaves it
gusset_t = 5                # gusset thickness
inset = 3                   # gusset in from each end
slot_len = bolt + 6         # 6 mm of travel
washer_r = bolt             # a washer under the bolt head: the flat a slot needs round it
hole_x = w / 2 - inset - gusset_t - washer_r - 0.5   # the washer clears the gusset
hole_y = -t                 # midway across the flat of the foot, in front of the bend
boss_z = h - boss_r - 6     # the boss sits near the top of the upright
boss_len = 8                # how far the boss stands proud of the upright

# The ranges above are chosen so every combination builds; these say which
# number to change if they are ever widened.
assert_true(hole_x - washer_r >= 4, msg="the bolt heads meet in the middle: widen the bracket or shrink bolt_d")
assert_true(boss_r - bore / 2 >= 2.5, msg="the boss wall round the bore is under 2.5 mm: grow boss_r or shrink bore_d")
assert_true(boss_z - boss_r >= t + bend + 4, msg="the boss runs into the bend: raise the upright or shrink boss_r")
assert_true(2 * boss_r <= w - 2 * (inset + gusset_t) - 4, msg="the boss is wider than the room between the gussets")
assert_true(d - 2 * t >= slot_len - bolt + 2 * washer_r, msg="a washer at the end of its slot would sit on the bend or hang off the front: deepen the foot")

# The angle is three plates that meet face to face, each drawn whole in its
# own sketch, so every hole and round is part of a profile rather than a cut:
# a foot with its two slots, a quarter-round bend, and an upright with
# rounded top corners. Their shared faces are identical rectangles, which
# `glue` merges into one shell.
back = d / 2
yb = back - 2 * t           # where the foot ends and the bend begins
zb = 2 * t                  # where the bend ends and the upright begins

# The foot, seen from above, with its front corners rounded and a slot on
# each side running toward the upright, so the bracket can be slid into
# line before its bolts are tightened.
foot_outline = sketch(XY, name="foot_outline").polygon([
    (-w / 2, -d / 2), (w / 2, -d / 2), (w / 2, yb), (-w / 2, yb),
]).fillet2d(8, corners=["corner[0]", "corner[1]"])
# A sketch plane whose x runs along world y, so the slots do too.
along_y = plane(origin=(0, 0, 0), normal=(0, 0, 1), x_dir=(0, 1, 0))
slots = sketch(along_y, name="slots").slot(slot_len, bolt, center=(hole_y, hole_x)).slot(slot_len, bolt, center=(hole_y, -hole_x))
foot_profile = foot_outline.subtract(slots, name="foot_profile")
foot = extrude(foot_profile, t, name="foot")

# The bend: a quarter of a ring, inside radius `bend`, outside one thickness
# more, pushed out along x.
ring = sketch(YZ.offset(-w / 2), name="ring").circle(bend + t, center=(yb, zb)).circle(bend, center=(yb, zb))
quarter = sketch(YZ.offset(-w / 2), name="quarter").rect(bend + t, bend + t, center=(yb + (bend + t) / 2, zb - (bend + t) / 2))
bend_body = extrude(ring.intersect(quarter), w, name="bend")

# The upright, seen from the front, with its top corners rounded.
upright_outline = sketch(XZ.offset(-back), name="upright_outline").polygon([
    (-w / 2, zb), (w / 2, zb), (w / 2, h), (-w / 2, h),
]).fillet2d(10, corners=["corner[2]", "corner[3]"])
upright = extrude(upright_outline, t, name="upright")

angle = glue(foot, bend_body, upright, name="angle")

# Gussets: right triangles tied into the foot and the upright. Their legs run
# along the middle of the plates and the heel cuts across the bend, so every
# face of a gusset crosses the angle's instead of lying on one.
gl = 0.7 * min(d - t, h - t)
def gusset(x, name):
    tri = sketch(YZ.offset(x), name=name + "_sketch").polygon([
        (back - t / 2, t + gl), (back - t / 2, 1.5 * t), (back - 1.5 * t, t / 2), (back - t - gl, t / 2),
    ])
    return extrude(tri, gusset_t, name=name)

gussets = [gusset(-w / 2 + inset, "gusset_left"), gusset(w / 2 - inset - gusset_t, "gusset_right")]
body = union(angle, gussets[0], gussets[1], name="body")

# The boss: a cylinder standing out of the upright toward the foot, bored
# straight through it and the upright.
face_y = back - t           # the upright's inside face
boss = extrude(sketch(XZ.offset(-(back - t / 2))).circle(boss_r, center=(0, boss_z)), t / 2 + boss_len, name="boss")
body = union(body, boss, name="bossed")

pivot = extrude(sketch(XZ.offset(-(back + 1))).circle(bore / 2, center=(0, boss_z)), t + boss_len + 2, name="bore")
machined = subtract(body, pivot, name="machined")

# Break the edges a hand or a pin meets: a round where the boss grows out
# of the upright, and a small one on the boss's face and the bore's mouth.
near_boss = ((-boss_r - 1, face_y - 1, boss_z - boss_r - 1), (boss_r + 1, face_y + 1, boss_z + boss_r + 1))
boss_root = machined.edges(on_plane=XZ.offset(-face_y), inside=near_boss)
boss_face = machined.faces(on_plane=XZ.offset(-(face_y - boss_len))).one()
rounded = fillet(machined, edges=boss_root, radius=2, name="boss_root")
bracket = fillet(rounded, edges=boss_face.edges, radius=0.8, name="bracket")

# Anodised aluminium in a mid blue: light enough to hold its shading on a
# dark page, saturated enough to read as a chosen finish.
bracket.material(color="#4b82c8", roughness=0.38, metallic=0.35)

export(bracket, "bracket.stl")

# Tests: `cadpole test examples/bracket.star`. A test runs after the model is
# built and fails by raising; nothing here runs in an ordinary evaluation.

# A probe a little smaller than a hole, laid along it, touches nothing when
# the hole is really there and really goes through.
def clear(probe, what):
    c = distance(probe, bracket)
    assert_true(not c.overlap and c.distance > 0.05, msg="%s is blocked: %s" % (what, c))

def one_solid_with_every_feature():
    assert_solid(bracket)
    assert_eq(bracket.kind, "brep")
    assert_bbox(bracket, (-w / 2, -d / 2, 0), (w / 2, d / 2, h))
    for x in [-hole_x, hole_x]:
        # A bolt goes through at both ends of its slot.
        for y in [hole_y - 3, hole_y + 3]:
            clear(cylinder(bolt / 2 - 0.2, t + 2, at=(x, y, -1)), "a slot")
    bore_rod = extrude(sketch(XZ.offset(-(back + 1))).circle(bore / 2 - 0.2, center=(0, boss_z)), t + boss_len + 2)
    clear(bore_rod, "the pivot bore")
    # The bore runs the whole length of the boss and the upright: a rod
    # just bigger than it bites into both.
    wall = distance(extrude(sketch(XZ.offset(-(back - 0.5))).circle(bore / 2 + 0.3, center=(0, boss_z)), t + boss_len - 1), bracket)
    assert_true(wall.overlap, msg="the bore has no wall round it")

test("one solid with its bore and slots", one_solid_with_every_feature)

def nothing_in_the_way():
    # A bolt head and its washer come straight down onto each slot: neither
    # the boss nor a gusset may stand over the washer anywhere along it.
    for x in [-hole_x, hole_x]:
        for y in [hole_y - 3, hole_y + 3]:
            clear(cylinder(washer_r, h + 10, at=(x, y, t + 0.1)), "the way down to a slot")
    # The gussets stand clear of the slots and of the boss.
    for g in gussets:
        for x in [-hole_x, hole_x]:
            slot = extrude(sketch(along_y.offset(-1)).slot(slot_len, bolt, center=(hole_y, x)), t + 2)
            assert_true(distance(g, slot).distance >= washer_r - bolt / 2, msg="a gusset crowds a slot")
        assert_true(distance(g, boss).distance >= 2, msg="a gusset crowds the boss")

test("the slots are reachable from above", nothing_in_the_way)

# Every corner of the sliders builds a clean part.
def solid():
    assert_solid(bracket)
    assert_eq(bracket.kind, "brep")
    nothing_in_the_way()
    return bracket.volume()

test("the extremes build", solid, sweep={"width": [52, 120], "depth": [36, 80], "height": [50, 90]})
test("the tightest part builds", solid, sweep={"width": [52], "depth": [36], "height": [50], "thickness": [8], "bolt_d": [6.6], "boss_r": [12], "bore_d": [10]})
test("the lightest part builds", solid, sweep={"thickness": [4], "bolt_d": [3], "boss_r": [8], "bore_d": [4]})
test("bigger bolts leave less bracket", solid, sweep={"bolt_d": [3, 5.5, 6.6]}, monotone=monotone.decreasing)
test("a bigger bore leaves less bracket", solid, sweep={"bore_d": [4, 8, 10]}, monotone=monotone.decreasing)
