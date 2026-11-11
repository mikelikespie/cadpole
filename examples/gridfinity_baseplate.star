# A Gridfinity baseplate: the cups-up plate a bin's feet drop into. W x L grid
# units (42 mm), in the three types the reference generator ships -- `light`,
# `skeletonized` and `full` -- with magnet sockets, screw holes and their
# countersinks, connection holes in the sides, side padding and the bin-Z
# clearance relief.
#
# The reference is the Fusion Gridfinity generator add-in. Every dimension
# below is one of its constants, named in capitals beside the value. Where
# this script builds something differently from the reference, a comment at
# that point says so.
#
# Frame (the reference's own): the *nominal* cup top plane is z = 0, the plate
# hangs below it, and cell (i, j) has its cup axis at
# (42i + 20.75, 42j + 20.75). The plate spans x in [0, 42W - 0.5]. A bin's foot
# top is also z = 0, so a seated bin and the plate share one origin -- which is
# what makes the fit assertions below a measurement rather than arithmetic.
#
# THE CUP IS EXACT, and it is the inverse of the bin's foot. A Gridfinity foot
# is the ruled loft of four rounded rectangles; the baseplate's socket is the
# same loft grown by one xy clearance per side, so its two 45-degree bands are
# cones at the corners and planes along the flats. `loft()` builds it
# directly -- no chamfer operation is involved and nothing is stepped. (A
# full-height 45-degree chamfer would collapse adjacent box chamfers into
# knife edges.)
#
# NO CHAMFER OPERATION IS ON THIS PART'S PATH. Both of the reference's
# chamfers are *ends* of a solid rather than blends of an existing one, so
# both are constructions: the plate's 0.5 mm bottom perimeter chamfer is a
# third loft section, 0.5 mm in and 0.5 mm below the full one, and the screw
# countersink is a cylinder plus a `cone`. A 45-degree chamfer of a filleted
# vertical edge is a cone, which is exactly what the loft's corner bands
# already are, so lofting it is exact.
#
# What this script does NOT reproduce, and why:
#
# 1. Face merging. The reference merges co-planar faces after every cut, which
#    keeps its outer wall one face instead of a dozen co-planar strips.
#    Cadpole has no face-merge pass, so the plate carries more faces than the
#    reference's and the render draws more crease lines across flat areas.
#    The solid is identical.
# 2. The reference cuts each of its ~90 tools out of the slab one at a time.
#    Here every cutter is fused into ONE body first (`pattern_linear(...,
#    merge=True)` plus one `union`) and the plate is a single `subtract`.
#    That is not a fidelity difference -- the regularised A - (B u C) and
#    (A - B) - C are the same solid -- but it is much faster, since a
#    multi-body `subtract` is sequential.
# 3. `kernel="brep"` is not offered. Two of the booleans here are exactly the
#    non-transversal cases the exact kernel refuses: the cup cutters of
#    adjacent cells share a face where they overlap, and the screw bore lands
#    flush on the cup floor. The mesh kernel takes both, so the plate is a
#    mesh body and cannot export STEP.
# 4. `cadpole check` reports one error -- self-intersecting triangle PAIRS --
#    and it is the false positive `gridfinity_bin.star` note 8 describes, not
#    a hole in the solid. The count tracks where the body sits (f32 mesh
#    storage) and the triangle count, not the geometry: two chord
#    approximations of surfaces meeting at a shallow angle cross by a fraction
#    of the chord error. The plate is watertight, has no non-manifold and no
#    reversed edges, and its volume is right.
# 5. `cadpole check`'s other three findings are all the same feature: thin
#    wall, overhang past 45 degrees and some zero-area triangles, all
#    of them on the outer rim at z = -0.5. That rim is a FEATHER EDGE, and it
#    is the reference's geometry rather than a mistake here: the reference
#    makes its plate 42N - 0.5 -- a *bin's* footprint -- so the outer cup's
#    45-degree wall reaches the plate's outer face exactly at the bin-Z relief
#    plane and the top face has zero width there. (The published Gridfinity
#    baseplate is 42N, which is what gives a real plate its 0.25 mm top rim.
#    The reference gives 83.5 x 125.5 for a 2x3.) A slicer prints a
#    feather edge as the last layer of a 45-degree ramp, not as a standing
#    wall; a triangle mesh represents it as slivers, which is where the
#    zero-area triangles and the one 90-degree facet come from.
# 6. The reference's connection holes always start 1 mm outside the *nominal*
#    edge, which buries them inside the padding when padding is on (they
#    become a closed void). Here they start outside the *padded* face and
#    still reach the reference's 21 mm depth from the nominal edge, so they
#    open.
#
# Booleans take Bodies or lists of Bodies; pass a list rather than a splat so
# `name=` still parses (see `gridfinity_bin.star`).

