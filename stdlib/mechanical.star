"""@cadpole/mechanical.star -- springs, threads, bolts and nuts, on `sweep_along` and `helix`.

    load("@cadpole/mechanical.star", "spring", "thread", "bolt", "nut",
         "iso_profile", "square_profile", "acme_profile", "thread_depth", "minor_diameter")

Every length is in millimetres. The library records nothing when loaded: it
declares no parameters and builds no bodies, so `load()`ing it is free, and
every body-building function takes `name=` and forwards it.

## Conventions

* A helix, a spring, a thread, a bolt and a nut all stand on **+Z** from
  `start` (default the origin): the spring's first coil centre, the
  thread's first pitch, the bolt's tip, the nut's bottom face are at
  `start`. Rotate or translate the body afterwards to place it.
* `hand` is `hand.right` (the default, a standard thread: counter-clockwise
  about +Z while climbing +Z) or `hand.left`.
* A thread profile is a dict describing the **groove** cut into the
  external thread's major cylinder, one pitch wide: `depth` (radially, from
  the major diameter), `top` (the groove's opening at the major diameter),
  `bottom` (its width at the minor diameter) and `angle` (each flank's
  angle from the radial direction, degrees). `iso_profile`, `square_profile`
  and `acme_profile` build the three standard ones for a pitch; a dict of
  the same shape is a custom one.
* `clearance` on an internal thread is the gap between the bolt's and the
  nut's surfaces *on the surface normal* — flank to flank, crest to root —
  so `distance(bolt, nut)` on a coaxial pair measures it. The nut's groove
  is offset outward by it: the flats radially, the flanks along their
  normals.
* **Every thread on an axis is phased by height, like a thread cut from
  stock**: a right-hand external groove's centre crosses the +X direction
  at `z = 0, p, 2p, …` and an internal thread's ridge at `z = p/2, 3p/2, …`,
  whatever `start` is. So a bolt and a nut built about the same axis mate at
  any height with no phase arithmetic; `phase=` turns a thread further, in
  degrees, when you want a particular start.

## How a thread is built

The profile is drawn in the axial plane at the helix's start and swept by
`sweep_along(..., frame=sweep_frame.frenet)`: on a helix the Frenet frame *is*
the screw motion about the axis, so the profile stays in the axial plane
as it turns and climbs -- a revolve with a pitch. The swept surface has no
NURBS form (a helix is not a rational curve), so it is a cubic B-spline
skin through stations along the helix, measured against the true sweep
and refined to within a hundredth of a millimetre (the sweep says how far
it got in an info diagnostic). An external thread is the major cylinder
**minus** the swept groove; an internal one is the nut blank minus the
**union** of the minor bore and the swept ridge.

The tangent case is avoided by construction. The groove's outer
face sits `overcut` (0.05 mm) outside the major cylinder and the ridge's
inner face `overcut` inside the bore, with the flanks continued, so the
only faces that cross the cylinder are the flanks, and they cross it at
the flank angle -- never coincident, never tangent. The helix runs
`overrun` (half a pitch plus 0.05 mm) past the ends of the thread, so the
cutter's caps sit in air beside the end faces rather than in them. A
thread's boolean is hundreds of NURBS faces against a cylinder, which is
the class of boolean the exact kernel's face budget routes to the mesh
kernel (docs/design.md); `kernel=` is exposed and defaults to `auto`.

## Formulas

ISO 68-1, with `H = sqrt(3)/2 * p`: the basic profile is the 60 degree
triangle truncated `H/8` at the crest (a flat `p/8` wide at the major
diameter `d`) and `H/4` at the root (a flat `p/4` wide at the minor
diameter `d1 = d - 5H/4 = d - 1.0825 p`); the thread depth is `5H/8 =
0.5413 p`. The groove is therefore `7p/8` wide at the major diameter and
`p/4` wide at the bottom, its flanks 30 degrees from the radial. A square
thread is `p/2` deep and `p/2` wide with radial flanks; an ACME thread is
`p/2` deep with a 29 degree included angle and flats of `0.3707 p`.
"""

_SQRT3 = 1.7320508075688772
_OVERCUT = 0.05
_END_GAP = 0.05


def _fail(where, msg):
    fail("mechanical.%s: %s" % (where, msg))


