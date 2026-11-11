# Gridfinity showcase: one script, every bin.
#
# A baseplate with a mix of bins seated in it -- 1x1, 2x1 and 3x2, some with
# dividers and label tabs, one with a scoop -- in three filaments. Switch
# `mode` to "bin" and the same script is a single parametric bin, which is what
# `cadpole sweep` and the tests walk across sizes and divider counts.
#
# Built on examples/gridfinity_bin.star and examples/gridfinity_baseplate.star
# and self-contained: the foot and the cup are the same exact four-section
# lofts those examples use (a 45-degree band of a filleted rectangle IS a cone
# at the corners, so `loft()` builds it with no chamfer operation), the bin
# body is a 28-gon prism hollowed top-down, and the baseplate is one slab minus
# one fused cutter. Their headers carry the long explanations; the short ones:
#
# * Feet are built at the full 42 mm pitch, carried 0.5 mm up into the body so
#   neighbouring feet share a face rather than an edge, and the whole bin is
#   trimmed to its 42N - 0.5 footprint at the end (gridfinity_bin.star, notes 2 and 3).
# * The booleans run on the default kernel="auto", which sends the larger ones
#   to the mesh kernel, so this is an STL model, not a STEP one (its note 9).
# * `cadpole check`: every body is a solid (closed, oriented, no crossings). Each
#   bin's one finding is the foot's 45-degree band, which the chords of its
#   corner cones put at 45.36 degrees; at --max-overhang 46 the bins report
#   nothing. The light plate reports thin walls: its cups overlap, so the
#   ridges between them and the outer rim run to a knife edge at the bin-Z
#   relief plane -- the reference's geometry -- and its
#   0.5 mm bottom chamfer is a 45-degree band.
#
# Added here, all planar so their booleans stay plane-plane:
# * dividers: 1.2 mm walls across the cavity, ends buried in the side walls;
# * label tab: a ledge along the back wall with a 45-degree underside, so it
#   prints without support;
# * scoop: a concave ramp (a 12-chord quarter circle) where the floor meets
#   the front wall, for fishing out small parts.
#
# Frame: the plate's cup top plane and each bin's foot top are z = 0, cell
# (i, j) has its axis at (42i + 20.75, 42j + 20.75), so a bin translated by
# whole pitches is a seated bin; the tests measure that, not assume it.

mode = param("mode", "scene", choices=["scene", "bin"],
             doc="scene: a baseplate filled with bins. bin: one bin from the parameters below")
W = param("width", 2, min=1, max=6, doc="Bin grid units along X (42 mm each), bin mode")
L = param("length", 1, min=1, max=6, doc="Bin grid units along Y (42 mm each), bin mode")
H = param("height", 6, min=3, max=12, doc="Bin height units (7 mm each, foot included), bin mode")
dividers = param("dividers", 2, min=0, max=7, doc="Divider walls splitting the bin along X (in scene mode, the 3x2 bin's)")
dividers_y = param("dividers_y", 0, min=0, max=5, doc="Divider walls splitting the bin along Y, bin mode")
label = param("label", True, doc="Label tab along the back wall, bin mode")
scoop = param("scoop", False, doc="Scoop ramp where the floor meets the front wall, bin mode")
filament = param("filament", "auto", choices=["auto", "off-white", "teal", "grey"],
                 doc="Bin colour in bin mode; auto picks by width")
plate_w = param("plate_width", 5, min=5, max=8, doc="Baseplate grid units along X, scene mode")
plate_l = param("plate_length", 4, min=4, max=8, doc="Baseplate grid units along Y, scene mode")
wall = param("wall", 1.2, unit="mm", min=0.8, max=1.6, doc="Bin wall and divider thickness")

