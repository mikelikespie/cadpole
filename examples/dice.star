# A six-sided die: a rounded cube with recessed pips, in two colours.
#
# What this example demonstrates:
#
#   * **contour what is smooth, boolean what has an edge.** The rounded cube
#     is `field.box(round=)` meshed once -- an exact distance function, so all
#     twelve edges carry the same round and none of them wobbles. The twenty-one
#     recesses are real `cylinder()` bodies cut by the **mesh kernel**, so each
#     rim is a circle tessellated from an actual circle. A spherical dimple
#     cut in the field instead comes out with a ragged rim: where a sphere
#     leaves a flat face it is nearly tangent to it, the normal barely turns,
#     and a grid cannot find that intersection. Contouring is for smooth
#     shapes; a boolean is for shapes with rims.
#   * **two bodies, two materials, one print.** The pips are not painted on:
#     each is the disc that fills its own recess, kept as geometry and fused
#     into a second body in a second colour. That is the only way a *render*
#     shows two colours -- `paint` reaches the 3MF and the viewport but neither
#     renderer -- and it costs nothing, because the disc is the cutter's
#     smaller twin. Two bodies with two materials also become two filament
#     slots in the 3MF on their own, with no `paint` call.
#   * `subtract` and `union` of tools that **cannot touch each other**, which
#     the mesh kernel takes in one boolean rather than twenty-one, because
#     their union is their concatenation. The pip union
#     is also the multi-lump case the exact kernel refuses outright: one B-rep
#     solid is an outer shell plus its voids, and twenty-one loose discs are
#     not that.
#   * the standard **right-handed** pip layout: opposite faces sum to seven,
#     and 1, 2, 3 run counter-clockwise about the vertex they share, which is
#     asserted as `(n1 x n2) . n3 > 0` rather than trusted. Each face carries
#     an in-face basis with `u x v = n`, so one layout table serves all six
#     without being mirrored on half of them.
#
# **`simplify=False` on the one `mesh()` call is what makes the rounded edges
# look right.** With the default `simplify=True` the contouring collapses
# flat regions using an error threshold of a quarter of a cell. That is a
# good trade on most parts, but it leaves the vertices *irregularly* spaced
# along a gently curved surface, the normals wobble from one patch to the
# next, and a glossy material turns that wobble into a specular band that
# undulates along every rounded edge. A finer cell does not fix it: the
# defect is the irregularity, not the sampling rate. Keeping the grid regular
# does, even at a much coarser cell.
#
#   cadpole render examples/dice.star --backend path --samples 192 --view az=24,el=22 -o /tmp/die.png
#   cadpole export examples/dice.star --run-exports --out-dir /tmp      # a two-slot 3MF

size = param("size", 16, unit="mm", min=15, max=40, doc="Across the flats")
corner = param("corner", 1.6, unit="mm", min=0.4, max=2.2, doc="Radius on all twelve edges")
pip_r = param("pip_r", 1.6, unit="mm", min=0.8, max=1.9, doc="Radius of a pip where it meets the face (under half the spread)")
pip_depth = param("pip_depth", 0.6, unit="mm", min=0.2, max=1.5, doc="How deep a pip is sunk")
spread = param("spread", 4.0, unit="mm", min=3.4, max=4.6, doc="Pip centre offset from the face centre (over twice the pip radius, and the pips on the flat)")
gap = param("gap", 0.18, unit="mm", min=0.12, max=0.5, doc="How much narrower a pip is than its recess, per side")
res = param("resolution", 0.25, unit="mm", min=0.08, max=0.6, doc="Contouring cell size for the rounded cube")

# A corner pip's far lip and the corner round must not meet, or the rim runs
# off the flat and over the rounding.
assert_true(math.sqrt(2.0) * spread + pip_r < size / 2.0 - corner + spread,
            msg="the corner pips crowd the rounded edges: reduce `spread`, `pip_r` or `corner`")
assert_true(pip_depth < pip_r, msg="a pip deeper than it is wide reads as a hole, not a dot")
# A pip lies on the flat of its face, inside the rounded edges.
assert_true(spread + pip_r < size / 2.0 - corner, msg="the outer pips run off the flat onto the rounded edges: reduce `spread`, `pip_r` or `corner`")
# Neighbouring pips are `spread` apart: their cutters must not touch, or the
# one mesh boolean below is no longer a union of separate tools.
assert_true(2.0 * pip_r + 0.1 < spread, msg="neighbouring pips touch: keep `pip_r` under half of `spread`")