def _named(name, suffix):
    """`name=` for a sub-feature, or nothing when the caller gave no name."""
    return {"name": name + suffix} if name else {}


def _thread_phase(z_start, pitch, hand, offset, extra):
    """The helix phase (degrees) that puts a thread crossing +X at
    `z = offset + k pitch`: at the helix's start height the thread centre
    sits `360 (z_start - offset) / pitch` round the axis for a right-hand
    thread, the other way for a left-hand one."""
    frac = (z_start - offset) / pitch
    frac = frac - math.floor(frac)
    sign = 1.0 if hand == "right" else -1.0
    return sign * 360.0 * frac + extra


def _check_hand(where, hand):
    if hand != "right" and hand != "left":
        _fail(where, "hand=%r is not a hand; valid: right, left" % hand)
    return hand


def _positive(where, what, v):
    if type(v) != "int" and type(v) != "float":
        _fail(where, "%s must be a number (got %s)" % (what, repr(v)))
    if v <= 0:
        _fail(where, "%s must be positive (got %s)" % (what, v))
    return float(v)


def _point3(where, what, p):
    if type(p) != "tuple" and type(p) != "list":
        _fail(where, "%s must be a point (x, y, z), got %s" % (what, repr(p)))
    if len(p) != 3:
        _fail(where, "%s must be a point (x, y, z), got %d numbers" % (what, len(p)))
    return (float(p[0]), float(p[1]), float(p[2]))


# ----------------------------------------------------------------- profiles


def iso_profile(pitch):
    """The ISO 68-1 basic profile for `pitch`: the 60 degree groove, `5H/8`
    deep, `7p/8` wide at the major diameter and `p/4` at the bottom."""
    p = _positive("iso_profile", "pitch", pitch)
    h = _SQRT3 / 2.0 * p
    return {"depth": 5.0 * h / 8.0, "top": 7.0 * p / 8.0, "bottom": p / 4.0, "angle": 30.0}


def square_profile(pitch):
    """A square thread for `pitch`: `p/2` deep, `p/2` wide, radial flanks."""
    p = _positive("square_profile", "pitch", pitch)
    return {"depth": p / 2.0, "top": p / 2.0, "bottom": p / 2.0, "angle": 0.0}


def acme_profile(pitch):
    """An ACME (29 degree) thread for `pitch`: `p/2` deep, flats of `0.3707 p`."""
    p = _positive("acme_profile", "pitch", pitch)
    flat = 0.3707 * p
    depth = p / 2.0
    return {"depth": depth, "top": p - flat, "bottom": flat, "angle": 14.5}


def _profile_of(where, pitch, profile):
    if profile == "iso":
        return iso_profile(pitch)
    if profile == "square":
        return square_profile(pitch)
    if profile == "acme":
        return acme_profile(pitch)
    if type(profile) == "dict":
        for key in ["depth", "top", "bottom", "angle"]:
            if key not in profile:
                _fail(where, "a custom profile needs %r (depth, top, bottom, angle)" % key)
        d = _positive(where, "profile depth", profile["depth"])
        top = _positive(where, "profile top", profile["top"])
        bottom = _positive(where, "profile bottom", profile["bottom"])
        if top >= pitch:
            _fail(where, "the groove's opening (%s) must be narrower than the pitch (%s)" % (top, pitch))
        if bottom > top:
            _fail(where, "the groove's bottom (%s) cannot be wider than its opening (%s)" % (bottom, top))
        expect = 2.0 * d * math.tan(float(profile["angle"]))
        if abs((top - bottom) - expect) > 1e-3 * pitch:
            _fail(where, "the profile's flanks do not close: top - bottom = %s but 2 depth tan(angle) = %s" % (top - bottom, expect))
        return {"depth": d, "top": top, "bottom": bottom, "angle": float(profile["angle"])}
    _fail(where, 'profile=%r is not a thread profile; valid: iso, square, acme, or a dict with depth, top, bottom, angle' % profile)


def thread_depth(pitch, profile="iso"):
    """The radial depth of a thread of `pitch`: `0.5413 p` for ISO."""
    return _profile_of("thread_depth", _positive("thread_depth", "pitch", pitch), profile)["depth"]


