# Section: the pillow block cut across its bore, the cut face hatched, the
# far half of the housing drawn beyond it, dimensioned between points of the
# model.
load("../../examples/pillow_block.star", "block", "bore_d", "centre_h", "foot_t", "length", "slot_x")
inch = 25.4
set = sheet_set("Pillow block PB-075", size=(220, 165), units="in")
page = set.sheet("S1", "SECTION A-A", scale=1)
x0, y0, x1, y1 = set.area()
# snippet
cut = page.section_view(block, plane="y=0",
                        at=((x0 + x1) / 2 + 8, y0 + 42))
cut.dimension((-2 * inch, 0), (2 * inch, 0), offset=-16)
# end snippet
half, axis, r = length / 2 * inch, centre_h * inch, bore_d / 2 * inch
# The bore's centre lines and the slots' centre lines, chain-dashed.
cut.polyline([(-r - 6, axis), (r + 6, axis)], dash="center", weight=0.2)
cut.polyline([(0, axis - r - 6), (0, axis + r + 6)], dash="center", weight=0.2)
for x in [-slot_x * inch, slot_x * inch]:
    cut.polyline([(x, -4), (x, foot_t * inch + 4)], dash="center", weight=0.2)
cut.dimension((-slot_x * inch, 0), (slot_x * inch, 0), offset=-6)
cut.dimension((-r, axis), (r, axis), offset=2 * inch - axis + 8)
cut.dimension((half, 0), (half, foot_t * inch), offset=-8)
cut.dimension((-half, 0), (-half, axis), offset=8)
cut.dimension((-half, 0), (-half, 2 * inch), offset=20)


# Tests: `cadpole test site/features/feat-section.star`.

def the_cut_crosses_the_bore_and_slots():
    s = block.section(XZ)
    # The cut at y = 0 crosses both slots, so the foot's two ends are cut
    # off from the body: three pieces, the bore a hole in the middle one.
    assert_true(s.loops == 4, msg="three pieces and the bore, got %d loops" % s.loops)
    assert_true(page.items() > 0, msg="the sheet holds the view")
    return s.area()

test("section A-A crosses the bore and both slots", the_cut_crosses_the_bore_and_slots)
