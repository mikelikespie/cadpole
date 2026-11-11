# Revolve: a profile turned about an axis.
# snippet
profile = sketch(XZ).polygon(
    [(4, 0), (18, 0), (18, 4),
     (9, 8), (9, 16), (4, 16)])
part = revolve(profile, angle=270, name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
