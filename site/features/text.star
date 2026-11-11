# Text: shaped type as a sketch, extruded like any outline.
tag = extrude(sketch(XY).rounded_rect(56, 22, 5), 4, name="tag")
# snippet
label = text("CADPOLE", size=9, weight=700,
             align="center", baseline="middle",
             plane=XY.offset(4))
letters = extrude(label, 1.2, name="letters")
# end snippet
tag.material(color="#6fbfa8", roughness=0.4)
for g in letters:
    g.material(color="#f0c987", roughness=0.4)