half = size / 2.0
# Both rims are tessellated from real circles at the document tolerance, so
# the ring only has to beat that, not the contouring cell.
assert_true(gap > 0.1, msg="the ring between a pip and its recess must be wider than the mesh tolerance")
assert_true(pip_r - gap > 0.5, msg="`gap` has eaten the pip")

# --- the six faces -----------------------------------------------------------
# Each face is an outward normal and an in-face basis with `u x v = n`, so a
# layout written in (u, v) reads the same way round on every face.
FACES = {
    "1": ((0, 0, 1), (1, 0, 0), (0, 1, 0)),
    "2": ((0, 1, 0), (0, 0, 1), (1, 0, 0)),
    "3": ((-1, 0, 0), (0, -1, 0), (0, 0, 1)),
    "4": ((1, 0, 0), (0, 1, 0), (0, 0, 1)),
    "5": ((0, -1, 0), (0, 0, 1), (-1, 0, 0)),
    "6": ((0, 0, -1), (1, 0, 0), (0, -1, 0)),
}

S = spread
LAYOUT = {
    "1": [(0, 0)],
    "2": [(-S, -S), (S, S)],
    "3": [(-S, -S), (0, 0), (S, S)],
    "4": [(-S, -S), (-S, S), (S, -S), (S, S)],
    "5": [(-S, -S), (-S, S), (0, 0), (S, -S), (S, S)],
    "6": [(-S, -S), (-S, 0), (-S, S), (S, -S), (S, 0), (S, S)],
}


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


# Every basis is right-handed, so a layout is not mirrored on some faces.
for name, f in FACES.items():
    assert_true(math.close(dot(cross(f[1], f[2]), f[0]), 1.0, tol=1e-9),
                msg="face %s's basis must satisfy u x v = n" % name)

# Opposite faces sum to seven.
for a, b in [("1", "6"), ("2", "5"), ("3", "4")]:
    n_a, n_b = FACES[a][0], FACES[b][0]
    assert_true(math.close(dot(n_a, n_b), -1.0, tol=1e-9), msg="%s and %s must be opposite faces" % (a, b))

# ...and it is the right-handed (Western) die: looking at the vertex 1, 2 and 3
# share, those three read counter-clockwise, which is exactly this sign.
assert_true(dot(cross(FACES["1"][0], FACES["2"][0]), FACES["3"][0]) > 0.5,
            msg="1, 2, 3 must run counter-clockwise about their shared vertex")


# A cylinder is built about +Z and turned so that +Z becomes the outward
# normal of the face it belongs to. `rotate` turns about the world origin, so
# the cylinder is positioned along its own axis first, while that axis is
# still Z, and only moved out to the face after the turn.
TURN = {
    "1": (0.0, (0, 0, 1)),
    "2": (-90.0, (1, 0, 0)),
    "3": (-90.0, (0, 1, 0)),
    "4": (90.0, (0, 1, 0)),
    "5": (90.0, (1, 0, 0)),
    "6": (180.0, (1, 0, 0)),
}

# The turn really does carry +Z onto each face's own normal. A cylinder that
# came out along the wrong axis would still be a perfectly good cylinder, and
# the die would quietly have its pips in the wrong places, so this is worth
# asserting rather than eyeballing in a render.
for name, f in FACES.items():
    ang, axis = TURN[name]
    probe = field.cylinder(0.5, 1.0).translate((0, 0, 10.0))
    if ang != 0.0:
        probe = probe.rotate(ang, axis=axis)
    lo_p, hi_p = probe.bounds
    for i in range(3):
        assert_true(math.close((lo_p[i] + hi_p[i]) / 2.0, 10.0 * f[0][i], tol=1e-6),
                    msg="face %s's cylinder must point along its own normal" % name)

# --- the body ----------------------------------------------------------------
# The rounded cube comes from an exact distance function, so all twelve edges
# carry the same round; the recesses are real `cylinder()` bodies cut by the
# **mesh kernel**, so their rims are circles tessellated from an actual circle
# rather than circles a grid had to guess at. That split is the whole point of
# this example: contour what is smooth, boolean what has an edge.
blank = mesh(field.box(size, size, size, round=corner), resolution=res, simplify=False, name="blank")
assert_eq(blank.kind, "mesh")