# ---------------------------------------------------------------- constants
# gridfinity-freecad/gridfinity/const.py, as in the two source examples.
PITCH = 42.0
UNIT = 7.0
CLEAR = 0.25
CORNER_R = 4.0
FOOT_TOP = 2.4
FOOT_MID = 1.8
FOOT_BOT = 0.8
FOOT_H = FOOT_TOP + FOOT_MID + FOOT_BOT      # 5.0
FLOOR = 1.0
LIP_H = 4.4
LIP_RECESS = 0.6
OVERLAP = 0.5
R_OUT = CORNER_R - CLEAR                     # 3.75
CELL_AXIS = PITCH / 2 - CLEAR                # 20.75
BIN_Z_CLEAR = 0.5
PLATE_CHAMFER = 0.5

FILAMENTS = {
    "off-white": "#e9e3d6",
    "teal": "#1d6f6c",
    "grey": "#7a7f86",
}
PLATE_COLOR = "#8a8f96"


def pla(body, colour):
    return body.material(preset="pla_matte", color=FILAMENTS.get(colour, colour), roughness=0.62)


ARC = [(1.0, 0.0), (0.9659258, 0.2588190), (0.8660254, 0.5), (0.7071068, 0.7071068),
       (0.5, 0.8660254), (0.2588190, 0.9659258), (0.0, 1.0)]


def rounded_polygon(w, h, r, at):
    hw, hh, cx, cy = w / 2, h / 2, at[0], at[1]
    pts = []
    for q, (sx, sy) in enumerate([(1, 1), (-1, 1), (-1, -1), (1, -1)]):
        for (c, s) in ARC:
            ux, uy = [(c, s), (-s, c), (-c, -s), (s, -c)][q]
            pts.append((cx + sx * (hw - r) + r * ux, cy + sy * (hh - r) + r * uy))
    return pts


def slab(w, h, r, z0, height, at, name=None):
    return extrude(sketch(XY.offset(z0)).polygon(rounded_polygon(w, h, r, at)), height, name=name)


def ring(w, h, r, z, at):
    return sketch(XY.offset(z)).rounded_rect(w, h, r, center=at)


def x_prism(profile, x0, x1, name):
    """A prism along X from x0 to x1 whose section in the YZ plane is `profile`."""
    return extrude(sketch(YZ.offset(x0)).polygon(profile), x1 - x0, name=name)


# The foot, (z, inset from the 42 mm pitch), top face carried OVERLAP up.
FOOT_PROFILE = [
    (-FOOT_H, FOOT_TOP + FOOT_BOT),
    (-(FOOT_TOP + FOOT_MID), FOOT_TOP),
    (-FOOT_TOP, FOOT_TOP),
    (0.0, 0.0),
]


def foot(name, weld=True):
    prof = FOOT_PROFILE + ([(OVERLAP, 0.0)] if weld else [])
    return loft([ring(PITCH - 2 * i, PITCH - 2 * i, CORNER_R - i, z, (CELL_AXIS, CELL_AXIS))
                 for (z, i) in prof], ruled=True, name=name)


# ---------------------------------------------------------------- one bin
def bin_geometry(w, l, h):
    aw = PITCH * w - 2 * CLEAR
    al = PITCH * l - 2 * CLEAR
    wall_top = (h - 1) * UNIT + max(0, UNIT - FOOT_H)
    top = wall_top + LIP_H - LIP_RECESS
    return aw, al, wall_top, top


