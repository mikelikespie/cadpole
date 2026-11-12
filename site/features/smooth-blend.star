# Smooth blends: distance fields joined with a soft radius.
# A lever knob: a round rose plate, a slender stem and a ball, each joint
# a fillet the blend radius sets. The stem starts inside the plate, well
# clear of its underside, so the blend never reaches the floor or the rim.
# snippet
f = field.cylinder(18, 6, round=2).translate((0, 0, 3))
stem = field.cylinder(4.5, 26, round=1).translate((0, 0, 19))
ball = field.sphere(9).translate((0, 0, 36))
f = f.smooth_union(stem, 4).smooth_union(ball, 3)
part = mesh(f, resolution=0.3, simplify=False,
            name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.35)


def a_flat_foot_and_a_true_rim():
    # The blend stays at the stem: the floor is flat at z=0 and the plate
    # keeps its 18 mm radius, so the knob sits square on a table.
    assert_solid(part)
    assert_bbox(part, (-18, -18, 0), (18, 18, 45), tol=0.15)
    floor = part.section(plane(origin=(0, 0, 0.1), normal=(0, 0, 1)))
    r = 16 + math.sqrt(2 * 2 - 1.9 * 1.9)  # the 2 mm round of the rim, 0.1 mm up
    assert_close(floor.area(), math.pi * r * r, rel=0.01, msg="the underside is a flat disc: the round rim, no sag under the stem")


def the_joints_are_filleted():
    # Just above the plate, the stem is wider than its 4.5 mm radius: the
    # blend's fillet. Halfway up it is the stem itself.
    def r_at(z):
        return math.sqrt(part.section(plane(origin=(0, 0, z), normal=(0, 0, 1))).area() / math.pi)

    assert_true(r_at(6.5) > 6.0, msg="a fillet where the stem meets the plate")
    assert_close(r_at(18), 4.5, tol=0.15, msg="the stem is its own radius away from the joints")
    assert_true(r_at(27.5) > 5.0, msg="a fillet where the stem meets the ball")


test("a flat foot and a true rim", a_flat_foot_and_a_true_rim)
test("the joints are filleted", the_joints_are_filleted)
