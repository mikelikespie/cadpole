# A solid-cherry bedside table: top slab, one drawer, an open shelf, four
# square legs with the case panels set between them.
#
# **This is furniture, not a printable part.** Nothing here is checked against
# a nozzle or a bed; the units are still millimetres and the parts are still
# the parts a cabinetmaker would cut. It is in `examples/` to show the API
# doing something that is not a 3D-printing fixture -- eleven separate solid
# bodies that touch and never boolean, a drawer that slides, and a cut list
# that exports to STEP.
#
# What this example demonstrates:
#
#   * **`loft` as an edge treatment.** Every board a hand would touch is a
#     four-section loft -- a rect inset by `ease`, two full rects, a rect
#     inset again -- so its two long edges carry a 45-degree arris instead of
#     a raw corner. It stays an exact B-rep body, it costs four sketches and
#     no boolean, and it is the same helper for a horizontal slab and for a
#     vertical drawer front because the helper takes the *plane*;
#   * **`plane(origin=, normal=, x_dir=)` to build a board on edge.** The
#     drawer front is lofted along -Y, so its sketch plane's own y axis has
#     to be world +Z: `normal=(0,-1,0)` with `x_dir=(1,0,0)` gives
#     `y_dir = normal x x_dir = (0,0,1)`, and the slab then grows forward out
#     of the case front;
#   * **an assembly of bodies that merely touch.** A leg, a side panel and the
#     top are three solids sharing faces. Union them and the exact kernel
#     refuses the coplanar contact; leave them as bodies and
#     you have what the piece actually is -- eleven parts, each one a board,
#     each one solid, each one exportable on its own;
#   * **a moving part.** `drawer_open` slides the front, the tray and the pull
#     forward together, because all three are built relative to one
#     `drawer_y` and translated by one vector. Sweep it to see the drawer run;
#   * **assertions that are joinery.** The reveal round the drawer front, the
#     shelf clearing the drawer, and the pull standing proud of the front are
#     each measured off the bodies' own bounding boxes rather than asserted
#     from the arithmetic that placed them.
#
#   cadpole render examples/nightstand.star --backend path --samples 96 \
#       --size 1200x900 --view az=30,el=18 --environment -o /tmp/nightstand.png
#   cadpole render examples/nightstand.star --param drawer_open=180 -o /tmp/open.png
#   cadpole check  examples/nightstand.star

width = param("width", 440, unit="mm", min=300, max=700, doc="Over the legs, left to right")
depth = param("depth", 400, unit="mm", min=250, max=600, doc="Over the legs, front to back")
height = param("height", 620, unit="mm", min=400, max=900, doc="Floor to the top surface")

top_t = param("top_thickness", 22, unit="mm", min=12, max=40, doc="Thickness of the top slab")
overhang = param("overhang", 10, unit="mm", min=0, max=40, doc="How far the top stands out past the legs, all four sides")
leg = param("leg", 35, unit="mm", min=20, max=60, doc="Square section of a leg")
panel = param("panel", 18, unit="mm", min=10, max=30, doc="Side and shelf board thickness")
back_t = param("back_thickness", 12, unit="mm", min=6, max=25, doc="Back panel thickness")
proud = param("leg_proud", 6, unit="mm", min=0, max=20, doc="How far the legs stand out past the case panels")

drawer_h = param("drawer_height", 120, unit="mm", min=60, max=250, doc="Height of the drawer front")
reveal = param("reveal", 3, unit="mm", min=0.5, max=10, doc="Gap all round the drawer front")
drawer_open = param("drawer_open", 0, unit="mm", min=0, max=300, doc="How far the drawer is pulled out")

shelf_z = param("shelf_height", 140, unit="mm", min=40, max=400, doc="Underside of the open shelf, above the floor")
shelf_t = param("shelf_thickness", 20, unit="mm", min=10, max=30)

pull_len = param("pull_length", 100, unit="mm", min=40, max=400, doc="Length of the bar pull, about 0.26 of the drawer front")
pull_h = param("pull_height", 28, unit="mm", min=10, max=60)
pull_t = param("pull_thickness", 18, unit="mm", min=8, max=30, doc="How thick the bar is, front to back")
standoff = param("standoff", 10, unit="mm", min=4, max=30, doc="Gap between the bar and the drawer front")

ease = param("ease", 1.5, unit="mm", min=0, max=6, doc="45-degree arris on the top, the shelf and the drawer front")

# --- the boards this is built from -------------------------------------------
case_top = height - top_t          # the plane every case part stops at
shelf_top = shelf_z + shelf_t
drawer_top = case_top - reveal
drawer_bot = drawer_top - drawer_h
case_front = proud                 # the plane the drawer front and shelf sit in
case_back = depth - proud - back_t

