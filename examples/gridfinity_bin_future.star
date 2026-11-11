# cadpole: planned-api -- a sketch, not a runnable example.
#
# A Gridfinity bin written the way a B-rep CAD script would build it: foot
# loft + hole cluster, feet pattern, body, compartments (scoop, floor fillets,
# label tab), stacking lip (band - inverted foot - recess - wall-to-lip
# frustum), fuse, checks, export. Frame: body corner at the origin, feet below
# z = 0. examples/gridfinity_bin.star is the bin that runs.
#
# STATUS. The examples test skips this file (the marker on the first line and
# the `_future.star` name). Every builtin it calls exists, but the script does
# not pass its own checks:
#
#   - the "footprint" assertion: the feet are full-pitch and untrimmed, so the
#     bin measures 84.0 mm across where 83.5 is expected. Trimming each foot to
#     the body outline needs a boolean between coincident faces.
#   - the STEP export: the floor fillet goes through the field path, so the
#     compartment cutter, and with it the bin, is a mesh body, and STEP export
#     needs exact surfaces. Printing (STL/3MF) is unaffected.

W = param("width", 2, min=1, max=8)
L = param("length", 3, min=1, max=8)
H = param("height", 6, min=2, max=20)
wall = param("wall", 1.2, unit="mm", min=0.8, max=3.0)
lip = param("lip", True)
magnets = param("magnets", True, doc="6.15 mm magnet sockets in every foot corner")
screws = param("screws", False, doc="3.0 mm screw holes concentric with the magnets")
scoop = param("scoop", False)
tab = param("label_tab", False)
nx = param("compartments_x", 1, min=1, max=6)
ny = param("compartments_y", 1, min=1, max=6)

PITCH, UNIT, CLEAR, CORNER_R = 42.0, 7.0, 0.25, 4.0
FOOT_TOP, FOOT_MID, FOOT_BOT = 2.4, 1.8, 0.8
FOOT_H = FOOT_TOP + FOOT_MID + FOOT_BOT
FLOOR = 1.0
LIP_H, LIP_RECESS = 4.4, 0.6
MAG_OFF = 8.0                    # magnet grid: 8.0 from each cell corner (7.75 in the body frame)
MAG_D, MAG_DEPTH, MAG_CHAMFER = 6.15, 2.4, 0.2
SCREW_D = 3.0
GROOVE = 0.6                     # keyhole print-helper groove above the magnet
SCOOP_R = 25.0
TAB_W, TAB_R, TAB_CLEAR = 13.0, 0.6, 0.5
TAN_22_5 = 0.41421356            # tan(22.5 deg)
TAB_SIDE = TAB_W + TAB_R / TAN_22_5   # 14.449: legs of the right-triangle tab profile

aw, al = PITCH * W - 2 * CLEAR, PITCH * L - 2 * CLEAR
R = CORNER_R - CLEAR
centre = (aw / 2, al / 2)
r_in = max(2.0, R - wall)        # 2.55: compartment corner and floor fillet
total_h = (H - 1) * UNIT + max(0, UNIT - FOOT_H)


def ring(size, inset, z, at):
    """A rounded rectangle inset from a `size` square footprint, at height z."""
    return sketch(XY.offset(z)).rounded_rect(size - 2 * inset, size - 2 * inset,
                                              CORNER_R - inset, center=at)


