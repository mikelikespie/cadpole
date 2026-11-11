# A knurled thumbscrew and the nut that runs on it: real ISO 68-1 metric
# threads, swept along an exact helix, with the pitch, the diameter and the
# running clearance as parameters.
#
#   * the screw's thread is `thread()` from `@cadpole/mechanical.star`:
#     the 60 degree ISO groove drawn in the axial plane and swept along a
#     helix under the Frenet frame -- on a helix that frame is the screw
#     motion, so the sweep is a revolve with a pitch -- and subtracted from
#     the shank. The tip is chamfered to the minor diameter so the thread
#     starts clean;
#   * the head is a straight knurl: a star outline lofted between two
#     chamfer rings of the same segment count, so every ridge runs out into
#     the chamfers as its own face, one exact loft with no boolean;
#   * the nut is the same thread turned inside out (`internal=True`),
#     offset by `clearance` on every surface, cut out of a hex blank whose
#     corners are chamfered by a revolved double cone, with a countersink
#     at each end of the bore. `nut_style="knurled"` swaps the
#     hex for a knurled thumb nut built like the head;
#   * the library phases every thread by height, so the nut mates wherever
#     it is built; to *move* it, the script turns it and lifts it together --
#     `nut_pos` lifts it `travel` mm and turns it `travel / pitch` turns, the
#     screw motion that keeps the threads engaged -- which is what the
#     `thread_on` animation drives;
#   * `cutaway=True` takes a quarter out of both parts so the flanks can be
#     seen engaging, and `layout="print"` stands the screw on its head and
#     lays the nut flat beside it, the way `cadpole check` should judge them;
#   * `thread_form="print"` swaps ISO's 60 degree form, whose ridges are
#     60 degree overhangs when printed standing up, for a trapezoid with
#     flanks 40 degrees off the axis, which prints without support. Every
#     chamfer that faces the bed (the head's top, the nut's lower corners,
#     the countersinks) is 35-40 degrees off vertical for the same reason;
#     what `cadpole check` still reports is the thread crests and run-outs,
#     a few mm^2 thinner than two perimeters, as every printed thread is.
#
#   cadpole test thumbscrew.star
#   cadpole check thumbscrew.star -p layout=print -p thread_form=print --printer bambu_h2d
#   cadpole render thumbscrew.star --backend path --samples 256 --size 1600x1200 --view az=-30,el=18 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o thumbscrew_hero.png
#   cadpole animate thumbscrew.star --name thread_on --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o thumbscrew.gif

load("@cadpole/mechanical.star", "iso_profile", "minor_diameter", "thread", "thread_depth")

major = param("diameter", 10.0, unit="mm", min=5, max=16, doc="Thread major diameter (M10)")
pitch = param("pitch", 1.5, unit="mm", min=0.8, max=2.5, doc="Thread pitch (M10 coarse is 1.5)")
length = param("length", 28.0, unit="mm", min=12, max=60, doc="Threaded shank length, tip to head")
clearance = param("clearance", 0.15, unit="mm", min=0.05, max=0.2, doc="Gap between the screw's and the nut's thread surfaces, on the surface normal")
head_ratio = param("head_ratio", 2.6, min=1.8, max=3.5, doc="Head diameter as a multiple of the thread diameter")
knurl = param("knurl", 36, min=12, max=72, doc="Ridges round the knurled head")
nut_style = param("nut_style", "hex", choices=["hex", "knurled"], doc="A hex nut, or a knurled thumb nut")
nut_pos = param("nut_pos", 0.55, min=0.0, max=1.0, doc="How far the nut is threaded on: 0 just off the tip, 1 against the head")
cutaway = param("cutaway", False, doc="Cut a quarter out of the screw and the nut to show the threads engaging")
cut_turn = param("cut_turn", 35.0, unit="deg", min=-90, max=90, doc="Which quarter the cutaway takes, turned about the axis from the -x,-y quarter")
thread_form = param("thread_form", "iso", choices=["iso", "print"], doc="iso: ISO 68-1 metric; print: a 40-degree-overhang trapezoid that prints standing up without support")
layout = param("layout", "assembled", choices=["assembled", "print"], doc="assembled: the nut on the screw; print: both parts laid out on the bed")

