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
#   * the head is a straight knurl of 90 degree teeth: a star prism
#     turned to shape by a revolved outline, a 35 degree chamfer at each
#     end and a crowned top round a flat land, so every ridge runs out into the
#     chamfer cone in a clean crescent, as a lathe leaves it. Below it a
#     shoulder collar, and a thread undercut where the thread stops short of
#     the collar, so the last turn runs out instead of ending in a wall;
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
#     flanks 40 degrees off the axis, which prints without support, and
#     flattens the head's crown. Every chamfer that faces the bed (the
#     head's top, the nut's lower corners, the countersinks, the undercut's
#     flank) is 35-40 degrees off vertical for the same reason;
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
knurl = param("knurl", 60, min=24, max=90, doc="Ridges round the knurled head; each is a 90 degree tooth")
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


def knurl_depth(rad, teeth):
    """The depth of a straight knurl's 90 degree teeth: half the pitch."""
    return 0.5 * math.pi * 2.0 * rad / teeth


def knurled_drum(z0, rad, height, teeth, chamfer, dome, name):
    """A straight-knurled drum on +Z from z0: a prism of 90 degree teeth
    turned to shape by a revolved outline -- a chamfer cone 35 degrees off
    the axis at each end (no overhang either way up) and, on top, a crown
    `dome` high round a flat land -- so the ridges run out into the cones as a
    lathe leaves them and both end faces are true circles."""
    kdepth = knurl_depth(rad, teeth)
    inset = 0.7 * chamfer
    rc = rad - inset
    top = z0 + height
    prism = translate(
        extrude(sketch(XY).polygon(star_loop(rad, kdepth, teeth, 0)), height + dome + 2.0),
        (0, 0, z0 - 1.0),
        name=name + "_teeth",
    )
    out = path((0.0, z0)).line((rc, z0)).line((rad + 1.0, z0 + (inset + 1.0) / 0.7))
    out = out.line((rad + 1.0, top - (inset + 1.0) / 0.7)).line((rc, top))
    if dome > 0.0:
        # A crowned top: an arc tangent to a flat central land half the
        # face's radius, which the part stands on when printed head down.
        land = 0.5 * rc
        run = rc - land
        big_r = (run * run + dome * dome) / (2.0 * dome)
        mid_z = top + dome - big_r + math.sqrt(big_r * big_r - 0.25 * run * run)
        out = out.arc((land, top + dome), via=(land + 0.5 * run, mid_z)).line((0.0, top + dome))
    else:
        out = out.line((0.0, top))
    turned = revolve(sketch(XZ).add(out), axis=(0, 0, 1), name=name + "_turned")
    return intersect(prism, turned, kernel="mesh", name=name)


def solid_of_revolution(pts, name):
    """A closed (radius, z) outline turned about +Z."""
    return revolve(sketch(XZ).polygon(pts), axis=(0, 0, 1), name=name)


# -------------------------------------------------------------------- screw

head_d = head_ratio * major
head_h = max(0.9 * major, 7.0)
head_ch = 0.08 * head_d
# A machined head is crowned; the print form's is flat, since a crown would
# hang over the bed when the screw stands on its head.
head_dome = 0.03 * head_d if thread_form == "iso" else 0.0
k_depth = knurl_depth(head_d / 2.0, knurl)

shank = thread(major, pitch, length, profile=prof, name="shank_thread")
# Chamfer the tip to just inside the minor diameter at 45 degrees, so the
# thread starts with a full flank instead of a feather edge.
tip_cut = solid_of_revolution([
    (0.0, -1.0),
    (minor_d / 2.0 - 0.3 - 1.0, -1.0),
    (r + 0.5, depth + 0.6),
    (r + 0.5, length + 1.0),
    (0.0, length + 1.0),
], "tip_chamfer")
shank = intersect(shank, tip_cut, kernel="mesh", name="shank")
shank_volume = shank.volume()

# The thread undercut: a groove just inside the minor diameter, 0.8 pitch of
# flat below the collar, its lower flank 35 degrees off the axis (which
# prints standing on the head), so the thread's last turn runs out into it.
uc_r = minor_d / 2.0 - 0.15
uc_z = length - 0.8 * pitch
undercut = solid_of_revolution([
    (uc_r, uc_z),
    (uc_r, length - 0.1),
    (r + 1.0, length - 0.1),
    (r + 1.0, uc_z - (r + 1.0 - uc_r) / math.tan(35.0)),
], "undercut")
shank = subtract(shank, undercut, kernel="mesh", name="shank_undercut")