W = param("width", 2, min=1, max=8, doc="Grid units along X (42 mm each)")
L = param("length", 3, min=1, max=8, doc="Grid units along Y (42 mm each)")
plate_type = param("type", "light", choices=["light", "skeletonized", "full"],
                   doc="light: the 5 mm frame alone. full: a solid 6.4 mm slab under it. skeletonized: that slab with a cross-shaped pocket milled out of every cell, keeping only the magnet pads and the ribs between them")
magnets = param("magnets", True, doc="Magnet sockets under each cup's four corners. The reference drills them only on a plate that HAS the bottom extension, so a `light` plate never gets them")
corner_magnets_only = param("corner_magnets_only", False, doc="Magnets and screws on the four corner cells only, not on every cell")
screws = param("screws", True, doc="Screw holes through the bottom extension, with a countersink on the underside. Extended-bottom plates only, as for magnets")
bottom_extension = param("bottom_extension", 6.4, unit="mm", min=0.0, max=20.0,
                         doc="Extra thickness under the 5 mm cup frame on `full` and `skeletonized` plates (BASEPLATE_EXTRA_HEIGHT)")
bin_z_clearance = param("bin_z_clearance", 0.5, unit="mm", min=0.0, max=2.0,
                        doc="How much is shaved off the plate's top face so a seated bin's underside clears it (BASEPLATE_BIN_Z_CLEARANCE)")
pad_left = param("pad_left", 0.0, unit="mm", min=0.0, max=50.0, doc="Side padding on -X")
pad_right = param("pad_right", 0.0, unit="mm", min=0.0, max=50.0, doc="Side padding on +X")
pad_bottom = param("pad_bottom", 0.0, unit="mm", min=0.0, max=50.0, doc="Side padding on -Y")
pad_top = param("pad_top", 0.0, unit="mm", min=0.0, max=50.0, doc="Side padding on +Y")
connection_holes = param("connection_holes", False, doc="A bolt hole through each edge cell's side wall, for joining plates")
connection_hole_diameter = param("connection_hole_diameter", 3.2, unit="mm", min=1.0, max=8.0,
                                 doc="Connection hole diameter (DIMENSION_PLATE_CONNECTION_SCREW_HOLE_DIAMETER)")
demo_bin = param("demo_bin", False, doc="Drop a bin into cell (0, 0) to show the fit. Its foot is the real four-section loft; the body above it is a plain hollow block, not the full bin")
kernel = param("kernel", "auto", choices=["auto", "mesh"],
               doc="Geometry kernel for the booleans. `brep` is not offered: see note 3 in the header")

# ---------------------------------------------------------------- constants
# The reference's constants. The add-in works in centimetres; these are
# millimetres.
PITCH = 42.0            # DIMENSION_DEFAULT_WIDTH_UNIT
HEIGHT_UNIT = 7.0       # DIMENSION_DEFAULT_HEIGHT_UNIT (the demo bin only)
WALL = 1.2              # BIN_WALL_THICKNESS (the demo bin only)
CLEAR = 0.25            # BIN_XY_CLEARANCE, per side
CORNER_R = 4.0          # BIN_CORNER_FILLET_RADIUS (nominal; 3.75 on the outside)
FOOT_TOP = 2.4          # BIN_BASE_TOP_SECTION_HEIGHT   (45-degree band)
FOOT_MID = 1.8          # BIN_BASE_MID_SECTION_HEIGHT   (straight band)
FOOT_BOT = 0.8          # BIN_BASE_BOTTOM_SECTION_HEIGHT(45-degree band)
BASE_H = FOOT_TOP + FOOT_MID + FOOT_BOT          # BIN_BASE_HEIGHT = 5.0
SCREW_OFFSET = 8.0      # DIMENSION_SCREW_HOLES_OFFSET, from each cell corner
SCREW_D = 3.2           # DIMENSION_PLATE_SCREW_HOLE_DIAMETER
HEAD_D = 6.0            # DIMENSION_SCREW_HEAD_CUTOUT_DIAMETER
HEAD_STRAIGHT = 0.5     # DIMENSION_SCREW_HEAD_CUTOUT_OFFSET_HEIGHT
MAGNET_D = 6.15         # DIMENSION_MAGNET_CUTOUT_DIAMETER (6.05 oversized 0.10 for print fit)
MAGNET_DEPTH = 2.4      # DIMENSION_MAGNET_CUTOUT_DEPTH
BOT_CHAMFER = 0.5       # the reference's 0.5 mm chamfer of the slab's bottom perimeter

