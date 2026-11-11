# Loft: one solid through several sections.
# snippet
base = sketch(XY).rounded_rect(36, 36, 6)
neck = sketch(XY.offset(14)).rounded_rect(20, 20, 6)
top = sketch(XY.offset(28)).rounded_rect(30, 30, 10)
part = loft([base, neck, top], name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
