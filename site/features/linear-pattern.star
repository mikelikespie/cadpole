# Linear pattern: copies of a body stepped along a vector. A finned heat
# sink: one fin drawn, eight more stepped 7 mm along x, all joined to the
# base plate.
base = extrude(sketch(XY).rounded_rect(72, 40, 3), 5, name="base")
mounts = [cylinder(1.7, 7, at=(x, y, -1)) for x in [-33, 33] for y in [-15, 15]]
base = subtract(base, mounts, name="plate")

# One fin: a blade 1.6 mm thick with a rounded tip, 38 mm long, standing
# 0.5 mm into the plate so the union crosses its top.
blade = sketch(XZ).rect(1.6, 22.5, center=(0, 15.75)).fillet2d(0.7, corners=[2, 3])
# snippet
fin = translate(extrude(blade, 38), (-28, 19, 0), name="fin")
fins = pattern_linear(fin, 9, (7, 0, 0), name="fins")
sink = union(base, fins, name="sink")
# end snippet
sink.material(color="#68c2ad", roughness=0.34, metallic=0.55)


# Tests: `cadpole test site/features/linear-pattern.star`.

def nine_fins_on_one_plate():
    assert_solid(sink)
    # A cut across the fins, above the plate, meets nine separate blades,
    # 7 mm apart; a cut through the plate is one piece with four holes.
    s = sink.section(XY.offset(14))
    assert_true(s.loops == 9, msg="nine fins, got %d" % s.loops)
    p = sink.section(XY.offset(2.5))
    assert_true(p.loops == 5, msg="the plate and its four mounting holes, got %d loops" % p.loops)
    lo, hi = sink.bbox()
    assert_true(abs(hi[2] - 27) < 1e-6, msg="the fins stand 22 mm above the 5 mm plate")
    return s.area()

test("nine fins stepped 7 mm, joined to the plate", nine_fins_on_one_plate)
