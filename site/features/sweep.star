# Sweep: a section carried along a path.
k = 20 * math.sqrt(0.5)
mid = (20 - k, 0, 20 + k)  # a point on the bend
# snippet
route = curve((0, 0, 0)).line((0, 0, 20))
route = route.arc((20, 0, 40), via=mid)
route = route.line((40, 0, 40))
ring = sketch(XY).circle(6).circle(4.5)
part = sweep_along(ring, route, name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
