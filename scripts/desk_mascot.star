# Cadpole, the mascot, as a printable desk figure: the tadpole mark stood up
# on a plinth that says "cadpole". From the front it is the logo -- the body
# curled into a C, the eye on the brow and the two-tone paddle at the end of
# the tail -- and from any other side a soft, solid toy.
#
# How it is built:
#   * The outline is the logo itself: assets/tadpole.svg, read with
#     `import_svg` into exact sketches standing in the XZ plane, the mark's
#     front facing -y.
#   * The figure is one signed-distance field: the silhouette as a prism
#     (extruded, then `field.from_body`), cut by the front and back planes.
#     The front edge is rounded over with `smooth_intersect`, a rolling-ball
#     fillet between the prism's walls and the front, so the outline seen
#     from the front stays the mark's own, sharp tip and crisp chin
#     included; the back is flat. It is meshed once.
#   * The colours are the logo's: the paddle's light rim is cut out of the
#     figure by a prism of the rim's outline, front to back, and the
#     eye's white and pupil are inlaid into its front skin, so every colour is
#     a separate body that touches its neighbours and never overlaps them --
#     one object of several parts in a multi-colour slicer.
#   * A mark that curls over on itself cannot be printed standing up without
#     supports (the head hangs over the mouth of the C), so the figure prints
#     lying on its flat back, where nothing overhangs, and stands up in the
#     plinth afterwards: a tab under the paddle's foot pushes into a slot.
#     The `print` layer lays both pieces out for the bed, and `export` writes
#     them as two objects, so `cadpole check` judges each the way it prints.
#   * The plinth is an exact B-rep (a filleted stadium, the slot cut before
#     the fillet so its edges stay sharp). The word is raised
#     on its top, centred on the straight stretch between the round ends and
#     shrunk to fit it, so any `word` stays on the flat.
#
#   cadpole test desk_mascot.star
#   cadpole check desk_mascot.star --layer print
#   cadpole render desk_mascot.star --backend path --samples 256 --size 1600x1200 --view az=-30,el=24 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o desk_mascot.png

size = param("size", 110.0, unit="mm", min=60, max=220, doc="Length of the plinth; everything scales with it")
depth = param("depth", 14.0, unit="mm", min=8, max=20, doc="Front-to-back thickness of the figure at size 110")
word = param("word", "cadpole", doc="What the plinth says")

DARK = "#2FBF9F"  # the logo's two greens
LIGHT = "#8FE3C4"
WHITE = "#F4F6F2"
INK = "#0B2E2A"
PLINTH = "#3B4A50"  # slate: the green figure stands off it, the white word reads on it, and it is light enough for a dark page

S = size / 110.0  # everything is drawn at size 110 and scaled by this
BASE_L = size
BASE_W = 50.0 * S
BASE_H = 11.0 * S
TOP = BASE_H

# --- The outline: the logo's own paths, in its 128-px tile. The silhouette
# (`mark`) spans 99.7 x 119.9 px, its box centred on SIL_C; its lowest point,
# the bottom of the paddle, is at SIL_LOW.
SIL_W = 99.66
SIL_H = 119.93
SIL_C = (65.97, 64.0)
SIL_LOW = 123.97
FIG_H = 64.0 * S  # how tall the mark stands
MM = FIG_H / SIL_H  # one px of the logo, in mm
PX = 25.4 / 96.0  # import_svg reads px as CSS px
FOOT_SINK = 1.0 * S  # the paddle's foot is cut flat this far above its lowest point, on the plinth's top
D_BODY = depth * S
R_EDGE = 0.3 * D_BODY  # the roundover of the front edge
Y_BACK = 15.0 * S  # the figure's flat back; its front faces -y, toward the word


def svg_to_world(p):
    """A point of the logo (px, y down) on the figure's back plane."""
    return ((p[0] - SIL_C[0]) * MM, Y_BACK, TOP - FOOT_SINK + (SIL_LOW - p[1]) * MM)


