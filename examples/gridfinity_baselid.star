# A Gridfinity "base-lid": one solid whose feet drop into the bin or baseplate
# below, while other bins stack into the cup sockets on top. An extension of
# the standard.
#
# Frame: the cup rim plane is z = 0, the slab hangs below it and the feet hang
# below that. Millimetres. The stack:
#
#   z   0      cup rim, the lid's top face
#   z  -4.25   cup floors
#   z  -6.15   foot tops (feet penetrate 0.5 mm into the slab and fuse)
#   z  -6.40   cup magnet pocket floors
#   z  -6.65   slab bottom, R3.75 corners, 0.5 mm bottom outer chamfer
#   z -11.15   foot bottoms
#
# The cup socket is the interesting part. It is the standard foot grown
# outward by a *true normal* offset of 0.25 mm, and on a 45
# degree chamfer a normal offset is not a lateral one: it shortens the
# vertical span by t*(sqrt(2)-1) and shifts the cone sideways by t*sqrt(2) at
# fixed z. That is what turns the foot's 0.8 / 1.8 / 2.4 stack into the
# cup stack 0.6964 / 1.8 / 1.7536. A lateral offset leaves 0.8 as 0.8, and
# the bins do not seat.
#
# Everything here is `loft` through rounded-rectangle sections, so the cones
# are exact, not stepped.
#
# `kernel="brep"` may refuse the union of the slab and the feet:
#
#   union failed (the boundaries meet, but not transversally: coincident or
#   tangent faces are a known kernel limitation)
#
# The slab's 0.5 mm bottom outer chamfer and the perimeter feet's own chamfer
# are the *same surface*: the slab ends at 0.25 mm in at z = -6.65 and so does
# the foot. The default `kernel="auto"` routes this union to the mesh kernel.
# The result is watertight and the right volume; what it costs is a few dozen
# sliver triangles along that shared chamfer, which `cadpole check` reports
# as self-intersections, and the STEP export below.

W = param("width", 2, min=1, max=8, doc="Grid units along X (42 mm each)")
L = param("length", 2, min=1, max=8, doc="Grid units along Y")
magnets = param("magnets", True, doc="6.15 mm magnet pockets in the cups and the feet")
vent = param("vent", 0.0, unit="mm", min=0.0, max=30.0,
             doc="Diameter of a hole through the centre of every cup, floor to foot bottom (0 = none). Not in the reference; it drains and vents a lid used as a tray")
kernel = param("kernel", "auto", choices=["auto", "brep", "mesh"],
               doc="Geometry kernel for the booleans; \"brep\" keeps exact surfaces for STEP")

# Gridfinity constants.
PITCH = 42.0
CLEAR = 0.25                     # xy clearance per side
CORNER_R = 4.0                   # corner fillet radius
FOOT_TOP = 2.4                   # the 45 deg top band
FOOT_MID = 1.8                   # the straight band
FOOT_BOT = 0.8                   # the 45 deg bottom band
FOOT_H = FOOT_TOP + FOOT_MID + FOOT_BOT      # 5.0
MAG_D = 6.15                     # magnet pocket diameter
MAG_OFF = 8.0                    # pocket centre, from each cell corner

# The base-lid's own dimensions.
SLAB_H = 6.65                    # rim to slab bottom
SLAB_CHAMFER = 0.5               # bottom outer chamfer
CUP_DEPTH = 4.25                 # rim to cup floor
SOCKET_CLEAR = 0.25              # the socket's normal offset from the foot
CUP_MAG_DEPTH = 2.15             # below the cup floor, opening upward
FOOT_MAG_DEPTH = 2.30            # the bin standard is 2.40
FOOT_OVERLAP = 0.5               # how far the feet penetrate the slab

# sqrt(2) - 1, the factor a normal offset shortens a 45 degree span by. Written
# as arithmetic rather than as a literal so it stays right if anything here
# ever stops being 45 degrees.
CHAMFER_SHRINK = math.sqrt(2) - 1

full_w = W * PITCH
full_l = L * PITCH
# The slab is FULL pitch and not inset, unlike a bin body: it spans
# [-clearance, N*pitch - clearance], so a 2x2 measures 84 x 84.
slab_at = (-CLEAR, -CLEAR)
slab_centre = (full_w / 2 - CLEAR, full_l / 2 - CLEAR)
R = CORNER_R - CLEAR             # 3.75 on the slab's own corners


def rounded_section(w, h, r, z, centre):
    """One rounded-rectangle section of a loft, on the plane z."""
    return sketch(XY.offset(z)).rounded_rect(w, h, max(0.05, r), center=centre)


# ------------------------------------------------------------------ the slab
# Top at z = 0, bottom at -6.65, with the outer bottom edge chamfered 0.5. The
# chamfer is a loft section rather than a fillet operation: a chamfer of a
# prism's bottom edge is just a smaller rounded rectangle one chamfer-height up.
slab = loft([
    rounded_section(full_w - 2 * SLAB_CHAMFER, full_l - 2 * SLAB_CHAMFER, R - SLAB_CHAMFER,
                    -SLAB_H, slab_centre),
    rounded_section(full_w, full_l, R, -SLAB_H + SLAB_CHAMFER, slab_centre),
    rounded_section(full_w, full_l, R, 0.0, slab_centre),
], ruled=True, name="slab")