def minor_diameter(major_d, pitch, profile="iso"):
    """The minor diameter of a thread: the major one less twice the depth
    (`d - 1.0825 p` for ISO)."""
    return _positive("minor_diameter", "major_d", major_d) - 2.0 * thread_depth(pitch, profile)


# ------------------------------------------------------------------ helpers


def _tan(angle):
    return math.tan(angle)


def _axial_plane(coil, start):
    """The plane containing the axis and the helix's start point, with x along
    the radial direction and y along the axis: where a thread profile is
    drawn, as (radial, axial) coordinates about the start point."""
    rad = (coil.start[0] - start[0], coil.start[1] - start[1], 0.0)
    n = (rad[1], -rad[0], 0.0)
    return plane(origin=coil.start, normal=n, x_dir=rad)


def _groove_points(r_major, prof, overcut):
    """The external groove, one pitch's worth, in (radial, axial) about the
    major radius: from the bottom out past the major cylinder by `overcut`
    with the flanks continued."""
    t = _tan(prof["angle"])
    half_top = prof["top"] / 2.0 + overcut * t
    half_bottom = prof["bottom"] / 2.0
    return [
        (r_major + overcut, -half_top),
        (r_major - prof["depth"], -half_bottom),
        (r_major - prof["depth"], half_bottom),
        (r_major + overcut, half_top),
    ]


def _ridge_points(r_major, pitch, prof, clearance, overcut):
    """The internal thread's groove -- the bolt's tooth, offset outward by
    `clearance` on every surface -- in (radial, axial) about the axis: from
    the nut's root flat at the major diameter down past the bore by
    `overcut` with the flanks continued."""
    t = _tan(prof["angle"])
    c = math.cos(prof["angle"])
    shift = clearance / c
    # The tooth's half-width at the major diameter and its flank slope.
    w_major = (pitch - prof["top"]) / 2.0
    r_root = r_major + clearance
    r_bore = r_major - prof["depth"] + clearance
    r_in = r_bore - overcut
    half_root = w_major - clearance * t + shift
    half_in = w_major + (r_major - r_in) * t + shift
    if 2.0 * half_root >= pitch:
        _fail("nut", "clearance %s leaves no root land between the nut's threads at pitch %s" % (clearance, pitch))
    # The crest land between the ridge's turns at the bore: what is left of
    # the pitch once the widened ridge is taken out. Under a tenth of a
    # pitch it is a fin the mesh kernel cannot keep (M8 x 1.25 ISO: a
    # clearance over about 0.2 mm), and a real nut would have no crest.
    land = pitch - 2.0 * (half_in - overcut * t)
    if land < 0.1 * pitch:
        _fail("nut", "clearance %s leaves a crest land of %s mm between the nut's threads at pitch %s, under a tenth of the pitch; the most this profile takes is about %s mm" % (clearance, math.round(land, 3), pitch, math.round(clearance - (0.1 * pitch - land) / 2.0 * c, 2)))
    return [(r_root, -half_root), (r_in, -half_in), (r_in, half_in), (r_root, half_root)]


def _sketch_polygon(pl, pts):
    return sketch(pl).polygon([(u, v) for u, v in pts])


def _fine(body, tolerance):
    """An operand of a mesh boolean tessellated at `tolerance` when one is
    given: the result carries that mesh, so a `distance()` on it
    measures to it."""
    if tolerance != None:
        body.tessellation(tolerance=tolerance)
    return body


# ------------------------------------------------------------------- spring


