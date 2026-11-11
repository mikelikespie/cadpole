# Offset: a sketch grown or shrunk by a fixed distance.
shape = sketch(XY).polygon(
    [(-20, -12), (20, -12), (20, 0), (4, 0), (4, 12), (-20, 12)])
# snippet
outer = shape.offset(3)
wall = outer.subtract(shape)
rim = extrude(wall, 10, name="rim")
# end snippet
floor = extrude(shape, 3, name="floor")
floor.material(color="#6fbfa8", roughness=0.4)
rim.material(color="#f0c987", roughness=0.4)
