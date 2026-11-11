# A plate with a hole through it: one parameter, two sketches, one boolean
# and one export. The smallest whole script.

w = param("width", 40, unit="mm", min=10, max=100)

plate = extrude(sketch(XY).rect(w, 20), 10)
hole = extrude(sketch(XY.offset(-1)).circle(3), 12)

part = subtract(plate, hole, name="part")
part.material(color="#6fbfa8", roughness=0.4)

export(part, "part.stl")


# Tests: `cadpole test examples/part.star`.

def the_plate_keeps_its_hole():
    assert_solid(part)
    assert_bbox(part, (-w / 2, -10, 0), (w / 2, 10, 10))
    assert_volume(part, w * 20 * 10 - math.pi * 3 * 3 * 10)

test("a plate with one through hole, at every width", the_plate_keeps_its_hole, sweep={"width": [10, 40, 100]})