def mark_plane(y):
    """The plane the mark is drawn on, at depth y: sketch x is world x, sketch y world z."""
    return plane(origin=(0.0, y, 0.0), normal=(0, -1, 0), x_dir=(1, 0, 0))


# Sketch coordinates on mark_plane: x = world x, y = world z.
ORIGIN = svg_to_world((0.0, 128.0))  # where the tile's bottom-left corner lands


def part(id, y, name):
    """One shape of the logo, in place on the plane at depth y."""
    return import_svg(asset("tadpole.svg"), ids=[id], plane=mark_plane(y), align="none", at=(ORIGIN[0], ORIGIN[2]), scale=MM / PX, name=name)


def svg_polygon(pts, y, name):
    return sketch(mark_plane(y), name=name).polygon([(svg_to_world(p)[0], svg_to_world(p)[2]) for p in pts])


Y_FRONT = Y_BACK - D_BODY


silhouette = part("mark", Y_BACK + 10.0, "silhouette")
# The silhouette as a prism through the figure and well past it both ways,
# so near the front and the back the field is the distance to its walls.
# (The prism is only the field's source, so it goes on a hidden layer.)
construction = layer("construction", doc="The prism the figure's field is measured from", visible=False)
prism = field.from_body(construction.add(extrude(silhouette, D_BODY + 20.0, name="silhouette_prism")))
front = field.half_space(origin=(0, Y_FRONT, 0), normal=(0, -1, 0))
back = field.half_space(origin=(0, Y_BACK, 0), normal=(0, 1, 0))
foot = field.half_space(origin=(0, 0, TOP), normal=(0, 0, 1))

# The tab: under the foot, into the plinth, flush with the flat back so it
# lies on the bed with the rest of the figure.
TAB_L = 9.0 * S
TAB_D = 6.0 * S
FOOT_X = svg_to_world((87.5, SIL_LOW))[0]  # the bottom of the paddle
TAB_T = 0.3 * D_BODY
TAB_MID = Y_BACK - TAB_T / 2.0
CLEAR = 0.15  # the slot is this much bigger than the tab on every side
tab_box = (TAB_L, TAB_T, TAB_D + 0.6)  # it starts inside the fin, above the foot

whole_field = prism.smooth_intersect(front, R_EDGE).intersect(back).subtract(foot)
whole_field = whole_field.union(field.box(tab_box[0], tab_box[1], tab_box[2]).translate((FOOT_X, TAB_MID, TOP - TAB_D + tab_box[2] / 2.0)))
RES = 0.2 * S  # the meshing cell
whole = mesh(whole_field, resolution=RES, name="figure_whole")

# --- The colours. The rim is cut out of the figure by a prism of its
# outline, front to back (the tab with it: it is under the rim); the eye is
# inlaid SKIN deep into the front of the head. Each colour's socket is GAP
# bigger than the part in it: they touch, never overlap.
GAP = 0.04
SKIN = 1.6 * S
# The rim's cutter runs 1.5 mm past the silhouette, so its walls cross the
# figure's only where the two tones meet, never along its outside; it is
# kept to the paddle (right of where the tail turns into it, below the
# mouth of the C), where the light tone is. The logo runs the rim out to a
# point at both ends, where it meets the outline at a glancing angle; the
# zone stops it a millimetre short of each point, on lines square to the
# outline (x = 73 px at the top, through (67.6, 115.2) px at the belly), so
# no colour leaves a sliver a print cannot hold.
RIM_ZONE = [(61.6, 117.9), (77.6, 110.7), (73.0, 100.0), (73.0, 80.0), (124.0, 80.0), (124.0, 134.0), (61.6, 134.0)]


