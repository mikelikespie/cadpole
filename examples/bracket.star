# A mounting bracket: a plate with two bolt holes and a boss.
# Units are millimetres.

w = param("width", 60, unit="mm", min=30, max=120, doc="Plate width (x)")
d = param("depth", 30, unit="mm", min=20, max=80, doc="Plate depth (y)")
t = param("thickness", 6, unit="mm", min=3, max=12)
bolt = param("bolt_d", 5.5, unit="mm", min=3, max=10, doc="Bolt hole diameter")
boss_r = param("boss_r", 8, unit="mm", min=4, max=15)

plate = box(w, d, t, center=True, name="plate")
# The boss starts 1 mm inside the plate: coplanar faces make booleans fail
# in the current kernel, so overlap instead of touching.
boss = cylinder(boss_r, 11, at=(0, 0, t - 1), name="boss")
body = union(plate, boss, name="body")

# Bolt holes, cut all the way through.
holes = [
    cylinder(bolt / 2, t + 2, at=(-w / 2 + 8, 0, -1), name="hole_left"),
    cylinder(bolt / 2, t + 2, at=(w / 2 - 8, 0, -1), name="hole_right"),
]
bracket = subtract(body, holes[0], holes[1], name="bracket")
bracket.material(color="#7fa6d9", roughness=0.5)

export(bracket, "bracket.stl")

# Tests: `cadpole test examples/bracket.star`. A test runs after the model is
# built and fails by raising; nothing here runs in an ordinary evaluation.

def a_plate_a_boss_and_two_holes():
    assert_solid(bracket)
    assert_bbox(bracket, (-w / 2, -d / 2, 0), (w / 2, d / 2, t + 10))
    # The boss stands 10 mm proud of the plate; both holes go right through it.
    hole = math.pi * (bolt / 2) * (bolt / 2) * t
    assert_volume(bracket, w * d * t + math.pi * boss_r * boss_r * 10 - 2 * hole)
    # Six plate faces, three boss faces, two halves per hole: the names the
    # features gave them survive the booleans.
    assert_eq(len(bracket.faces()), 13)
    bracket.face("plate/top")

test("a plate, a boss and two through holes", a_plate_a_boss_and_two_holes)

def material():
    assert_solid(bracket)
    return bracket.volume()

test("bigger bolts leave less bracket", material, sweep={"bolt_d": [3, 5.5, 10]}, monotone=monotone.decreasing)