# The skeletonized pocket.
SPIDER_R = max(MAGNET_D, HEAD_D) / 2 + 1.0       # 4.075; Fusion's "+0.1 cm"
RIB_INSET = FOOT_TOP + FOOT_BOT - 2 * CLEAR      # 2.70: the cup's own bottom-face inset

OVERLAP = 0.5           # boolean overlap; the kernel tolerance is 0.05
SQRT_HALF = 0.7071067811865476                   # sqrt(1/2)

# Derived, exactly as the reference derives them.
extended = plate_type != "light"                 # BaseplateParams.has_extended_bottom
skeleton = plate_type == "skeletonized"          # BaseplateParams.has_skeletonized_bottom
ext_h = bottom_extension if extended else 0.0
slab_h = BASE_H + ext_h                          # 5.0 light, 11.4 full/skeletonized
true_w = PITCH * W - 2 * CLEAR                   # 83.5 for W = 2
true_l = PITCH * L - 2 * CLEAR                   # 125.5 for L = 3
R_OUT = CORNER_R - CLEAR                         # 3.75, the outer corner radius
fw = true_w + pad_left + pad_right               # padded footprint
fl = true_l + pad_bottom + pad_top
fcentre = ((true_w - pad_left + pad_right) / 2, (true_l - pad_bottom + pad_top) / 2)

# The cup: the foot grown by one clearance per side, top face at z = 0.
# The reference builds its socket from a 42.5 mm footprint at corner R4.25 with
# its origin at (-0.5, -0.5), so the cup axis lands on the cell's foot axis.
CUP_SIZE = PITCH + 2 * CLEAR                     # 42.5
CUP_R = CORNER_R + CLEAR                         # 4.25
CELL_AXIS = PITCH / 2 - CLEAR                    # 20.75, the cup and magnet-grid centre

# (z, inset from the 42.5 mm footprint). The corner radius is always
# CUP_R - inset, which is what makes each 45-degree band a cone at the corners.
# Note what the reference does NOT do: this is an *xy* offset of the foot, not
# a normal offset, so the z breakpoints are the foot's unchanged and the true
# clearance across a 45-degree face is 0.25/sqrt(2) = 0.177 mm, not 0.25. The
# base *lid* (`gridfinity_baselid.star`) is the one that takes the normal
# offset; the baseplate does not.
CUP_PROFILE = [
    (-BASE_H, FOOT_TOP + FOOT_BOT),              # z = -5.0, inset 3.2, R 1.05
    (-(FOOT_TOP + FOOT_MID), FOOT_TOP),          # z = -4.2, inset 2.4, R 1.85
    (-FOOT_TOP, FOOT_TOP),                       # z = -2.4, inset 2.4, R 1.85
    (0.0, 0.0),                                  # z =  0.0, full 42.5, R 4.25
    (OVERLAP, 0.0),                              # above the slab top, so the cut is transversal
]

# The reference's own defaults are exactly this -- type=light with magnets and
# screws both on -- and it quietly drills neither, because both bores live in
# the bottom extension a light plate does not have. Say so once rather than
# leave someone hunting for the holes.
if (magnets or screws) and not extended:
    warn("a `light` baseplate gets no magnet sockets and no screw holes: both bores live in " +
         "the bottom extension, and a light plate has none. " +
         "Use type=full or type=skeletonized")


def ring(size, r, z, at):
    """One rounded-rectangle section of a loft, centred on `at`."""
    return sketch(XY.offset(z)).rounded_rect(size, size, r, center=at)


