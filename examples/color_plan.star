# A one-room studio in a garden, drawn the way its section renders look --
# as vectors: the colour look paints every surface the eye sees in
# its material's colour, fills the cut in the colour of what it cuts, and
# writes the room names the model carries as real text in the bundled face
# (Inter, embedded in the PDF as a subset; `pdftotext` reads every word).
#
#   cadpole export examples/color_plan.star --view top --section z=1200 \
#       --crop=-2000,-3000,9500,8500 --title "THE STUDIO, PLAN" \
#       --legend "#c9774f=walls" --legend "#a9c4d4=glass" -o /tmp/plan.pdf     # one plate
#   cadpole export examples/color_plan.star --section x=3000 --title "SECTION" -o /tmp/section.pdf
#   cadpole export examples/color_plan.star --run-exports --out-dir /tmp          # /tmp/color_plan.pdf, the set
#   cadpole test examples/color_plan.star
#
# The labels are model geometry, as a render needs them to be: each room's
# name is `extrude(text(...), 20)` lying on the floor, so `cadpole render
# --section z=1200` shows it, and the drawing writes the same string at the
# same size and place as text instead of as its glyphs' outlines. A label
# moved or cut after it was extruded is ordinary geometry again.
#
# Frame: x east, y north, z up from the slab, millimetres. The studio and
# its garden are fictional.

units(units.ft_in)
render_defaults(north=0)

W = param("width", 6400.0, unit="mm", min=6000, max=8000, step=100, doc="The studio east to west, outside the walls (the south window ends 5800 from the west)")
D = param("depth", 4800.0, unit="mm", min=3600, max=6000, step=100, doc="The studio north to south, outside the walls")
show_labels = param("labels", True, doc="Lay the room names on the floors")

T = 200.0       # exterior walls
P = 100.0       # the partition
H = 2700.0      # floor to ceiling
SLAB = 150.0

def mat(b, color, **kw):
    b.material(color=color, roughness=0.8, **kw)
    return b

L_WALLS = layer("walls", doc="The studio's walls, slab and roof")
L_FURN = layer("furniture", doc="What stands on the floors")
L_LAB = layer("labels", doc="The room names and areas", visible=show_labels)

# ---- The shell: walls on a slab, a door and two windows left open in
# them, a flat roof over it. The walls are boxes laid around the openings
# rather than a ring with the openings subtracted: an exact boolean's edge
# order differs in its last bits between the native and the wasm builds,
# and every example must evaluate the same on both.
DOOR = (1200.0, 2200.0)      # x, the south wall, 2100 tall
WIN_S = (3400.0, 5800.0)     # x, the south wall
WIN_E = (1600.0, 3200.0)     # y, the east wall
SILL, HEAD = 700.0, 2100.0

def wall(name, x0, x1, y0, y1, z0, z1):
    return mat(box(x1 - x0, y1 - y0, z1 - z0, at=(x0, y0, z0), name=name), "#c9774f")

WALLS = [
    wall("wall_s1", 0, DOOR[0], 0, T, 0, H),
    wall("wall_s_door_head", DOOR[0], DOOR[1], 0, T, 2100, H),
    wall("wall_s2", DOOR[1], WIN_S[0], 0, T, 0, H),
    wall("wall_s_sill", WIN_S[0], WIN_S[1], 0, T, 0, SILL),
    wall("wall_s_head", WIN_S[0], WIN_S[1], 0, T, HEAD, H),
    wall("wall_s3", WIN_S[1], W, 0, T, 0, H),
    wall("wall_e1", W - T, W, T, WIN_E[0], 0, H),
    wall("wall_e_sill", W - T, W, WIN_E[0], WIN_E[1], 0, SILL),
    wall("wall_e_head", W - T, W, WIN_E[0], WIN_E[1], HEAD, H),
    wall("wall_e2", W - T, W, WIN_E[1], D - T, 0, H),
    wall("wall_n", 0, W, D - T, D, 0, H),
    wall("wall_w", 0, T, T, D - T, 0, H),
]
for b in WALLS:
    L_WALLS.add(b)
slab = L_WALLS.add(mat(box(W, D, SLAB, at=(0, 0, -SLAB), name="slab"), "#d9cdb8"))
roof = L_WALLS.add(mat(box(W + 400, D + 400, 250, at=(-200, -200, H), name="roof"), "#3a3d42"))
glass_s = mat(box(2400, 24, 1400, at=(3400, T / 2 - 12, 700), name="glass_south"), "#a9c4d4", opacity=0.35)
glass_e = mat(box(24, 1600, 1400, at=(W - T / 2 - 12, 1600, 700), name="glass_east"), "#a9c4d4", opacity=0.35)
L_WALLS.add(glass_s)
L_WALLS.add(glass_e)

# Partitions closing off the bath in the north-west corner, its door an
# 800 mm gap at the corner.
BATH_X = 2000.0
BATH_Y = 2600.0
PARTITIONS = [
    mat(box(P, D - T - BATH_Y, H, at=(BATH_X, BATH_Y, 0), name="partition_east"), "#efebe4"),
    mat(box(BATH_X - T - 800, P, H, at=(T, BATH_Y, 0), name="partition_south"), "#efebe4"),
]
for b in PARTITIONS:
    L_WALLS.add(b)

