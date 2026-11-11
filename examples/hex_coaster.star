# A honeycomb coaster: a disc with a lattice of hexagonal pockets that hold
# a glass's condensation, printed in oak-coloured wood-fill filament.
#
# What this example demonstrates:
#
#   * a sketch with dozens of hole loops laid out by a formula -- `ngon`
#     on a hexagonal grid, kept inside a margin -- so the whole lattice is one
#     `extrude` and the wall between any two pockets is exactly `wall`;
#   * `union` of that lattice onto a slightly wider base disc, overlapping it
#     by `OVERLAP` rather than touching it (two flush faces are a boolean
#     the exact kernel refuses);
#   * a `kernel` choice, because every pocket wall is a face and the union is
#     well over the exact kernel's face budget, so `"auto"` routes it to the
#     mesh kernel and says so. `"brep"` is not offered: a cylinder wall
#     crossing a planar face that has sixty holes in it is beyond the exact
#     kernel;
#   * the `wood` procedural texture: growth rings around the Z axis, so the
#     disc reads as a slice off a log, and `scale=` sets the ring spacing.
#
#   cadpole render examples/hex_coaster.star --backend path --view az=30,el=40 -o /tmp/coaster.png
#   cadpole render examples/hex_coaster.star --param cell=5 --param wall=1.2 -o /tmp/fine.png

radius = param("radius", 45, unit="mm", min=25, max=80, doc="Radius of the lattice")
thickness = param("thickness", 6, unit="mm", min=3, max=15, doc="Overall height")
floor = param("floor", 1.6, unit="mm", min=0.8, max=5, doc="Solid floor under the pockets")
lip = param("lip", 1.5, unit="mm", min=0, max=6, doc="How far the base disc stands out past the lattice")
cell = param("cell", 5, unit="mm", min=3, max=15, doc="Hex grid pitch, centre to corner")
wall = param("wall", 1.4, unit="mm", min=0.8, max=4, doc="Wall between two pockets")
margin = param("margin", 3, unit="mm", min=1, max=12, doc="Solid rim inside the lattice's edge")
kernel = param("kernel", "auto", choices=["auto", "mesh"],
               doc="Geometry kernel for the union. `brep` is not offered: see the header")

OVERLAP = 0.5   # the lattice sinks this far into the base so the union is transversal

# --- the lattice --------------------------------------------------------------
# Pointy-top hexagons (`rotate=30` puts a corner on +y). The grid pitch is
# `cell` from a hex's centre to its corner; neighbouring centres are then
# sqrt(3) * cell apart along a row and 1.5 * cell between rows, with every
# other row shifted by half a step. A pocket is the grid hex shrunk so the
# material left between two of them, measured across the flats, is `wall`.
pocket_r = cell - wall / math.sqrt(3.0)
assert_true(pocket_r > 1.0, msg="the pockets have vanished: `wall` is too large for `cell`")

lattice_sketch = sketch(XY.offset(floor - OVERLAP), name="lattice_profile").circle(radius)
reach = radius - margin
rows = int(reach / (1.5 * cell)) + 1
cols = int(reach / (math.sqrt(3.0) * cell)) + 1
pockets = 0
for j in range(-rows, rows + 1):
    for i in range(-cols, cols + 1):
        x = (i + 0.5 * (j % 2)) * math.sqrt(3.0) * cell
        y = j * 1.5 * cell
        if math.hypot(x, y) + pocket_r <= reach:
            lattice_sketch = lattice_sketch.ngon(6, pocket_r, center=(x, y), rotate=30)
            pockets += 1
assert_true(pockets >= 7, msg="fewer than a ring of pockets fits: shrink `cell` or `margin`")

lattice = extrude(lattice_sketch, thickness - floor + OVERLAP, name="lattice")

# --- the base ---------------------------------------------------------------
# A plain disc, `lip` wider than the lattice so the two outer walls are not
# the same cylinder (coincident faces are the other thing the exact kernel
# refuses), and `floor` thick so that is what is under every pocket.
base = extrude(sketch(XY, name="base_profile").circle(radius + lip), floor, name="base")

coaster = union(base, lattice, name="coaster", kernel=kernel)

# The pockets stop at the floor, not below it, and the part is as tall as asked.
lo, hi = coaster.bbox()
assert_true(math.close(hi[2], thickness, tol=0.01), msg="the coaster is `thickness` tall")
assert_true(math.close(lo[2], 0.0, tol=0.01), msg="the coaster sits on z = 0")
solid = math.pi * (radius + lip) * (radius + lip) * floor + math.pi * radius * radius * (thickness - floor)
assert_true(coaster.volume() < solid, msg="the pockets removed material")
assert_true(coaster.volume() > 0.3 * solid, msg="but not all of it")

# Oak, with the rings tightened to suit a 90 mm part. The rings run around Z,
# which on a flat disc is exactly a slice through the log.
coaster.material(preset="wood_oak", scale=10, roughness=0.55)

export(coaster, "hex_coaster.stl")