# A shoulder between the shank and the head, then the knurled head.
collar_h = 0.25 * major
collar = solid_of_revolution([
    (0.0, length - 0.2),
    (r * 1.45, length - 0.2),
    (r * 1.45, length + collar_h),
    (0.0, length + collar_h),
], "collar")
head = knurled_drum(length + collar_h - 0.01, head_d / 2.0, head_h, knurl, head_ch, head_dome, "head")
screw = union(shank, collar, head, kernel="mesh", name="screw")
screw_top = length + collar_h - 0.01 + head_h + head_dome

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
    blank = knurled_drum(0.0, nut_d / 2.0, nut_h, int(knurl * 0.75), 0.06 * nut_d, 0.0, "nut_blank")

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
nut.material(preset=preset.steel, color="#b4bac2", roughness=0.2)

export([screw, nut], "thumbscrew.3mf")

# Three-quarters from the left, a little above the head, on a 24 degree
# lens: the knurl's run-outs, the crowned top and the nut's chamfered corners
# all in light. The eye stands 30 degrees left of the front at 20 degrees up,
# at a distance that fills the frame with the assembled screw.
if layout == "assembled":
    cam_target = (0.0, 0.0, 0.5 * screw_top)
    cam_d = 0.56 * screw_top / math.tan(12.0) + 0.5 * head_d
    render_defaults(camera=view(
        eye=(-cam_d * math.cos(20.0) * math.sin(30.0), -cam_d * math.cos(20.0) * math.cos(30.0), cam_target[2] + cam_d * math.sin(20.0)),
        target=cam_target,
        fov=24,
    ))

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


def radii_at(body, z):
    """The smallest and largest radius of a body's section at height z."""
    rs = []
    for loop in body.section(XY.offset(z)).points():
        for (x, y) in loop:
            rs.append(math.sqrt(x * x + y * y))
    return (min(rs), max(rs), len(rs))


def head_z0():
    return length + collar_h - 0.01


def each_part_is_one_solid():
    """No stray bodies: the screw and the nut each come out as one closed
    solid, with or without the cutaway and with either nut."""
    for part in [screw, nut]:
        assert_solid(part)
        assert_true(print_check(part).components == 1, msg="each part is one piece")
    return True


test("each part is one solid", each_part_is_one_solid, sweep={"nut_style": ["hex", "knurled"], "cutaway": [False, True]})


def head_is_a_straight_knurl():
    """Mid-height, the head's outline is the knurl: every tooth there, its
    tips on the head's diameter and its valleys half a pitch deeper -- a 90
    degree tooth. At the chamfers the outline is a true circle inside the
    valleys, so every ridge runs out into the cone; the top is crowned."""
    lo, hi, n = radii_at(screw, head_z0() + 0.5 * head_h)
    assert_true(n >= 2 * knurl, msg="every tooth of the knurl is there")
    assert_close(hi, head_d / 2.0, tol=0.02)
    assert_close(hi - lo, k_depth, tol=0.03)
    assert_close(k_depth, 0.5 * math.pi * head_d / knurl, tol=1e-6)
    for z in [head_z0() + 0.2 * head_ch, head_z0() + head_h - 0.2 * head_ch]:
        c_lo, c_hi, _ = radii_at(screw, z)
        assert_true(c_hi < head_d / 2.0 - k_depth, msg="the chamfer runs below the knurl's valleys")
        assert_true(c_hi - c_lo < 0.06, msg="the chamfer is round, within the chord tolerance")
    assert_close(screw.bbox()[1][2], screw_top, tol=0.02)
    return hi - lo


test("the head is a straight knurl that runs out into its chamfers", head_is_a_straight_knurl, sweep={"knurl": [40, 60, 80], "head_ratio": [2.0, 2.6, 3.2]})


def thread_runs_out_into_the_undercut():
    """Under the collar the thread stops: the shank there is round and
    inside the minor diameter, so the last turn runs out into a groove."""
    lo, hi, _ = radii_at(screw, length - 0.4 * pitch)
    assert_true(hi < minor_d / 2.0, msg="the undercut is inside the minor diameter")
    assert_true(hi - lo < 0.06, msg="the undercut is round, within the chord tolerance")
    t_lo, t_hi, _ = radii_at(screw, length - 4.0 * pitch)
    assert_true(t_hi > r - 0.1, msg="below the undercut the thread is full")
    return hi


test("the thread runs out into an undercut below the collar", thread_runs_out_into_the_undercut, sweep={"pitch": [1.0, 1.5, 2.0]})


# The nut runs on: 0.7 of the travel is about 13 turns of an M10 x 1.5, at
# a constant speed slow enough (about 24 degrees a frame at 24 fps) that the
# hex is not mistaken for turning backwards -- it repeats every 60 degrees.
animation("thread_on", [
    keyframe(0.0, nut_pos=0.0),
    keyframe(8.0, nut_pos=0.7),
    keyframe(8.75, nut_pos=0.7),
], fps=24, ease=ease.linear)