assert_true(2 * leg + 4 * reveal < width, msg="the legs leave no room for a drawer front")
assert_true(shelf_top + 40 < drawer_bot, msg="the shelf and the drawer would collide: lower `shelf_height`")
assert_true(2 * ease < min(top_t, shelf_t), msg="the arris is more than half the board's thickness")
assert_true(proud + panel < leg, msg="a side panel set into a leg has to be thinner than the leg is deep")


def cherry(body):
    """One material for every board, so the piece reads as one tree.

    Deliberately **not** `texture="wood"`. That texture's growth rings run
    around Z and nothing in the API turns them (the pattern is a
    pure function of world millimetres, with no direction argument), so on a
    horizontal top slab they come out as concentric circles -- end grain, on
    the one face of this piece whose grain most obviously runs along its
    length. `noise` at a 70 mm blob is the honest substitute: figure without
    a direction, which is the most this material model can say about a board.
    """
    return body.material(color="#6e3a23", roughness=0.44, texture="noise", scale=70)


def eased_slab(base, w, h, t, name):
    """A board `w` x `h` in `base`'s own axes and `t` thick along its normal,
    with a 45-degree arris of `ease` on both `t`-ends.

    Four rect sections and one `loft`: the two inset rects make the chamfers
    and the two full ones make the faces between them. Rects are 4-segment
    loops, so the sections pair segment to segment in construction order and
    every chamfer band comes out an exact plane. With `ease = 0` there is no
    arris: the two inset sections would sit on the full ones, a band of no
    height, so the board is the loft of the two full rects alone.
    """
    stations = [(0.0, ease), (ease, 0.0), (t - ease, 0.0), (t, ease)] if ease > 0 else [(0.0, 0.0), (t, 0.0)]
    secs = []
    for at, inset in stations:
        secs.append(sketch(base.offset(at)).rect(w - 2.0 * inset, h - 2.0 * inset))
    return cherry(loft(secs, name=name))


def flat_board(w, d, t, at, name):
    """A horizontal board: `eased_slab` on an XY plane, positioned by its
    minimum corner the way `box(..., at=)` is."""
    base = plane(origin=(at[0] + w / 2.0, at[1] + d / 2.0, at[2]), normal=(0, 0, 1), x_dir=(1, 0, 0))
    return eased_slab(base, w, d, t, name)


# --- the top ------------------------------------------------------------------
# A slab standing `overhang` proud on
# every side, with a softened arris top and bottom.
top = flat_board(width + 2 * overhang, depth + 2 * overhang, top_t,
                 (-overhang, -overhang, case_top), "top")

# --- the legs -----------------------------------------------------------------
# Square section, full height, stopping under the top. Plain boxes: a leg's
# edges are crisp.
leg_h = case_top
legs = []
for i, (x, y) in enumerate([(0.0, 0.0), (width - leg, 0.0), (0.0, depth - leg), (width - leg, depth - leg)]):
    legs.append(cherry(box(leg, leg, leg_h, at=(x, y, 0), name="leg_%d" % i)))
assert_eq(len(legs), 4)

# --- the case -----------------------------------------------------------------
# Side panels *between* the legs, not lapped over them: each one runs from the
# back of the front leg to the front of the back leg, so it shares no volume
# with any leg and the whole case is a set of bodies that merely touch. Their
# outer face is `proud` inside the leg's, which is the shadow line that makes
# a leg read as a leg.
case_h = case_top - shelf_z
sides = [
    cherry(box(panel, depth - 2 * leg, case_h, at=(proud, leg, shelf_z), name="side_left")),
    cherry(box(panel, depth - 2 * leg, case_h, at=(width - proud - panel, leg, shelf_z), name="side_right")),
]
back = cherry(box(width - 2 * leg, back_t, case_h, at=(leg, case_back, shelf_z), name="back"))

# The shelf spans between the side panels and is flush with the case front.
shelf = flat_board(width - 2 * (proud + panel), case_back - case_front, shelf_t,
                   (proud + panel, case_front, shelf_z), "shelf")

# --- the drawer ---------------------------------------------------------------
# The front is a board on edge. Its sketch plane faces forward (-Y) so that
# `offset()` grows it out of the case front, and its own y axis is world +Z:
# `normal x x_dir = (0,-1,0) x (1,0,0) = (0,0,1)`.
front_t = panel
front_w = width - 2 * leg - 2 * reveal
front_face = plane(origin=((width) / 2.0, case_front + front_t, (drawer_top + drawer_bot) / 2.0),
                   normal=(0, -1, 0), x_dir=(1, 0, 0))