def rect_ring(w, h, r, z, at):
    return sketch(XY.offset(z)).rounded_rect(w, h, r, center=at)


# ---------------------------------------------------------------- the slab
# One loft: bottom section 0.5 mm in and 0.5 mm down, then the full section
# twice. That IS the reference's `makeChamfer(0.5, perimeter)` -- see the
# header -- and it is why the plate needs no chamfer operation.
slab = loft([
    rect_ring(fw - 2 * BOT_CHAMFER, fl - 2 * BOT_CHAMFER, R_OUT - BOT_CHAMFER,
              -slab_h, fcentre),
    rect_ring(fw, fl, R_OUT, -slab_h + BOT_CHAMFER, fcentre),
    rect_ring(fw, fl, R_OUT, 0.0, fcentre),
], ruled=True, name="slab")

# Padding is one rounded rectangle, not four fused pads. The reference fuses a
# box per side, fillets the outer corners at R3.75 and then intersects the
# whole plate with a single R3.75 rounded rectangle over the padded extent --
# so the pads and their fillets never survive into the result. Drawing the
# padded outline directly is the same solid and three fewer booleans.


# ---------------------------------------------------------------- the cups
def cup_at(at, name=None):
    sections = []
    for (z, inset) in CUP_PROFILE:
        sections.append(ring(CUP_SIZE - 2 * inset, CUP_R - inset, z, at))
    return loft(sections, ruled=True, name=name)


cup = cup_at((CELL_AXIS, CELL_AXIS), name="cup")

# Cups of adjacent cells overlap by 2 x CLEAR where they meet (42.5 wide on a
# 42 pitch), so they share real area and merge cleanly; that is the same
# requirement `gridfinity_bin.star` note 3 spells out for the feet, met here
# by the geometry itself rather than by an added overlap.
cups = pattern_linear(pattern_linear(cup, W, (PITCH, 0, 0), merge=True, name="cup_col"),
                      L, (0, PITCH, 0), merge=True, name="cups")
grid_lo, grid_hi = cups.bbox()       # measured before anything else touches it


# ---------------------------------------------------------------- the foot
# The bin foot the cup is the inverse of: `examples/gridfinity_bin.star`'s
# FOOT_PROFILE, verbatim, at the full 42 mm pitch with its top face at z = 0.
FOOT_PROFILE = [
    (-BASE_H, FOOT_TOP + FOOT_BOT),              # bottom face: inset 3.2, R 0.8
    (-(FOOT_TOP + FOOT_MID), FOOT_TOP),          # bottom of the straight band
    (-FOOT_TOP, FOOT_TOP),                       # top of the straight band
    (0.0, 0.0),                                  # top face: full pitch, R 4.0
]


def foot_at(at, name=None, profile=FOOT_PROFILE):
    return loft([ring(PITCH - 2 * inset, CORNER_R - inset, z, at)
                 for (z, inset) in profile], ruled=True, name=name)


# The same foot carried OVERLAP mm above its top face, so it can be welded to a
# body that starts at z = 0 with real shared volume instead of one shared face.
# `gridfinity_bin.star` note 3 is the long version of why that matters. Only
# the demo bin uses this; the plate's fit measurement uses the true foot.
WELD_PROFILE = FOOT_PROFILE + [(OVERLAP, 0.0)]


# ---- the drop test, as an operation rather than an opinion ----
# A seated foot occupies only space the cup already is, so FUSING one into the
# cup-shaped cutter must not change that cutter's volume at all. If the cup
# were a hair too small anywhere -- one section at the wrong inset, a corner
# radius that did not taper with it -- the foot would poke out and the volume
# would move. It is measured, not assumed (the assertion is at the end).
#
# Why on the cutter and not on the plate: subtracting the foot from the
# finished plate measures the same thing, but the foot's flat bottom is
# COINCIDENT with the cup floor, and that boolean can leave non-manifold edges
# behind. Fused into the cutter instead, the plate comes out the same as the
# plate built without the foot at all, and watertight at every size and type.
# The foot is also *consumed* here, which is the only way a body stops being
# part of the model: Cadpole has no hide(), so a body built purely to be
# measured would otherwise be rendered, checked and exported.
foot = foot_at((CELL_AXIS, CELL_AXIS), name="foot")
cutter_before = cups.volume()
cups = union(cups, foot, name="cups_with_foot", kernel="mesh")
cutter_after = cups.volume()

