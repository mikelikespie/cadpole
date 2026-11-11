# Four things `sweep_along` makes: a compression spring, an M8 bolt
# with a real ISO thread, the nut that fits it with a measured clearance,
# and a bent tube.
#
#   * the spring is a round wire swept along `helix()` under the
#     rotation-minimising frame: the helix is stored exactly and the swept
#     surface is a cubic B-spline skin through stations along it, measured
#     against the true sweep (the sweep's info diagnostic says how far it
#     sits from it -- a few ten-thousandths of a millimetre);
#   * the thread is `@cadpole/mechanical.star`'s `bolt()`: the ISO 68-1 groove
#     drawn in the axial plane and swept along a helix under the Frenet
#     frame -- on a helix that frame *is* the screw motion, a revolve with a
#     pitch -- then subtracted from the shank. The groove sits 0.05 mm proud
#     of the major cylinder and its helix runs half a pitch past the tip, so
#     the only faces that cross the shank are the flanks and they cross it at
#     30 degrees: the tangent case is avoided by construction. The
#     boolean is hundreds of NURBS faces against a cylinder, which the exact
#     kernel's face budget routes to the mesh kernel;
#   * the nut is the same thread turned inside out, offset by `clearance` on
#     every surface, and the test *measures* that gap with `distance()`;
#   * the tube is an annulus along line-arc-line: a translation, a rotation
#     about the arc's axis and a translation, every one exact -- the bend is
#     a true torus (`faces(kind=kind.torus)` finds it) and its volume is the
#     annulus times the centreline's length.
#
#   cadpole test examples/spring_and_bolt.star
#   cadpole inspect examples/spring_and_bolt.star
#   cadpole render examples/spring_and_bolt.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/spring_and_bolt.png

load("@cadpole/mechanical.star", "bolt", "iso_profile", "minor_diameter", "nut", "spring")

wire = param("wire_d", 2.0, unit="mm", min=0.5, max=4, doc="Spring wire diameter")
coil = param("coil_d", 16.0, unit="mm", min=6, max=40, doc="Spring coil diameter, centre to centre")
pitch = param("pitch", 5.0, unit="mm", min=2.5, max=12, doc="Spring pitch")
turns = param("turns", 6.0, min=2, max=12, doc="Spring turns")
major = param("thread_d", 8.0, unit="mm", min=4, max=16, doc="Bolt thread major diameter (M8)")
thread_pitch = param("thread_pitch", 1.25, unit="mm", min=0.5, max=2.5, doc="Thread pitch (M8 coarse)")
shank = param("shank", 16.0, unit="mm", min=8, max=40, doc="Bolt shank length")
clearance = param("clearance", 0.15, unit="mm", min=0.05, max=0.18, doc="Gap between the bolt's and the nut's thread surfaces (an M8 x 1.25 ISO nut keeps a crest land up to about 0.18 mm)")
bend_r = param("bend_r", 12.0, unit="mm", min=8, max=30, doc="Centreline radius of the tube's bend")

# --- The spring, standing on the left.
spr = translate(spring(wire, coil, pitch, turns, ends="open", name="spring"), (-34, 0, wire / 2.0), name="spring_placed")

# --- The bolt, tip down, and the nut run half way up its shank.
blt = bolt(major, thread_pitch, shank, 13, 5.3, name="bolt")
nut_h = 6.5
nt = nut(major, thread_pitch, 13, nut_h, clearance=clearance, start=(0, 0, shank / 2.0 - nut_h / 2.0), name="nut")

# --- A bent tube: an annulus along line, quarter arc, line.
r_out = 5.0
r_in = 4.0
rise = 12.0
run = 18.0
c = 0.7071067811865476
route = (curve((30, 0, 0))
         .line((30, 0, rise))
         .arc((30 + bend_r, 0, rise + bend_r), via=(30 + bend_r - bend_r * c, 0, rise + bend_r * c))
         .line((30 + bend_r + run, 0, rise + bend_r)))
