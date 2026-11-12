# Drawing: a dimensioned three-view sheet with hidden lines dashed.
inch = 25.4
plate = extrude(sketch(XY).rounded_rect(3 * inch, 2 * inch, 0.25 * inch),
                0.5 * inch, name="plate")
# The boss starts inside the plate, so the union is a clean crossing.
boss = cylinder(0.625 * inch, 1.0 * inch, at=(0, 0, 0.25 * inch), name="boss")
bore = cylinder(0.375 * inch, 2 * inch, at=(0, 0, -5), name="bore")
# Four mounting holes at the corners.
holes = [cylinder(0.14 * inch, inch, at=(sx * 1.125 * inch, sy * 0.625 * inch, -5))
         for sx in (-1, 1) for sy in (-1, 1)]
part = subtract(union(plate, boss), bore, holes, name="part")
part.material(color="#6fbfa8", roughness=0.4)
# snippet
sheet = drawing(part, view="three-view", sheet="A4",
                units="in", title="Flanged boss")
export_pdf(sheet, "flanged_boss.pdf")
export_dxf(sheet, "flanged_boss.dxf")
# end snippet

def one_exact_part():
    assert_solid(part)
    assert_eq(part.kind, "brep")
    assert_bbox(part, (-1.5 * inch, -inch, 0), (1.5 * inch, inch, 1.25 * inch))

test("one exact flanged boss, bored through", one_exact_part)