def spring(wire_d, coil_d, pitch, turns, hand="right", ends="open", start=(0, 0, 0), kernel="auto", name=None):
    """A compression spring: a round wire of `wire_d` swept along a helix of
    coil diameter `coil_d` (centre to centre), `pitch` per turn, `turns`
    turns, standing on +Z from `start` (the centre of the first coil).

    `ends="open"` leaves the wire's ends as the sweep left them, round;
    `ends="ground"` cuts the spring flat at `start.z` and at the top of the
    last turn (half the wire ground away at each end, the way a ground
    spring seats). The ground cut is a boolean of the spring's B-spline
    faces against two planes, on `kernel=` (default `auto`).

    Feature: `<name>_coil` (the helix), `<name>` (the sweep, or the cut).
    The wire is `sweep_along` under `sweep_frame.rmf`; a round wire cannot tell
    the frames apart. The exact volume is `pi (wire_d/2)^2` times the coil's
    length, and the sweep's info diagnostic says how far its surface sits
    from the true one."""
    w = _positive("spring", "wire_d", wire_d)
    d = _positive("spring", "coil_d", coil_d)
    p = _positive("spring", "pitch", pitch)
    n = _positive("spring", "turns", turns)
    _check_hand("spring", hand)
    if ends != "open" and ends != "ground":
        _fail("spring", "ends=%r is not an end style; valid: open, ground" % ends)
    if w >= d:
        _fail("spring", "the wire (%s) must be thinner than the coil diameter (%s) or the coils cross the axis" % (w, d))
    if w > p:
        _fail("spring", "the wire (%s) is thicker than the pitch (%s), so the coils would overlap" % (w, p))
    s = _point3("spring", "start", start)
    coil = helix(d / 2.0, p, n, hand=hand, start=s, **_named(name, "_coil"))
    wire = sweep_along(sketch(coil.start_plane).circle(w / 2.0), coil, frame=sweep_frame.rmf, **(_named(name, "") if ends == "open" else {}))
    if ends == "open":
        return wire
    height = p * n
    slab = box(d + w + 2.0, d + w + 2.0, height, center=True, at=(s[0], s[1], s[2]))
    return intersect(wire, slab, kernel=kernel, name=name)


# ------------------------------------------------------------------- thread


def thread(major_d, pitch, length, profile="iso", internal=False, hand="right", clearance=0.0, start=(0, 0, 0), phase=0.0, overrun=None, tolerance=None, kernel="mesh", name=None):
    """A threaded rod, or the tool that cuts a threaded hole.

    External (`internal=False`): a cylinder of `major_d` and `length` on
    +Z from `start`, with the groove of `profile` swept helically out of
    it at `pitch` -- the thread runs out at both ends. Internal
    (`internal=True`): the body to **subtract** from a nut or a boss to
    leave a threaded hole: the minor bore plus the swept ridge that the
    bolt's teeth occupy, offset outward by `clearance` on every surface,
    `length` tall from `start` and reaching 1 mm past both ends so the cut
    goes through.

    `overrun` is how far the helix runs past each end of the thread
    (default half a pitch plus 0.05 mm), which puts the sweep's caps in the
    air beside the end faces rather than in them (the tangent case,
    avoided). The thread is phased by height (see the module's
    conventions) plus `phase` degrees. `kernel=` is the boolean's, and the
    default is the mesh kernel: the exact kernel's contact classifier names
    the helix crossing the end faces at the helix angle (a few degrees: an
    edge-on-face contact) and refuses after six seconds of trying, and the
    mesh kernel does it in a tenth of one. `tolerance=` tessellates the
    operands at that chord tolerance, so the threaded body's mesh -- and a
    `distance()` measured on it -- is that fine (the document's otherwise).
    The result is a mesh body.

    Features: `<name>_helix`, `<name>_groove` (the sweep; its info
    diagnostic reports the approximation), `<name>_blank` (the cylinder)
    and `<name>` (the result)."""
    d = _positive("thread", "major_d", major_d)
    p = _positive("thread", "pitch", pitch)
    length = _positive("thread", "length", length)
    prof = _profile_of("thread", p, profile)
    _check_hand("thread", hand)
    if clearance < 0:
        _fail("thread", "clearance must not be negative (got %s)" % clearance)
    if 2.0 * prof["depth"] >= d:
        _fail("thread", "a thread %s deep does not fit a major diameter of %s" % (prof["depth"], d))
    s = _point3("thread", "start", start)
    over = (p / 2.0 + _END_GAP) if overrun == None else float(overrun)
    r = d / 2.0
    turns = (length + 2.0 * over) / p
    z0 = s[2] - over
    if internal:
        ph = _thread_phase(z0, p, hand, p / 2.0, phase)
        coil = helix(r, p, turns, hand=hand, start=(s[0], s[1], z0), phase=ph, **_named(name, "_helix"))
        pl = _axial_plane(coil, (s[0], s[1], z0))
        pts = _ridge_points(r, p, prof, clearance, _OVERCUT)
        ridge = _fine(sweep_along(_sketch_polygon(pl, [(u - r, v) for u, v in pts]), coil, frame=sweep_frame.frenet, **_named(name, "_ridge")), tolerance)
        r_bore = r - prof["depth"] + clearance
        # The bore's two seam edges lie on +X and -X, which is where the
        # ridge's start cap would lie in their plane; a quarter turn puts
        # them out of it.
        bore = rotate(_fine(cylinder(r_bore, length + 2.0, at=(s[0], s[1], s[2] - 1.0), **_named(name, "_bore_blank")), tolerance), 90.0 + ph, axis=(0, 0, 1), origin=s, **_named(name, "_bore"))
        return union(bore, ridge, kernel=kernel, **_named(name, ""))
    ph = _thread_phase(z0, p, hand, 0.0, phase)
    coil = helix(r, p, turns, hand=hand, start=(s[0], s[1], z0), phase=ph, **_named(name, "_helix"))
    pl = _axial_plane(coil, (s[0], s[1], z0))
    pts = _groove_points(r, prof, _OVERCUT)
    groove = _fine(sweep_along(_sketch_polygon(pl, [(u - r, v) for u, v in pts]), coil, frame=sweep_frame.frenet, **_named(name, "_groove")), tolerance)
    blank = _fine(cylinder(r, length, at=s, **_named(name, "_blank")), tolerance)
    return subtract(blank, groove, kernel=kernel, **_named(name, ""))


