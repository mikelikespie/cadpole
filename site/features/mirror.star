# Mirror: a reflected twin across a plane.
# One half of a gusseted angle bracket, modelled up to the YZ plane; its
# mirror image completes it. The big hole in the upright is half drawn on
# each side, so it only becomes round when the two halves meet.
# The L of foot and upright drawn once and run 30 mm out from the plane.
angle = extrude(sketch(YZ).polygon(
    [(-18, 0), (12, 0), (12, 36), (8, 36), (8, 4), (-18, 4)]), 30, name="angle")
angle = fillet(angle, 5, edges=angle.edges(direction=(0, 0, 1), nearest=(30, -18, 2)))
angle = fillet(angle, 6, edges=angle.edges(direction=(0, 1, 0), nearest=(30, 10, 36)))
gusset = extrude(sketch(YZ.offset(25)).polygon([(8.5, 3.5), (-12, 3.5), (8.5, 24)]), 3.5, name="gusset")
half = union(angle, gusset, name="frame")
# Through the foot a slot; through the upright (XZ faces -y, so the plane
# at y = 14 cuts back through it) a screw hole and half of the big bore.
slot = extrude(sketch(XY.offset(-1)).slot(11, 5.4, center=(14, -9)), 6)
holes = extrude(sketch(XZ.offset(-14)).circle(7, center=(0, 20))
                .circle(2.7, center=(15, 27)), 8)
half = subtract(subtract(half, slot), holes, name="half")
# snippet
both = mirror(half, plane=YZ, merge=False)
twin = both[1]
# end snippet
half.material(color="#6fbfa8", roughness=0.4)
twin.material(color="#f0c987", roughness=0.4)

def mirrored_halves():
    assert_solid(half)
    assert_solid(twin)
    lo, hi = half.bbox()
    assert_close(lo[0], 0.0, tol=1e-3, msg="the half ends on the mirror plane")
    tlo, thi = twin.bbox()
    assert_close(thi[0], 0.0, tol=1e-3, msg="the twin ends on the mirror plane")
    assert_close(tlo[0], -hi[0], tol=1e-3, msg="the twin is the half reflected")
    assert_close(twin.volume(), half.volume(), rel=1e-3)
    gap = distance(half, twin)
    assert_true(not gap.overlap, msg="the halves meet face to face without overlapping")

test("two mirrored halves that meet on the plane", mirrored_halves)
