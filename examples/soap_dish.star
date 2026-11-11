# A soap dish with drainage ribs and slots, rounded everywhere, built as a
# signed distance field and printed in translucent PETG.
#
# What this example demonstrates:
#
#   * the `field` namespace end to end: `box(round=)`, `half_space`,
#     `capsule`, and the *smooth* booleans -- `smooth_intersect` to round the
#     rim and the foot, `smooth_subtract` to hollow the dish with a filleted
#     inner edge, `smooth_union` to blend the ribs into the floor -- so there
#     is no sharp edge anywhere on the part and no `fillet()` call either;
#   * `mesh(field, resolution=)`: one contouring pass turns the whole
#     expression into a watertight mesh body, at a cost that follows the
#     surface area over the cell area (docs/starlark-api.md);
#   * a translucent print material under `--backend path`: `petg_translucent`
#     absorbs over 14 mm and scatters over 3 mm, so the 2.4 mm walls glow and
#     the ribs, twice as thick, read darker -- which is what the printed part
#     looks like on a windowsill.
#
# There is no `kernel` parameter: a field has no kernel, and the one mesh
# body it produces is watertight by construction (`mesh()` refuses to return
# anything that is not). What `mesh()` does not promise is that no two
# triangles cross, and on this part the contouring leaves a handful of
# folded pairs at one blend junction at cells finer than the 0.5 mm default;
# `cadpole check` reports them. The header of the ribs section says which
# junction it is.
#
#   cadpole render examples/soap_dish.star --backend path --samples 128 --view az=30,el=24 -o /tmp/soap.png
#   cadpole check  examples/soap_dish.star --param resolution=0.4     # see the folded pairs it reports

length = param("length", 100, unit="mm", min=60, max=160, doc="Outside, along x")
width = param("width", 68, unit="mm", min=40, max=120, doc="Outside, along y")
height = param("height", 22, unit="mm", min=12, max=40)
wall = param("wall", 2.4, unit="mm", min=1.6, max=5)
floor = param("floor", 3.0, unit="mm", min=2, max=6, doc="Floor under the ribs")
corner = param("corner", 9, unit="mm", min=3, max=20, doc="Plan-view corner radius")
rim = param("rim", 1.6, unit="mm", min=0.5, max=4, doc="Rounding of the top and bottom edges")
ribs = param("ribs", 5, min=0, max=12, doc="Drainage ribs across the floor")
rib_r = param("rib_r", 1.5, unit="mm", min=0.8, max=4, doc="Rib radius (a rib is a half-buried rod)")
slots = param("slots", 4, min=0, max=12, doc="Drain slots through the floor, between the ribs")
slot_w = param("slot_w", 3.5, unit="mm", min=2, max=8)
res = param("resolution", 0.5, unit="mm", min=0.25, max=1.5, doc="Contouring cell size; 0.5 contours this part clean, see the header")

assert_true(corner > wall, msg="the plan corner must be larger than the wall, or the inside corner has no radius")
assert_true(2 * rib_r < floor + 2, msg="ribs this fat would stand higher than the floor is thick")

# --- the shell ------------------------------------------------------------
# A rounded box taller than the dish, clipped flat at z = 0 and z = height by
# two half-spaces (a half-space's `normal` points *out* of the kept side, as
# a face normal does). The clips are *smooth* intersections, which is what
# rounds the rim and the foot by `rim`; the box's own `round=` only ever has
# to be the plan-view corner radius, because its top and bottom never survive.
tall = height + 2 * corner
outer = field.box(length, width, tall, round=corner).translate((0, 0, height / 2.0))
outer = outer.smooth_intersect(field.half_space(origin=(0, 0, height), normal=(0, 0, 1)), rim)
outer = outer.smooth_intersect(field.half_space(origin=(0, 0, 0), normal=(0, 0, -1)), rim)

# The cavity: the same box `wall` smaller, so the wall is `wall` thick on the
# straights and round the corners alike, raised so `floor` is left under it.
# Its own rounding is what fillets the inside of the floor; the blend radius
# on the subtraction is what fillets the inner edge of the rim.
cavity = field.box(length - 2 * wall, width - 2 * wall, tall, round=corner - wall)
cavity = cavity.translate((0, 0, floor + tall / 2.0))
dish = outer.smooth_subtract(cavity, wall * 0.6)

# --- ribs and slots ----------------------------------------------------------
# Ribs are rods lying half buried in the floor, blended in so water runs off
# them, and they run wall to wall: each end is a millimetre *inside* the
# wall, so no end cap is ever exposed. That is deliberate. A rod that stops
# on the floor has a spherical cap meeting a plane, and the blend around it
# is where the contouring produces slivers (folded triangles); a rod that
# runs into the
# wall only ever meets the floor along its length, which contours cleanly.
# Slots go through the floor between the ribs, well short of the corners.
pitch = (width - 2 * wall - 2 * corner) / max(ribs, 1)
reach = length / 2.0 - wall - 1.0
for i in range(ribs):
    y = (i - (ribs - 1) / 2.0) * pitch
    rib = field.capsule((-reach, y, floor), (reach, y, floor), rib_r)
    dish = dish.smooth_union(rib, rib_r * 0.65)
slot_len = length - 2 * wall - 2 * corner - 2 * rib_r - 4.0 - 2 * slot_w
for i in range(slots):
    y = (i - (slots - 1) / 2.0) * pitch
    slot = field.box(slot_len, slot_w, 3 * floor, round=slot_w * 0.45).translate((0, y, 0))
    dish = dish.smooth_subtract(slot, 0.6)

soap_dish = mesh(dish, resolution=res, name="soap_dish")

# The rounding never grows the part: it is exactly `length` x `width` x
# `height` to within a contouring cell, and it sits on the bed.
lo, hi = soap_dish.bbox()
assert_true(math.close(hi[2] - lo[2], height, tol=res), msg="the dish is `height` tall")
assert_true(math.close(lo[2], 0.0, tol=res), msg="the dish sits on z = 0")
assert_true(math.close(hi[0] - lo[0], length, tol=res), msg="the dish is `length` long")
assert_true(soap_dish.volume() < length * width * height, msg="hollow")
assert_true(soap_dish.volume() > length * width * floor * 0.5, msg="but not gone")

# Sea-glass translucent PETG. Only the path tracer reads the absorption and
# scattering the preset sets; the rasterizer shows a plain green solid.
soap_dish.material(preset=preset.petg_translucent, color="#8fd4c1", roughness=0.35)

export(soap_dish, "soap_dish.stl")
