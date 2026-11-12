# Circular pattern: a weld-neck flange whose eight bolt holes are one hole
# turned round the axis.
bore = 8
pcd = 22.5        # bolt circle radius
# The half section, revolved: raised face, flange, tapered neck.
half = sketch(XZ, name="section").polygon([
    (bore, 0), (16, 0), (16, 1.5), (30, 1.5), (30, 8), (15, 8),
    (11, 22), (bore, 22),
]).fillet2d(3, corners=["corner[5]"]).chamfer2d(0.8, corners=["corner[0]", "corner[1]", "corner[3]", "corner[4]", "corner[7]", "corner[8]"])
flange = revolve(half, axis=(0, 0, 1), name="flange")
hole = cylinder(3.2, 10, at=(pcd, 0, -0.5), name="hole")
# snippet
holes = pattern_circular(hole, 8, axis=(0, 0, 1))
part = subtract(flange, holes, name="part")
# end snippet
part.material(preset="steel", color="#9fd8c8", roughness=0.3)

def exact_flange():
    assert_solid(part)
    assert_eq(part.kind, "brep")
    rims = part.edges(on_plane=XY.offset(8), min_radius=3.1, max_radius=3.3)
    for e in rims:
        assert_close(math.hypot(e.center[0], e.center[1]), pcd, tol=0.1)
    # One hole every 45 degrees round the bolt circle.
    slots = {int(math.round(math.atan2(e.center[1], e.center[0]) / 45)) % 8: 1 for e in rims}
    assert_eq(len(slots), 8)

test("an exact flange with eight bolt holes", exact_flange)