def make_bin(name, w, l, h, nx=0, ny=0, tab=False, ramp=False, cell=(0, 0), turn=False):
    """A w x l x h Gridfinity bin with a stacking lip, nx/ny dividers, an
    optional label tab and scoop, seated in plate cell `cell` (turned end for
    end if `turn`, so a scoop faces the camera)."""
    aw, al, wall_top, top = bin_geometry(w, l, h)
    centre = (aw / 2, al / 2)

    # Wall box, hollowed top-down: the lofted lip socket, then the cavity.
    body = slab(aw, al, R_OUT, 0.0, top, centre, name=name + "_body")
    nw, nl, nr = aw + 4 * CLEAR, al + 4 * CLEAR, CORNER_R + 2 * CLEAR

    def socket_ring(inset, z):
        return ring(nw - 2 * inset, nl - 2 * inset, nr - inset, z, centre)

    socket = loft([
        socket_ring(FOOT_TOP, wall_top - OVERLAP),
        socket_ring(FOOT_TOP, wall_top + FOOT_BOT + FOOT_MID),
        socket_ring(0.0, wall_top + FOOT_H),
    ], name=name + "_socket")
    # The cavity flares out over its top `blend` mm (about 27 degrees from
    # vertical on the flats, 30 at the corners), from the wall
    # face out to the socket's straight band (inset 1.9, corner R2.1), so the
    # step from wall to lip is a printable chamfer instead of the flat 0.7 mm
    # ledge gridfinity_bin.star leaves there (its note 4).
    sock_in = FOOT_TOP - 2 * CLEAR                 # 1.9 from the body face
    blend = 2.0 * (sock_in - wall)
    cav = [(FLOOR, wall, R_OUT - wall), (wall_top - blend, wall, R_OUT - wall),
           (wall_top, sock_in, nr - FOOT_TOP)]
    cavity = loft([sketch(XY.offset(z)).polygon(rounded_polygon(aw - 2 * i, al - 2 * i, r, centre))
                   for (z, i, r) in cav], ruled=True, name=name + "_cavity")
    hollow = subtract(body, [socket, cavity], name=name + "_hollow")

    # Inserts: dividers, label tab, scoop. Each one is buried OVERLAP into the
    # wall or floor it stands on, so every union is transversal.
    inserts = []
    x_in0, x_in1 = wall, aw - wall
    y_in0, y_in1 = wall, al - wall
    div_top = wall_top - 2.0
    tab_top = wall_top - 0.6
    for k in range(1, nx + 1):
        x = x_in0 + (x_in1 - x_in0) * k / (nx + 1) - wall / 2
        inserts.append(box(wall, (y_in1 - y_in0) + 2 * OVERLAP, div_top - FLOOR + OVERLAP,
                           at=(x, y_in0 - OVERLAP, FLOOR - OVERLAP), name="%s_divx_%d" % (name, k)))
    for k in range(1, ny + 1):
        y = y_in0 + (y_in1 - y_in0) * k / (ny + 1) - wall / 2
        inserts.append(box((x_in1 - x_in0) + 2 * OVERLAP, wall, div_top - FLOOR + OVERLAP,
                           at=(x_in0 - OVERLAP, y, FLOOR - OVERLAP), name="%s_divy_%d" % (name, k)))
    if tab:
        lip = 1.2      # the tab's flat front edge
        drop = 1.25    # underside slope, rise over run: 51 degrees, steeper than 45
        depth = min(12.0, (wall_top - FLOOR - lip - 1.5) / drop)
        if depth < 5.0:
            warn("%s: too short for a label tab" % name)
        else:
            inserts.append(x_prism([
                (y_in1 + OVERLAP, tab_top),
                (y_in1 - depth, tab_top),
                (y_in1 - depth, tab_top - lip),
                (y_in1 + OVERLAP, tab_top - lip - drop * (depth + OVERLAP)),   # sloped underside
            ], x_in0 - OVERLAP, x_in1 + OVERLAP, name + "_tab"))
    if ramp:
        r = min(16.0, 0.55 * (wall_top - FLOOR), 0.4 * (y_in1 - y_in0))
        # Centre pulled 0.2 into the floor and the wall so the arc crosses
        # both rather than ending tangent on them: a tangent arc leaves the
        # union a zero-thickness contact to resolve.
        yc, zc = y_in0 + r - 0.2, FLOOR + r - 0.2
        prof = [(y_in0 - OVERLAP, FLOOR - 0.3), (yc, FLOOR - 0.3)]
        for k in range(13):
            t = 90.0 * k / 12
            prof.append((yc - r * math.sin(t), zc - r * math.cos(t)))
        prof.append((y_in0 - OVERLAP, zc))
        inserts.append(x_prism(prof, x_in0 - OVERLAP, x_in1 + OVERLAP, name + "_scoop"))
    if inserts:
        hollow = union([hollow] + inserts, name=name + "_fitted", kernel="mesh")

    feet = pattern_linear(pattern_linear(foot(name + "_foot"), w, (PITCH, 0, 0), merge=True,
                                         name=name + "_foot_col"),
                          l, (0, PITCH, 0), merge=True, name=name + "_feet")
    welded = union(hollow, feet, name=name + "_welded", kernel="mesh")
    footprint = slab(aw, al, R_OUT, -FOOT_H - OVERLAP, FOOT_H + top + OVERLAP, centre,
                     name=name + "_footprint")
    b = intersect(welded, footprint, name=name, kernel="mesh")
    if turn:
        b = rotate(b, 180, axis=(0, 0, 1), origin=(aw / 2, al / 2, 0), name=name + "_turned")
    if cell != (0, 0):
        b = translate(b, (cell[0] * PITCH, cell[1] * PITCH, 0), name=name + "_seated")
    return b


