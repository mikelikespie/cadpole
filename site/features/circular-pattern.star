# Circular pattern: copies spread round an axis.
disc = cylinder(24, 6, name="disc")
hole = cylinder(3.5, 8, at=(16, 0, -1), name="hole")
# snippet
holes = pattern_circular(hole, 8, axis=(0, 0, 1))
part = subtract(disc, holes, name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