# ---------------------------------------------------------------- bolt, nut


_TAN30 = 0.5773502691896257


def _hex_chamfer(centre, z0, z1, af, bottom, top, tolerance, name):
    """The solid of revolution a hex blank is intersected with for the
    standard 30 degree chamfer: a cone through the bearing circle (0.95 of
    the width across flats) at each chamfered face, which trims the
    corners and leaves the arcs a forged head or a nut shows on every flat.
    `None` when the blank is too short for the two cones."""
    rc = 0.475 * af
    big = af / _SQRT3 + 0.05 * af
    drop = (big - rc) * _TAN30
    if (bottom and top and z1 - z0 <= 2.0 * drop + 0.01) or z1 - z0 <= drop + 0.01:
        return None
    reach = 0.5 * rc
    rise = reach * _TAN30
    pts = [(0, z0 - rise), (rc - reach, z0 - rise), (big, z0 + drop)] if bottom else [(0, z0 - 1.0), (big, z0 - 1.0)]
    pts += [(big, z1 - drop), (rc - reach, z1 + rise), (0, z1 + rise)] if top else [(big, z1 + 1.0), (0, z1 + 1.0)]
    # The revolve's seam turned off the hex corners and flats.
    tool = rotate(revolve(sketch(XZ).polygon(pts), **_named(name, "_cone")), 15.0, axis=(0, 0, 1), **_named(name, "_cone_turned"))
    return _fine(translate(tool, (centre[0], centre[1], 0), **_named(name, "_chamfer_tool")), tolerance)


