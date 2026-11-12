# Gridfinity showcase: one script, every bin.
#
# A baseplate with a mix of bins in it -- 1x1, 2x1 and 3x2, some with
# dividers and label tabs, two with scoops -- in three filaments. One 2x1 is
# lifted out of its two cells (`lift`) and tipped towards the camera (`tilt`),
# so its stepped feet face the cups they seat in, and a short cup stands in
# the stacking lip of a 1x1. Switch `mode` to "bin" and the same script is a
# single parametric bin, which is what `cadpole sweep` and the tests walk
# across sizes and divider counts.
#
# Every bin in the scene faces the same way: the drawer opens towards +y, so
# each label tab runs along a bin's -y wall and each scoop along its +y wall,
# where a finger comes in. The camera stands behind the drawer, on the tab
# side, and looks over the tabs into the scoops.
#
# Exports: an STL per bin and one for the baseplate, and a Bambu Studio
# project, gridfinity_set.3mf, with the baseplate on one plate and the bins
# laid out on a plate per filament, each bin a named object.
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
# * Each bin's hollowing subtract stays on the exact kernel; fitting the
#   inserts, welding the feet and trimming the footprint run on the mesh
#   kernel, so this is an STL and 3MF model, not a STEP one (its note 9).
# * `cadpole check`: every body is a solid (closed, oriented, no crossings). Each
#   bin's one finding is the foot's 45-degree band, which the chords of its
#   corner cones put at 45.36 degrees; at --max-overhang 46 the bins report
#   nothing. The light plate reports thin walls: its cups overlap, so the
#   ridges between them and the outer rim run to a knife edge at the bin-Z
#   relief plane -- the reference's geometry -- and its
#   0.5 mm bottom chamfer is a 45-degree band. The tipped 2x1 is checked as
#   it stands in the scene and reports overhang there; its STL and its 3MF
#   object are upright, and the tests check every bin that way.
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
lift = param("lift", 32.0, unit="mm", min=0.0, max=60.0,
             doc="How far the lowest point of the 2x1 in cells (3, 0)-(4, 0) is lifted above its seat, scene mode (0 seats it)")
tilt = param("tilt", 62.0, unit="deg", min=0.0, max=90.0,
             doc="How far the lifted 2x1 is tipped, its near edge up, so its feet show; scene mode")

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
# A bin stacked in another's lip: the lip's 45-degree band is the foot's own
# band grown by one CLEAR, starting FOOT_BOT + FOOT_MID above the wall top, so
# the two bands meet when the upper foot top is FOOT_H - CLEAR above the lower
# bin's wall top. STACK_GAP more keeps them apart rather than coincident.
STACK_SEAT = FOOT_H - CLEAR                  # 4.75
STACK_GAP = 0.2
MIN_GAP = 0.14                               # the closest any two bodies in the scene come

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


