# A Gridfinity bin: W x L grid units (42 mm), H height units (7 mm, foot
# included), hollow, with an optional stacking lip. No magnets.
#
# Frame: the body's corner is at the origin, feet hang below z = 0 (the foot
# top), the body rises above. Millimetres.
#
# The foot is exact. A Gridfinity base is a ruled loft through four rounded
# rectangles -- its two 45-degree bands are cones at the corners and planes
# along the flats, because a chamfer of a filleted vertical edge *is* a cone and
# its radius tapers linearly with the inset. The script builds that directly
# with `loft()`, so no chamfer operation is involved and nothing is
# approximated. `stepped_foot=True` builds a staircase of slabs instead, for
# comparison; see note 1.
#
# Notes on the construction, and what is approximated:
#
# 1. `stepped_foot=True` rebuilds the foot as a stack of rounded-rectangle
#    slabs whose insets follow the true profile, inscribed in it. It is the one
#    all-planar construction of the foot: every boolean it takes part in is a
#    plane-plane intersection that the exact B-rep kernel computes quickly,
#    whereas the lofted foot's corner cones make a curved-pair boolean, which
#    is slower and more fragile. It registers on the step edges instead of
#    the full 45-degree face, and `chamfer_steps` trades fidelity for boolean
#    count. Compare the two by rendering the base from underneath.
# 2. Feet are built at the full 42 mm pitch and the whole bin is trimmed to the
#    body footprint by one intersect at the end -- the outer feet lose 0.25 mm
#    per side and pick up the body's R3.75 corner.
# 3. `pattern_linear(..., merge=True)` fuses the copies in one boolean instead
#    of a chain of script-level unions. Each foot carries OVERLAP mm of prism
#    above z = 0 so that neighbouring feet share a *face* rather than only an
#    edge -- see the profile below for why that matters. That shared face is
#    handled by `glue`, a topological merge with no boolean in
#    it, so `kernel="brep"` fuses the feet *exactly*. The default
#    `kernel="auto"` sends the fuse to the mesh kernel, which is quicker on
#    this many faces and does not need the result to stay exact.
# 4. The stacking lip socket is a loft too (an inverted foot: no bottom
#    chamfer, footprint pitch + one clearance per side, corner R4.5, top face
#    5 mm above the wall top), so its 45-degree flare is exact. The 2.55 mm
#    wall-to-lip blend frustum is omitted: the lip underside is a
#    horizontal ledge, which prints without support.
# 5. No cavity floor fillet (R2.55 in the Gridfinity profile).
# 6. Outlines other than the foot are 28-gon polygons (6 chords per quarter arc,
#    chord error under 0.04 mm on the R3.75 body corner), so the body's own
#    booleans stay plane-plane. `true_arcs=True` switches them to real arcs.
#    The foot and the lip socket always use real arcs.
# 7. Boolean order: the cavity and the lip socket are cut from the plain wall
#    box first, top-down, while the body has a dozen faces and each cutter
#    crosses the boundary as it stands. The feet are welded on afterwards and
#    the footprint intersect is last.
# 8. `cadpole check` can report a handful of self-intersecting triangle pairs
#    on the seams where the mesh kernel joined two curved bodies: the two
#    sides of a seam are chord approximations of surfaces that meet at a
#    shallow angle, so their facets cross by a fraction of the chord error.
#    The body is watertight and the volume is right.
# 9. STEP export needs exact surfaces, and the default `kernel="auto"` sends
#    the larger unions to the mesh kernel. A mesh body cannot become a B-rep
#    again, so the export is refused with a diagnostic. Run with
#    `--param kernel=brep` to keep the whole bin exact (slower):
#
#      cadpole export examples/gridfinity_bin.star --param kernel=brep \
#          --format step -o /tmp/bin.step
#
#    On `kernel=brep` the footprint trim of note 2 is skipped with a warning,
#    so the perimeter feet keep the full 42 mm pitch.
#
# Booleans take Bodies or lists of Bodies: starlark-rust rejects `name=` after a
# `*list` splat, so `union(parts, name=...)` rather than `union(*parts, name=...)`.

