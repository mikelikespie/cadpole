# A mounting plate: a centre bore and two counterbored screw holes whose
# spacing follows the width. One parameter, one sketch, a few primitives.

w = param("width", 60, unit="mm", min=50, max=120)
depth, thick = 30, 6
pitch = w - 16                 # screw centres, 8 mm in from each end
cb_r, cb_depth = 4, 3.0        # counterbore for a low socket head M4

plate = extrude(sketch(XY).rounded_rect(w, depth, 5), thick, name="plate")
plate = fillet(plate, 0.8, edges=plate.faces(normal=(0, 0, 1)).one().edges, name="edge")

# The counterbores are cut first, so each 4.5 mm screw hole then only
# pierces a floor. The centre bore is one cutter: a 12 mm cylinder through a
# 45-degree countersink cone, standing proud of the cone's top.
ends = [-pitch / 2, pitch / 2]
counterbored = subtract([plate] + [cylinder(cb_r, cb_depth + 2, at=(x, 0, thick - cb_depth)) for x in ends])
bore = union(cylinder(6, thick + 4, at=(0, 0, -1)), cone(4.5, 8.5, 4, at=(0, 0, thick - 3)))
part = subtract([counterbored, bore] + [cylinder(2.25, thick + 2, at=(x, 0, -1)) for x in ends], name="part")
part.material(color="#6fbfa8", roughness=0.35, metallic=0.3)
part.tessellation(tolerance=0.01)   # small round holes stay round up close

export(part, "part.stl")


# Tests: `cadpole test examples/part.star`.

def the_plate_is_one_piece_with_three_holes():
    assert_solid(part)
    assert_eq(part.kind, "brep", msg="exact surfaces all the way, so it exports to STEP")
    r = print_check(part)
    assert_eq(r.components, 1)
    assert_eq(r.genus, 3, msg="the bore and both screw holes go right through")
    assert_bbox(part, (-w / 2, -depth / 2, 0), (w / 2, depth / 2, thick))

def the_holes_follow_the_width():
    for x in [-pitch / 2, pitch / 2]:
        rims = part.edges(min_radius=cb_r - 0.01, max_radius=cb_r + 0.01,
                          inside=((x - cb_r - 0.1, -cb_r - 0.1, 0), (x + cb_r + 0.1, cb_r + 0.1, thick + 0.1)))
        assert_true(rims.count() >= 2, msg="a counterbore at x=%s, its floor and its mouth" % x)
    floors = part.faces(kind="plane", normal=(0, 0, 1), on_plane=XY.offset(thick - cb_depth))
    assert_eq(floors.count(), 2, msg="two counterbore floors for the screw heads")

def every_hole_keeps_its_wall():
    # Just under the top face the plate is one outline with three separate
    # holes: no counterbore runs into the countersink or out of an end.
    loops = part.section(XY.offset(thick - 0.9)).points()
    assert_eq(len(loops), 4)
    assert_true(w / 2 - pitch / 2 - cb_r >= 3, msg="3 mm of wall past each counterbore")
    assert_true(pitch / 2 - cb_r - 7.5 >= 3, msg="3 mm between each counterbore and the countersink")

def the_plate_weighs_what_it_should():
    corner = (4 - math.pi) * 5 * 5
    edge = (1 - math.pi / 4) * 0.8 * 0.8 * (2 * (w - 10) + 2 * (depth - 10) + 2 * math.pi * 5)
    screw = math.pi * 2.25 * 2.25 * (thick - cb_depth) + math.pi * cb_r * cb_r * cb_depth
    sink = math.pi / 3 * 1.5 * (36 + 45 + 56.25) - math.pi * 36 * 1.5
    bore_v = math.pi * 36 * thick + sink
    assert_volume(part, (w * depth - corner) * thick - edge - 2 * screw - bore_v)

test("one piece with three through holes", the_plate_is_one_piece_with_three_holes, sweep={"width": [50, 60, 120]})
test("the screw holes spread with the width", the_holes_follow_the_width, sweep={"width": [50, 60, 120]})
test("every hole keeps its wall", every_hole_keeps_its_wall, sweep={"width": [50, 60, 120]})
test("the volume matches the drawing", the_plate_weighs_what_it_should, sweep={"width": [50, 60, 120]})