cutters = [cups]

# ------------------------------------------------- magnet / screw clusters
# One corner's magnet socket, screw bore and countersink, at
# (8.0 - 0.25, 8.0 - 0.25) from the cell origin -- 7.75, and
# 7.75 + 26 = 33.75 for the far corners, which is the 26 mm magnet grid the
# whole standard is built on.
cluster_parts = []
if extended and magnets:
    # Drilled DOWNWARD from the cup floor, so the magnet stays flush with the
    # cup and any extra bottom thickness is a floor below it. The extra
    # OVERLAP on top is inside the cup void the socket already removed, so the
    # pocket is still exactly MAGNET_DEPTH deep below the floor.
    cluster_parts.append(cylinder(MAGNET_D / 2, MAGNET_DEPTH + OVERLAP,
                                  at=(0, 0, -BASE_H - MAGNET_DEPTH), name="magnet"))
    # No lead-in chamfer here. The reference gives the *bin's* magnet mouths a
    # 0.2 mm 45-degree lead-in (DIMENSION_MAGNET_CUTOUT_CHAMFER) and gives the
    # baseplate's none, because the baseplate magnet is pushed in from the
    # open cup side.
if extended and screws:
    # Through the bottom extension only: from the plate's underside up to the
    # cup floor. Both ends are carried OVERLAP past the face they break, so
    # the fused cutter crosses the slab boundary transversally.
    cluster_parts.append(cylinder(SCREW_D / 2, ext_h + 2 * OVERLAP,
                                  at=(0, 0, -slab_h - OVERLAP), name="screw"))
    # The countersink, as the reference builds it: a HEAD_D cylinder
    # HEAD_STRAIGHT + (HEAD_D - SCREW_D)/2 tall with its top edge chamfered by
    # (HEAD_D - SCREW_D)/2. That is 0.5 mm of straight bore and then a
    # 45-degree cone closing on the screw hole -- so it is a cylinder plus a
    # `cone()`, which Cadpole has, and no chamfer is needed.
    cluster_parts.append(cylinder(HEAD_D / 2, HEAD_STRAIGHT + OVERLAP,
                                  at=(0, 0, -slab_h - OVERLAP), name="head"))
    cluster_parts.append(cone(HEAD_D / 2, SCREW_D / 2, (HEAD_D - SCREW_D) / 2,
                             at=(0, 0, -slab_h + HEAD_STRAIGHT), name="countersink"))

if cluster_parts:
    corner = union(cluster_parts, name="corner_cluster", kernel="mesh") if len(cluster_parts) > 1 else cluster_parts[0]
    spacing = PITCH - 2 * SCREW_OFFSET            # 26.0
    quad = pattern_linear(pattern_linear(corner, 2, (spacing, 0, 0), merge=True, name="corner_row"),
                          2, (0, spacing, 0), merge=True, name="cell_cluster")
    quad = translate(quad, (SCREW_OFFSET - CLEAR, SCREW_OFFSET - CLEAR, 0), name="cell_cluster_at")
    if corner_magnets_only:
        # The reference places the 2x2 cell cluster on the four corner CELLS by
        # patterning it at the plate's full span, degenerating to one copy on a
        # single-cell axis.
        nx = 2 if W > 1 else 1
        ny = 2 if L > 1 else 1
        holes = pattern_linear(pattern_linear(quad, nx, ((W - 1) * PITCH, 0, 0), merge=True, name="hole_col"),
                               ny, (0, (L - 1) * PITCH, 0), merge=True, name="holes")
    else:
        holes = pattern_linear(pattern_linear(quad, W, (PITCH, 0, 0), merge=True, name="hole_col"),
                               L, (0, PITCH, 0), merge=True, name="holes")
    cutters.append(holes)


