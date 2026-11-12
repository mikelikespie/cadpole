# Twist: a field turned about its axis, then meshed.
# A three-bladed column on a round foot under a round cap: the blades
# turn 300 degrees on the way up, so the helix reads from any side.
blades = field.cylinder(4, 56)
for a in (0, 120, 240):
    blade = field.box(16, 5, 56, round=2).translate((5, 0, 0)).rotate(a)
    blades = blades.smooth_union(blade, 1.5)
blades = blades.translate((0, 0, 34))
foot = field.cylinder(16, 6, round=1.5).translate((0, 0, 3))
cap = field.cylinder(14, 5, round=1.5).translate((0, 0, 64.5))
# snippet
f = blades.smooth_union(foot, 2).smooth_union(cap, 2)
f = f.twist(5.0)  # degrees per mm, about Z
part = mesh(f, resolution=0.35, simplify=False,
            name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.35)


def one_turned_column():
    assert_solid(part)
    assert_bbox(part, (-16, -16, 0), (16, 16, 67), tol=0.2)


def the_blades_turn_with_height():
    # A blade reaches 13 mm from the axis. Untwisted, one would point
    # along +x at every height; turned 5 degrees per mm, one points along
    # +x only where the turn is a multiple of 120 degrees (z = 24, 48)
    # and none does halfway between (z = 36).
    def solid_at(x, z):
        return distance(part, sphere(0.3, center=(x, 0, z))).overlap

    assert_true(solid_at(11.5, 24), msg="at z=24 (120 degrees) a blade points along +x")
    assert_true(solid_at(11.5, 48), msg="at z=48 (240 degrees) a blade points along +x")
    assert_true(not solid_at(11.5, 36), msg="at z=36 (180 degrees) none does")


test("one turned column", one_turned_column)
test("the blades turn with height", the_blades_turn_with_height)