def rim_outline(grow, name):
    """The rim's cutter, grown by `grow` toward the dark tone and across the
    zone's ends (a socket is the cutter grown by a gap). Each piece is grown
    before they are combined: the combined outline has corners an offset
    would fold."""
    y = Y_BACK + 1.0
    tab = sketch(mark_plane(y), name=name + "_tab").rect(TAB_L + 2.0, TAB_D + 3.0, center=(FOOT_X, TOP - TAB_D / 2.0))
    out = union2d(part("mark", y, name + "_mark").offset(1.5 + grow), tab, name=name + "_and_tab")
    zone = svg_polygon(RIM_ZONE, y, name + "_zone")
    out = intersect2d(out, zone.offset(grow) if grow > 0 else zone, name=name + "_in_zone")
    # the dark tone's point inside the rim rounded to 0.4 mm, for the same reason
    dark_tone = part("body", y, name + "_body").offset(-0.4 * S).offset(0.4 * S - grow)
    return subtract2d(out, dark_tone, name=name)



def prism(sk, name):
    """The sketch (on the plane 1 mm behind the back) as a prism through the figure."""
    return extrude(sk, D_BODY + 3.0, name=name)
eye_c = svg_to_world((80.1, 25.8))
pupil_c = svg_to_world((83.0, 26.5))
EYE_R = 14.0 * MM
PUPIL_R = 7.5 * MM


def disc(c, r, name, deeper=0.0):
    """A cylinder along y through the front skin of the head, centred on c,
    `deeper` past the skin (a socket longer than what it cuts shares no cap
    with it)."""
    return extrude(sketch(mark_plane(Y_FRONT - 1.0 - deeper)).circle(r, center=(c[0], c[2])), -(1.0 + SKIN + 2.0 * deeper), name=name)


# The rim is meshed from the field (the figure's, cut by the prism) on a finer
# grid, rather than cut out of the mesh: it runs out to a point at both ends,
# where a mesh boolean leaves triangles crossing. Its socket in the body is
# RIM_GAP bigger, wider than the finer grid's error.
RIM_GAP = 0.1 * S
rim = mesh(whole_field.intersect(field.from_body(construction.add(prism(rim_outline(0.0, "rim_cut"), "rim_prism")))), resolution=0.5 * RES, name="paddle_rim")
dark = subtract(whole, prism(rim_outline(RIM_GAP, "rim_socket_cut"), "rim_socket"), name="figure_dark", kernel="mesh")
white = subtract(intersect(dark, disc(eye_c, EYE_R, "eye_disc"), name="eye_inlay", kernel="mesh"), disc(pupil_c, PUPIL_R + GAP, "pupil_socket", deeper=0.5), name="eye_white", kernel="mesh")
pupil = intersect(dark, disc(pupil_c, PUPIL_R, "pupil_disc"), name="pupil", kernel="mesh")
body = subtract(dark, disc(eye_c, EYE_R + GAP, "eye_socket"), name="cadpole_body", kernel="mesh")
FIGURE = [body, rim, white, pupil]

# --- The plinth: a filleted stadium with the word on its top and the slot.
base_sk = sketch(XY).slot(BASE_L, BASE_W)
base = extrude(base_sk, BASE_H, name="plinth")
slot = box(TAB_L + 2.0 * CLEAR, TAB_T + 2.0 * CLEAR, TAB_D + 0.4 + 1.0, at=(FOOT_X - TAB_L / 2.0 - CLEAR, TAB_MID - TAB_T / 2.0 - CLEAR, TOP - TAB_D - 0.4), name="slot")
base = subtract(base, slot, name="plinth_slotted")
# the outline's top edge rounded, the slot's left sharp (its edges are the short ones)
base = fillet(base, radius=2.0 * S, edges=base.edges(on_plane=XY.offset(BASE_H), min_length=3.0 * TAB_L), name="plinth_round")
WORD_RELIEF = 1.0 * S
# The word lies on the plinth's top, in front of the figure, so it prints flat
# (a colour change at one layer height does it on a one-nozzle printer). It
# is centred on the straight stretch of the stadium, between the two round
# ends, and kept a margin inside the top's fillet: set once at full size,
# measured, then shrunk if it is longer than that stretch and set again
# with its lowest point the margin above the front edge.
ROUND = 2.0 * S  # the top edge's fillet
WORD_MARGIN = 2.5 * S
FLAT_HALF = BASE_L / 2.0 - BASE_W / 2.0  # half the straight stretch of the stadium
word_plane = plane(origin=(0.0, 0.0, TOP), normal=(0, 0, 1), x_dir=(1, 0, 0))


