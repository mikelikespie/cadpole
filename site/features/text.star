# Text: shaped type as a sketch, extruded like any outline.
# A nameplate: the name raised, a line engraved under it, and a
# countersunk screw hole at each end.
# The top edge is chamfered 0.6 mm, built into the profile as a loft.
plate = loft([sketch(XY).rounded_rect(72, 26, 5),
              sketch(XY.offset(3.4)).rounded_rect(72, 26, 5),
              sketch(XY.offset(4)).rounded_rect(70.8, 24.8, 4.4)], name="plate")
holes = []
for x in (-29, 29):
    holes.append(cylinder(1.7, 6, at=(x, 0, -1)))
    holes.append(cone(1.7, 3.8, 2.1, at=(x, 0, 2.0)))  # the countersink
plate = subtract(plate, holes, name="plate_holes")
# snippet
label = text("CADPOLE", size=9, weight=700,
             align="center", baseline="middle",
             plane=XY.offset(4), at=(0, 2.5))
letters = extrude(label, 1.2, name="letters")
line = text("PARAMETRIC CAD", size=3.4, weight=700, align="center",
            baseline="middle", plane=XY.offset(3.4), at=(0, -6.5))
plate = subtract(plate, extrude(line, 1, name="cut"), name="tag")
# end snippet
plate.material(color="#6fbfa8", roughness=0.4)
paint(plate, faces=plate.faces(name="cut*", normal=(0, 0, 1)), color="#f0c987")
for g in letters:
    g.material(color="#f0c987", roughness=0.4)


def a_plate_with_raised_and_engraved_type():
    assert_solid(plate)
    for g in letters:
        assert_solid(g)
        lo, hi = g.bbox()
        assert_close(lo[2], 4.0, tol=1e-6, msg="the raised letters stand on the plate's top")
        assert_close(hi[2], 5.2, tol=1e-6, msg="and rise 1.2 mm from it")
    floors = plate.faces(name="cut*", normal=(0, 0, 1))
    assert_true(len(floors) >= 10, msg="every engraved letter has a floor")
    for f in floors:
        assert_close(f.centroid[2], 3.4, tol=1e-3, msg="the engraving is 0.6 mm deep")


def two_countersunk_holes():
    for x in (-29, 29):
        # Through the plate, and nearly twice as wide just under the top.
        rim = distance(plate, cylinder(3.2, 0.1, at=(x, 0, 3.85)))
        assert_true(not rim.overlap, msg="the hole at x=%s is countersunk" % x)
        assert_true(not distance(plate, cylinder(1.6, 6, at=(x, 0, -1))).overlap, msg="the hole at x=%s goes through" % x)


test("a plate with raised and engraved type", a_plate_with_raised_and_engraved_type)
test("two countersunk holes", two_countersunk_holes)