def bolt(major_d, pitch, length, head_af, head_h, thread_length=None, profile="iso", hand="right", start=(0, 0, 0), phase=0.0, chamfer=True, tolerance=None, kernel="mesh", name=None):
    """A hex-head bolt: the tip at `start`, the shank `length` long up +Z,
    the hex head (`head_af` across flats, `head_h` tall) sitting on top of
    it. The thread covers `thread_length` from the tip (default the whole
    shank less one pitch, so the groove ends inside the shank rather than
    running into the head).

    `chamfer=True` (the default) finishes it the way a bolt is made: the
    head's top corners turned off at 30 degrees from the bearing circle
    (0.95 of `head_af`), and the tip chamfered at 45 degrees down to the
    thread's minor diameter, so the thread starts clean. `chamfer=False`
    is the bare hex prism and cylinder.

    Features: the thread's (`<name>_helix`, `<name>_groove`,
    `<name>_blank`, `<name>_shank`), `<name>_head` (and with `chamfer`,
    `<name>_head_chamfer`) and `<name>`."""
    d = _positive("bolt", "major_d", major_d)
    p = _positive("bolt", "pitch", pitch)
    length = _positive("bolt", "length", length)
    af = _positive("bolt", "head_af", head_af)
    hh = _positive("bolt", "head_h", head_h)
    s = _point3("bolt", "start", start)
    tl = (length - p) if thread_length == None else _positive("bolt", "thread_length", thread_length)
    if tl > length:
        _fail("bolt", "thread_length (%s) is longer than the shank (%s)" % (tl, length))
    prof = _profile_of("bolt", p, profile)
    _check_hand("bolt", hand)
    r = d / 2.0
    over = p / 2.0 + _END_GAP
    turns = (tl + over) / p
    z0 = s[2] - over
    ph = _thread_phase(z0, p, hand, 0.0, phase)
    coil = helix(r, p, turns, hand=hand, start=(s[0], s[1], z0), phase=ph, **_named(name, "_helix"))
    pl = _axial_plane(coil, (s[0], s[1], z0))
    pts = _groove_points(r, prof, _OVERCUT)
    groove = _fine(sweep_along(_sketch_polygon(pl, [(u - r, v) for u, v in pts]), coil, frame=sweep_frame.frenet, **_named(name, "_groove")), tolerance)
    tip = min(prof["depth"], 0.5 * length) if chamfer else 0.0
    if tip > 0:
        # Turned with the seam a quarter turn off +X, where the groove's
        # start cap lies.
        blank = _fine(translate(rotate(revolve(sketch(XZ).polygon([(0, 0), (r - tip, 0), (r, tip), (r, length), (0, length)]), **_named(name, "_blank_profile")), 90.0, axis=(0, 0, 1), **_named(name, "_blank_turned")), s, **_named(name, "_blank")), tolerance)
    else:
        blank = _fine(cylinder(r, length, at=s, **_named(name, "_blank")), tolerance)
    shank = subtract(blank, groove, kernel=kernel, **_named(name, "_shank"))
    head = _fine(extrude(sketch(XY.offset(s[2] + length), origin=(s[0], s[1], s[2] + length)).ngon(6, af / _SQRT3), hh, **_named(name, "_head")), tolerance)
    cone = _hex_chamfer(s, s[2] + length, s[2] + length + hh, af, False, True, tolerance, _named(name, "_head").get("name")) if chamfer else None
    if cone != None:
        head = intersect(head, cone, kernel=kernel, **_named(name, "_head_chamfer"))
    return union(shank, head, kernel=kernel, **_named(name, ""))


def nut(major_d, pitch, af, height, clearance=0.1, profile="iso", hand="right", start=(0, 0, 0), phase=0.0, chamfer=True, tolerance=None, kernel="mesh", name=None):
    """A hex nut for a thread of `major_d` and `pitch`: `af` across flats,
    `height` tall, standing on +Z from `start`, its thread offset outward by
    `clearance` so a coaxial bolt clears it by that much on every surface
    (`distance(bolt, nut)` measures it).

    `chamfer=True` (the default) turns both faces' corners off at 30
    degrees from the bearing circle (0.95 of `af`), the double chamfer of a
    standard hex nut; `chamfer=False` is the bare hex prism.

    Features: the internal thread's (`<name>_helix`, `<name>_ridge`,
    `<name>_bore`, `<name>_tool`), `<name>_blank` (and with `chamfer`,
    `<name>_blank_chamfer`) and `<name>`."""
    d = _positive("nut", "major_d", major_d)
    p = _positive("nut", "pitch", pitch)
    af = _positive("nut", "af", af)
    h = _positive("nut", "height", height)
    s = _point3("nut", "start", start)
    if af <= d:
        _fail("nut", "a nut %s across flats has no wall around a %s thread" % (af, d))
    tool = thread(d, p, h, profile=profile, internal=True, hand=hand, clearance=clearance, start=s, phase=phase, tolerance=tolerance, kernel=kernel, **_named(name, "_tool"))
    blank = _fine(extrude(sketch(XY.offset(s[2]), origin=(s[0], s[1], s[2])).ngon(6, af / _SQRT3), h, **_named(name, "_blank")), tolerance)
    cone = _hex_chamfer(s, s[2], s[2] + h, af, True, True, tolerance, _named(name, "_blank").get("name")) if chamfer else None
    if cone != None:
        blank = intersect(blank, cone, kernel=kernel, **_named(name, "_blank_chamfer"))
    return subtract(blank, tool, kernel=kernel, **_named(name, ""))