drawer_front = eased_slab(front_face, front_w, drawer_h, front_t, "drawer_front")

# The box behind it: one subtract, the only boolean in the file. The cutter is
# taller than the tray so the top face opens completely rather than meeting it
# flush, which is the contact the exact kernel refuses.
tray_w = front_w - 60
tray_d = case_back - case_front - front_t - 20
tray_h = drawer_h - 18
tray_wall = 12
tray_floor = 10
tray_x = (width - tray_w) / 2.0
tray_y = case_front + front_t
tray_z = drawer_bot + 9
tray = subtract(
    box(tray_w, tray_d, tray_h, at=(tray_x, tray_y, tray_z), name="tray_outer"),
    box(tray_w - 2 * tray_wall, tray_d - 2 * tray_wall, tray_h,
        at=(tray_x + tray_wall, tray_y + tray_wall, tray_z + tray_floor), name="tray_inner"),
    name="drawer_box",
)
assert_solid(tray, msg="the drawer box must be a closed solid")
cherry(tray)

# The pull: a flat bar held off the front on two stand-offs, with a shadow
# gap behind the bar and the stand-offs visible through it.
pull_y = case_front - standoff - pull_t
pull_cz = (drawer_top + drawer_bot) / 2.0
pull = cherry(box(pull_len, pull_t, pull_h,
                  at=((width - pull_len) / 2.0, pull_y, pull_cz - pull_h / 2.0), name="pull_bar"))
boss = min(22.0, pull_len / 3.0)
standoffs = []
for i, sx in enumerate([(width - pull_len) / 2.0 + 14.0, (width + pull_len) / 2.0 - 14.0 - boss]):
    standoffs.append(cherry(box(boss, standoff, boss,
                                at=(sx, case_front - standoff, pull_cz - boss / 2.0),
                                name="pull_boss_%d" % i)))

# One vector moves every part of the drawer, which is what makes `drawer_open`
# a single parameter rather than five.
drawer = [drawer_front, tray, pull] + standoffs
if drawer_open > 0:
    drawer = [translate(b, (0, -drawer_open, 0), name="pull_out_%d" % i) for i, b in enumerate(drawer)]

# --- checks ---------------------------------------------------------------------
# Measured off the bodies, not off the arithmetic that placed them.
lo, hi = top.bbox()
assert_true(math.close(hi[2], height, tol=0.01), msg="the top surface is `height` above the floor")
assert_true(math.close(hi[0] - lo[0], width + 2 * overhang, tol=0.01), msg="the top overhangs both sides")
assert_true(math.close(lo[1], -overhang, tol=0.01), msg="and the front")

dlo, dhi = drawer[0].bbox()
llo, lhi = legs[0].bbox()
assert_true(math.close(dlo[0] - lhi[0], reveal, tol=0.01), msg="`reveal` of daylight between the drawer front and the leg")
assert_true(math.close(case_top - dhi[2], reveal, tol=0.01), msg="and the same under the top")

slo, shi = shelf.bbox()
assert_true(shi[2] < dlo[2], msg="the shelf sits below the drawer")
assert_true(math.close(shi[1], case_back, tol=0.01), msg="the shelf reaches the back panel")

plo, phi = pull.bbox()
assert_true(phi[1] < dlo[1] - standoff + 0.01, msg="the bar stands off the drawer front, it does not sit on it")
assert_true(math.close(phi[0] - plo[0], pull_len, tol=0.01), msg="the bar is `pull_length` long")
assert_true(phi[0] - plo[0] < 0.4 * (dhi[0] - dlo[0]), msg="a bar pull, not a full-width rail")

# Every board, in one list, so a scene script can `load()` this file and put
# something on top of it. `TOP_HEIGHT` is the surface things stand on.
NIGHTSTAND = legs + sides + [back, shelf, top] + drawer
TOP_HEIGHT = height
TOP_WIDTH = width + 2 * overhang
TOP_DEPTH = depth + 2 * overhang

# A cut list a machine can read: every board as an exact B-rep solid. STEP
# rather than STL, because nothing here is going on a print bed.
export(NIGHTSTAND, "nightstand.step")


# The ends of the declared ranges build: at `ease = 0` the boards are square
# lofts of two sections (a four-section loft would have bands of no height).
def the_boards_are_solid():
    for b in NIGHTSTAND:
        assert_solid(b)


test("square-edged boards at ease 0", the_boards_are_solid, sweep={"ease": [0, 6]})
