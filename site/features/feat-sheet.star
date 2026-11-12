# Dimensioned sheet: front, top, right and a pictorial view of the pillow
# block on one letter-size page, dimensioned in inches, with the bore and
# slots called out and their centre lines drawn.
load("../../examples/pillow_block.star", "block", "bore_d", "centre_h", "depth", "foot_t", "length", "slot_l", "slot_w", "slot_x")
inch = 25.4
# snippet
set = sheet_set("Pillow block PB-075", size=sheet.letter, units="in",
                title_block=title_block(project="PILLOW BLOCK PB-075"))
page = set.sheet("D1", "PILLOW BLOCK", scale=1)
front = page.elevation_view(block, direction="front", at=(84, 58),
                            hidden="dashed")
front.dimension((-2 * inch, 0), (2 * inch, 0), offset=-20)
export(set, "pillow_block.pdf")
export(set, "pillow_block.dxf")
# end snippet
x0, y0, x1, y1 = set.area()
half, axis, r, d2 = length / 2 * inch, centre_h * inch, bore_d / 2 * inch, depth / 2 * inch
sx, foot = slot_x * inch, foot_t * inch
top = page.elevation_view(block, direction="top", at=(84, 140), hidden="dashed")
right = page.elevation_view(block, direction="right", at=(170, 58), hidden="dashed")
page.axon_view(block, az=-35, el=28, scale=0.6, box=(126, 110, x1 - 2, y1 - 4))

def inches(x):
    # A length as the drawing writes it: sixteenths, reduced (3/4, 1 3/8).
    n = int(math.round(x * 16))
    whole, rest, den = n // 16, n % 16, 16
    for _ in range(4):
        if rest > 0 and rest % 2 == 0:
            rest, den = rest // 2, den // 2
    if rest == 0:
        return "%d\"" % whole
    return ("%d " % whole if whole else "") + "%d/%d\"" % (rest, den)

def centre_cross(view, x, y, a, b):
    view.polyline([(x - a, y), (x + a, y)], dash="center", weight=0.18)
    view.polyline([(x, y - b), (x, y + b)], dash="center", weight=0.18)

# FRONT: the overall sizes, the slots' pitch, the bore's height and size.
centre_cross(front, 0, axis, r + 5, r + 5)
for x in [-sx, sx]:
    front.polyline([(x, -3), (x, foot + 3)], dash="center", weight=0.18)
front.dimension((-sx, 0), (sx, 0), offset=-9)
front.dimension((-half, 0), (-half, axis), offset=9)
front.dimension((-half, 0), (-half, 2 * inch), offset=20)
front.label("Ø" + inches(bore_d) + " BORE", at=(0.95 * inch, 2.05 * inch), to=(r * 0.7071, axis + r * 0.7071))

# TOP: the depth, and the two slots.
top.polyline([(0, -d2 - 4), (0, d2 + 4)], dash="center", weight=0.18)
for x in [-sx, sx]:
    centre_cross(top, x, 0, slot_l * inch / 2 + 3, slot_w * inch / 2 + 3)
top.dimension((-half, -d2), (-half, d2), offset=9)
top.label("2X SLOT " + format(slot_w, ".2f") + " x " + format(slot_l, ".2f"), at=(half - 0.9 * inch, d2 + 0.32 * inch), to=(sx + 0.1 * inch, slot_w * inch / 2))

# RIGHT: the depth and the foot's thickness, the bore's axis.
right.polyline([(-d2 - 4, axis), (d2 + 4, axis)], dash="center", weight=0.18)
right.dimension((-d2, 0), (d2, 0), offset=-9)
right.dimension((d2, 0), (d2, foot), offset=-8)


# The views' names, under each orthographic view and over the top one.
page.text("FRONT", at=(84, 26), size=2.5, align="center")
page.text("RIGHT", at=(170, 26), size=2.5, align="center")
page.text("TOP", at=(84 - 1.2 * inch, 140 + d2 + 5), size=2.5, align="center")


# Tests: `cadpole test site/features/feat-sheet.star`.

def the_sheet_is_dimensioned():
    assert_true(set.sheets() == ["D1"], msg="one sheet")
    assert_true(inches(1.375) == "1 3/8\"" and inches(0.75) == "3/4\"" and inches(4.0) == "4\"", msg="fractions reduce")
    # The views do not run into each other or the title column.
    for v in [front, top, right]:
        lo_x, lo_y, hi_x, hi_y = v.bounds
        assert_true(lo_x >= x0 and hi_x <= x1 and lo_y >= y0 and hi_y <= y1, msg="a view inside the drawing area")
    assert_true(front.bounds[3] < top.bounds[1], msg="the top view clears the front view")
    assert_true(front.bounds[2] < right.bounds[0], msg="the right view clears the front view")
    return page.items()

test("three views and a pictorial, dimensioned, inside the frame", the_sheet_is_dimensioned)