# ------------------------------------------------- skeletonized cross pocket
# A 16-edge closed outline -- four quarter arcs
# CONCENTRIC with the corner magnets, radius max(magnet, screw head)/2 + 1.0,
# joined by straight ribs that step out to the cell wall offset. What is
# removed is the cell interior MINUS the four magnet pads, i.e. a cross with
# rounded inner corners; the ribs tie every magnet pad to the perimeter wall,
# so the plate stays one solid. The outline is one `path` with four
# `arc(to, via=)` segments.
def pocket_outline(z):
    e = RIB_INSET                                  # 2.70, the cup's bottom-face inset
    w = PITCH - 2 * CLEAR                          # 41.5, the cell's own width
    a = SPIDER_R                                   # 4.075
    m0 = SCREW_OFFSET - CLEAR                      # 7.75
    m1 = m0 + PITCH - 2 * SCREW_OFFSET             # 33.75
    k = a * SQRT_HALF                              # the 45-degree point on the arc
    p = path((m0 + a, e))
    p = p.line((m1 - a, e))
    p = p.line((m1 - a, m0))
    p = p.arc((m1, m0 + a), via=(m1 - k, m0 + k))
    p = p.line((w - e, m0 + a))
    p = p.line((w - e, m1 - a))
    p = p.line((m1, m1 - a))
    p = p.arc((m1 - a, m1), via=(m1 - k, m1 - k))
    p = p.line((m1 - a, w - e))
    p = p.line((m0 + a, w - e))
    p = p.line((m0 + a, m1))
    p = p.arc((m0, m1 - a), via=(m0 + k, m1 - k))
    p = p.line((e, m1 - a))
    p = p.line((e, m0 + a))
    p = p.line((m0, m0 + a))
    p = p.arc((m0 + a, m0), via=(m0 + k, m0 + k))
    return sketch(XY.offset(z)).add(p)


if skeleton:
    # The reference cuts from the slab bottom up to the cup's top-section
    # shoulder (`slab_h - BIN_BASE_TOP_SECTION_HEIGHT`). Everything above the
    # cup floor is already void, so what this actually removes is the cross
    # through the bottom extension -- but the depth is the reference's.
    pocket = extrude(pocket_outline(-slab_h - OVERLAP),
                     slab_h - FOOT_TOP + OVERLAP, name="pocket")
    pockets = pattern_linear(pattern_linear(pocket, W, (PITCH, 0, 0), merge=True, name="pocket_col"),
                             L, (0, PITCH, 0), merge=True, name="pockets")
    cutters.append(pockets)


# ------------------------------------------------------- connection holes
# One bore per edge cell, entering the side wall on the cell's centre line at
# half the base height, DIMENSION_PLATE_CONNECTION_SCREW_HOLE_DIAMETER across
# and reaching `base_width / 2` in from the nominal edge.
if connection_holes:
    cr = connection_hole_diameter / 2
    mid_z = -BASE_H / 2                            # -2.5
    reach = PITCH / 2                              # 21.0, the reference's `depth`
    if pad_left > 0 or pad_right > 0 or pad_bottom > 0 or pad_top > 0:
        warn("connection holes with padding: the reference starts each bore 1 mm outside the " +
             "NOMINAL edge, which buries it in the padding; these start outside the padded " +
             "face instead and still reach " + str(reach) + " mm in from the nominal edge")

    def bore(length, axis_angle, axis, at):
        c = cylinder(cr, length, at=(0, 0, 0))
        c = rotate(c, axis_angle, axis=axis)
        return translate(c, at)

    # -Y and +Y walls, one per column of cells.
    south = bore(reach + 1 + pad_bottom, -90, (1, 0, 0), (CELL_AXIS, -1 - pad_bottom, mid_z))
    north = bore(reach + 1 + pad_top, 90, (1, 0, 0), (CELL_AXIS, true_l + 1 + pad_top, mid_z))
    cutters.append(pattern_linear(south, W, (PITCH, 0, 0), merge=True, name="conn_south"))
    cutters.append(pattern_linear(north, W, (PITCH, 0, 0), merge=True, name="conn_north"))
    # -X and +X walls, one per row of cells.
    west = bore(reach + 1 + pad_left, 90, (0, 1, 0), (-1 - pad_left, CELL_AXIS, mid_z))
    east = bore(reach + 1 + pad_right, -90, (0, 1, 0), (true_w + 1 + pad_right, CELL_AXIS, mid_z))
    cutters.append(pattern_linear(west, L, (0, PITCH, 0), merge=True, name="conn_west"))
    cutters.append(pattern_linear(east, L, (0, PITCH, 0), merge=True, name="conn_east"))


