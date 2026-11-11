# A vendor part, imported: the 608 bearing that `examples/bearing_608.star`
# built and exported as STEP, read back by `import_step` as nine
# exact B-rep bodies -- two races and seven balls -- and pressed into a
# printed housing. The STEP under `examples/assets/bearing_608.step` is a
# file Cadpole wrote, so this is the round trip: every face comes back with
# its class (`faces(kind=kind.sphere)` finds the balls), every body is a
# solid, and the volumes are the ones the source script measured.
#
# The faces of an imported body are named by their position in the file,
# `bearing/face[0]`, `bearing/face[1]`, ... -- the file's own face order is
# the closest thing to construction order a vendor part has -- and the
# bodies `bearing`, `bearing#1`, ... in the file's solid order.
#
#   cadpole test examples/vendor_bearing.star
#   cadpole render examples/vendor_bearing.star --view iso -o /tmp/vendor_bearing.png

fit = param("fit", 0.1, unit="mm", min=0.02, max=0.4, doc="Radial clearance between the outer race and the housing bore")
wall = param("wall", 6.0, unit="mm", min=3, max=12, doc="Housing wall around the bore")

# The bearing as the source script's numbers say it is (bearing_608.star).
bore = 8.0
od = 22.0
width = 7.0
ball_d = 3.97

parts = import_step("bearing_608.step", name="bearing")
inner_race = parts[0]
outer_race = parts[1]
balls = parts[2:]

# A square housing plate with a bore the outer race drops into.
half = od / 2.0 + fit + wall
plate = extrude(sketch(XY, name="plate_outline").polygon([(-half, -half), (half, -half), (half, half), (-half, half)]), width, name="plate")
cutter = extrude(sketch(XY, name="bore_outline").circle(od / 2.0 + fit), width, name="bore_cutter")
housing = subtract(plate, cutter, name="housing")

# The bearing sits centred on z = 0 in the file; lift it into the plate.
placed = [translate(b, (0, 0, width / 2.0), name="placed_%d" % i) for i, b in enumerate(parts)]
inner_placed = placed[0]
outer_placed = placed[1]
balls_placed = placed[2:]

housing.material(preset=preset.petg_glossy, color="#2f3d4f")
for b in placed:
    b.material(preset=preset.steel)


def the_import_is_the_bearing():
    assert_eq(len(parts), 9)
    for b in parts:
        assert_eq(b.kind, "brep")
        assert_solid(b)
    # The races are revolves of eight-segment profiles, three faces per
    # full turn; every one of their curved faces is a true cylinder.
    assert_eq(inner_race.faces().count(), 24)
    assert_eq(inner_race.faces(kind=kind.cylinder).count(), 12)
    assert_eq(inner_race.faces(kind=kind.plane).count(), 12)
    assert_eq(outer_race.faces(kind=kind.cylinder).count(), 12)
    # The balls: two spherical caps each, Cadpole's own sphere.
    for b in balls:
        assert_eq(b.faces().count(), 2)
        assert_eq(b.faces(kind=kind.sphere).count(), 2)
    # Names by position in the file, under the feature's id on every body
    # the feature made (as an extrude's bodies share its generator's names).
    names = inner_race.faces().names()
    assert_true("bearing/face[0]" in names, msg=str(names))
    assert_true("bearing/face[23]" in names, msg=str(names))
    assert_eq(sorted(balls[0].faces().names()), ["bearing/face[0]", "bearing/face[1]"])
    # The volumes the source script measured, measured the same way.
    r_ball = ball_d / 2.0
    assert_close(balls[0].volume(), 4.0 / 3.0 * math.pi * r_ball * r_ball * r_ball, rel=0.06)
    assert_close(inner_race.volume() + outer_race.volume(), 1204.388, rel=5e-3)
    return inner_race.volume() + outer_race.volume()


test("the STEP comes back as the bearing: nine solids, every face classified", the_import_is_the_bearing)


def the_bearing_fits_the_housing():
    assert_solid(housing)
    gap = distance(outer_placed, housing)
    assert_true(not gap.overlap, msg="the outer race must not cut the housing")
    # Measured between two chorded meshes: the bore's chords sit inside its
    # circle, so the gap reads under the fit by up to the chord.
    assert_true(gap.distance <= fit + 1e-6, msg="a chorded mesh sits inside its surface")
    assert_true(gap.distance >= fit - 2.0 * gap.tolerance, msg="the gap is the fit within the chord contract")
    assert_true(not distance(inner_placed, housing).overlap)


test("the outer race sits in the bore with the fit asked for", the_bearing_fits_the_housing, sweep={"fit": [0.05, 0.2]})
