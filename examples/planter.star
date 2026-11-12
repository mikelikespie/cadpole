# A flowerpot with a glazed collar, a floor that falls to a drainage hole,
# and channels under the foot that let the water out when the pot stands
# flat. The wall is one revolved profile, its corners rounded with 2D fillets
# so every curve is a true torus; the channels are one subtract.

h = param("height", 100, unit="mm", min=40, max=200, doc="Overall height")
r_top = param("top_radius", 56, unit="mm", min=20, max=120, doc="Outer radius of the collar")
r_bottom = param("bottom_radius", 40, unit="mm", min=15, max=100, doc="Outer radius of the foot")
wall = param("wall", 2.4, unit="mm", min=1.2, max=6, doc="Wall thickness (3 perimeters at 0.8)")
base = param("base", 6, unit="mm", min=3, max=10, doc="Floor thickness at the drain")
drain = param("drain_radius", 7, unit="mm", min=2, max=12, doc="Drainage hole radius")
drains = param("drains", 6, min=0, max=8, doc="Channels under the foot, from the hole to the rim of the foot")

# The collar: a band at the top that stands `proud` off the wall.
proud = 3.0
collar = min(max(0.16 * h, 9.0), 18.0)
# The floor rises `fall` from the drain to the wall, so water runs to the hole.
fall = 1.5
z_floor = base + fall

slope = (r_top - proud - r_bottom) / h    # outer wall, radius per mm of height

def outer(z):
    return r_bottom + slope * z

def inner(z):
    return outer(z) - wall

# Every face that looks down rises at 40 degrees from vertical, or steeper,
# so the pot prints without support: the collar's underside and the chamfer
# round the foot. A wall that flares more than that is beyond help.
LEAN = math.tan(40)                 # math works in degrees
steep = slope < 0.6
z_collar = h - collar - (proud / (LEAN - slope) if steep else proud)

assert_true(drain + 6 < inner(z_floor), msg="the floor reaches past the drainage hole")

# The foot's outer edge is chamfered rather than rounded: a rounded edge faces
# the bed and prints as an overhang.
foot_chamfer = 1.2
z_chamfer = foot_chamfer / (LEAN - slope) if steep else 0

# (x, y) on the XZ plane is (radius, height). The corners in drawing order:
corners = [(drain, 0)]                       # the hole, underside
if z_chamfer > 0:
    corners += [(r_bottom - foot_chamfer, 0), (outer(z_chamfer), z_chamfer)]
else:
    corners += [(r_bottom, 0)]
first = len(corners)                         # the corners that get rounded:
corners += [
    (outer(z_collar), z_collar),             # +0 where the collar leaves the wall
    (outer(h - collar) + proud, h - collar),  # +1 the collar's lower edge
    (r_top, h),                              # +2 the rim, outside
    (inner(h), h),                           # +3 the rim, inside
    (inner(z_floor), z_floor),               # +4 the floor meets the wall
    (drain, base),                           # +5 the hole, top edge
]
# Each corner's fillet, highest index first: a fillet inserts an arc after its
# corner, so working downward keeps the lower indices where they were.
inner_round = min(6.0, 0.3 * (inner(z_floor) - drain))
rounds = [
    (5, 0.8),
    (4, inner_round),
    (3, min(1.0, 0.3 * wall)),
    (2, 2.0),
    (1, 1.2),
    (0, 1.0),
]
profile = sketch(XZ, name="profile").polygon(corners)
for k, r in rounds:
    profile = profile.fillet2d(r, corners=[first + k])

shell = revolve(profile, axis=(0, 0, 1), name="shell")

# The channels: a box per channel from just inside the hole out past the foot,
# turned so that two of them flank the front of the pot. The cutter's inner
# end stays inside the hole across its whole width, and neighbouring channels
# part before they reach the hole, so every channel's roof is a short bridge
# between two pads rather than a ring hanging round the hole.
channel_w = min(9.0, 1.4 * drain)
if drains > 2:
    channel_w = min(channel_w, 1.8 * drain * math.sin(180.0 / drains))
channel_d = min(4.0, 0.65 * base)
if drains > 0:
    start = 0.3 * drain
    run = r_bottom + 4 - start
    cutter = box(run, channel_w, 2 * channel_d, center=True,
                 at=(start + run / 2, 0, -channel_d), name="channel").rotate(240 - 180 / max(drains, 1), axis=(0, 0, 1))
    cutters = pattern_circular(cutter, drains, name="channels") if drains > 1 else [cutter]
    # The mesh kernel, asked for by name: the exact kernel's cut through the
    # chamfered foot does not converge, and would hand over to it anyway.
    pot = subtract(shell, cutters, name="pot", kernel="mesh")
else:
    pot = shell

# Fired clay, with the collar dipped in a cream glaze.
pot.material(color="#a2543a", roughness=0.9)
glaze = pot.faces(above=XY.offset(z_collar - 0.01), allow_empty=True)
paint(pot, faces=list(glaze), color="#dccfb6")
export(pot, "planter.stl")

# Tests. They run after the model is built -- `cadpole test examples/planter.star`
# -- and never in an ordinary evaluation, so the viewer does not pay for them.
# A `sweep=` rebuilds the model at each value and runs the test there.

def hollow_and_printable():
    assert_solid(pot)
    assert_bbox(pot, (-r_top, -r_top, 0), (r_top, r_top, h), tol=0.6)
    # A shell holds less material than the cone it sits in.
    assert_true(pot.volume() < math.pi * r_top * r_top * h, msg="the pot is hollow")
    assert_printable(pot)

test("a hollow, printable pot on its footprint", hollow_and_printable)

def drains_through():
    # Just above the channels the floor is a disc with the hole in it.
    cut = pot.section(XY.offset(channel_d + 0.5))
    r = outer(channel_d + 0.5)
    assert_true(cut.area() < math.pi * (r * r - 0.9 * drain * drain), msg="the hole goes through the floor")

test("the drainage hole goes through the floor", drains_through)

def channels_reach_out():
    # Inside the channels' depth the foot is split into one piece per channel,
    # which is only true if every channel runs from the hole out past the foot.
    cut = pot.section(XY.offset(channel_d / 2))
    assert_eq(cut.loops, drains if drains > 1 else 1)

test("each channel runs from the hole to the edge of the foot", channels_reach_out, sweep={"drains": [2, 4, 6]})

def floor_falls():
    # Half way up the fall the floor is a ring: the middle has already dropped
    # away toward the drain.
    cut = pot.section(XY.offset(base + fall / 2))
    r = outer(base + fall / 2)
    assert_true(cut.area() < math.pi * (r * r - (drain + 3) * (drain + 3)), msg="the floor falls toward the drain")

test("the floor falls toward the drain", floor_falls)

def collar_proud():
    # Below the collar the pot is narrower than the collar by `proud`.
    loops = pot.section(XY.offset(z_collar - 1)).points()
    widest = max([math.hypot(p[0], p[1]) for loop in loops for p in loop])
    assert_close(widest, outer(z_collar - 1), tol=0.1)
    assert_true(widest < r_top - proud + 0.1, msg="the collar stands proud of the wall")

test("the collar stands proud of the wall", collar_proud)

def material():
    assert_solid(pot)
    return pot.volume()

test("thicker walls use more material", material, sweep={"wall": [1.2, 2.4, 4.0]}, monotone=monotone.increasing)
test("a solid at every top radius", material, sweep={"top_radius": [20, 56, 120]})