def word_extent(sk):
    """(min x, max x, min y, max y) of a sketch's loops, in its plane."""
    xs = [p[0] for loop in sk.points() for p in loop]
    ys = [p[1] for loop in sk.points() for p in loop]
    return (min(xs), max(xs), min(ys), max(ys))


def set_word(size, at, name):
    return text(word, size=size, weight=700, align="center", baseline="middle", spacing=0.6 * size / 9.0, plane=word_plane, at=at, name=name)


WORD_SIZE = 9.0 * S
(wx0, wx1, wy0, wy1) = word_extent(set_word(WORD_SIZE, (0.0, 0.0), "word_measure"))
WORD_SIZE = WORD_SIZE * min(1.0, 2.0 * (FLAT_HALF - WORD_MARGIN) / (wx1 - wx0))
(wx0, wx1, wy0, wy1) = word_extent(set_word(WORD_SIZE, (0.0, 0.0), "word_fitted"))
WORD_X = -0.5 * (wx0 + wx1)
WORD_Y = -BASE_W / 2.0 + ROUND + WORD_MARGIN - wy0
label = set_word(WORD_SIZE, (WORD_X, WORD_Y), "word")
letters = extrude(label, WORD_RELIEF, name="word_relief")
letters = letters if type(letters) == "list" else [letters]

# --- Materials.
body.material(preset="pla_silk", color=DARK, roughness=0.3)
rim.material(preset="pla_silk", color=LIGHT, roughness=0.3)
white.material(preset="pla_matte", color=WHITE, roughness=0.35)
pupil.material(preset="petg_glossy", color=INK, roughness=0.12)
base.material(preset="pla_matte", color=PLINTH, roughness=0.6)
for g in letters:
    g.material(preset="pla_matte", color=WHITE, roughness=0.4)

# --- Printing: two pieces. The plinth with its word prints as it stands; the
# figure lies on its flat back beside it, the tab toward the plinth.
print_layer = layer("print", doc="The two pieces as they print: the figure on its back beside the plinth", visible=False)


def lay_flat(b, name):
    """The figure turned onto its back (the back plane on z = 0), in front of the plinth."""
    b2 = rotate(copy(b), -90, axis=(1, 0, 0))  # +y (the back's normal) turns to -z, +z (up) to +y
    return print_layer.add(translate(b2, (0, -BASE_W / 2.0 - 4.0 * S - (TOP + FIG_H), Y_BACK), name=name))


LAID = [lay_flat(b, n + "_flat") for b, n in zip(FIGURE, ["body", "paddle_rim", "eye_white", "pupil"])]
for b, c in zip(LAID, [DARK, LIGHT, WHITE, INK]):
    b.material(preset="pla_matte", color=c)
PLINTH_PRINT = [base] + letters
export(PLINTH_PRINT, "desk_mascot.3mf", name="plinth")
export(LAID, "desk_mascot.3mf", name="figure")

report("figure_height_mm", whole.bbox()[1][2] - TOP)
report("parts", len(FIGURE) + len(PLINTH_PRINT))


# --- Tests.
def every_part_is_a_solid():
    for b in FIGURE + PLINTH_PRINT:
        assert_solid(b, msg="%s is not a solid" % b.color())
    return len(FIGURE) + len(PLINTH_PRINT)


test("every part is a solid", every_part_is_a_solid, sweep={"depth": [8.0, 14.0, 20.0], "size": [70.0, 110.0, 160.0]})