# ---------------------------------------------------------------- baseplate
def make_plate(pw, pl):
    """A light baseplate (the 5 mm cup frame), as gridfinity_baseplate.star
    builds it: one lofted slab minus one fused cutter."""
    fw, fl = PITCH * pw - 2 * CLEAR, PITCH * pl - 2 * CLEAR
    fc = (fw / 2, fl / 2)
    s = loft([
        ring(fw - 2 * PLATE_CHAMFER, fl - 2 * PLATE_CHAMFER, R_OUT - PLATE_CHAMFER, -FOOT_H, fc),
        ring(fw, fl, R_OUT, -FOOT_H + PLATE_CHAMFER, fc),
        ring(fw, fl, R_OUT, 0.0, fc),
    ], ruled=True, name="plate_slab")
    cs, cr = PITCH + 2 * CLEAR, CORNER_R + CLEAR
    cup = loft([ring(cs - 2 * i, cs - 2 * i, cr - i, z, (CELL_AXIS, CELL_AXIS))
                for (z, i) in FOOT_PROFILE + [(OVERLAP, 0.0)]], ruled=True, name="cup")
    cups = pattern_linear(pattern_linear(cup, pw, (PITCH, 0, 0), merge=True, name="cup_col"),
                          pl, (0, PITCH, 0), merge=True, name="cups")
    relief = box(fw + 20.0, fl + 20.0, BIN_Z_CLEAR + 2.0, at=(-10.0, -10.0, -BIN_Z_CLEAR),
                 name="bin_z_relief")
    tool = union([cups, relief], name="plate_tool", kernel="mesh")
    p = subtract(s, tool, name="baseplate")
    return p.material(preset="pla_matte", color=PLATE_COLOR, roughness=0.7)


# ---------------------------------------------------------------- the model
# (name, w, l, h, x dividers, y dividers, label, scoop, cell, filament, turned)
SCENE = [
    ("parts_3x2", 3, 2, 6, dividers, 1, True, False, (0, 2), "teal", False),
    ("scoop_2x1", 2, 1, 3, 0, 0, True, True, (0, 0), "off-white", True),
    ("tray_2x1", 2, 1, 3, 3, 0, False, False, (0, 1), "grey", False),
    ("bits_1x1_a", 1, 1, 3, 1, 0, True, False, (2, 0), "teal", False),
    ("bits_1x1_b", 1, 1, 4, 0, 0, True, False, (3, 0), "off-white", False),
    ("tall_1x1", 1, 1, 9, 0, 0, False, False, (3, 3), "grey", False),
    ("drawer_2x1", 2, 1, 6, 1, 0, True, False, (3, 2), "off-white", False),
    ("cup_1x1", 1, 1, 4, 0, 0, False, True, (4, 1), "teal", True),
]

bins = {}
plate = None
if mode == "scene":
    plate = make_plate(plate_w, plate_l)
    for (n, w, l, h, nx, ny, t, s, c, f, u) in SCENE:
        bins[n] = pla(make_bin(n, w, l, h, nx, ny, t, s, c, u), f)
