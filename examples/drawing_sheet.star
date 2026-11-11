# A dimensioned three-view sheet of the bracket, exported as SVG.
#
# `drawing()` is the 2D pipeline: it projects the solid orthographically,
# removes the edges the solid hides (drawing them dashed instead), lays the
# front, top and right views out in third-angle projection with an isometric in
# the corner, fits them to the sheet at a standard scale, and figures the
# overall sizes. `export_svg()` records the file the same way `export()` does —
# the host writes it, never the script.
#
#   cadpole export examples/drawing_sheet.star --run-exports --out-dir /tmp
#   cadpole export examples/drawing_sheet.star --format svg -o /tmp/sheet.svg
#   cadpole render examples/drawing_sheet.star -o /tmp/bracket.png
#
# Open /tmp/sheet.svg in a browser: it is real vector line work at real size,
# so printing it at 100 % gives a drawing you can measure.

w = param("width", 60, unit="mm", min=30, max=120, doc="Plate width (x)")
d = param("depth", 30, unit="mm", min=20, max=80, doc="Plate depth (y)")
t = param("thickness", 6, unit="mm", min=3, max=12)
bolt = param("bolt_d", 5.5, unit="mm", min=3, max=10, doc="Bolt hole diameter")
boss_r = param("boss_r", 8, unit="mm", min=4, max=15)

plate = box(w, d, t, center=True, name="plate")
boss = cylinder(boss_r, 11, at=(0, 0, t - 1), name="boss")
body = union(plate, boss, name="body")

holes = [
    cylinder(bolt / 2, t + 2, at=(-w / 2 + 8, 0, -1), name="hole_left"),
    cylinder(bolt / 2, t + 2, at=(w / 2 - 8, 0, -1), name="hole_right"),
]
bracket = subtract(body, holes[0], holes[1], name="bracket")
bracket.material(color="#7fa6d9", roughness=0.5)

# The sheet. `scale=None` (the default) fits the part to the paper and snaps to
# a standard ratio, which the title block then states.
sheet = drawing(
    bracket,
    view="three-view",
    sheet="A4",
    hidden="dashed",
    units="mm",
    title="Bracket",
    name="sheet",
)
export_svg(sheet, "bracket_sheet.svg")
export_pdf(sheet, "bracket_sheet.pdf")

# A section through the boss, to show the wall under it. `section:` cuts with a
# plane and hatches the cut face.
cut = drawing(bracket, view="section:y=0", sheet="A4", title="Bracket - section at y=0", name="cut")
export_svg(cut, "bracket_section.svg")

# And the flat pattern of the plate outline as DXF, for a shop that wants
# geometry rather than a drawing: one layer, arcs kept as arcs.
outline = sketch(XY).rect(w, d).circle(bolt / 2, center=(-w / 2 + 8, 0)).circle(bolt / 2, center=(w / 2 - 8, 0))
export_dxf(outline, "bracket_plate.dxf")

assert_eq(sheet.sheet, "A4")
assert_true(sheet.items > 20, msg="the sheet has line work on it")