tube = sweep_along(sketch(XY).circle(r_out, center=(30, 0)).circle(r_in, center=(30, 0)), route, name="tube")

spr.material(preset=preset.steel)
blt.material(preset=preset.steel)
nt.material(preset=preset.brass)
tube.material(preset=preset.aluminum)

export([spr, blt, nt, tube], "spring_and_bolt.3mf")
export([spr, blt, nt, tube], "spring_and_bolt.step")


# ------------------------------------------------------------------ tests


def spring_is_a_wire_of_the_coils_length():
    """A sweep of a circle keeps the circle's area along the whole path:
    the volume is pi r^2 times the helix's length, to the tessellation."""
    assert_solid(spr)
    length = turns * math.hypot(math.pi * coil, pitch)
    assert_close(spr.volume(), math.pi * (wire / 2.0) * (wire / 2.0) * length, rel=0.01)
    return spr.volume()


test("the spring is a round wire along its coil, and a solid", spring_is_a_wire_of_the_coils_length, sweep={"pitch": [3, 6], "turns": [3, 8]})


def bolt_and_nut_clear_by_the_clearance():
    """The nut's thread is the bolt's offset by `clearance` on every surface,
    so the closest pair of faces on the coaxial pair is that far apart --
    within the chord contract: a mesh sits inside its
    surface by up to the tolerance and the mesh kernel's cleanup moves a
    trimmed edge by up to it, so the measured gap is never more than the
    clearance and at most twice the tolerance under it. At the default
    0.05 mm the pair measures 0.115 for 0.15 (the nut's root chords against
    the bolt's crest); `cadpole test --tolerance 0.01` measures 0.14."""
    assert_solid(blt)
    assert_solid(nt)
    gap = distance(blt, nt)
    assert_true(not gap.overlap, msg="the nut must not touch the bolt")
    assert_true(gap.distance <= clearance + 1e-6, msg="a chorded mesh sits inside its surface")
    assert_true(gap.distance >= clearance - 2.0 * gap.tolerance, msg="the gap is the clearance within the chord contract")
    return gap.distance


test("the nut runs on the bolt with the clearance asked for", bolt_and_nut_clear_by_the_clearance, sweep={"clearance": [0.1, 0.15]})


def bolt_has_a_thread_of_the_right_depth():
    """The threaded shank's bounding box is the major diameter across, and
    its volume sits between the minor and the major cylinders': a thread
    was cut, and not more than one."""
    lo, hi = blt.bbox()
    assert_close(hi[0] - lo[0], 13 / 0.8660254037844386, tol=0.05)
    minor_d = minor_diameter(major, thread_pitch)
    head_v = 5.3 * 13 * 13 * 0.8660254037844386
    v_minor = math.pi * (minor_d / 2.0) * (minor_d / 2.0) * shank
    v_major = math.pi * (major / 2.0) * (major / 2.0) * shank
    body_v = blt.volume() - head_v
    assert_true(body_v > v_minor and body_v < v_major, msg="the shank's volume is between the minor and major cylinders'")
    prof = iso_profile(thread_pitch)
    assert_close(prof["depth"], 0.5413 * thread_pitch, tol=1e-3)
    return body_v


test("the bolt is threaded to the ISO depth", bolt_has_a_thread_of_the_right_depth)


def tube_is_exact():
    """Line, arc, line: every segment exact, the bend a torus, the volume
    the annulus times the centreline."""
    assert_solid(tube)
    assert_eq(tube.faces(kind=kind.torus).count(), 4)
    length = rise + bend_r * math.pi / 2.0 + run
    assert_close(tube.volume(), math.pi * (r_out * r_out - r_in * r_in) * length, rel=0.015)
    return tube.volume()


test("the tube's bend is a torus and its volume is the annulus times the centreline", tube_is_exact, sweep={"bend_r": [8, 20]})