else:
    colour = filament
    if colour == "auto":
        colour = ["teal", "off-white", "grey"][(W - 1) % 3]
    bins["bin"] = pla(make_bin("bin", W, L, H, dividers, dividers_y, label, scoop), colour)

for n, b in bins.items():
    export(b, "gridfinity_%s.stl" % n)
if plate:
    export(plate, "gridfinity_baseplate_%dx%d.stl" % (plate_w, plate_l))


# ---------------------------------------------------------------- tests
def footprint_and_height():
    if mode != "bin":
        return
    b = bins["bin"]
    aw, al, wall_top, top = bin_geometry(W, L, H)
    assert_bbox(b, (0, 0, -FOOT_H), (aw, al, top), tol=0.02)


def every_bin_is_closed():
    # max_overhang=46: the foot's 45-degree bands measure 45.36 on the chords
    # of their corner cones, which a default check flags; that is the
    # Gridfinity foot itself, printed unsupported by every Gridfinity bin.
    # Anything steeper than that -- a tab, a ledge -- still fails here.
    for n, b in bins.items():
        r = print_check(b, max_overhang=46)
        assert_true(r.solid, msg="%s is a solid (closed, oriented, no crossings)" % n)
        assert_true(r.watertight, msg="%s is watertight" % n)
        assert_true(r.unsupported_area < 1.0, msg="%s needs no support" % n)
        assert_true(r.components == 1, msg="%s is one piece" % n)
        assert_true(r.fits_bed, msg="%s fits a 256 mm bed" % n)
        assert_true(r.min_wall >= 0.8 - 0.05, msg="%s has no wall under two perimeters" % n)


def material_used():
    return bins["bin" if mode == "bin" else "parts_3x2"].volume()


def bins_seat_in_the_plate():
    if mode != "scene":
        return
    plo, phi = plate.bbox()
    assert_close(phi[2], -BIN_Z_CLEAR, tol=0.02, msg="plate top one bin-Z clearance below z = 0")
    boxes = []
    for (n, w, l, h, nx, ny, t, s, c, f, u) in SCENE:
        lo, hi = bins[n].bbox()
        # Seated: foot bottom on the cup floor, footprint on the cell grid.
        assert_close(lo[2], plo[2], tol=0.02, msg="%s foot bottom on the cup floor" % n)
        assert_close(lo[0], c[0] * PITCH, tol=0.02, msg="%s on its cell in X" % n)
        assert_close(lo[1], c[1] * PITCH, tol=0.02, msg="%s on its cell in Y" % n)
        assert_true(hi[0] <= phi[0] + 0.02 and hi[1] <= phi[1] + 0.02, msg="%s inside the plate" % n)
        boxes.append((n, lo, hi))
    # No two bins claim the same cell: their footprints leave the 0.5 mm gap.
    for a in range(len(boxes)):
        for b in range(a + 1, len(boxes)):
            (na, la, ha), (nb, lb, hb) = boxes[a], boxes[b]
            apart = ha[0] <= lb[0] or hb[0] <= la[0] or ha[1] <= lb[1] or hb[1] <= la[1]
            assert_true(apart, msg="%s and %s overlap" % (na, nb))


test("the scene: every bin seated on its own cells", bins_seat_in_the_plate)
test("the scene: every bin closed, one piece, walls of two perimeters", every_bin_is_closed)
test("one script, every bin: sizes x dividers", every_bin_is_closed,
     sweep={"mode": ["bin"], "width": [1, 2, 3], "length": [1, 2], "dividers": [0, 1, 3]})
test("footprint is 42N - 0.5 and the lip tops out where it should", footprint_and_height,
     sweep={"mode": ["bin"], "width": [1, 3, 6], "height": [3, 6, 12]})
test("label tab and scoop at every height", every_bin_is_closed,
     sweep={"mode": ["bin"], "height": [3, 4, 8, 12], "scoop": [True], "label": [True]})
test("more dividers, more plastic", material_used,
     sweep={"dividers": [0, 1, 2, 3, 5]}, monotone=monotone.increasing)