W = param("width", 2, min=1, max=8, doc="Grid units along X (42 mm each)")
L = param("length", 3, min=1, max=8, doc="Grid units along Y (42 mm each)")
H = param("height", 6, min=1, max=20, doc="Height units (7 mm each, foot included)")
wall = param("wall", 1.2, unit="mm", min=0.8, max=3.0, doc="Wall thickness")
lip = param("lip", True, doc="Stacking lip on top")
stepped_foot = param("stepped_foot", False, doc="Approximate the foot with a stack of slabs instead of lofting it (note 1)")
n_top = param("chamfer_steps", 4, min=1, max=12, doc="Steps approximating the 2.4 mm top chamfer, when stepped_foot is set")
n_bot = param("bottom_steps", 2, min=1, max=6, doc="Steps approximating the 0.8 mm bottom chamfer, when stepped_foot is set")
true_arcs = param("true_arcs", False, doc="Draw the body outline with true arcs instead of a 28-gon (note 6)")
kernel = param("kernel", "auto", choices=["auto", "brep", "mesh"], doc="Geometry kernel for the booleans. STEP export needs \"brep\": the mesh kernel's result is triangles and cannot go back to exact surfaces (note 9)")

# Gridfinity constants.
PITCH = 42.0
UNIT = 7.0
CLEAR = 0.25                     # xy clearance per side
CORNER_R = 4.0                   # nominal corner radius; 3.75 effective on the body
FOOT_TOP = 2.4                   # 45-degree top chamfer band
FOOT_MID = 1.8                   # straight band
FOOT_BOT = 0.8                   # 45-degree bottom chamfer band
FOOT_H = FOOT_TOP + FOOT_MID + FOOT_BOT   # 5.0
FLOOR = 1.0                      # compartment floor above the foot top
LIP_H = 4.4                      # lip band height
LIP_RECESS = 0.6                 # shaved off the lip top
LIP_WALL = FOOT_TOP - CLEAR      # 2.15: the lip wall the socket leaves
OVERLAP = 0.5                    # boolean overlap (kernel tolerance is 0.05)

aw = PITCH * W - 2 * CLEAR       # 83.5 for W = 2
al = PITCH * L - 2 * CLEAR       # 125.5 for L = 3
R = CORNER_R - CLEAR             # 3.75
centre = (aw / 2, al / 2)
wall_top = (H - 1) * UNIT + max(0, UNIT - FOOT_H)     # 37.0 for H = 6
top = wall_top + (LIP_H - LIP_RECESS if lip else 0)   # 40.8 with the lip

if wall > LIP_WALL:
    warn("wall %s mm is thicker than the 2.15 mm lip wall; the lip socket will cut into the wall top" % wall)

# Unit quarter circle in 15-degree steps.
ARC = [(1.0, 0.0), (0.9659258, 0.2588190), (0.8660254, 0.5), (0.7071068, 0.7071068),
       (0.5, 0.8660254), (0.2588190, 0.9659258), (0.0, 1.0)]


def rounded_polygon(w, h, r, at):
    """A w x h rectangle with corner radius r as a 28-gon: 6 chords per quarter
    arc, counter-clockwise, corner centres where rounded_rect() puts them."""
    hw, hh, cx, cy = w / 2, h / 2, at[0], at[1]
    if r <= 0.01:
        return [(cx - hw, cy - hh), (cx + hw, cy - hh), (cx + hw, cy + hh), (cx - hw, cy + hh)]
    pts = []
    for q, (sx, sy) in enumerate([(1, 1), (-1, 1), (-1, -1), (1, -1)]):
        for (c, s) in ARC:
            ux, uy = [(c, s), (-s, c), (-c, -s), (s, -c)][q]
            pts.append((cx + sx * (hw - r) + r * ux, cy + sy * (hh - r) + r * uy))
    return pts


def outline(w, h, r, z, at, arcs=False):
    """A rounded-rectangle sketch on z. `arcs=True` draws true arcs; the default
    is the planar 28-gon (note 6)."""
    s = sketch(XY.offset(z))
    if arcs:
        return s.rounded_rect(w, h, max(r, 0.0), center=at)
    return s.polygon(rounded_polygon(w, h, r, at))


def slab(w, h, r, z0, height, at, name=None, arcs=False):
    """A rounded-rectangle prism w x h with corner radius r, from z0 up by height."""
    return extrude(outline(w, h, r, z0, at, arcs), height, name=name)