def make_bin(name, w, l, h, nx=0, ny=0, tab=False, ramp=False, turn=False):
    """A w x l x h Gridfinity bin with a stacking lip, nx/ny dividers, an
    optional label tab and scoop, in cell (0, 0) with its foot top at z = 0.
    The tab is on the +y wall and the scoop on the -y wall; `turn` swaps them
    end for end."""
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
    # ledge gridfinity_bin.star leaves there (its note 4). The chamfer is
    # carried OVERLAP on past the band, along the same ruling, so it crosses
    # the band at z = wall_top instead of ending in it: an edge lying in a
    # face is a contact the exact kernel refuses, a crossing is not.
    sock_in = FOOT_TOP - 2 * CLEAR                 # 1.9 from the body face
    blend = 2.0 * (sock_in - wall)
    r_wall, r_sock = R_OUT - wall, nr - FOOT_TOP
    run = OVERLAP / blend
    cav = [(FLOOR, wall, r_wall), (wall_top - blend, wall, r_wall),
           (wall_top + OVERLAP, sock_in + (sock_in - wall) * run, r_sock + (r_sock - r_wall) * run)]
    cavity = loft([sketch(XY.offset(z)).polygon(rounded_polygon(aw - 2 * i, al - 2 * i, r, centre))
                   for (z, i, r) in cav], ruled=True, name=name + "_cavity")
    hollow = subtract(body, [socket, cavity], name=name + "_hollow")

    # Inserts: dividers, label tab, scoop. Each one is buried OVERLAP into the
    # wall or floor it stands on, so every union is transversal.
    inserts = []
    x_in0, x_in1 = wall, aw - wall
    y_in0, y_in1 = wall, al - wall
    tab_top = wall_top - 0.6
    lip = 1.2      # the tab's flat front edge
    # Dividers stop 2 mm under the wall top, or, under a label tab, run
    # OVERLAP up into its front edge so they meet it crossing, not 0.2 mm short.
    div_top = tab_top - lip + OVERLAP if tab else wall_top - 2.0
    for k in range(1, nx + 1):
        x = x_in0 + (x_in1 - x_in0) * k / (nx + 1) - wall / 2
        inserts.append(box(wall, (y_in1 - y_in0) + 2 * OVERLAP, div_top - FLOOR + OVERLAP,
                           at=(x, y_in0 - OVERLAP, FLOOR - OVERLAP), name="%s_divx_%d" % (name, k)))
    for k in range(1, ny + 1):
        y = y_in0 + (y_in1 - y_in0) * k / (ny + 1) - wall / 2
        inserts.append(box((x_in1 - x_in0) + 2 * OVERLAP, wall, div_top - FLOOR + OVERLAP,
                           at=(x_in0 - OVERLAP, y, FLOOR - OVERLAP), name="%s_divy_%d" % (name, k)))
    if tab:
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
    b = intersect(welded, footprint, name=name + "_trimmed", kernel="mesh")
    if turn:
        b = rotate(b, 180, axis=(0, 0, 1), origin=(aw / 2, al / 2, 0), name=name + "_turned")
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
# (name, w, l, h, x dividers, y dividers, label, scoop, cell, filament, rest)
# rest: "seated" in its cells, "lifted" (tipped `tilt` degrees about its long
# axis, its lowest point `lift` mm above where its feet seat), or the name of
# the bin it stands in, seated in that bin's stacking lip.
SCENE = [
    ("parts_3x2", 3, 2, 6, dividers, 1, True, False, (0, 2), "teal", "seated"),
    ("scoop_2x1", 2, 1, 4, 0, 0, True, True, (0, 0), "teal", "seated"),
    ("tray_2x1", 2, 1, 3, 3, 0, False, False, (0, 1), "grey", "seated"),
    ("bits_1x1", 1, 1, 3, 1, 0, True, False, (2, 0), "grey", "seated"),
    ("small_1x1", 1, 1, 4, 0, 0, True, False, (2, 1), "off-white", "seated"),
    ("sorter_2x1", 2, 1, 3, 1, 0, True, False, (3, 0), "off-white", "lifted"),
    ("deep_2x1", 2, 1, 4, 1, 0, True, False, (3, 1), "teal", "seated"),
    ("drawer_2x1", 2, 1, 4, 1, 0, True, False, (3, 2), "off-white", "seated"),
    ("base_1x1", 1, 1, 5, 0, 0, False, False, (3, 3), "grey", "seated"),
    ("cup_1x1", 1, 1, 3, 0, 0, False, True, (3, 3), "off-white", "base_1x1"),
    ("tall_1x1", 1, 1, 8, 0, 0, False, False, (4, 3), "teal", "seated"),
]
SPEC = {e[0]: e for e in SCENE}


def rest_height(rest):
    """The foot top's height for a bin resting as `rest` says."""
    if rest == "seated":
        return 0.0
    if rest == "lifted":
        return lift
    under = SPEC[rest]
    return rest_height(under[10]) + bin_geometry(under[1], under[2], under[3])[2] + STACK_SEAT + STACK_GAP


def shelf_layout(sizes, width=220.0, gap=8.0):
    """Places (name, w, l) footprints in rows across a bed, longest first:
    {name: (x, y)}."""
    order = sorted(sizes, key=lambda e: (-e[2], -e[1]))
    at, x, y, row = {}, 0.0, 0.0, 0.0
    for (n, w, l) in order:
        if x > 0 and x + w > width:
            x, y, row = 0.0, y + row + gap, 0.0
        at[n] = (x, y)
        x, row = x + w + gap, max(row, l)
    return at


