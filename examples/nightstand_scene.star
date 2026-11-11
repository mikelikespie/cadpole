# A scene: the lamp and the charging stand standing on the nightstand.
#
# **This is a scene, not a part.** It is the only example in the set that
# builds no geometry of its own -- every body in it comes from one of three
# sibling scripts, and all this file does is decide where they stand.
#
# What this example demonstrates:
#
#   * **`load()` as composition.** `examples/nightstand.star`,
#     `table_lamp.star` and `charging_stand.star` each end with a list of
#     every body they made and a couple of measurements a caller needs
#     (`TOP_HEIGHT`, `LAMP_WIDTH`). Loading one evaluates it against *this*
#     document's feature tree, so its bodies are already here -- and because
#     `translate` consumes the body it moves, holding that list is what lets a
#     scene pick the whole object up rather than leave a copy at the origin.
#     Three `load()`s and two list comprehensions are the entire file;
#   * **why the exported list has to be complete.** Miss one body out of
#     `LAMP` and it stays behind on the floor at the origin, in shot. The
#     assertions below catch exactly that: every object's bodies are measured
#     *after* placement and have to be up on the tabletop;
#   * **a scene assertion that is a real constraint** rather than arithmetic:
#     the lamp's shade is 300 mm across and the stand is 120 mm across on a
#     460 mm top, so the two footprints have to be checked for clearance, not
#     assumed. `clearance` is the parameter, and moving either object until it
#     fails is the point of it being one;
#   * **one light lighting a scene.** The lamp's bulb is emissive, so under
#     `--backend path` the cherry top, the steel arm and the charger's pucks
#     are all lit by the lamp that is standing on them -- which is the whole
#     reason to render the three together rather than side by side.
#
# Placement: the lamp at the back left of the top, its shade overhanging the
# back edge; the charger in front of it and further left, close to the front
# left corner, with the phone puck facing out into the room.
#
#   cadpole render examples/nightstand_scene.star --backend path --samples 192 \
#       --size 1200x900 --view az=-32,el=26 --environment -o /tmp/scene.png
#   cadpole render examples/nightstand_scene.star --param drawer_open=160 -o /tmp/scene_open.png
#   cadpole check  examples/nightstand_scene.star

load("nightstand.star", "NIGHTSTAND", "TOP_HEIGHT", "TOP_WIDTH", "TOP_DEPTH")
load("table_lamp.star", "LAMP", "LAMP_WIDTH")
load("charging_stand.star", "STAND")

lamp_x = param("lamp_x", 210, unit="mm", min=0, max=600, doc="Where the lamp stands, across the top")
lamp_y = param("lamp_y", 300, unit="mm", min=-50, max=600, doc="and how far back")
stand_x = param("stand_x", 78, unit="mm", min=0, max=600, doc="Where the charging stand stands, across the top")
stand_y = param("stand_y", 62, unit="mm", min=-50, max=600, doc="and how far back")
clearance = param("clearance", 10, unit="mm", min=0, max=200, doc="Daylight required between the two objects' footprints")

# Both scripts build at the origin with z = 0 at the surface they stand on, so
# placing one is a single translate of every body it made.
lamp = [translate(b, (lamp_x, lamp_y, TOP_HEIGHT), name="lamp_%d" % i) for i, b in enumerate(LAMP)]
stand = [translate(b, (stand_x, stand_y, TOP_HEIGHT), name="stand_%d" % i) for i, b in enumerate(STAND)]

assert_true(len(NIGHTSTAND) >= 11, msg="the nightstand is eleven boards or more")
assert_eq(len(lamp), len(LAMP), msg="every body of the lamp was placed")
assert_eq(len(stand), len(STAND), msg="every body of the stand was placed")


def footprint(bodies):
    """The plan-view box the placed bodies occupy, and their lowest point."""
    lo, hi = bodies[0].bbox()
    x0, y0, z0 = lo[0], lo[1], lo[2]
    x1, y1 = hi[0], hi[1]
    for b in bodies[1:]:
        lo, hi = b.bbox()
        x0, y0, z0 = min(x0, lo[0]), min(y0, lo[1]), min(z0, lo[2])
        x1, y1 = max(x1, hi[0]), max(y1, hi[1])
    return (x0, y0, x1, y1, z0)


lf = footprint(lamp)
sf = footprint(stand)

# Nothing was left behind at the origin: the lowest point of each object is
# the tabletop it stands on, not the floor.
assert_true(math.close(lf[4], TOP_HEIGHT, tol=0.5), msg="the whole lamp went up onto the top")
assert_true(math.close(sf[4], TOP_HEIGHT, tol=0.5), msg="and the whole charging stand with it")

# The two objects clear each other. This is measured between the placed
# bodies, so it is still true if either script's proportions change.
overlap_x = min(lf[2], sf[2]) - max(lf[0], sf[0])
overlap_y = min(lf[3], sf[3]) - max(lf[1], sf[1])
assert_true(overlap_x < -clearance or overlap_y < -clearance,
            msg="the lamp's shade and the charging stand are into each other: move `lamp_x`/`stand_x`")

# Both stand on the top rather than over its edge -- except the lamp's shade,
# which overhangs the back, so only the part
# that carries weight is checked.
assert_true(lf[0] > -20 and lf[2] < TOP_WIDTH, msg="the lamp is on the top, across it")
assert_true(sf[0] > -20 and sf[2] < TOP_WIDTH, msg="and the charging stand too")
assert_true(math.close(LAMP_WIDTH / TOP_DEPTH, 0.71, tol=0.12),
            msg="the shade is a good two-thirds the depth of the top")
