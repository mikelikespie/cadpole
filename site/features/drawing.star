# Drawing: a dimensioned three-view sheet with hidden lines dashed.
inch = 25.4
plate = extrude(sketch(XY).rounded_rect(3 * inch, 2 * inch, 0.25 * inch),
                0.5 * inch, name="plate")
boss = cylinder(0.625 * inch, 1.25 * inch, name="boss")
bore = cylinder(0.375 * inch, 2 * inch, at=(0, 0, -5), name="bore")
part = subtract(union(plate, boss), bore, name="part")
part.material(color="#6fbfa8", roughness=0.4)
# snippet
sheet = drawing(part, view="three-view", sheet="A4",
                units="in", title="Flanged boss")
export_pdf(sheet, "flanged_boss.pdf")
export_dxf(sheet, "flanged_boss.dxf")
# end snippet
