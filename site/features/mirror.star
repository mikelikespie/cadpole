# Mirror: a reflected twin across a plane.
arm = sketch(XY).polygon([(4, -12), (30, -12), (30, -4), (14, 12), (4, 12)])
half = extrude(arm.circle(3, center=(22, -6)), 8, name="half")
# snippet
both = mirror(half, plane=YZ, merge=False)
twin = both[1]
# end snippet
half.material(color="#6fbfa8", roughness=0.4)
twin.material(color="#f0c987", roughness=0.4)