# ---------------------------------------------------------------- Stage 1: foot
def foot(cx, cy, z_top, name):
    """One 42 mm foot: a ruled loft through four rounded-rectangle sections,
    top face at z_top, then the magnet / screw / keyhole cluster pinwheeled x4
    about the foot centre."""
    sections = [
        ring(PITCH, FOOT_TOP + FOOT_BOT, z_top - FOOT_H, (cx, cy)),      # bottom face: inset 3.2, R0.8
        ring(PITCH, FOOT_TOP, z_top - FOOT_TOP - FOOT_MID, (cx, cy)),   # straight band bottom
        ring(PITCH, FOOT_TOP, z_top - FOOT_TOP, (cx, cy)),              # straight band top
        ring(PITCH, 0.0, z_top, (cx, cy)),                              # top face: full pitch, R4
    ]
    f = loft(sections, ruled=True, name=name)
    if not (magnets or screws):
        return f
    z0 = z_top - FOOT_H
    mx, my = cx - PITCH / 2 + MAG_OFF, cy - PITCH / 2 + MAG_OFF
    tools = []
    if screws:
        tools.append(cylinder(SCREW_D / 2, FOOT_H + 1.0, at=(mx, my, z0 - 0.5), name=name + "_screw"))
    if magnets:
        tools.append(cylinder(MAG_D / 2, MAG_DEPTH + 0.5, at=(mx, my, z0 - 0.5), name=name + "_magnet"))
        # 0.2 x 45 degree lead-in at the bed-side mouth.
        tools.append(cone(MAG_D / 2 + MAG_CHAMFER, MAG_D / 2, MAG_CHAMFER, at=(mx, my, z0), name=name + "_mouth"))
        if screws:
            # Keyhole groove: the stepped slot is ONE L-shaped profile,
            # intersected with the groove cylinder and rotated -45 degrees
            # toward the foot centre.
            g0 = z0 + MAG_DEPTH
            # Lower layer: x from hole + md/2, md long; upper layer: x from hole + sd/2,
            # sd long; both 0.3 tall, sd wide in y.
            lo_x, hi_x = mx + MAG_D / 2, mx + SCREW_D / 2
            prof = (path((lo_x, g0)).h(lo_x + MAG_D).v(g0 + GROOVE / 2).h(hi_x + SCREW_D)
                    .v(g0 + GROOVE).h(hi_x).v(g0 + GROOVE / 2).h(lo_x))
            slot = extrude(sketch(XZ.offset(-(my + SCREW_D / 2))).add(prof), SCREW_D,
                           name=name + "_slot")
            groove = cylinder(MAG_D / 2, GROOVE, at=(mx, my, g0), name=name + "_groove")
            tools.append(rotate(intersect(groove, slot), -45, origin=(mx, my, 0)))
    cluster = union(tools, name=name + "_cluster") if len(tools) > 1 else tools[0]
    pinwheel = pattern_circular(cluster, 4, origin=(cx, cy, 0), name=name + "_pin")
    return subtract(f, pinwheel, name=name + "_drilled")


foot0 = foot(PITCH / 2 - CLEAR, PITCH / 2 - CLEAR, 0.0, "foot")
# merge=True unions the copies into one multi-lump body (feet only touch along
# their top edges).
feet = pattern_linear(pattern_linear(foot0, W, (PITCH, 0, 0), merge=True),
                      L, (0, PITCH, 0), merge=True, name="feet")

# ---------------------------------------------------------------- Stage 2: body
body = extrude(sketch(XY).rounded_rect(aw, al, R, center=centre), total_h, name="body")

# ---------------------------------------------------------------- Stage 3: compartments
cuts = []
merges = []
min_x, max_x = wall, aw - wall
min_y = (FOOT_TOP - 2 * CLEAR) if (lip and scoop) else wall     # 1.9: the scoop side thins to the lip wall
max_y = al - wall
cw = (max_x - min_x - (nx - 1) * wall) / nx
cl = (max_y - min_y - (ny - 1) * wall) / ny
depth = total_h - FLOOR

