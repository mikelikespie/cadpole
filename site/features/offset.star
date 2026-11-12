# Offset: a sketch grown or shrunk by a fixed distance.
# A tray from one L-shaped outline: grown 3 mm for the wall, shrunk for a
# groove in the floor. Outward, the convex corners come out round and the
# inner corner stays sharp; inward it is the other way about.
L = [(-20, -12), (20, -12), (20, 0), (4, 0), (4, 12), (-20, 12)]
shape = sketch(XY).polygon(L)
# snippet
wall = shape.offset(3).subtract(shape)          # grow
rim = extrude(wall, 10, name="rim")
inset = sketch(XY.offset(1.8)).polygon(L)
groove = inset.offset(-2).subtract(inset.offset(-3.4))   # shrink
# end snippet
slab = extrude(shape, 3, name="slab")
floor = subtract(slab, extrude(groove, 2), name="floor")
floor.material(color="#6fbfa8", roughness=0.4)
rim.material(color="#f0c987", roughness=0.4)

def grown_and_shrunk():
    assert_solid(floor)
    assert_solid(rim)
    # The wall is the 3 mm band round the outline: its area is the
    # outline's length times 3 plus the round corners' quarter discs.
    assert_close(wall.area(), shape.offset(3).area() - shape.area(), rel=1e-6)
    assert_bbox(rim, (-23, -15, 0), (23, 15, 10), tol=0.01)
    # The groove is cut 1.2 mm into the floor, 2 mm in from the wall.
    assert_close(slab.volume() - floor.volume(), groove.area() * 1.2, rel=0.01)
    gap = distance(floor, rim)
    assert_true(not gap.overlap, msg="the floor sits inside the wall without overlapping it")

test("the wall grows the outline, the groove shrinks it", grown_and_shrunk)