# ---- Furniture: boxes at the heights they stand, so the plan shows what
# is on the floor and the cut shows nothing above it.
FURN = [
    ("bed", (W - T - 2100, D - T - 1600, 0), (2100, 1600, 450), "#b9c6d6"),
    ("desk", (BATH_X + 400, D - T - 650, 0), (1500, 650, 740), "#9c7b5a"),
    ("sofa", (T + 300, 600, 0), (2000, 850, 420), "#8a8f96"),
    ("table", (3700, 1100, 0), (1200, 800, 740), "#a8865f"),
    ("vanity", (T + 100, D - T - 550, 0), (900, 550, 850), "#f4f2ee"),
    ("shower", (T + 1100, D - T - 900, 0), (600, 900, 60), "#dbe5ea"),
]
FURNITURE = [mat(box(size[0], size[1], size[2], at=at, name=n), c) for n, at, size, c in FURN]
for b in FURNITURE:
    L_FURN.add(b)

# ---- The garden: a deck off the door, a lawn, a path.
deck = mat(box(3200, 1800, 120, at=(600, -1900, -120), name="deck"), "#a88d6c")
lawn = mat(box(W + 3000, 2600, 40, at=(-1500, -4600, -160), name="lawn"), "#7fa35a")
path = mat(box(900, 2600, 60, at=(1250, -4600, -140), name="path"), "#c4bfb4")

# ---- The labels: lying on the floor, 20 mm thick, centred where the
# room's name goes, the area under it.
LABELS = []

def label(words, x, y, size, name):
    t = text(words, size=size, align="center", baseline="middle", plane=XY.offset(10), at=(x, y), name=name + "_text")
    gl = extrude(t, 20, name=name)
    gl = gl if type(gl) == "list" else [gl]
    for g in gl:
        g.material(color="#1d2733", roughness=0.6)
        L_LAB.add(g)
        LABELS.append(g)
    return gl

ROOMS = [
    ("Studio", (BATH_X + T, T, W - T, D - T), (3150, 2500)),
    ("Bath", (T, BATH_Y + P, BATH_X, D - T), (1100, 3300)),
]
for i, (name, r, at) in enumerate(ROOMS):
    area_sf = (r[2] - r[0]) * (r[3] - r[1]) / (304.8 * 304.8)
    label(name, at[0], at[1] + 160, 300, "room_%d" % i)
    label("%d sf" % int(area_sf + 0.5), at[0], at[1] - 220, 190, "area_%d" % i)
label("Deck", 2200, -1000, 260, "deck_label")

# ---- The set: the colour plan and section on one ARCH B sheet, and the
# drafting plan beside it for comparison.
set = sheet_set(
    "color_plan",
    size=sheet.arch_b,
    font="inter",
    title_block=title_block(project="A STUDIO IN A GARDEN", stamp=["COLOUR LOOK", "NOT FOR CONSTRUCTION"], issue="EXAMPLE", date="2026-10-03", drawn="CADPOLE"),
)
x0, y0, x1, y1 = set.area()
BODIES = WALLS + [slab, roof, glass_s, glass_e] + PARTITIONS + FURNITURE + [deck, lawn, path] + (LABELS if show_labels else [])
s = set.sheet("A1.0", "PLAN AND SECTION", scale="1:50")
CROP = (-1300, -2300, W + 300, D + 300)    # the studio, its deck and the edge of the lawn
plan = s.plan_view(BODIES, cut=1200, look="color", crop=CROP, box=(x0 + 10, y0 + 112, x0 + 170, y1 - 8), name="plan")
s.view_title("1", "PLAN, CUT AT 1200", at=(x0 + 10, y0 + 100), width=120)
s.north_arrow(at=(x0 + 170, y1 - 22))
lines = s.plan_view(BODIES, cut=1200, hatch="solid", crop=CROP, box=(x0 + 185, y0 + 112, x1 - 10, y1 - 8), name="lines")
s.view_title("2", "THE SAME PLAN, DRAFTING LINES", at=(x0 + 185, y0 + 100), width=120)
sec = s.section_view(BODIES, plane="x=3000", look="color", crop=(-2500, -400, D + 600, H + 500), box=(x0 + 10, y0 + 14, x1 - 10, y0 + 90), name="section")
s.view_title("3", "SECTION AT X 3000, LOOKING WEST", at=(x0 + 10, y0 + 6), width=150)
export(set, "color_plan.pdf")

# ---- Tests.
def the_labels_are_text_labels():
    return len(LABELS) >= 5

def the_studio_is_the_size_asked():
    north = WALLS[10]
    return abs(north.bbox()[1][0] - north.bbox()[0][0] - W) < 1.0

test("the room names are glyph bodies on their own layer", the_labels_are_text_labels)
test("the studio", the_studio_is_the_size_asked)


# The narrowest studio still has a wall east of the south window.
def the_walls_are_solid():
    for b in WALLS:
        assert_solid(b)


test("the narrowest studio", the_walls_are_solid, sweep={"width": [6000]})