# ---------------------------------------------------------------- wall box, hollowed
# One rounded-rectangle prism from z = 0 to the top (lip band included).
body = slab(aw, al, R, 0.0, top, centre, name="body", arcs=true_arcs)

# Lip socket: an inverted foot (no bottom chamfer) whose footprint is the pitch
# plus one clearance per side (0.5 wider than the body) and whose corner radius
# is 4.5, with its top face 5 mm above the wall top. Measured up from the wall
# top: inset 2.4 for 0..2.6 (a straight socket leaving a 1.9 mm wall), then a
# 45-degree flare that reaches the full footprint at +5. Three sections, lofted:
# the flare is exact, so the lip's inner corners are cones like the foot's.
cutters = []
if lip:
    nw, nl, nr = aw + 4 * CLEAR, al + 4 * CLEAR, CORNER_R + 2 * CLEAR
    straight = FOOT_BOT + FOOT_MID                # 2.6: socket height before the flare

    def socket_ring(inset, z):
        return sketch(XY.offset(z)).rounded_rect(nw - 2 * inset, nl - 2 * inset,
                                                 nr - inset, center=centre)

    cutters.append(loft([
        socket_ring(FOOT_TOP, wall_top - OVERLAP),          # below the wall top, into the cavity's room
        socket_ring(FOOT_TOP, wall_top + straight),         # top of the straight band
        socket_ring(0.0, wall_top + FOOT_H),                # full footprint, well above the lip top
    ], name="socket"))

# Cavity: inset by the wall, floor 1.0 mm above the foot top, inner corners
# R - wall (2.55). With a lip it stops at the wall top (the socket above is
# already open, so it breaks into that void); without one it pokes out of the top.
cavity_top = wall_top if lip else top + OVERLAP
cutters.append(slab(aw - 2 * wall, al - 2 * wall, R - wall, FLOOR, cavity_top - FLOOR, centre,
                    name="cavity", arcs=true_arcs))
hollow = subtract(body, cutters, name="hollow", kernel=kernel)


# ---------------------------------------------------------------- feet
# The foot profile, as (z, inset from the 42 mm pitch): the inset is -z down
# the 45-degree top chamfer, 2.4 through the straight band, and 2.4 + (-4.2 - z)
# down the 45-degree bottom chamfer. Corner radius is always CORNER_R - inset,
# which is what makes each chamfer band a cone at the corners.
FOOT_PROFILE = [
    (-FOOT_H, FOOT_TOP + FOOT_BOT),           # bottom face: inset 3.2, R0.8
    (-(FOOT_TOP + FOOT_MID), FOOT_TOP),       # bottom of the straight band
    (-FOOT_TOP, FOOT_TOP),                    # top of the straight band
    (0.0, 0.0),                               # the nominal top face: full pitch, R4
    (OVERLAP, 0.0),                           # OVERLAP mm of prism above it; see below
]

# Why the profile does not stop at z = 0. At the nominal top face two feet of
# the grid meet along a *line* -- 42 mm pitch, 42 mm feet, and below z = 0 the
# chamfer pulls them apart again. The union of two solids that share only an
# edge is not a 2-manifold there, and no mesh can represent it, so fusing the
# feet to each other first would produce a body that is closed but not
# manifold. Carrying each foot OVERLAP mm further up turns that line into a
# face: the feet then share real area, their union is an ordinary solid, and
# the extra material is inside the body (which starts at z = 0) and inside the
# footprint trim, so it changes nothing about the part.

# Full pitch, centred on the cell: the foot spans [-0.25, 41.75] and is trimmed
# to the body footprint later.
foot_centre = (PITCH / 2 - CLEAR, PITCH / 2 - CLEAR)


def lofted_foot():
    sections = []
    for (z, inset) in FOOT_PROFILE:
        size = PITCH - 2 * inset
        sections.append(sketch(XY.offset(z)).rounded_rect(size, size, CORNER_R - inset,
                                                          center=foot_centre))
    return loft(sections, ruled=True, name="foot")


