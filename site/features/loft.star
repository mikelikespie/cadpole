# Loft: one solid through several sections.
# A vase that starts round at the foot, pinches at the waist and opens into a
# soft square mouth: one smooth skin through all the sections.
# snippet
sections = [
    sketch(XY).rounded_rect(30, 30, 14),               # round foot
    sketch(XY.offset(4)).rounded_rect(24, 24, 11),
    sketch(XY.offset(17)).rounded_rect(18, 18, 7.5),   # waist
    sketch(XY.offset(31)).rounded_rect(27, 27, 8),
    sketch(XY.offset(39)).rounded_rect(30, 30, 7),     # square mouth
    sketch(XY.offset(43)).rounded_rect(30, 30, 7),
]
skin = loft(sections, ruled=False, name="skin")
# end snippet

# The rim is a level cut through the skin at 40 mm. The inside is the same
# loft through every section drawn `wall` in, so the wall keeps its
# thickness all the way round, cut off above a weighted 5 mm floor.
height, wall, floor = 40, 1.6, 5
vase = intersect(skin, box(60, 60, height, center=True), name="vase")
inside = loft([s.offset(-wall) for s in sections], ruled=False, name="inside")
inside = intersect(inside, box(60, 60, 60, center=True, at=(0, 0, floor)), name="cavity")
part = subtract(vase, inside, name="part")
part.tessellation(tolerance=0.01, angle=4)
part.material(color="#6fbfa8", roughness=0.35)

def one_hollow_solid():
    assert_solid(part)
    assert_bbox(part, (-15, -15, 0), (15, 15, height), tol=0.3)
    assert_true(part.volume() < 0.5 * vase.volume(), msg="the vase is hollow")

def even_wall():
    # At every height above the floor the inside stays at least 1.4 mm in
    # from the outside, measured level.
    for z in [floor + 0.5, 8, 11, 14, 17, 20, 23, 26, 29, 32, 35, 38, height - 0.1]:
        outer = vase.section(XY.offset(z))
        hole = inside.section(XY.offset(z))
        kept = outer.offset(-1.4).intersect(hole).area()
        assert_close(kept, hole.area(), rel=0.002, msg="wall under 1.4 mm at z=%s" % z)

test("one hollow solid, open at the top", one_hollow_solid)
test("the wall is at least 1.4 mm all the way up", even_wall)