def stands_on_the_plinth():
    (lo, hi) = whole.bbox()
    assert_close(lo[2], TOP - TAB_D, tol=0.3, msg="only the tab goes below the plinth's top")
    above = intersect(whole, box(4.0 * BASE_L, 4.0 * BASE_L, 10.0 * FIG_H, at=(-2.0 * BASE_L, -2.0 * BASE_L, TOP + 0.01)), name="above_top", kernel="mesh")
    assert_close(above.bbox()[0][2], TOP, tol=0.3, msg="the figure's foot is on the plinth")
    assert_true(hi[0] < BASE_L / 2.0 and lo[0] > -BASE_L / 2.0, msg="the figure stays on the plinth along x")
    assert_true(hi[1] < BASE_W / 2.0 and lo[1] > -BASE_W / 2.0, msg="... and across it")
    assert_true(not distance(whole, base).overlap, msg="the tab fits its slot")
    return hi[2]


test("the figure stands on the plinth, inside its outline", stands_on_the_plinth, sweep={"depth": [8.0, 14.0, 20.0], "size": [70.0, 110.0, 160.0]})


def reads_as_the_mark():
    """From the front the figure is the logo: its outline is the mark's, as
    tall and as wide as the mark drawn at FIG_H, less the foot."""
    (lo, hi) = whole.bbox()
    assert_close(hi[2] - TOP, FIG_H - FOOT_SINK, rel=0.01, msg="the figure stands as tall as the mark")
    assert_close(hi[0] - lo[0], SIL_W * MM, rel=0.03, msg="... and is as wide (the paddle's tip is rounded a little)")
    assert_close(hi[1] - lo[1], D_BODY, rel=0.02, msg="the body is `depth` thick")
    return hi[2] - TOP


test("from the front, the figure is the mark", reads_as_the_mark, sweep={"size": [70.0, 110.0, 160.0]})


def scales_with_size():
    (lo, hi) = base.bbox()
    assert_close(hi[0] - lo[0], size, rel=0.01)
    return body.volume()


test("everything scales with size", scales_with_size, sweep={"size": [70.0, 110.0, 160.0]}, monotone="increasing")


def colours_do_not_overlap():
    for a, b in [(body, rim), (body, white), (white, pupil), (body, pupil), (body, base), (rim, base), (base, letters[0])]:
        c = distance(a, b)
        assert_true(not c.overlap, msg="%s and %s overlap" % (a.color(), b.color()))
    return 0


test("the colour parts touch and never overlap", colours_do_not_overlap)


def lies_flat_to_print():
    for b in LAID:
        (lo, hi) = b.bbox()
        assert_true(lo[2] > -0.01, msg="%s goes below the bed" % b.color())
    (lo, hi) = LAID[0].bbox()
    assert_close(lo[2], 0.0, tol=0.05, msg="the figure lies on its flat back")
    assert_close(hi[2], D_BODY, rel=0.02, msg="... and stands `depth` high")
    (blo, bhi) = base.bbox()
    for b in LAID:
        assert_true(b.bbox()[1][1] < blo[1], msg="the figure is clear of the plinth on the bed")
    return hi[2]


test("the figure lies flat on its back to print", lies_flat_to_print)


def word_sits_on_the_top():
    flat = sketch(word_plane).slot(BASE_L - 2.0 * ROUND, BASE_W - 2.0 * ROUND)
    off = label.subtract(flat).area()
    assert_true(off < 1e-6, msg="%s mm^2 of the word is off the plinth's flat top" % off)
    (x0, x1, y0, y1) = word_extent(label)
    assert_true(x0 > -FLAT_HALF and x1 < FLAT_HALF, msg="the word runs onto the plinth's round ends")
    for b in letters:
        assert_true(not distance(b, whole).overlap, msg="the word runs into the figure")
    return label.area()


test("the word sits on the plinth's flat top", word_sits_on_the_top, sweep={"size": [70.0, 110.0, 160.0], "word": ["cadpole", "tadpoles of the desk"]})