bins = {}       # where each bin is in the scene
parts = {}      # each bin at the origin, as it prints
plate = None
if mode == "scene":
    plate = make_plate(plate_w, plate_l)
    for (n, w, l, h, nx, ny, t, s, c, f, rest) in SCENE:
        b = pla(make_bin(n, w, l, h, nx, ny, t, s, turn=True), f)
        parts[n] = b
        if rest == "lifted" and lift > 0:
            # Tipped about the bin's long axis, near (-y) edge up, then raised
            # so its lowest point is `lift` above the seated foot bottom.
            al = PITCH * l - 2 * CLEAR
            b = rotate(b, -tilt, axis=(1, 0, 0), origin=(0, al / 2, 0), name=n + "_tipped")
            low = b.bbox()[0][2]
            bins[n] = translate(b, (c[0] * PITCH, c[1] * PITCH, -FOOT_H + lift - low), name=n + "_placed")
        else:
            bins[n] = translate(b, (c[0] * PITCH, c[1] * PITCH, rest_height(rest)), name=n + "_placed")
else:
    colour = filament
    if colour == "auto":
        colour = ["teal", "off-white", "grey"][(W - 1) % 3]
    bins["bin"] = pla(make_bin("bin", W, L, H, dividers, dividers_y, label, scoop), colour)
    parts["bin"] = bins["bin"]

# The default camera: behind the drawer and to the right (35 degrees round
# from -y towards +x, 40 up), looking over the label tabs into the scoops and
# at the lifted bin's feet, far enough back that a 24-degree lens takes the
# plate.
def default_camera(az=35.0, el=40.0, fov=24.0):
    fw, fl = PITCH * plate_w, PITCH * plate_l
    target = (fw / 2, fl / 2 - 6.0, 14.0)
    reach = 1.32 * math.sqrt(fw * fw + fl * fl) / 2 / math.tan(fov / 2)
    ce = math.cos(el)
    eye = (target[0] + reach * ce * math.sin(az), target[1] - reach * ce * math.cos(az),
           target[2] + reach * math.sin(el))
    return view(eye=eye, target=target, fov=fov)


render_defaults(camera=default_camera())

# ---------------------------------------------------------------- exports
# An STL per bin, as it prints, and one for the baseplate; and a Bambu Studio
# project: the baseplate on its own plate, then one plate per filament with
# that filament's bins laid out in rows, each bin an object named after it.
for n, b in parts.items():
    export(b, "gridfinity_%s.stl" % n)
if plate:
    export(plate, "gridfinity_baseplate_%dx%d.stl" % (plate_w, plate_l))
    project = "gridfinity_set.3mf"
    export(plate, project, flavor=flavor.bambu, plate="Baseplate")
    print_layer = layer("print_layout", doc="The bins as the 3MF lays them out on their plates", visible=False)
    for f in ["teal", "off-white", "grey"]:
        mine = [(e[0], PITCH * e[1] - 2 * CLEAR, PITCH * e[2] - 2 * CLEAR) for e in SCENE if e[9] == f]
        at = shelf_layout(mine)
        laid = [translate(parts[n], (at[n][0], at[n][1], 0), name=n, layer=print_layer) for (n, _, _) in mine]
        export(laid, project, flavor=flavor.bambu, plate="%s bins" % f.capitalize())


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
    for n, b in parts.items():
        r = print_check(b, max_overhang=46)
        assert_true(r.solid, msg="%s is a solid (closed, oriented, no crossings)" % n)
        assert_true(r.watertight, msg="%s is watertight" % n)
        assert_true(r.unsupported_area < 1.0, msg="%s needs no support" % n)
        assert_true(r.components == 1, msg="%s is one piece" % n)
        assert_true(r.fits_bed, msg="%s fits a 256 mm bed" % n)
        assert_true(r.min_wall >= 0.8 - 0.05, msg="%s has no wall under two perimeters" % n)


def material_used():
    return parts["bin" if mode == "bin" else "parts_3x2"].volume()