# The thread form. "iso" is ISO 68-1: 60 degree included, its flanks 30
# degrees off the radial plane -- which, printed standing up, leaves every
# ridge's underside a 60 degree overhang. "print" is a trapezoid whose
# flanks stand 50 degrees off the radial (40 from the axis), so each ridge
# is self-supporting under the 45 degree rule, 0.28 p deep with a crest of
# 0.13 p, close to ISO's 0.125 p.
if thread_form == "iso":
    prof = iso_profile(pitch)
else:
    p_depth = 0.28 * pitch
    p_bottom = 0.2 * pitch
    prof = {"depth": p_depth, "bottom": p_bottom, "top": p_bottom + 2.0 * p_depth * math.tan(50.0), "angle": 50.0}

# The nut's thread keeps a crest land between its turns only while the
# clearance is under (bottom - p/10) / (2 (sec a - tan a)) -- the library
# refuses a land under a tenth of the pitch -- so a fine pitch runs a little
# tighter than asked. ISO: 0.13 p.
clr_max = (prof["bottom"] - 0.1 * pitch) / (2.0 * (1.0 / math.cos(prof["angle"]) - math.tan(prof["angle"])))
clr = min(clearance, 0.95 * clr_max)

assert_true(pitch <= major / 5.0, msg="a pitch of %s is too coarse for an M%s thread" % (pitch, major))

depth = thread_depth(pitch, prof)
minor_d = minor_diameter(major, pitch, prof)
r = major / 2.0

# ------------------------------------------------------------------ helpers


def polar(rad, deg):
    return (rad * math.cos(deg), rad * math.sin(deg))


def ring_loop(rad, n, rot):
    return [polar(rad, 360.0 * i / n + rot) for i in range(n)]


def star_loop(rad, d, teeth, rot):
    n = 2 * teeth
    return [polar(rad if i % 2 == 0 else rad - d, 360.0 * i / n + rot) for i in range(n)]


def knurled_drum(z0, rad, height, teeth, chamfer, kdepth, name):
    """A straight-knurled drum on +Z from z0: a star outline between two
    chamfer rings drawn with the star's segment count, one exact loft."""
    n = 2 * teeth
    inset = 0.7 * chamfer    # 35 degrees off the axis: no overhang either way up
    secs = [
        sketch(XY.offset(z0)).polygon(ring_loop(rad - inset, n, 0)),
        sketch(XY.offset(z0 + chamfer)).polygon(star_loop(rad, kdepth, teeth, 0)),
        sketch(XY.offset(z0 + height - chamfer)).polygon(star_loop(rad, kdepth, teeth, 0)),
        sketch(XY.offset(z0 + height)).polygon(ring_loop(rad - inset, n, 0)),
    ]
    return loft(secs, name=name)


def solid_of_revolution(pts, name):
    """A closed (radius, z) outline turned about +Z."""
    return revolve(sketch(XZ).polygon(pts), axis=(0, 0, 1), name=name)


# -------------------------------------------------------------------- screw

head_d = head_ratio * major
head_h = max(0.9 * major, 7.0)
head_ch = 0.08 * head_d
k_depth = max(0.5, 0.025 * head_d)

shank = thread(major, pitch, length, profile=prof, name="shank_thread")
# Chamfer the tip to the minor diameter at 45 degrees, so the thread starts
# with a full flank instead of a feather edge.
tip_cut = solid_of_revolution([
    (0.0, -1.0),
    (minor_d / 2.0 - 0.1 - 1.0, -1.0),
    (r + 0.5, depth + 0.6),
    (r + 0.5, length + 1.0),
    (0.0, length + 1.0),
], "tip_chamfer")
shank = intersect(shank, tip_cut, kernel="mesh", name="shank")
shank_volume = shank.volume()

