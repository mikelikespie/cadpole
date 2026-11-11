# Emboss: artwork wrapped onto a curved face and raised.
cup = cylinder(16, 26, name="cup")
side = cup.faces(kind=kind.cylinder).nearest((0, -16, 13))
# snippet
art = text("42", size=14, weight=800,
           align="center", baseline="middle")
part = emboss(cup, side, art, 1.2, wrap=wrap.unroll,
              origin=(0, -16, 15), up=(0, 0, 1))
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
