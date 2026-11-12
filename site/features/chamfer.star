# Chamfer: a vise jaw, its top edges cut back to a flat.
block = box(80, 14, 26, center=True, name="block")
# snippet
top = block.faces(normal=(0, 0, 1)).one()
part = chamfer(block, 2, edges=top.edges,
               name="part")
# end snippet

# The jaw's working face (y = -7): a horizontal and a vertical vee for round
# stock, and two countersunk screw holes.
vee_h = extrude(sketch(YZ).polygon([(-8, 4), (-4, 8), (-8, 12)]), 82, name="vee_h").translate((-41, 0, 0))
vee_v = extrude(sketch(XY).polygon([(-5, -8), (0, -3), (5, -8)]), 28, name="vee_v").translate((0, 0, -1))

def screw(x):
    shank = cylinder(3.2, 15).rotate(-90, axis=(1, 0, 0)).translate((x, -6, 17.5))
    sink = cone(6.5, 3.0, 3.5).rotate(-90, axis=(1, 0, 0)).translate((x, -8, 17.5))
    return union(shank, sink, name="screw%d" % (1 if x < 0 else 2))

jaw = subtract(part, vee_h, vee_v, screw(-26), screw(26), name="jaw")
jaw.material(preset="steel", color="#9fd8c8", roughness=0.3)
paint(jaw, faces=jaw.faces(name="part/face[*]"), color="#f0c987")

def jaw_checks():
    assert_solid(jaw)
    assert_eq(jaw.kind, "brep")
    flats = jaw.faces(name="part/face[*]")
    # The two ends and the back keep their flats; the front one is split by
    # the vertical vee.
    assert_eq(len(flats), 5)
    for f in flats:
        assert_close(abs(f.normal[2]), math.sqrt(0.5), tol=1e-3)
    # Two countersunk holes go right through.
    back = [e for e in jaw.edges(min_radius=3.1, max_radius=3.3) if e.center[1] > 6.9]
    assert_eq(len({int(math.sign(e.center[0])): 1 for e in back}), 2)

test("one exact jaw, its top edges chamfered at 45 degrees", jaw_checks)