# A shoulder between the shank and the head, then the knurled head.
collar_h = 0.25 * major
collar = solid_of_revolution([
    (0.0, length - 0.2),
    (r * 1.45, length - 0.2),
    (r * 1.45, length + collar_h),
    (0.0, length + collar_h),
], "collar")
head = knurled_drum(length + collar_h - 0.01, head_d / 2.0, head_h, knurl, head_ch, k_depth, "head")
screw = union(shank, collar, head, kernel="mesh", name="screw")
screw_top = length + collar_h - 0.01 + head_h

# ---------------------------------------------------------------------- nut

nut_h = 0.8 * major
nut_af = 1.6 * major + 1.0
# Countersinks: cones 35 degrees off the axis from 0.3 mm past the major
# radius at each face down to inside the minor bore.
cs_r0 = r + 0.3
cs_r1 = r - depth - 0.3
cs_h = (cs_r0 - cs_r1) / math.tan(35.0)

if nut_style == "hex":
    blank = extrude(sketch(XY).ngon(6, nut_af / math.sqrt(3.0)), nut_h, name="nut_hex")
    corner = nut_af / math.sqrt(3.0)
    # The top corners take the classic 30 degree chamfer; the bottom ones,
    # which face the bed, a 55 degree one, steep enough to print.
    c = 0.12 * major
    chamf = solid_of_revolution([
        (0.0, -0.01),
        (corner - 0.6 * c, -0.01),
        (corner + 1.0, (0.6 * c + 1.0) * math.tan(55.0) - 0.01),
        (corner + 1.0, nut_h - (c + 1.0) * math.tan(30.0)),
        (corner - c, nut_h + 0.01),
        (0.0, nut_h + 0.01),
    ], "nut_chamfer")
    blank = intersect(blank, chamf, kernel="mesh", name="nut_blank")
else:
    nut_d = 2.0 * major + 4.0
    blank = knurled_drum(0.0, nut_d / 2.0, nut_h, int(knurl * 0.75), 0.06 * nut_d, max(0.4, 0.02 * nut_d), "nut_blank")

tool = thread(major, pitch, nut_h, profile=prof, internal=True, clearance=clr, name="nut_thread")
sinks = solid_of_revolution([
    (0.0, -1.0),
    (cs_r0 + 1.0 * math.tan(35.0), -1.0),
    (cs_r1, cs_h),
    (cs_r1, nut_h - cs_h),
    (cs_r0 + 1.0 * math.tan(35.0), nut_h + 1.0),
    (0.0, nut_h + 1.0),
], "nut_countersinks")
nut = subtract(blank, tool, sinks, kernel="mesh", name="nut_raw")

# Thread it on: lift `travel` and turn `travel / pitch` turns (right hand:
# counter-clockwise about +Z while climbing +Z).
travel_max = length - nut_h - 0.6
travel = -nut_h + nut_pos * (travel_max + nut_h)

# --------------------------------------------------------------- placement

if layout == "print":
    screw = rotate(screw, 180.0, axis=(1, 0, 0), name="screw_flipped")
    screw = translate(screw, (-(head_d / 2.0 + 4.0), 0, screw_top), name="screw_print")
    nut = translate(nut, (nut_af / 2.0 + 4.0, 0, 0), name="nut_print")
