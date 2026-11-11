# Linear pattern: copies stepped along a vector.
plate = box(64, 20, 4, center=True, name="plate")
# snippet
peg = cylinder(4, 10, at=(-24, 0, 4), name="peg")
pegs = pattern_linear(peg, 5, (12, 0, 0),
                      name="pegs")
# end snippet
plate.material(color="#6fbfa8", roughness=0.4)
for p in pegs:
    p.material(color="#f0c987", roughness=0.4)
