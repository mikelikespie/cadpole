# Extrude: a flat outline pulled up into a solid.
# snippet
outline = sketch(XY).rounded_rect(40, 24, 5)
outline = outline.circle(5)
part = extrude(outline, 10, name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