# ------------------------------------------------------ bin-Z clearance relief
# Shave `bin_z_clearance` off the whole top face, so a seated bin's underside
# stands clear of the plate instead of resting on it. The reference cuts a box
# over the padded footprint; this one is deliberately 10 mm oversized in X and
# Y so that none of its side faces lands on the slab's. A coincident-face cut
# is the non-transversal case, and even on the mesh kernel it can leave
# non-manifold edges and degenerate triangles behind. Oversized, the plate
# comes out watertight and to the same volume.
if bin_z_clearance > 0:
    cutters.append(box(fw + 20.0, fl + 20.0, bin_z_clearance + 2.0,
                       at=(-pad_left - 10.0, -pad_bottom - 10.0, -bin_z_clearance),
                       name="bin_z_relief"))

# NOT REPRODUCED, because it is not needed: the reference finishes
# with `plate.common(prism)`, an intersect against the nominal rounded
# rectangle, because its cups are oversized on purpose and the outer ones
# protrude half a millimetre past the slab edge. A cut can only remove, and
# this slab is drawn at the nominal footprint from the start, so the protruding
# cups take material out of the outer wall and never add any: the footprint is
# 42N - 0.5 for every value of `bin_z_clearance`, which the assertion below
# checks. Dropping the clip is what makes the plate watertight.

# ---------------------------------------------------------------- assemble
tool = union(cutters, name="tool", kernel="mesh") if len(cutters) > 1 else cutters[0]
plate = subtract(slab, tool, name="plate", kernel=kernel)
plate.material(color="#55606b", roughness=0.8)


# ---------------------------------------------------------------- a bin in it
# A 1x1x3 bin standing in cell (0, 0). Its foot is the real thing -- a second
# copy of the same four-section loft -- and the body above it is a plain hollow
# block, not the full bin: no stacking lip, no floor fillet, no scoop. What has
# to be exact for this picture is the foot, and
# `examples/gridfinity_bin.star` is where the rest of a bin is built.
#
# Seated means seated: the foot's flat bottom lands on the cup's flat floor,
# both at z = -5, so the bin needs no z offset at all in this frame. The
# assertions below are what say that is the seated position and not a guess.
if demo_bin:
    bin_h = 2 * HEIGHT_UNIT + (HEIGHT_UNIT - BASE_H)   # a 3-unit bin: 16 mm above z = 0
    # The foot is built at the full 42 mm pitch and trimmed to the bin's own
    # 41.5 mm footprint, exactly as the reference does and as
    # `gridfinity_bin.star` note 2 describes: the outer feet lose 0.25 mm per
    # side and pick up the body's R3.75 corner. Trimming the FOOT rather than
    # the welded bin keeps the intersect transversal -- the foot is genuinely
    # wider than the prism -- where trimming the whole bin puts the prism's
    # side faces exactly on the body's and leaves non-manifold edges behind
    # (the coincident-face case, reached on the mesh kernel).
    trim = extrude(ring(PITCH - 2 * CLEAR, R_OUT, -BASE_H - 1.0, (CELL_AXIS, CELL_AXIS)),
                   BASE_H + 1.0 + 2 * OVERLAP, name="bin_footprint")
    bin_foot = intersect(foot_at((CELL_AXIS, CELL_AXIS), name="bin_foot", profile=WELD_PROFILE),
                         trim, name="bin_foot_trimmed", kernel="mesh")
    body = extrude(ring(PITCH - 2 * CLEAR, R_OUT, 0.0, (CELL_AXIS, CELL_AXIS)),
                   bin_h, name="bin_body")
    hollow = subtract(body, extrude(ring(PITCH - 2 * CLEAR - 2 * WALL, R_OUT - WALL, 1.0,
                                         (CELL_AXIS, CELL_AXIS)),
                                    bin_h, name="bin_cavity"),
                      name="bin_hollow", kernel=kernel)
    bin = union(hollow, bin_foot, name="bin", kernel="mesh")
    bin.material(color="#d9a441", roughness=0.7)


# ---------------------------------------------------------------- measured
lo, hi = plate.bbox()

