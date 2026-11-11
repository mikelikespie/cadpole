# Smooth blends: distance fields joined with a soft radius.
# snippet
f = field.box(34, 22, 10, round=2)
post = field.cylinder(7, 26, round=2)
post = post.translate((6, 0, 10))
f = f.smooth_union(post, 5)
part = mesh(f, resolution=0.4, simplify=False,
            name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