else:
    nut = rotate(nut, 360.0 * travel / pitch, axis=(0, 0, 1), name="nut_turned")
    nut = translate(nut, (0, 0, travel), name="nut")
    if cutaway:
        # One quarter out of both parts, the same quarter, through the
        # axis: the screw's teeth are seen in section sitting in the nut's
        # grooves with the clearance between them. The quarter is turned so
        # that, from the hero camera (az=-30), one cut face is seen nearly
        # square on and the other obliquely.
        big = 4.0 * head_d
        for_screw = rotate(box(big, big, screw_top + nut_h + 8.0, at=(-big, -big, -nut_h - 4.0)), cut_turn, axis=(0, 0, 1), name="cut_screw_box")
        for_nut = rotate(box(big, big, screw_top + nut_h + 8.0, at=(-big, -big, -nut_h - 4.0)), cut_turn, axis=(0, 0, 1), name="cut_nut_box")
        screw = subtract(screw, for_screw, kernel="mesh", name="screw_cut")
        nut = subtract(nut, for_nut, kernel="mesh", name="nut_cut")

screw.material(preset=preset.brass, roughness=0.22)
nut.material(preset=preset.steel, color="#9aa0a8", roughness=0.24)

export([screw, nut], "thumbscrew.3mf")

report("thread", "M%s x %s" % (major, pitch))
report("minor_d", minor_d)
report("clearance", clr)

# ------------------------------------------------------------------- tests


def screw_and_nut_clear():
    """The nut's thread is the screw's offset by `clearance` on every
    surface, so the pair never shares material and the closest faces are
    that far apart -- within the chord contract: a mesh sits inside its
    true surface by up to the tolerance, so the measured gap is at most the
    clearance and at least the clearance less twice the tolerance."""
    assert_solid(screw)
    assert_solid(nut)
    gap = distance(screw, nut)
    assert_true(not gap.overlap, msg="the nut must not cut into the screw")
    assert_true(gap.distance <= clr + 0.02, msg="the threads are engaged, so the gap is the clearance")
    assert_true(gap.distance >= clr - 2.0 * gap.tolerance - 0.02, msg="the gap is the clearance within the chord contract")
    return gap.distance


test("the nut runs on the screw at every pitch", screw_and_nut_clear, sweep={"pitch": [0.8, 1.0, 1.25, 1.5, 1.75, 2.0]})
test("the nut runs on the screw at every size", screw_and_nut_clear, sweep={"diameter": [6, 8, 10, 12, 16], "pitch": [1.0, 1.2]})
test("the nut clears wherever it is threaded to", screw_and_nut_clear, sweep={"nut_pos": [0.3, 0.55, 0.8, 1.0], "nut_style": ["hex", "knurled"]})
test("the print thread form runs at every pitch", screw_and_nut_clear, sweep={"pitch": [0.8, 1.25, 2.0, 2.5], "thread_form": ["print"], "diameter": [14]})


def shank_is_threaded_to_the_iso_depth():
    """The threaded shank's volume (measured before the head went on) lies
    between the minor and the major cylinders': a thread was cut, and not
    more than one; the head sets the width."""
    lo, hi = screw.bbox()
    assert_close(hi[0] - lo[0], head_d, tol=0.1)
    v_minor = math.pi * (minor_d / 2.0) * (minor_d / 2.0) * length
    v_major = math.pi * r * r * length
    assert_true(shank_volume > v_minor and shank_volume < v_major, msg="the shank's volume is between the minor and major cylinders'")
    if thread_form == "iso":
        assert_close(depth, 0.5413 * pitch, tol=1e-3)
    return shank_volume


test("the shank is threaded to the ISO depth", shank_is_threaded_to_the_iso_depth, sweep={"pitch": [1.0, 1.5, 2.0], "thread_form": ["iso", "print"]})


# The nut runs on: 0.7 of the travel is about 13 turns of an M10 x 1.5, at
# a constant speed slow enough (about 24 degrees a frame at 24 fps) that the
# hex is not mistaken for turning backwards -- it repeats every 60 degrees.
animation("thread_on", [
    keyframe(0.0, nut_pos=0.0),
    keyframe(8.0, nut_pos=0.7),
    keyframe(8.75, nut_pos=0.7),
], fps=24, ease=ease.linear)