for i in range(nx):
    for j in range(ny):
        x0, y0 = min_x + i * (cw + wall), min_y + j * (cl + wall)
        name = "comp_%d_%d" % (i, j)
        cut = extrude(sketch(XY.offset(FLOOR)).rounded_rect(cw, cl, r_in, center=(x0 + cw / 2, y0 + cl / 2)),
                      depth + 1.0, name=name)
        if scoop:
            r = min(SCOOP_R, min(depth, cl) - 0.1)      # strictly smaller than both adjacent extents
            front = sorted(cut.edges(on_plane=XY.offset(FLOOR), direction=(1, 0, 0)),
                           key=lambda e: e.start[1])[0]
            cut = fillet(front, r, name=name + "_scoop")
        # Floor edges as one tangent chain.
        cut = fillet(cut.edges(on_plane=XY.offset(FLOOR)), r_in, name=name + "_floor")
        cuts.append(cut)
        if tab:
            # Right-triangle profile in a YZ plane hanging from the back wall's top,
            # 0.5 mm below it; the R0.6 front edge runs along X, so it is a 2D fillet
            # of the profile: tangent_arc. The tab overlaps 0.5 mm into the
            # side walls and the back wall so the fuse is transversal.
            zt = total_h - TAB_CLEAR
            yb = y0 + cl + 0.5
            t = TAB_R / TAN_22_5                        # 1.449: tangent length at the 45-degree corner
            k = 0.70710678
            prof = (path((yb, zt - TAB_SIDE)).line((yb, zt)).line((yb - TAB_SIDE + t, zt))
                    .tangent_arc((yb - TAB_SIDE + t * k, zt - t * k)))
            merges.append(extrude(sketch(YZ.offset(x0 - 0.5)).add(prof), cw + 1.0, name=name + "_tab"))
if nx * ny > 1:
    # 0.5 mm top relief across all compartments so divider tops sit below the rim.
    cuts.append(extrude(sketch(XY.offset(total_h - TAB_CLEAR)).rounded_rect(max_x - min_x, max_y - min_y, r_in,
                                                                             center=((min_x + max_x) / 2, (min_y + max_y) / 2)),
                        TAB_CLEAR + 1.0, name="relief"))

# ---------------------------------------------------------------- Stage 4: stacking lip
if lip:
    band = extrude(sketch(XY.offset(total_h)).rounded_rect(aw, al, R, center=centre), LIP_H, name="lip")
    # Inverted foot, no bottom chamfer, footprint pitch + 2 clearances, corner R4.5,
    # top face 5 mm above the wall top; its bottom is 0.5 mm below the band so no
    # faces coincide.
    neg_size = PITCH * W + 2 * CLEAR
    negative = loft([
        ring(neg_size, FOOT_TOP, total_h - 0.5, centre),
        ring(neg_size, FOOT_TOP, total_h + FOOT_BOT + FOOT_MID, centre),
        ring(neg_size, 0.0, total_h + FOOT_H, centre),
    ], ruled=True, name="lip_negative")
    # Top recess: sketch on the band's top face and cut 0.6 mm down (the
    # "cut from a face" idiom; the cut starts on a face coplanar with the band).
    recess = extrude(sketch(on=band.face("lip/end")).rounded_rect(aw + 2, al + 2, R), -LIP_RECESS,
                     name="lip_recess")
    # Wall-to-lip blend: a 45-degree frustum from the cavity outline up by r_in
    # (a loft, because a chamfer equal to the face height is degenerate). Scoop
    # bins keep the front wall vertical.
    # The top section is a 4-segment rectangle and the bottom an 8-segment
    # rounded one: match="arclen" splits both at the union of their vertices
    # so the ruled bands still pair corner to corner.
    blend = loft([
        sketch(XY.offset(total_h - 0.5)).rounded_rect(aw - 2 * wall, al - 2 * wall, r_in, center=centre),
        sketch(XY.offset(total_h + r_in)).rounded_rect(aw - 2 * wall - 2 * r_in, al - 2 * wall - 2 * r_in, 0.0, center=centre),
    ], ruled=True, match="arclen", name="lip_blend")
    merges.append(subtract(band, negative, recess, blend, name="lip_cut"))

# ---------------------------------------------------------------- Stage 5: fuse, check, export
# Feet meet the body on z = 0 and the lip meets it on the wall top: coplanar
# contacts.
bin = subtract(union(body, feet, merges, name="shell"), cuts, name="bin")
bin.material(color="#d9a441", roughness=0.7)

# Expected for a 2x3x6 bin: 83.5 x 125.5, z from -5 to 40.8.
assert_true(abs(bin.bbox()[1][0] - bin.bbox()[0][0] - aw) < 0.01, msg="footprint")
print_check(bin, min_wall=1.0, max_overhang=50)
export(bin, "gridfinity_bin.step")
export(bin, "gridfinity_bin.3mf")
