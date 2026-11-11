# Twist: a field turned about its axis, then meshed.
# snippet
f = field.box(20, 20, 44, round=3)
f = f.twist(2.0)
part = mesh(f, resolution=0.5, simplify=False,
            name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