def stepped_foot_body():
    """The stepped approximation: slabs inscribed in the true profile, each
    poking OVERLAP into the larger slab above it (the top one into the body)."""
    steps = []
    ht = FOOT_TOP / n_top
    for k in range(n_top - 1):
        steps.append(((k + 1) * ht, -(k + 1) * ht, -k * ht + OVERLAP))
    steps.append((FOOT_TOP, -(FOOT_TOP + FOOT_MID), -(n_top - 1) * ht + OVERLAP))
    steps.append((0.0, 0.0, OVERLAP))       # the shared face the feet fuse on
    hb = FOOT_BOT / n_bot
    z_mid = -(FOOT_TOP + FOOT_MID)
    for j in range(1, n_bot + 1):
        steps.append((FOOT_TOP + j * hb, z_mid - j * hb, z_mid - (j - 1) * hb + OVERLAP))
    parts = []
    for i, (inset, z0, z1) in enumerate(steps):
        size = PITCH - 2 * inset
        parts.append(slab(size, size, CORNER_R - inset, z0, z1 - z0, foot_centre,
                          name="foot_step_%d" % i))
    return union(parts, name="foot", kernel=kernel)


foot = stepped_foot_body() if stepped_foot else lofted_foot()

# Tile the foot across the grid and fuse the copies in one boolean each way.
# Neighbouring feet share the whole OVERLAP band face, exactly: one is the
# other translated by the pitch, so the two faces have the same boundary. That
# is `glue`'s case, so on kernel="brep" the fuse is a
# topological merge with no boolean in it at all, and the feet stay exact. The
# default kernel="auto" sends it to the mesh kernel, which is faster on
# this many faces and does not need the result to stay exact.
feet = pattern_linear(pattern_linear(foot, W, (PITCH, 0, 0), merge=True, name="foot_col",
                                     kernel=kernel),
                      L, (0, PITCH, 0), merge=True, name="feet", kernel=kernel)

# Weld the feet on, then trim to the body footprint (note 2). The foot corner
# centres already coincide with the body's, so the perimeter feet come out with
# the body's R3.75 corner and lose 0.25 mm on their outer flanks; the interior
# feet are well inside and are untouched. Trimming last, not trimming the
# feet alone, keeps the feather edge out of the other booleans.
#
# The trim is skipped on the exact kernel, with a warning: the footprint
# prism's side faces lie exactly on the body's own while the perimeter feet
# cross those same planes, a partial coincidence the exact kernel refuses.
# What you get there is perimeter feet at the full 42 mm pitch, 0.25 mm proud
# on their outer flanks -- still drops into a baseplate, but not the trimmed
# geometry.
# Named "bin" when it is the final body, so the exported solid carries the same
# name whichever kernel ran.
welded = union(hollow, feet, name=("bin" if kernel == "brep" else "welded"), kernel=kernel)
if kernel == "brep":
    # Starlark has no implicit string concatenation; the `+` is required.
    warn("kernel=brep: skipping the footprint trim, so the perimeter feet keep the full 42 mm " +
         "pitch (a coincident-face intersect is not supported on the exact kernel yet)")
    bin = welded
else:
    footprint = slab(aw, al, R, -FOOT_H - OVERLAP, FOOT_H + top + OVERLAP, centre,
                     name="footprint", arcs=true_arcs)
    bin = intersect(welded, footprint, name="bin", kernel=kernel)
bin.material(color="#d9a441", roughness=0.7)

# The bin is exactly 42N - 0.5 wide and 42L - 0.5 deep, and runs from the foot
# bottom to the lip top (83.5 x 125.5, z from -5 to 40.8 at the defaults).
lo, hi = bin.bbox()
if kernel != "brep":
    assert_true(abs(hi[0] - lo[0] - aw) < 0.02, msg="footprint width")
    assert_true(abs(hi[1] - lo[1] - al) < 0.02, msg="footprint depth")
assert_true(abs(lo[2] + FOOT_H) < 0.02, msg="foot bottom")
assert_true(abs(hi[2] - top) < 0.02, msg="lip top")

export(bin, "gridfinity_bin_%dx%dx%d.stl" % (W, L, H))
if kernel == "brep":
    # Exact surfaces, for FreeCAD and the rest of the CAD world.
    export(bin, "gridfinity_bin_%dx%dx%d.step" % (W, L, H))
