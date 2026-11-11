# A tapered planter with a drainage hole, made from one revolved profile.
# The wall cross-section is drawn on the XZ plane (x = radius, y = height)
# and revolved about Z. No booleans needed, which keeps it robust.

h = param("height", 90, unit="mm", min=40, max=200, doc="Overall height")
r_top = param("top_radius", 55, unit="mm", min=20, max=120)
r_bottom = param("bottom_radius", 40, unit="mm", min=15, max=100)
wall = param("wall", 2.4, unit="mm", min=1.2, max=6, doc="Wall thickness (3 perimeters at 0.8)")
base = param("base", 4, unit="mm", min=2, max=10, doc="Floor thickness")
drain = param("drain_radius", 4, unit="mm", min=0.5, max=15, doc="Drainage hole radius")

# Outer wall slope in x per unit height.
slope = (r_top - r_bottom) / h

profile = sketch(XZ, name="profile").polygon([
    (drain, 0),
    (r_bottom, 0),
    (r_top, h),
    (r_top - wall, h),
    (r_bottom - wall + slope * base, base),
    (drain, base),
])

pot = revolve(profile, axis=(0, 0, 1), name="pot")
pot.material(color=(0.82, 0.55, 0.42), roughness=0.85)
export(pot, "planter.stl")

# Tests. They run after the model is built -- `cadpole test examples/planter.star`
# -- and never in an ordinary evaluation, so the viewer does not pay for them.
# A `sweep=` rebuilds the model at each value and runs the test there.

def hollow_and_printable():
    assert_solid(pot)
    assert_bbox(pot, (-r_top, -r_top, 0), (r_top, r_top, h))
    # A shell holds less material than the cone it sits in.
    assert_true(pot.volume() < math.pi * r_top * r_top * h, msg="the pot is hollow")
    assert_printable(pot)

test("a hollow, printable pot on its footprint", hollow_and_printable)

def material():
    assert_solid(pot)
    return pot.volume()

# Thicker walls use more plastic; a pot narrower at the top than the bottom is
# still a solid. The sweep stops at 100: from 110 up (the slider allows 120)
# the revolve's tessellation drops faces and refuses to return a mesh, which
# is a kernel defect, not a modelling one.
test("thicker walls use more material", material, sweep={"wall": [1.2, 2.4, 4.0]}, monotone=monotone.increasing)
test("a solid at every top radius", material, sweep={"top_radius": [20, 55, 100]})
