# A dimensioned drawing of the pivot bracket in examples/bracket.star, as
# SVG, PDF and DXF.
#
# The part is loaded, not copied: the sheet always draws the bracket as it
# stands, and a slider moved there moves every view and every number here.
#
# Two ways to draw it:
#
# * `drawing()` is the whole pipeline in one call: it projects the solid
#   orthographically, removes the edges the solid hides (drawing them dashed
#   instead), lays the front, top and right views out in third-angle
#   projection with an isometric in the corner, fits them to the sheet at a
#   standard scale, and figures the overall sizes.
# * A sheet (`sheet_set`) is laid out by hand: views placed where they go, a
#   section through the bore, and the dimensions a shop needs written between
#   points of the model, so each number is the model's own distance.
#
# `export_svg()` and `export()` record the file the same way -- the host
# writes it, never the script.
#
#   cadpole export examples/drawing_sheet.star --run-exports --out-dir /tmp
#   cadpole export examples/drawing_sheet.star --format svg -o /tmp/sheet.svg
#
# Open /tmp/bracket_sheet.svg in a browser: it is real vector line work at real
# size, so printing it at 100 % gives a drawing you can measure.

load(
    "bracket.star",
    "bracket", "machined", "w", "d", "h", "t", "bolt", "boss_r", "bore", "bend",
    "back", "hole_x", "hole_y", "slot_len", "boss_z", "boss_len", "foot_profile",
)

# ---- The quick one: three views and the overall sizes, all automatic. The
# scale is fitted to the paper and snapped to a standard ratio, which the
# title block then states.
views = drawing(bracket, view="three-view", sheet="A4", hidden="dashed", units="mm", title="Pivot bracket", name="views")
export_svg(views, "bracket_views.svg")

# ---- The shop drawing: one A4 sheet, full size unless the sliders have made
# the part too big for it. It draws the part before its small edge rounds,
# as a shop drawing does, and says so in a note: a round that only breaks
# an edge is not a line anyone measures.
k = 1.0 if w + d <= 130 and h + d <= 120 else 0.5
shop = sheet_set(
    "pivot_bracket_set",
    size=sheet.a4,
    units="mm",
    scale="1:1" if k == 1 else "1:2",
    font="inter",
    margin=8,
    title_block=title_block(
        project="PIVOT BRACKET",
        stamp=["6061-T6", "CLEAR ANODISE"],
        notes=["Dimensions in mm. Unless noted: +/-0.2. Boss root R2, boss and bore edges R0.8, not drawn; other edges broken 0.3."],
        drawn="CADPOLE",
    ),
)
x0, y0, x1, y1 = shop.area()
page = shop.sheet("1", "PIVOT BRACKET")

# Third-angle: the plan above the front view, section A-A to the right of
# the front view. Each view's frame has its origin at the model's origin.
fx, fy = x0 + 24 + k * w / 2, y0 + 32
front = page.elevation_view(machined, direction="front", at=(fx, fy), hidden="dashed", name="front")
front.dimension((-w / 2, 0), (w / 2, 0), offset=-10)
front.dimension((w / 2, 0), (w / 2, h), offset=-8)
front.dimension((-w / 2, 0), (-w / 2, boss_z), offset=8)
front.label("Ø%s THRU" % format(bore, "g"), at=(w / 2 + 6 / k, boss_z + 14 / k), to=(bore / 2 * 0.7, boss_z + bore / 2 * 0.7))
front.label("Ø%s" % format(2 * boss_r, "g"), at=(-w / 2 - 2 / k, h + 8 / k), to=(-boss_r * 0.7, boss_z + boss_r * 0.7))
c45 = math.sqrt(0.5)
front.label("R10", at=(-w / 2 - 10 / k, h - 3 / k), to=(-w / 2 + 10 - 10 * c45, h - 10 + 10 * c45))
page.view_title("1", "FRONT", at=(fx - k * w / 2 - 14, y0 + 7), width=k * w + 24)

ty = fy + k * h + 24 + k * d / 2
top = page.elevation_view(machined, direction="top", at=(fx, ty), name="top")
top.dimension((-hole_x, -d / 2), (hole_x, -d / 2), offset=-10)
top.dimension((-w / 2, back), (-w / 2, -d / 2), offset=-8)
top.label("2X %s WIDE x %s SLOT" % (format(bolt, "g"), format(slot_len, "g")), at=(w / 2 + 4 / k, -d / 2 - 8 / k), to=(hole_x + bolt / 2, hole_y - 1))
top.label("R8", at=(-w / 2 - 6 / k, -d / 2 - 6 / k), to=(-w / 2 + 8 - 8 * c45, -d / 2 + 8 - 8 * c45))
# Where section A-A cuts, and which way it looks.
a_end = d / 2 + 3 / k
top.polyline([(0, -a_end), (0, a_end)], weight=0.25, dash="center")
for y in [-a_end, a_end]:
    top.polyline([(0, y), (-7 / k, y)], weight=0.5)
    top.polyline([(-7 / k, y + 1.2 / k), (-7 / k - 2.5 / k, y), (-7 / k, y - 1.2 / k)], weight=0.5, closed=True)
    top.text("A", at=(-7 / k - 5 / k, y), size=3.5, bold=True, align="center", valign="middle")

sx = fx + k * w / 2 + 34 + k * d / 2
cut = page.section_view(machined, plane="x=0", at=(sx, fy), hatch="lines", name="section")
cut.dimension((-d / 2, 0), (back, 0), offset=-10)
cut.dimension((-d / 2, 0), (-d / 2, t), offset=6)
cut.dimension((back - t - boss_len, h), (back, h), offset=6)
cut.label("R%s INSIDE" % format(bend, "g"), at=(-d / 2 - 2 / k, 3 * t + 10 / k), to=(back - t - bend * 0.3, t + bend * 0.3))
page.view_title("2", "SECTION A-A", at=(sx - k * d / 2 - 10, y0 + 7), width=k * d + 30)

# The part in the round, top right, at half the sheet's scale.
iso = page.axon_view(machined, az=-35, el=30, box=(sx - k * d / 2, ty - 10, x1 - 4, y1 - 4), scale="1:2" if k == 1 else "1:4", name="iso")

export(page, "bracket_sheet.svg")
export(page, "bracket_sheet.pdf")

# And the foot's outline with its slots as DXF, for a shop that wants
# geometry rather than a drawing: one layer, arcs kept as arcs.
export_dxf(foot_profile, "bracket_foot.dxf")

assert_eq(views.sheet, "A4")
assert_true(views.items > 20, msg="the sheet has line work on it")