cutters = []
pip_solids = []
for face in ["1", "2", "3", "4", "5", "6"]:
    n, bu, bv = FACES[face]
    ang, axis = TURN[face]
    for i, (u, v) in enumerate(LAYOUT[face]):
        at = (u * bu[0] + v * bv[0] + half * n[0],
              u * bu[1] + v * bv[1] + half * n[1],
              u * bu[2] + v * bv[2] + half * n[2])
        # The cutter stands `pip_depth` into the face and 2 mm proud of it, so
        # no end of it is ever level with the surface it cuts.
        cut = cylinder(pip_r, pip_depth + 2.0, at=(0, 0, -pip_depth), name="cut_%s_%d" % (face, i))
        # The pip fills the recess: `gap` narrower so a ring of body colour
        # shows all round, top flush with the face, underside on the floor.
        pip = cylinder(pip_r - gap, pip_depth, at=(0, 0, -pip_depth), name="pip_%s_%d" % (face, i))
        if ang != 0.0:
            cut = cut.rotate(ang, axis=axis, name="cut_turn_%s_%d" % (face, i))
            pip = pip.rotate(ang, axis=axis, name="pip_turn_%s_%d" % (face, i))
        cutters.append(cut.translate(at, name="cut_at_%s_%d" % (face, i)))
        pip_solids.append(pip.translate(at, name="pip_at_%s_%d" % (face, i)))
assert_eq(len(cutters), 21, msg="a die has twenty-one pips")

# One boolean, not twenty-one: tools that cannot touch each other have a union
# that is their concatenation, so the mesh kernel takes them in one go.
die = subtract(blank, cutters, name="die", kernel=kernel.mesh).translate((0, 0, half), name="die_body")

# --- the pips ----------------------------------------------------------------
# Twenty-one separate solids fused into one body so they share a material and
# a filament slot. They never touch, so this is the mesh kernel's multi-lump
# union -- the exact kernel cannot hold separate lumps in one solid.
pips = union(pip_solids, name="pips", kernel=kernel.mesh).translate((0, 0, half), name="pip_bodies")

# --- checks ------------------------------------------------------------------
lo, hi = die.bbox()
assert_true(math.close(hi[2] - lo[2], size, tol=res), msg="the die is `size` across")
assert_true(math.close(lo[2], 0.0, tol=res), msg="the die sits on z = 0")
assert_true(math.close(hi[0] - lo[0], size, tol=res), msg="and is square in plan")

# The rounding removes the eight corners and twelve edges, so the die is
# lighter than the cube it came from; the pips take a little more.
cube = size * size * size
assert_true(die.volume() < cube, msg="rounding and pips removed material")
assert_true(die.volume() > 0.85 * cube, msg="but only the corners and the dimples")

# Every pip is a disc `pip_r - gap` in radius and `pip_depth` deep: twenty-one
# of them, short of the closed form by what the polygon of a small rim at the
# document tolerance leaves out (2.5 % at the default radius, 10 % at the
# smallest).
discs = 21 * math.pi * (pip_r - gap) * (pip_r - gap) * pip_depth
assert_true(pips.volume() > 0.85 * discs, msg="the pips are the discs that fill their recesses")
assert_true(pips.volume() < 1.001 * discs, msg="and no more")

# A cap is clipped by the face it sits in, so its flat side is *flush* with
# that face and only its curved side is clear of the cavity. Every one of the
# six faces carries pips, so the pips' box reaches all six flats and no
# further: flush, never proud.
plo, phi = pips.bbox()
for axis in range(3):
    assert_true(math.close(plo[axis], lo[axis], tol=2 * res), msg="the pips reach the flat, and no further")
    assert_true(math.close(phi[axis], hi[axis], tol=2 * res), msg="the pips reach the flat, and no further")
    # Both bodies are contoured, so "flush" is only ever flush to within a
    # cell; what this rules out is a pip standing visibly proud of its face.
    assert_true(plo[axis] >= lo[axis] - res and phi[axis] <= hi[axis] + res, msg="no pip stands proud")

# --- colour ------------------------------------------------------------------
# Two bodies, two materials: the renderer shows both, and the 3MF gets two
# filament slots numbered in feature order with no `paint` call needed.
die.material(preset=preset.pla_silk, color="#c8322f", roughness=0.26)
pips.material(preset=preset.pla_matte, color="#f2efe6", roughness=0.55)

export([die, pips], "dice.3mf")
export([die, pips], "dice.stl")



# The ends of the declared ranges build: the pips never touch each other or
# run off the flats.
def the_die_and_pips_are_solid():
    assert_solid(die)
    assert_solid(pips)


test("the smallest die", the_die_and_pips_are_solid, sweep={"size": [15]})
test("pip radius at its ends", the_die_and_pips_are_solid, sweep={"pip_r": [0.8, 1.9]})
test("pip spread at its ends", the_die_and_pips_are_solid, sweep={"spread": [3.4, 4.6]})