def bins_sit_in_the_plate():
    if mode != "scene":
        return
    plo, phi = plate.bbox()
    assert_close(phi[2], -BIN_Z_CLEAR, tol=0.02, msg="plate top one bin-Z clearance below z = 0")
    cells = {}
    for (n, w, l, h, nx, ny, t, s, c, f, rest) in SCENE:
        lo, hi = bins[n].bbox()
        # On the cell grid, inside the plate, the foot as high as it rests.
        assert_close(lo[0], c[0] * PITCH, tol=0.02, msg="%s on its cell in X" % n)
        assert_true(hi[0] <= phi[0] + 0.02 and hi[1] <= phi[1] + 0.02, msg="%s inside the plate" % n)
        assert_close(lo[2], plo[2] + rest_height(rest), tol=0.02, msg="%s lowest point where it rests" % n)
        if rest != "lifted" or lift == 0:
            assert_close(lo[1], c[1] * PITCH, tol=0.02, msg="%s on its cell in Y" % n)
        if rest == "seated" or (rest == "lifted" and lift == 0):
            # Seated: the foot's bands clear the cup's by CLEAR all round.
            d = distance(bins[n], plate)
            assert_true(not d.overlap and d.distance > 0.1, msg="%s clears its cups: %s" % (n, d))
            assert_true(d.distance < 0.3, msg="%s sits in its cups, not above them: %s" % (n, d))
        if rest in SPEC:
            # Stacked: the foot rests in the lower bin's lip. The closest
            # approach is at the corners of the two straight bands, whose
            # arcs (R1.6 on the foot, R2.1 in the lip) leave 0.14 mm there.
            d = distance(bins[n], bins[rest])
            assert_true(not d.overlap and d.distance >= MIN_GAP - 2 * d.tolerance,
                        msg="%s clears %s's lip: %s" % (n, rest, d))
            assert_true(d.distance < 0.3, msg="%s sits in %s's lip: %s" % (n, rest, d))
            continue
        for i in range(w):
            for j in range(l):
                k = (c[0] + i, c[1] + j)
                assert_true(k not in cells, msg="%s and %s claim cell %s" % (n, cells.get(k), k))
                cells[k] = n
    if (plate_w, plate_l) == (5, 4):
        assert_true(len(cells) == 20, msg="every cell of the 5 x 4 plate is a bin's")


def nothing_interferes():
    if mode != "scene":
        return
    names = [e[0] for e in SCENE]
    for a in range(len(names)):
        for b in range(a + 1, len(names)):
            d = distance(bins[names[a]], bins[names[b]])
            assert_true(not d.overlap and d.distance >= MIN_GAP - 2 * d.tolerance,
                        msg="%s and %s: %s" % (names[a], names[b], d))


def lifted_bin_shows_its_feet():
    if mode != "scene" or lift == 0:
        return
    d = distance(bins["sorter_2x1"], plate)
    assert_true(not d.overlap and d.distance >= lift - FOOT_H, msg="the 2x1 hangs clear of its cups: %s" % d)
    lo, hi = bins["sorter_2x1"].bbox()
    assert_true(lo[0] >= 3 * PITCH - 0.02 and hi[0] <= 5 * PITCH and lo[1] >= -0.02,
                msg="the 2x1 stays over its own cells, inside the plate's front edge")


def the_scene_has_every_feature():
    if mode != "scene":
        return
    sizes = [(e[1], e[2]) for e in SCENE]
    for s in [(1, 1), (2, 1), (3, 2)]:
        assert_true(s in sizes, msg="a %dx%d bin" % s)
    assert_true(len([e for e in SCENE if e[4] + e[5] > 0]) >= 3, msg="bins with dividers")
    assert_true(len([e for e in SCENE if e[6]]) >= 3, msg="bins with label tabs")
    assert_true(len([e for e in SCENE if e[7]]) >= 2, msg="bins with scoops")
    assert_true(len({e[9]: 1 for e in SCENE}) == 3, msg="three filaments")
    assert_true(len([e for e in SCENE if e[10] in SPEC]) >= 1, msg="a bin stacked in a lip")


test("the scene: every bin on its own cells, seated with clearance", bins_sit_in_the_plate)
test("the scene: seated with the lifted bin put back", bins_sit_in_the_plate, sweep={"lift": [0.0]})
test("the scene: no two bins touch", nothing_interferes)
test("the scene: the lifted 2x1 hangs over its cups", lifted_bin_shows_its_feet)
test("the scene: every size, dividers, tabs, scoops, a stack", the_scene_has_every_feature)
test("the scene: every bin closed, one piece, walls of two perimeters", every_bin_is_closed)
test("one script, every bin: sizes x dividers", every_bin_is_closed,
     sweep={"mode": ["bin"], "width": [1, 2, 3], "length": [1, 2], "dividers": [0, 1, 3]})
test("footprint is 42N - 0.5 and the lip tops out where it should", footprint_and_height,
     sweep={"mode": ["bin"], "width": [1, 3, 6], "height": [3, 6, 12]})
test("label tab and scoop at every height", every_bin_is_closed,
     sweep={"mode": ["bin"], "height": [3, 4, 8, 12], "scoop": [True], "label": [True]})
test("more dividers, more plastic", material_used,
     sweep={"dividers": [0, 1, 2, 3, 5]}, monotone=monotone.increasing)
