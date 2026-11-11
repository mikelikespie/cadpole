# Booleans: join, cut and keep-the-overlap, side by side.
def pair(x):
    a = box(20, 20, 10, center=True, at=(x, 0, 0))
    b = cylinder(9, 16, at=(x + 6, -6, 0))
    return a, b

a1, b1 = pair(-30)
a2, b2 = pair(0)
a3, b3 = pair(30)
# snippet
joined = union(a1, b1, name="joined")
cut = subtract(a2, b2, name="cut")
overlap = intersect(a3, b3, name="overlap")
# end snippet
joined.material(color="#6fbfa8", roughness=0.4)
cut.material(color="#6fbfa8", roughness=0.4)
overlap.material(color="#f0c987", roughness=0.4)