# Footprint. 42N - 0.5, as the reference gives it (a 2x3 baseplate is
# 83.5 x 125.5 mm), plus the padding.
assert_true(abs((hi[0] - lo[0]) - fw) < 0.02, msg="baseplate footprint width")
assert_true(abs((hi[1] - lo[1]) - fl) < 0.02, msg="baseplate footprint depth")

# Height. BIN_BASE_HEIGHT (+ BASEPLATE_EXTRA_HEIGHT) - BASEPLATE_BIN_Z_CLEARANCE:
# 4.5 mm for light, 10.9 for full and skeletonized.
assert_true(abs((hi[2] - lo[2]) - (slab_h - bin_z_clearance)) < 0.02, msg="baseplate height")
assert_true(abs(hi[2] + bin_z_clearance) < 0.02, msg="plate top face sits one bin-Z clearance below z = 0")

# Cup pitch, measured off the tiled cup cutter rather than asserted from the
# constant: the grid is CUP_SIZE wide plus one PITCH per extra cell.
if W > 1:
    assert_true(abs(((grid_hi[0] - grid_lo[0]) - CUP_SIZE) / (W - 1) - PITCH) < 1e-6,
                msg="cup pitch in X")
if L > 1:
    assert_true(abs(((grid_hi[1] - grid_lo[1]) - CUP_SIZE) / (L - 1) - PITCH) < 1e-6,
                msg="cup pitch in Y")

# ---- the fit: a real bin's foot drops into a cup ----
# Both bodies are built here and both are measured, so this is a measurement of
# geometry and not a restatement of the constants. `examples/gridfinity_bin.star`
# evaluates to a 2x3x6 bin of 83.5 x 125.5 x [-5, 40.8], and its foot is the
# same FOOT_PROFILE loft used above.
#
# The tolerance is the Gridfinity standard's own: BIN_XY_CLEARANCE = 0.25 mm
# per side, so a cup opening is exactly 0.5 mm wider than a foot top. 1e-6 mm
# is the comparison window, not the fit -- both numbers come out of the same
# exact loft construction, so anything above f32 noise would be a real error.
# What that 0.25 buys in practice is less than 0.25: because the cup is an xy
# offset of the foot and not a normal offset, the gap measured perpendicular to
# the two 45-degree bands is 0.25/sqrt(2) = 0.177 mm. That is the reference's
# choice, reproduced, and it is why a bin still rattles very slightly in a
# plate printed to size.
flo, fhi = foot.bbox()
clo, chi = cup.bbox()
assert_true(abs((chi[0] - clo[0]) - (fhi[0] - flo[0]) - 2 * CLEAR) < 1e-6,
            msg="cup opening is the foot top plus one xy clearance per side, in X")
assert_true(abs((chi[1] - clo[1]) - (fhi[1] - flo[1]) - 2 * CLEAR) < 1e-6,
            msg="cup opening is the foot top plus one xy clearance per side, in Y")
# Depth: the cup is exactly as deep as the foot is tall, so a seated foot lands
# on the cup floor (both flat, both at z = -5) rather than wedging on the
# 45-degree band. That is what sets the bin's height above the plate.
assert_true(abs((chi[2] - OVERLAP - clo[2]) - (fhi[2] - flo[2])) < 1e-6, msg="cup depth equals foot height")
assert_true(abs(clo[2] - flo[2]) < 1e-6, msg="cup floor and foot bottom are the same plane")
# And the point of BASEPLATE_BIN_Z_CLEARANCE: with the foot bottomed out, the
# bin's underside (its foot top, z = 0) stands this far above the plate's top
# face.
assert_true(abs((fhi[2] - hi[2]) - bin_z_clearance) < 0.02,
            msg="a seated bin clears the plate top by one bin-Z clearance")

# ---- the drop test ----
# A whole foot, seated, fused into the whole cup grid, measured by volume.
# 1e-9 relative is far tighter than the tessellation tolerance the mesh kernel
# works at, so a foot poking out anywhere on a cup's wall would show. What
# stays under the threshold is the mesh kernel re-emitting the same triangles
# in a different order, not material.
assert_true(abs(cutter_before - cutter_after) < 1e-9 * cutter_before,
            msg="a seated foot adds nothing to the cup: it fits inside")

export(plate, "gridfinity_baseplate_%dx%d_%s.stl" % (W, L, plate_type))