# ------------------------------------------------------------- the cup socket
# The foot profile, offset outward by a true normal offset. `delta` is how much
# wider than the foot's straight band each section is; on the straight band it
# is the offset itself, and on a cone it is the offset plus however far the
# cone has travelled from the band.
straight_half = PITCH / 2 - FOOT_TOP + SOCKET_CLEAR            # 18.85
r_straight = (CORNER_R - FOOT_TOP) + SOCKET_CLEAR              # 1.85
cup_bottom_chamfer = FOOT_BOT - SOCKET_CLEAR * CHAMFER_SHRINK  # 0.6964
z_floor = -CUP_DEPTH                                           # -4.25
z_chamfer = z_floor + cup_bottom_chamfer                       # -3.5536
z_cone = z_chamfer + FOOT_MID                                  # -1.7536
z_mouth = 1.0                                                  # the cone runs past the rim

CUP_SECTIONS = [
    (z_floor, -cup_bottom_chamfer),
    (z_chamfer, 0.0),
    (z_cone, 0.0),
    (z_mouth, z_mouth - z_cone),
]


def cup(cx, cy, name):
    """One cup socket, as the tool that will be cut out of the slab."""
    sections = []
    for (z, delta) in CUP_SECTIONS:
        half = straight_half + delta
        sections.append(rounded_section(2 * half, 2 * half, r_straight + delta, z, (cx, cy)))
    return loft(sections, ruled=True, name=name)


# ---------------------------------------------------------------- the feet
# The standard foot, top face at the slab bottom plus the overlap so it fuses
# into the slab rather than merely touching it.
foot_top_z = -SLAB_H + FOOT_OVERLAP                            # -6.15
FOOT_PROFILE = [
    (-FOOT_H, FOOT_TOP + FOOT_BOT),          # bottom face: inset 3.2, R0.8
    (-(FOOT_TOP + FOOT_MID), FOOT_TOP),      # bottom of the straight band
    (-FOOT_TOP, FOOT_TOP),                   # top of the straight band
    (0.0, 0.0),                              # the nominal top face: full pitch, R4
    (FOOT_OVERLAP, 0.0),                     # the prism that buries into the slab
]


def foot(cx, cy, name):
    sections = []
    for (dz, inset) in FOOT_PROFILE:
        size = PITCH - 2 * inset
        sections.append(rounded_section(size, size, CORNER_R - inset, foot_top_z + dz, (cx, cy)))
    return loft(sections, ruled=True, name=name)


# ------------------------------------------------------------------ assembly
# One cup and one foot per cell, at the cell centres.
cells = []
for i in range(W):
    for j in range(L):
        cells.append((PITCH / 2 + i * PITCH - CLEAR, PITCH / 2 + j * PITCH - CLEAR, i, j))

cups = [cup(cx, cy, "cup_%d_%d" % (i, j)) for (cx, cy, i, j) in cells]
feet = [foot(cx, cy, "foot_%d_%d" % (i, j)) for (cx, cy, i, j) in cells]

hollow = subtract(slab, cups, name="hollow", kernel=kernel)
lid = union(hollow, feet, name="lid", kernel=kernel)

# ------------------------------------------------------- magnet pockets, both sides
# Cut last, and from the fused solid, not from the bare slab. The foot
# tops reach 0.5 mm up into the slab, so a cup pocket drilled before the fuse
# would be filled in again from below; drilling the combined solid
# leaves a 2.45 mm web between the two pockets.
tools = []
if vent > 0:
    # Through the cup floor the whole way down and out of the foot, so it
    # drains. One tall cylinder per cell, started below the foot bottom and
    # run past the cup floor into the cup void, so neither end lands flush on
    # a face -- a coincident end is the contact case the exact kernel refuses.
    for (cx, cy, i, j) in cells:
        tools.append(cylinder(vent / 2, SLAB_H + FOOT_H + 2.0,
                              at=(cx, cy, foot_top_z - FOOT_H - 1.0),
                              name="vent_%d_%d" % (i, j)))

if magnets:
    for (cx, cy, i, j) in cells:
        for dx in (-PITCH / 2 + MAG_OFF, PITCH / 2 - MAG_OFF):
            for dy in (-PITCH / 2 + MAG_OFF, PITCH / 2 - MAG_OFF):
                at = (cx + dx, cy + dy)
                # Cup side: opens upward into the cup void, floor at -6.40.
                tools.append(cylinder(MAG_D / 2, CUP_MAG_DEPTH + 0.005,
                                      at=(at[0], at[1], z_floor - CUP_MAG_DEPTH),
                                      name="cup_magnet_%d_%d_%s_%s" % (i, j, str(dx), str(dy))))
                # Foot side: opens downward at the foot bottom.
                tools.append(cylinder(MAG_D / 2, FOOT_MAG_DEPTH + 0.005,
                                      at=(at[0], at[1], foot_top_z - FOOT_H - 0.005),
                                      name="foot_magnet_%d_%d_%s_%s" % (i, j, str(dx), str(dy))))

if len(tools) > 0:
    lid = subtract(lid, tools, name="baselid", kernel=kernel)

lid.material(color="#7f9db9", roughness=0.5)

# ------------------------------------------------------------------ checks
# Against the nominal numbers, not against what this script just
# computed: a 2x2 lid is 84 x 84, and spans the rim down to the foot bottoms.
lo, hi = lid.bbox()
assert_true(math.close(hi[0] - lo[0], full_w, tol=0.02), msg="slab width is full pitch")
assert_true(math.close(hi[1] - lo[1], full_l, tol=0.02), msg="slab depth is full pitch")
assert_true(math.close(hi[2], 0.0, tol=0.02), msg="the cup rim is the top face, at z = 0")
assert_true(math.close(lo[2], foot_top_z - FOOT_H, tol=0.02), msg="the feet reach -11.15")

export(lid, "gridfinity_baselid_%dx%d.stl" % (W, L))
if kernel == "brep":
    # Exact surfaces; see the note on `kernel="brep"` at the top.
    export(lid, "gridfinity_baselid_%dx%d.step" % (W, L))
