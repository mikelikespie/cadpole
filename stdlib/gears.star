"""@cadpole/gears.star -- involute gears: profiles, bodies and the arithmetic.

    load("@cadpole/gears.star", "spur", "helical", "herringbone", "internal", "rack", "bevel",
         "involute_profile", "layout", "tooth_faces",
         "pitch_diameter", "tip_diameter", "root_diameter", "base_diameter", "tooth_thickness",
         "centre_distance", "mesh_turn", "min_teeth", "auto_shift", "planetary", "planetary_set")

Every length is in millimetres, every angle in degrees (the unit `math.*`,
`rotate()` and `revolve(angle=)` speak), and every tooth count is an int.
The library records nothing when loaded: it declares no parameters and
builds no bodies, so `load()`ing it is free, and every function takes
`name=` and forwards it.

## Conventions

* `module` is the **normal** module `m_n`, the tooth size a hob or a 3D
  printer sees. A spur gear's transverse module is the same number; a
  helical gear's is `m_t = m_n / cos(helix_angle)`, so its pitch diameter is
  `m_n * teeth / cos(helix_angle)` (`pitch_diameter(helix_angle=)`), and two
  helical gears mesh when they share `m_n` and `helix_angle` with opposite
  hands. `backlash`, `clearance`, `fillet` and `tolerance` are measured in
  the **transverse** plane, the plane of the outline that is drawn.
* `pressure_angle` is the normal pressure angle `alpha_n` (20 by default);
  the transverse one is `tan(alpha_t) = tan(alpha_n) / cos(helix_angle)`.
* `profile_shift` is the coefficient `x`, in modules: the generating rack is
  moved `x * m_n` away from the gear centre. Positive shift thickens the
  tooth and avoids undercut on small pinions (`auto_shift`).
* `backlash` is how much **thinner** than the theoretical value each tooth
  is at the pitch circle, in mm. Thinning both gears of a pair by `b` gives
  the pair a circumferential backlash of `2 b`, and the flanks of the
  centred mesh a normal gap of `b * cos(alpha)`.
* `clearance` is the root clearance coefficient `c` (0.25), `fillet` the
  generating rack's tip radius coefficient (0.38, ISO 53 profile A), both in
  modules.
* Tooth 0 of an external gear is centred on **+x**; an internal gear's
  tooth *space* 0 is centred on +x (so its teeth sit at `(k + 1/2) * 360 /
  teeth`), which is what a planet on the +x axis meets. A rack's tooth 0 is
  centred on x = 0 with its pitch line on y = 0 and its teeth pointing +y.
  A bevel gear has its pitch-cone apex at the origin and its axis on +z.
* `hand` is `hand.right` or `hand.left` (`"right"` / `"left"`): a right-hand
  tooth turns counter-clockwise (the right-hand rule about +z) as z grows.
  A herringbone's `hand` is the hand of its lower half (z from 0 to
  `width / 2`); the upper half has the other.

## Formulas

With `z` teeth, transverse module `m_t`, transverse pressure angle
`alpha_t`, shift `x`, clearance `c` and rack tip radius `rho = fillet * m_n`:

    r   = m_t z / 2                      reference (pitch) radius
    r_b = r cos(alpha_t)                 base radius
    r_a = r + m_n (1 + x)                tip radius        (d_a = m (z + 2 + 2x) for a spur)
    r_f = r - m_n (1 + c - x)            root radius       (d_f = m (z - 2.5 + 2x) at c = 0.25)
    s_t = m_t (pi/2 + 2 x tan(alpha_n)) - backlash          tooth thickness at r, transverse
    inv(a) = tan(a) - a                                     the involute function, radians

The flank is the involute of the base circle: a point with roll parameter
`t` sits at radius `r_b sqrt(1 + t^2)`, at the angle `psi_b - inv(atan t)`
from the tooth's centreline, where `psi_b = s_t / (2 r) + inv(alpha_t)` is the
half tooth angle at the base circle. It is the envelope of the generating
rack's straight flank. The root is the **trochoid** the rack's rounded tip
sweeps: the rack space centred on the tooth has flanks
`a(b) = s_0/2 - (b - x m_n) tan(alpha_t)` (`a` along the pitch line, `b`
radial, `s_0 = pi m_t / 2 - backlash`), its tip line at `b_t = -(1 + c - x)
m_n`, and a tip fillet of radius `rho` centred at `(a_t + rho (1 - sin
alpha_t) / cos alpha_t, b_t + rho)`. A point of that fillet at direction
`psi` from its centre cuts the gear when its normal passes through the pitch
point, i.e. at the roll angle `phi = -(a_c - b_c cot psi) / r`; the gear-frame
point is `R(-phi) (r + b_Q, a_Q + r phi)`. The rack's straight flank meets the
involute on its live branch only while the fillet's tangency point is within
`r sin^2(alpha_t)` of the pitch line:

    undercut  <=>  (1 + c - x) m_n - rho (1 - sin alpha_t) > r sin^2(alpha_t)
    min_teeth  =  2 ((1 + c - x) - fillet (1 - sin alpha_t)) / sin^2(alpha_t)      (17.1 at 20 deg)

Below that count the trochoid crosses the involute, and the library
generates the real undercut: the crossing is found between the two curves
and the flank is the trochoid below it, the involute above. Both curves are
sampled densely and then thinned to `tolerance` (Douglas-Peucker), so the
segment count adapts to the module and the tooth count.

An internal gear's tooth **space** is that external tooth: the same base
circle, the space width `e = m_t (pi/2 + 2 x tan alpha_n) + backlash` at the
pitch circle, the ring's tip at `r - m_n (1 - x)` (never inside the base
circle: a small ring's addendum is shortened to it) and its root at
`r + m_n (1 + c + x)`, with a circular fillet of radius `rho` where the
involute meets the root circle.

A helical gear is the transverse profile turned by `dz tan(helix) / r`
radians per `dz` of height, built as a smooth `loft` through rotated
copies of the outline, so every flank is one B-spline surface interpolating
sections a few degrees apart. A herringbone is two such halves, one per
hand, joined by `glue` on their shared mid-plane face. A straight bevel is
Tredgold's approximation: the outline is the involute of the virtual spur
gear with `z / cos(delta)` teeth drawn on the developed back cone and mapped
onto the transverse plane (radii scaled by `cos delta`, angles by `1 / cos
delta`), and the toe is that outline scaled about the apex, so a ruled loft
between them has every flank line through the apex. It is not the spherical
involute; the tooth is right at the heel and slightly off toward the toe.

## Names

Each body is the feature `<name>` (an `extrude`, a `loft`, or a `glue` of
`<name>_lower` and `<name>_upper`); its profile sketches are `<name>_profile`
or `<name>_section_<k>`. Faces: the caps `<name>/start` (z = 0) and
`<name>/end` -- the extrude's and the loft's own names; on a herringbone
`<name>_lower/start` and `<name>_upper/end` -- and the
outline's segments `<name>/side[0.<s>]`, `s` counted round loop 0 from the
root land before tooth 0. `layout(...)` says which segments are which, and
`tooth_faces(body, layout, i)` turns that into Selections named `root`,
`fillet_right`, `flank_right`, `tip`, `flank_left`, `fillet_left`, where
`flank_left` is the flank on the counter-clockwise side of tooth `i`'s
centreline (the +y side of tooth 0). A bore is loop 1: `<name>/side[1.*]`.
"""

# ----------------------------------------------------------------- helpers

_DENSE_FLANK = 64  # dense samples on an involute before thinning
_DENSE_FILLET = 40  # dense samples on a trochoid or fillet arc before thinning


def _polar(r, angle):
    return (r * math.cos(angle), r * math.sin(angle))


def _inv(alpha):
    """The involute function of an angle in degrees, in radians."""
    return math.tan(alpha) - math.radians(alpha)


def _inv_inverse(y):
    """The angle (degrees) whose involute function is `y` (radians), by Newton."""
    a = math.pow(3.0 * y, 1.0 / 3.0) if y > 0 else 0.0  # radians, the small-angle start
    for _ in range(40):
        t = math.tan(math.degrees(a))
        f = t - a - y
        d = t * t
        if d == 0:
            break
        step = f / d
        a = a - step
        if abs(step) < 1e-14:
            break
    return math.degrees(a)


def _fail(where, msg):
    fail("gears.%s: %s" % (where, msg))


def _check_hand(where, hand):
    if hand != "right" and hand != "left":
        _fail(where, "hand=%r is not a hand; valid: right, left" % hand)
    return 1.0 if hand == "right" else -1.0


def _check_common(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance):
    if type(teeth) != "int":
        _fail(where, "teeth must be an int (got %s)" % repr(teeth))
    if teeth < 3:
        _fail(where, "teeth must be at least 3 (got %d)" % teeth)
    if module <= 0:
        _fail(where, "module must be positive (got %s)" % module)
    if pressure_angle <= 0 or pressure_angle >= 45:
        _fail(where, "pressure_angle must be between 0 and 45 degrees (got %s)" % pressure_angle)
    if abs(helix_angle) >= 60:
        _fail(where, "helix_angle must be below 60 degrees in magnitude (got %s)" % helix_angle)
    if clearance < 0:
        _fail(where, "clearance must be >= 0 (got %s)" % clearance)
    if fillet < 0:
        _fail(where, "fillet must be >= 0 (got %s)" % fillet)
    if backlash < 0:
        _fail(where, "backlash must be >= 0 (got %s)" % backlash)
    if tolerance <= 0:
        _fail(where, "tolerance must be positive (got %s)" % tolerance)
    if profile_shift <= -1 or profile_shift >= 1:
        _fail(where, "profile_shift must be between -1 and 1 (got %s)" % profile_shift)


def _simplify(pts, tol):
    """Douglas-Peucker: drop the points within `tol` of the chord between
    their kept neighbours. The end points are always kept."""
    n = len(pts)
    if n <= 2 or tol <= 0:
        return list(pts)
    keep = [False] * n
    keep[0] = True
    keep[n - 1] = True
    stack = [(0, n - 1)]
    for _ in range(2 * n + 2):
        if not stack:
            break
        a, b = stack.pop()
        if b - a < 2:
            continue
        ax, ay = pts[a]
        bx, by = pts[b]
        dx, dy = bx - ax, by - ay
        length = math.hypot(dx, dy)
        best, best_i = -1.0, -1
        for i in range(a + 1, b):
            px, py = pts[i]
            if length > 0:
                d = abs(dx * (py - ay) - dy * (px - ax)) / length
            else:
                d = math.hypot(px - ax, py - ay)
            if d > best:
                best, best_i = d, i
        if best > tol:
            keep[best_i] = True
            stack.append((a, best_i))
            stack.append((best_i, b))
    return [pts[i] for i in range(n) if keep[i]]


def _arc_segments(radius, sweep, tol):
    """Chords for an arc of `sweep` degrees whose sag stays under `tol`."""
    if sweep <= 0:
        return 1
    if tol >= radius:
        return 1
    half = math.acos(1.0 - tol / radius)  # degrees, half the chord's angle
    return max(1, int(math.ceil(sweep / (2.0 * half))))


def _land_segments(radius, half_sweep, tol):
    """The root land between two fillets: chords per half, or 0 when the whole
    land (2 * half_sweep) is so short that a vertex at its middle would sit
    within tol / 4 of the chord between the fillets -- a sliver the cap's
    triangulation cannot take (a 100-tooth gear's root land is 0.15 mm)."""
    if radius * (1.0 - math.cos(half_sweep)) < tol / 4.0:
        return 0
    return _arc_segments(radius, half_sweep, tol)


def _segments_cross(p1, p2, q1, q2):
    """The crossing of segments p1-p2 and q1-q2, or None."""
    rx, ry = p2[0] - p1[0], p2[1] - p1[1]
    sx, sy = q2[0] - q1[0], q2[1] - q1[1]
    den = rx * sy - ry * sx
    if abs(den) < 1e-15:
        return None
    qx, qy = q1[0] - p1[0], q1[1] - p1[1]
    t = (qx * sy - qy * sx) / den
    u = (qx * ry - qy * rx) / den
    if t < 0 or t > 1 or u < 0 or u > 1:
        return None
    return (p1[0] + t * rx, p1[1] + t * ry)


def _to_polar(p):
    return (math.hypot(p[0], p[1]), math.atan2(p[1], p[0]))


def _point_segment_distance(p, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    l2 = dx * dx + dy * dy
    if l2 == 0:
        return math.hypot(p[0] - ax, p[1] - ay)
    t = ((p[0] - ax) * dx + (p[1] - ay) * dy) / l2
    t = math.clamp(t, 0.0, 1.0)
    return math.hypot(p[0] - (ax + t * dx), p[1] - (ay + t * dy))


# ----------------------------------------------------------------- arithmetic


def pitch_diameter(module, teeth, *, helix_angle=0.0):
    """The reference (pitch) diameter `module * teeth / cos(helix_angle)`, mm."""
    return module * teeth / math.cos(helix_angle)


def base_diameter(module, teeth, *, pressure_angle=20.0, helix_angle=0.0):
    """The base-circle diameter, where the involute starts: `d cos(alpha_t)`."""
    alpha_t = math.atan(math.tan(pressure_angle) / math.cos(helix_angle))
    return pitch_diameter(module, teeth, helix_angle=helix_angle) * math.cos(alpha_t)


def tip_diameter(module, teeth, *, profile_shift=0.0, helix_angle=0.0):
    """The outside diameter `d + 2 m (1 + x)`: `m (z + 2 + 2 x)` for a spur gear."""
    return pitch_diameter(module, teeth, helix_angle=helix_angle) + 2.0 * module * (1.0 + profile_shift)


def root_diameter(module, teeth, *, profile_shift=0.0, clearance=0.25, helix_angle=0.0):
    """The root diameter `d - 2 m (1 + c - x)`: `m (z - 2.5 + 2 x)` at c = 0.25."""
    return pitch_diameter(module, teeth, helix_angle=helix_angle) - 2.0 * module * (1.0 + clearance - profile_shift)


def tooth_thickness(module, *, pressure_angle=20.0, profile_shift=0.0, backlash=0.0, helix_angle=0.0):
    """The transverse tooth thickness on the pitch circle:
    `m_t (pi/2 + 2 x tan(alpha_n)) - backlash`, i.e. `pi m / 2 - backlash` unshifted."""
    m_t = module / math.cos(helix_angle)
    return m_t * (math.pi / 2.0 + 2.0 * profile_shift * math.tan(pressure_angle)) - backlash


def centre_distance(module, teeth1, teeth2, *, shift1=0.0, shift2=0.0, pressure_angle=20.0, helix_angle=0.0):
    """The centre distance of two external gears: `m_t (z1 + z2) / 2` when the
    shifts cancel, otherwise `m_t (z1 + z2) / 2 * cos(alpha_t) / cos(alpha_wt)` with
    `inv(alpha_wt) = inv(alpha_t) + 2 (x1 + x2) tan(alpha_n) / (z1 + z2)`.
    For an internal gear pass its tooth count negative: `centre_distance(m, 12, -36)`
    is the sun-to-planet distance of a planetary, `m (36 - 12) / 2`."""
    m_t = module / math.cos(helix_angle)
    alpha_t = math.atan(math.tan(pressure_angle) / math.cos(helix_angle))
    zsum = teeth1 + teeth2
    if zsum == 0:
        _fail("centre_distance", "teeth1 + teeth2 must not be zero")
    if shift1 + shift2 == 0:
        return abs(m_t * zsum / 2.0)
    if teeth1 < 0 or teeth2 < 0:
        _fail("centre_distance", "profile shifts on an internal pair are not implemented; pass shift1=shift2=0")
    inv_w = _inv(alpha_t) + 2.0 * (shift1 + shift2) * math.tan(pressure_angle) / zsum
    if inv_w <= 0:
        _fail("centre_distance", "the shifts %s and %s bring the working pressure angle to zero" % (shift1, shift2))
    alpha_wt = _inv_inverse(inv_w)
    return abs(m_t * zsum / 2.0) * math.cos(alpha_t) / math.cos(alpha_wt)


def mesh_turn(teeth):
    """The turn, in degrees, that puts a gear's *space* on the -x axis when its
    tooth 0 is on +x: half a pitch for an even tooth count, nothing for an odd
    one. A wheel placed at `(centre_distance, 0)` and turned by this meshes
    with a pinion at the origin whose tooth 0 points at it."""
    return 180.0 / teeth if teeth % 2 == 0 else 0.0


def min_teeth(*, pressure_angle=20.0, profile_shift=0.0, clearance=0.25, fillet=0.38, helix_angle=0.0):
    """The smallest tooth count the generating rack cuts without undercut:
    `2 ((1 + c - x) - fillet (1 - sin alpha_t)) / sin^2(alpha_t)`, transverse
    for a helical gear. 17.1 at 20 degrees with the ISO rack."""
    alpha_t = math.atan(math.tan(pressure_angle) / math.cos(helix_angle))
    s = math.sin(alpha_t)
    return 2.0 * ((1.0 + clearance - profile_shift) - fillet * (1.0 - s)) / (s * s) * math.cos(helix_angle)


def auto_shift(teeth, *, pressure_angle=20.0, clearance=0.25, fillet=0.38, helix_angle=0.0):
    """The smallest non-negative profile shift at which `teeth` is not undercut;
    0 when the count is already above `min_teeth`."""
    alpha_t = math.atan(math.tan(pressure_angle) / math.cos(helix_angle))
    s = math.sin(alpha_t)
    x = (1.0 + clearance) - fillet * (1.0 - s) - teeth / math.cos(helix_angle) * s * s / 2.0
    return max(0.0, x)


def planetary(sun, planet, *, planets=3, module=1.0, helix_angle=0.0):
    """The arithmetic of a simple planetary with `sun` and `planet` teeth and
    `planets` equally spaced planets. Returns a dict:

      ring                 the ring's tooth count, `sun + 2 planet`
      centre_distance      sun-to-planet centre distance, `module (sun + planet) / (2 cos helix_angle)`
                           -- the transverse module's, so a helical or herringbone set
                           passes its `helix_angle` here as it does to the gears
      ratio_fixed_ring     the reduction `1 + ring / sun` (sun in, carrier out, ring held)
      ratio_fixed_carrier  `-ring / sun` (sun in, ring out, carrier held; the sign is the reversal)
      ratio_fixed_sun      `1 + sun / ring` (ring in, carrier out, sun held)
      planet_angles        where planet k sits, degrees round +x
      planet_turns         the rotation of planet k (built with tooth 0 on +x) so it meshes
                           with the sun and the ring at once
      ring_turn            the rotation of the ring (built with space 0 on +x)

    Refuses when the assembly condition `(sun + ring) % planets == 0` fails --
    the planets could not be spaced equally -- or when neighbouring planets'
    tips would collide."""
    for v, what in ((sun, "sun"), (planet, "planet"), (planets, "planets")):
        if type(v) != "int" or v < 1:
            _fail("planetary", "%s must be a positive int (got %r)" % (what, v))
    if sun < 3 or planet < 3:
        _fail("planetary", "sun and planet need at least 3 teeth")
    if abs(helix_angle) >= 60:
        _fail("planetary", "helix_angle must be below 60 degrees in magnitude (got %s)" % helix_angle)
    ring = sun + 2 * planet
    if (sun + ring) % planets != 0:
        ok = [n for n in range(1, 13) if (sun + ring) % n == 0 and (n == 1 or (sun + planet) * math.sin(180.0 / n) > planet + 2)]
        _fail(
            "planetary",
            "%d planets cannot be spaced equally: sun + ring = %d + %d = %d is not a multiple of %d (counts that work: %s)"
            % (planets, sun, ring, sun + ring, planets, ", ".join([str(n) for n in ok])),
        )
    if planets > 1 and (sun + planet) * math.sin(180.0 / planets) <= planet + 2:
        _fail(
            "planetary",
            "%d planets of %d teeth on a %d-tooth sun collide tip to tip: (sun + planet) sin(180/n) = %s must exceed planet + 2 = %d"
            % (planets, planet, sun, math.round((sun + planet) * math.sin(180.0 / planets), 2), planet + 2),
        )
    angles = [360.0 * k / planets for k in range(planets)]
    turns = []
    pitch = 360.0 / planet
    for k in range(planets):
        # The planet must show the sun a space where the sun shows it a tooth:
        # phase (in pitches, 0 = tooth centred) toward the sun is 1/2 - k sun / n.
        phase = 0.5 - k * sun / planets
        turn = angles[k] + 180.0 - pitch * phase
        turn = turn - pitch * math.floor(turn / pitch)
        turns.append(turn)
    # The ring shows the planet on +x a tooth when the planet (even) shows it
    # a space there, and a space when the planet (odd) shows a tooth.
    ring_turn = 180.0 / ring if planet % 2 == 0 else 0.0
    return {
        "sun": sun,
        "planet": planet,
        "ring": ring,
        "planets": planets,
        "module": module,
        "helix_angle": helix_angle,
        "centre_distance": module * (sun + planet) / (2.0 * math.cos(helix_angle)),
        "ratio_fixed_ring": 1.0 + ring / sun,
        "ratio_fixed_carrier": -ring / sun,
        "ratio_fixed_sun": 1.0 + sun / ring,
        "planet_angles": angles,
        "planet_turns": turns,
        "ring_turn": ring_turn,
    }


# ----------------------------------------------------------------- the profile


def _geometry(module, teeth, pressure_angle, profile_shift, clearance, helix_angle, internal):
    """The radii and angles of one gear, transverse."""
    cos_b = math.cos(helix_angle)
    m_t = module / cos_b
    alpha_t = math.atan(math.tan(pressure_angle) / cos_b)
    r = m_t * teeth / 2.0
    r_b = r * math.cos(alpha_t)
    if internal:
        r_tip = r - module * (1.0 - profile_shift)
        r_root = r + module * (1.0 + clearance + profile_shift)
    else:
        r_tip = r + module * (1.0 + profile_shift)
        r_root = r - module * (1.0 + clearance - profile_shift)
    return {"m_t": m_t, "alpha_t": alpha_t, "r": r, "r_b": r_b, "r_tip": r_tip, "r_root": r_root}


def _external_half(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tol):
    """The +y half of one external tooth, transverse, as two polylines in
    (radius, angle-in-degrees) from the tooth's centreline:
    `fillet` from the root circle up to the junction, `flank` from the
    junction up to the tip. Also the tip half-angle and the root half-angle."""
    g = _geometry(module, teeth, pressure_angle, profile_shift, clearance, helix_angle, False)
    m_t, alpha_t, r, r_b, r_a, r_f = g["m_t"], g["alpha_t"], g["r"], g["r_b"], g["r_tip"], g["r_root"]
    rho = fillet * module
    sin_a, cos_a, tan_a = math.sin(alpha_t), math.cos(alpha_t), math.tan(alpha_t)

    # The generating rack, transverse: datum at b = x m_n, flank a(b).
    s0 = math.pi * m_t / 2.0 - backlash
    h_f = module * (1.0 + clearance - profile_shift)
    b_t = -h_f
    a_t = s0 / 2.0 + h_f * tan_a
    b_c = b_t + rho
    a_c = a_t + rho * (1.0 - sin_a) / cos_a
    if r_f <= 0:
        _fail(where, "the root circle collapses: module %s with %d teeth leaves no material at the centre" % (module, teeth))
    theta_root = a_c / r  # radians: where the +y fillet meets the root circle
    if theta_root >= math.pi / teeth:
        rho_max = (math.pi * m_t / 2.0 - a_t) * cos_a / (1.0 - sin_a)
        _fail(
            where,
            "the root fillet does not fit between the teeth: fillet=%s but at most %s (in modules) fits with %s teeth, %s backlash and shift %s"
            % (fillet, math.round(rho_max / module, 3), teeth, backlash, profile_shift),
        )

    # The involute: half tooth angle at the base circle, then the tip.
    s_t = s0 + 2.0 * profile_shift * module * tan_a
    psi_b = s_t / (2.0 * r) + _inv(alpha_t)
    if r_a <= r_b:
        _fail(where, "the tip circle is inside the base circle (%d teeth, shift %s)" % (teeth, profile_shift))
    t_a = math.sqrt((r_a / r_b) * (r_a / r_b) - 1.0)
    theta_a = psi_b - (t_a - math.atan(t_a) * math.pi / 180.0)
    if theta_a <= 0:
        _fail(
            where,
            "the tooth comes to a point below the tip circle (%d teeth, shift %s, %s backlash): reduce profile_shift or backlash"
            % (teeth, profile_shift, backlash),
        )

    def involute_at(t):
        rr = r_b * math.sqrt(1.0 + t * t)
        th = psi_b - (t - math.radians(math.atan(t)))
        return (rr * math.cos(math.degrees(th)), rr * math.sin(math.degrees(th)))

    def trochoid_at(psi):  # psi in degrees, the fillet point's direction from its centre
        cot = math.cos(psi) / math.sin(psi)
        a_i = a_c - b_c * cot
        phi = -a_i / r  # radians, the gear's rotation
        q_a = a_c + rho * math.cos(psi)
        q_b = b_c + rho * math.sin(psi)
        wx, wy = r + q_b, q_a + r * phi
        phi_deg = math.degrees(phi)
        c, s = math.cos(-phi_deg), math.sin(-phi_deg)
        return (wx * c - wy * s, wx * s + wy * c)

    involute = [involute_at(t_a * i / _DENSE_FLANK) for i in range(_DENSE_FLANK + 1)]
    psi_end = 180.0 + alpha_t
    trochoid = [trochoid_at(270.0 - (270.0 - psi_end) * i / _DENSE_FILLET) for i in range(_DENSE_FILLET + 1)]

    undercut = h_f - rho * (1.0 - sin_a) > r * sin_a * sin_a
    crossing = None
    if undercut:
        # The trochoid crosses the involute's live branch: find where, walking
        # the trochoid from the root. At the marginal count the two curves
        # only touch at the base circle, and no crossing is the tangent case.
        for i in range(len(trochoid) - 1):
            for j in range(len(involute) - 1):
                x = _segments_cross(trochoid[i], trochoid[i + 1], involute[j], involute[j + 1])
                if x != None:
                    crossing = (i, j, x)
                    break
            if crossing != None:
                break
    if crossing != None:
        i, j, x = crossing
        fillet_pts = trochoid[: i + 1] + [x]
        flank_pts = [x] + involute[j + 1 :]
    else:
        junction = trochoid[-1]
        rj = math.hypot(junction[0], junction[1])
        t_j = math.sqrt(max(0.0, (rj / r_b) * (rj / r_b) - 1.0))
        fillet_pts = trochoid
        flank_pts = [involute_at(t_j)] + [involute_at(t_j + (t_a - t_j) * i / _DENSE_FLANK) for i in range(1, _DENSE_FLANK + 1)]

    fillet_pts = _simplify(fillet_pts, tol)
    flank_pts = _simplify(flank_pts, tol)
    return {
        "fillet": [_to_polar(p) for p in fillet_pts],  # root -> junction
        "flank": [_to_polar(p) for p in flank_pts],  # junction -> tip
        "theta_tip": math.degrees(theta_a),
        "theta_root": math.degrees(theta_root),
        "r_tip": r_a,
        "r_root": r_f,
        "undercut": undercut,
        "geometry": g,
    }


def _internal_half(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tol):
    """The +y half of one tooth *space* of an internal gear, in the same shape
    as `_external_half`: `fillet` from the root circle (outer) down to the
    tangency, `flank` from the tangency down to the ring's tip circle."""
    g = _geometry(module, teeth, pressure_angle, profile_shift, clearance, helix_angle, True)
    m_t, alpha_t, r, r_b, r_tip, r_root = g["m_t"], g["alpha_t"], g["r"], g["r_b"], g["r_tip"], g["r_root"]
    rho = fillet * module
    # The ring's tip may not sit inside the base circle: shorten the addendum.
    r_tip = max(r_tip, r_b)
    e_t = m_t * (math.pi / 2.0 + 2.0 * profile_shift * math.tan(pressure_angle)) + backlash
    psi_b = e_t / (2.0 * r) + _inv(alpha_t)

    def theta_at(rr):  # the space's half angle at radius rr, radians
        t = math.sqrt(max(0.0, (rr / r_b) * (rr / r_b) - 1.0))
        return psi_b - (t - math.radians(math.atan(t)))

    def involute_at(rr):
        th = math.degrees(theta_at(rr))
        return (rr * math.cos(th), rr * math.sin(th))

    theta_tip = theta_at(r_tip)  # the space's half angle at the ring's tip circle
    if theta_tip >= math.pi / teeth:
        _fail(where, "the ring's teeth vanish: %d teeth with shift %s and %s backlash leave no tip land" % (teeth, profile_shift, backlash))
    if theta_at(r_root) <= 0:
        _fail(where, "the tooth space closes before the root circle (%d teeth, shift %s)" % (teeth, profile_shift))

    dense = [involute_at(r_tip + (r_root - r_tip) * i / _DENSE_FLANK) for i in range(_DENSE_FLANK + 1)]  # tip -> root

    def sharp():
        flank = _simplify(dense, tol)
        return {
            "fillet": [_to_polar(flank[-1])],
            "flank": [_to_polar(p) for p in reversed(flank)],
            "theta_tip": math.degrees(theta_tip),
            "theta_root": math.degrees(theta_at(r_root)),
            "r_tip": r_tip,
            "r_root": r_root,
            "undercut": False,
            "geometry": g,
        }

    if rho <= 0:
        return sharp()
    # The root fillet: an arc of radius rho tangent to the root circle (its
    # centre at r_root - rho) and to the involute. Bisection on the centre's
    # angle: at the space's centreline it is farthest from the flank, on the
    # flank it is at distance zero.
    r_c = r_root - rho
    if r_c <= r_tip:
        _fail(where, "fillet=%s is larger than the tooth depth of a %d-tooth ring" % (fillet, teeth))

    def distance_to_flank(gamma):  # gamma in degrees, the centre's angle
        c = (r_c * math.cos(gamma), r_c * math.sin(gamma))
        best = 1e300
        for i in range(len(dense) - 1):
            d = _point_segment_distance(c, dense[i], dense[i + 1])
            if d < best:
                best = d
        return best

    # A small ring's space closes fast toward its root circle, and the ISO
    # rack's 0.38 m may not fit: the ring's root fillet is `fillet * m` or
    # the largest radius that fits, whichever is smaller (it is cosmetic --
    # the planet's tip clears the root by `clearance * m` anyway).
    if distance_to_flank(0.0) < rho:
        lo_r, hi_r = 0.0, rho
        for _ in range(40):
            mid = (lo_r + hi_r) / 2.0
            rho = mid
            r_c = r_root - rho
            if distance_to_flank(0.0) >= rho:
                lo_r = mid
            else:
                hi_r = mid
        rho = lo_r * 0.98
        r_c = r_root - rho
        if rho <= 1e-6:
            return sharp()
    gamma_hi = math.degrees(theta_at(r_c))  # the centre on the flank
    lo, hi = 0.0, gamma_hi
    for _ in range(60):
        mid = (lo + hi) / 2.0
        if distance_to_flank(mid) > rho:
            lo = mid
        else:
            hi = mid
    gamma = (lo + hi) / 2.0
    if gamma >= 180.0 / teeth:
        _fail(where, "the root fillets of neighbouring spaces overlap on a %d-tooth ring: reduce fillet=%s" % (teeth, fillet))
    centre = (r_c * math.cos(gamma), r_c * math.sin(gamma))
    # The tangency on the involute: the closest dense point, refined by projection.
    best, best_i = 1e300, 0
    for i in range(len(dense) - 1):
        d = _point_segment_distance(centre, dense[i], dense[i + 1])
        if d < best:
            best, best_i = d, i
    a, b = dense[best_i], dense[best_i + 1]
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    t = math.clamp(((centre[0] - a[0]) * dx + (centre[1] - a[1]) * dy) / l2, 0.0, 1.0) if l2 > 0 else 0.0
    tangency = (a[0] + t * dx, a[1] + t * dy)
    # The arc from the root circle (direction gamma from the centre, outward)
    # to the tangency, as seen from the centre.
    ang_root = gamma
    ang_tan = math.atan2(tangency[1] - centre[1], tangency[0] - centre[0])
    sweep = ang_tan - ang_root
    if sweep > 180.0:
        sweep = sweep - 360.0
    if sweep < -180.0:
        sweep = sweep + 360.0
    arc = [
        (centre[0] + rho * math.cos(ang_root + sweep * i / _DENSE_FILLET), centre[1] + rho * math.sin(ang_root + sweep * i / _DENSE_FILLET))
        for i in range(_DENSE_FILLET + 1)
    ]
    arc[-1] = tangency
    fillet_pts = _simplify(arc, tol)
    r_tan = math.hypot(tangency[0], tangency[1])
    flank_dense = [tangency] + [involute_at(r_tan + (r_tip - r_tan) * i / _DENSE_FLANK) for i in range(1, _DENSE_FLANK + 1)]
    flank_pts = _simplify(flank_dense, tol)
    return {
        "fillet": [_to_polar(p) for p in fillet_pts],  # root circle -> tangency
        "flank": [_to_polar(p) for p in flank_pts],  # tangency -> ring tip
        "theta_tip": math.degrees(theta_tip),
        "theta_root": gamma,
        "r_tip": r_tip,
        "r_root": r_root,
        "undercut": False,
        "geometry": g,
    }


def _tooth_sequence(half, teeth, tol, internal):
    """One tooth's vertices as (radius, angle) relative to the tooth's centre,
    counter-clockwise from the middle of the root land before it, the closing
    vertex excluded, plus the segment layout of the run."""
    fillet, flank = half["fillet"], half["flank"]
    theta_tip, theta_root = half["theta_tip"], half["theta_root"]
    r_tip, r_root = half["r_tip"], half["r_root"]
    half_pitch = 180.0 / teeth
    n_f = len(fillet) - 1
    n_i = len(flank) - 1
    if internal:
        # A ring tooth sits between two spaces. Relative to the tooth centre,
        # the space before it is at -half_pitch and the one after at +half_pitch.
        # fillet: root circle (outer) -> tangency; flank: tangency -> ring tip.
        # Root run: from the space's centre to its fillet (angle theta_root of the space).
        n_rl = _land_segments(r_root, theta_root, tol)
        n_t = _arc_segments(r_tip, 2.0 * (half_pitch - theta_tip), tol)
        seq = []
        # Right side (clockwise of the tooth): space at -half_pitch, its +y half.
        for k in range(n_rl):
            seq.append((r_root, -half_pitch + theta_root * k / n_rl))
        for rr, th in fillet:
            seq.append((rr, -half_pitch + th))
        for rr, th in flank[1:]:
            seq.append((rr, -half_pitch + th))
        # Tip land of the ring tooth, from -half_pitch + theta_tip to +half_pitch - theta_tip.
        a0, a1 = -half_pitch + theta_tip, half_pitch - theta_tip
        for k in range(1, n_t + 1):
            seq.append((r_tip, a0 + (a1 - a0) * k / n_t))
        # Left side: the space at +half_pitch, its -y half, walked tip -> root.
        for rr, th in reversed(flank[:-1]):
            seq.append((rr, half_pitch - th))
        for rr, th in reversed(fillet[:-1]):
            seq.append((rr, half_pitch - th))
        for k in range(1, n_rl):
            seq.append((r_root, half_pitch - theta_root + theta_root * k / n_rl))
    else:
        n_rl = _land_segments(r_root, half_pitch - theta_root, tol)
        n_t = _arc_segments(r_tip, 2.0 * theta_tip, tol)
        seq = []
        for k in range(n_rl):
            seq.append((r_root, -half_pitch + (half_pitch - theta_root) * k / n_rl))
        for rr, th in fillet:  # root -> junction, mirrored to -y
            seq.append((rr, -th))
        for rr, th in flank[1:]:  # junction -> tip, -y side
            seq.append((rr, -th))
        for k in range(1, n_t + 1):
            seq.append((r_tip, -theta_tip + 2.0 * theta_tip * k / n_t))
        for rr, th in reversed(flank[:-1]):  # tip -> junction, +y side
            seq.append((rr, th))
        for rr, th in reversed(fillet[:-1]):  # junction -> root
            seq.append((rr, th))
        for k in range(1, n_rl):
            seq.append((r_root, theta_root + (half_pitch - theta_root) * k / n_rl))
    # With no land vertex the land is the one segment that closes each
    # tooth's run onto the next tooth's first vertex.
    segments = (2 * n_rl if n_rl > 0 else 1) + 2 * n_f + 2 * n_i + n_t
    if len(seq) != segments:
        _fail("layout", "internal error: %d vertices for %d segments" % (len(seq), segments))
    parts = {}
    parts["root"] = [(0, n_rl), (segments - n_rl, segments)] if n_rl > 0 else [(segments - 1, segments)]
    o = n_rl
    parts["fillet_right"] = [(o, o + n_f)]
    o += n_f
    parts["flank_right"] = [(o, o + n_i)]
    o += n_i
    parts["tip"] = [(o, o + n_t)]
    o += n_t
    parts["flank_left"] = [(o, o + n_i)]
    o += n_i
    parts["fillet_left"] = [(o, o + n_f)]
    layout_ = {
        "teeth": teeth,
        "segments": segments,
        "parts": parts,
        "internal": internal,
        "undercut": half["undercut"],
        "r_tip": r_tip,
        "r_root": r_root,
        "r_pitch": half["geometry"]["r"],
        "r_base": half["geometry"]["r_b"],
    }
    return seq, layout_


def _half(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, internal):
    _check_common(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance)
    if internal:
        return _internal_half(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance)
    return _external_half(where, module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance)


def _outline(seq, teeth, turn, r_scale=1.0, a_scale=1.0):
    """The closed outline: `seq` repeated round `teeth` times, turned by `turn`
    degrees. `r_scale`/`a_scale` are the bevel gear's Tredgold mapping."""
    pts = []
    for i in range(teeth):
        c = 360.0 * i / teeth + turn
        for rr, th in seq:
            pts.append(_polar(rr * r_scale, c + th * a_scale))
    return pts


def layout(module, teeth, *, pressure_angle=20.0, profile_shift=0.0, backlash=0.0, clearance=0.25, fillet=0.38, helix_angle=0.0, tolerance=0.02, internal=False):
    """Which outline segments belong to which part of a tooth, for the same
    arguments as `involute_profile`. A dict: `teeth`, `segments` (per tooth),
    `parts` (name -> list of `(first, past_last)` segment ranges within a
    tooth's run), `internal`, `undercut`, and the radii `r_tip`, `r_root`,
    `r_pitch`, `r_base`. Tooth `i` owns segments `i * segments` up to
    `(i + 1) * segments` of loop 0. Pure arithmetic: no feature is recorded."""
    half = _half("layout", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, internal)
    _, lay = _tooth_sequence(half, teeth, tolerance, internal)
    return lay


def tooth_faces(body, layout, tooth):
    """The faces of tooth `tooth` of a gear `body` built from `layout`, as a
    dict of Selections: `root`, `fillet_right`, `flank_right`, `tip`,
    `flank_left`, `fillet_left`. Works on spur, helical, herringbone and
    internal gears (on a herringbone each Selection holds both halves' faces),
    after any transform, because names survive them."""
    z, per = layout["teeth"], layout["segments"]
    if type(tooth) != "int" or tooth < 0 or tooth >= z:
        _fail("tooth_faces", "tooth must be an int in 0..%d (got %r)" % (z - 1, tooth))
    base = tooth * per
    out = {}
    for part, ranges in layout["parts"].items():
        sel = None
        for a, b in ranges:
            for s in range(a, b):
                f = body.faces(name="side[0.%d]" % (base + s))
                sel = f if sel == None else sel | f
        out[part] = sel
    return out


def involute_profile(
    module,
    teeth,
    *,
    pressure_angle=20.0,
    profile_shift=0.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    helix_angle=0.0,
    tolerance=0.02,
    internal=False,
    turn=0.0,
    plane=XY,
    name=None,
):
    """A Sketch of the full gear outline as one loop: the transverse involute
    profile of `teeth` teeth of normal module `module`, tooth 0 centred on +x
    (an internal gear's space 0 on +x), turned by `turn` degrees. Add a bore
    or a keyway with the Sketch's own loop methods and `extrude` it, or let
    `spur` do that. `helix_angle` only changes the transverse section (what a
    helical gear's end face shows); the twist is `helical`'s job.

    Names: the sketch is `<name>`; segment `s` of loop 0 becomes face
    `side[0.<s>]` of whatever is built from it -- see `layout`."""
    half = _half("involute_profile", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, internal)
    seq, _ = _tooth_sequence(half, teeth, tolerance, internal)
    phase = 180.0 / teeth if internal else 0.0
    return sketch(plane, name=name).polygon(_outline(seq, teeth, phase + turn))


# ----------------------------------------------------------------- bodies


def _sub(name, suffix):
    return None if name == None else name + suffix


def _section(z, pts, hole_r, name, turn=0.0):
    """The outline `pts` and a second loop: a circle of radius `hole_r` (none
    when 0), or, when `hole_r` is a list of polar points `(r, degrees)`, that
    polygon turned with the outline by `turn` degrees."""
    s = sketch(XY.offset(z), name=name).polygon(pts)
    if type(hole_r) == "list":
        s = s.polygon([_polar(r, th + turn) for r, th in hole_r])
    elif hole_r > 0:
        s = s.circle(hole_r)
    return s


def _stack(name, seq, teeth, phase, width, turn0, turn1, hole_r, suffix, z0=0.0):
    """A smooth loft of the outline from z0 to z0 + width, turned by `turn0`
    degrees at the bottom and `turn1` at the top. Equal turns are one extrude."""
    if turn0 == turn1:
        s = _section(z0, _outline(seq, teeth, phase + turn0), hole_r, _sub(name, "_profile"), turn0)
        return extrude(s, width, name=name)
    n = max(4, int(math.ceil(abs(turn1 - turn0) / 20.0)) + 1)
    sections = []
    for k in range(n + 1):
        f = k / n
        turn = turn0 + (turn1 - turn0) * f
        sections.append(_section(z0 + width * f, _outline(seq, teeth, phase + turn), hole_r, _sub(name, "%s_section_%d" % (suffix, k)), turn))
    return loft(sections, ruled=False, name=name)


def _twist(where, width, helix_angle, r_pitch, hand):
    """The turn over `width`, degrees: `width tan(helix) / r_pitch` radians, signed by hand."""
    return math.degrees(width * math.tan(helix_angle) / r_pitch) * _check_hand(where, hand)


def _gear_body(where, name, seq, geom, teeth, width, helix_angle, hand, herringbone, hole_r, internal):
    if width <= 0:
        _fail(where, "width must be positive (got %s)" % width)
    phase = 180.0 / teeth if internal else 0.0
    if helix_angle == 0 and not herringbone:
        return _stack(name, seq, teeth, phase, width, 0.0, 0.0, hole_r, "")
    if helix_angle == 0:
        _fail(where, "a herringbone needs a helix_angle (got 0)")
    twist = _twist(where, width, helix_angle, geom["r"], hand)
    if not herringbone:
        return _stack(name, seq, teeth, phase, width, 0.0, twist, hole_r, "")
    # Two halves of opposite hand that share the mid-plane section, glued on it.
    lower = _stack(_sub(name, "_lower"), seq, teeth, phase, width / 2.0, 0.0, twist / 2.0, hole_r, "_lower")
    upper = _stack(_sub(name, "_upper"), seq, teeth, phase, width / 2.0, twist / 2.0, 0.0, hole_r, "_upper", z0=width / 2.0)
    return glue(lower, upper, name=name)


def spur(
    module,
    teeth,
    width,
    *,
    bore=0.0,
    pressure_angle=20.0,
    profile_shift=0.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    name=None,
):
    """A spur gear: `involute_profile` extruded `width` along +z from z = 0,
    tooth 0 on +x, with a plain bore of diameter `bore` (0 for none). One
    extrude, so it is an exact B-rep body that exports to STEP.

    Names: `<name>`, `<name>_profile`; faces `<name>/start`, `<name>/end`,
    `<name>/side[0.*]` (the teeth, see `layout`), `<name>/side[1.*]` (the bore)."""
    half = _half("spur", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, 0.0, tolerance, False)
    seq, lay = _tooth_sequence(half, teeth, tolerance, False)
    if bore < 0 or bore / 2.0 >= lay["r_root"]:
        _fail("spur", "bore %s does not fit inside the root circle (diameter %s)" % (bore, math.round(2 * lay["r_root"], 3)))
    return _gear_body("spur", name, seq, half["geometry"], teeth, width, 0.0, "right", False, bore / 2.0, False)


def helical(
    module,
    teeth,
    width,
    helix_angle,
    *,
    hand="right",
    bore=0.0,
    pressure_angle=20.0,
    profile_shift=0.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    name=None,
):
    """A helical gear of normal module `module`: the transverse profile (module
    `m / cos(helix_angle)`, pressure angle `atan(tan(alpha) / cos(helix_angle))`)
    lofted smoothly through sections turned by `dz tan(helix_angle) / r_pitch`,
    `hand` right or left. Its pitch diameter is `pitch_diameter(module, teeth,
    helix_angle=)`; a mate needs the same module and helix angle and the
    other hand. Exact B-rep; every flank is one surface.

    Names: `<name>`, `<name>_section_<k>`; faces as `spur`."""
    if helix_angle == 0:
        _fail("helical", "helix_angle is 0; use spur() for a straight gear")
    half = _half("helical", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, False)
    seq, lay = _tooth_sequence(half, teeth, tolerance, False)
    if bore < 0 or bore / 2.0 >= lay["r_root"]:
        _fail("helical", "bore %s does not fit inside the root circle (diameter %s)" % (bore, math.round(2 * lay["r_root"], 3)))
    return _gear_body("helical", name, seq, half["geometry"], teeth, width, helix_angle, hand, False, bore / 2.0, False)


def herringbone(
    module,
    teeth,
    width,
    helix_angle,
    *,
    hand="right",
    bore=0.0,
    pressure_angle=20.0,
    profile_shift=0.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    name=None,
):
    """A double-helical gear: a `hand` helical half from z = 0 to `width / 2`
    and the opposite hand above it, glued on the mid-plane they share, so the
    teeth form a V that locks the gear axially against its mate. Each half is
    a smooth loft; the apex is sharp.

    Names: `<name>` (the glue), `<name>_lower`, `<name>_upper`; faces keep the
    halves' names, and `tooth_faces` selects both halves at once."""
    if helix_angle == 0:
        _fail("herringbone", "helix_angle is 0; a herringbone needs one")
    half = _half("herringbone", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, False)
    seq, lay = _tooth_sequence(half, teeth, tolerance, False)
    if bore < 0 or bore / 2.0 >= lay["r_root"]:
        _fail("herringbone", "bore %s does not fit inside the root circle (diameter %s)" % (bore, math.round(2 * lay["r_root"], 3)))
    return _gear_body("herringbone", name, seq, half["geometry"], teeth, width, helix_angle, hand, True, bore / 2.0, False)


def internal(
    module,
    teeth,
    width,
    rim,
    *,
    helix_angle=0.0,
    hand="right",
    herringbone=False,
    pressure_angle=20.0,
    profile_shift=0.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    flutes=0,
    flute_depth=0.8,
    flute_radius=None,
    name=None,
):
    """A ring gear: the internal involute (the tooth space is the external
    tooth of the same base circle; addendum and dedendum swap) as the inner
    loop of a sketch whose outer loop is a circle `rim` beyond the root
    circle, extruded -- or lofted with a helix, or as a herringbone. Space 0
    is centred on +x. One sketch, no boolean, exact.

    `flutes` > 0 makes the outer loop a grip instead of a circle: that many
    round grooves of radius `flute_radius` (default `2 * flute_depth`) sunk
    `flute_depth` into it (`fluted_outline`). The grooves are part of the
    section, so they turn with the teeth: straight on a spur ring, helical on
    a helical one and chevrons on a herringbone, and still no boolean.

    Names: as `spur`; loop 0 is the teeth, loop 1 the housing."""
    if rim <= 0:
        _fail("internal", "rim must be positive (got %s)" % rim)
    half = _half("internal", module, teeth, pressure_angle, profile_shift, backlash, clearance, fillet, helix_angle, tolerance, True)
    seq, lay = _tooth_sequence(half, teeth, tolerance, True)
    housing = lay["r_root"] + rim
    if flutes > 0:
        if flute_depth <= 0 or flute_depth >= rim:
            _fail("internal", "flute_depth %s must be positive and under the rim %s" % (flute_depth, rim))
        housing = fluted_outline(housing, flutes, flute_depth, radius=flute_radius, tolerance=tolerance / 4.0)
    return _gear_body("internal", name, seq, half["geometry"], teeth, width, helix_angle, hand, herringbone, housing, True)


def fluted_outline(r, count, depth, *, radius=None, tolerance=0.005):
    """A circle of radius `r` with `count` round grooves sunk `depth` into it,
    each an arc of radius `radius` (default `2 * depth`) centred `radius -
    depth` outside the circle; the first groove is centred half a pitch off
    +x. The closed outline as polar points `(r, degrees)`, counter-clockwise,
    every arc divided so its chords stand within `tolerance` of it: what
    `internal(flutes=)` lofts as a ring's housing."""
    if count < 3:
        _fail("fluted_outline", "count must be at least 3 (got %s)" % count)
    if radius == None:
        radius = 2.0 * depth
    if depth <= 0 or depth >= radius:
        _fail("fluted_outline", "depth %s must be positive and under the groove radius %s" % (depth, radius))
    rc = r + radius - depth
    # The groove meets the circle where |C + radius (cos a, sin a)| = r, a
    # in degrees from the outward radial at the groove's centre.
    cos_a0 = (r * r - rc * rc - radius * radius) / (2.0 * rc * radius)
    a0 = math.acos(max(-1.0, min(1.0, cos_a0)))
    hit = _to_polar((rc + radius * math.cos(a0), -radius * math.sin(a0)))  # (r, deg) of the groove's leading corner, about its centre's angle
    pitch = 360.0 / count
    if 2.0 * abs(hit[1]) >= pitch:
        _fail("fluted_outline", "%s grooves of radius %s overlap on a circle of radius %s" % (count, radius, r))
    n_groove = max(4, int(math.ceil((360.0 - 2.0 * a0) / _arc_step(radius, tolerance))))
    land = pitch - 2.0 * abs(hit[1])
    n_land = max(1, int(math.ceil(land / _arc_step(r, tolerance))))
    pts = []
    for i in range(count):
        c = pitch * (i + 0.5)
        for j in range(n_groove + 1):
            a = (360.0 - a0) - (360.0 - 2.0 * a0) * j / n_groove  # down through 180: the inner arc
            pr, pt = _to_polar((rc + radius * math.cos(a), radius * math.sin(a)))
            pts.append((pr, c + pt))
        for j in range(1, n_land):
            pts.append((r, c + abs(hit[1]) + land * j / n_land))
    return pts


def _arc_step(r, tolerance):
    """The largest angle, degrees, whose chord on radius `r` sags at most `tolerance`."""
    return 2.0 * math.acos(max(0.0, 1.0 - tolerance / r))


def rack(
    module,
    length,
    width,
    height,
    *,
    pressure_angle=20.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    name=None,
):
    """A straight rack: teeth along x centred on x = 0 (tooth 0 at x = 0, pitch
    `pi * module`), the pitch line on y = 0, tips at y = +module, roots at
    `-(1 + clearance) module`, the back at `y = -height`, extruded `width`
    along +z. Flanks are straight at `pressure_angle`, roots rounded by an
    arc of `fillet * module` -- the rack is its own generator, so this is
    exact. A rack thinned by `backlash` meshes with a gear on the +y side
    whose pitch circle touches y = 0. No helical racks yet.

    Names: `<name>`, `<name>_profile`; faces `<name>/side[0.*]` in outline
    order from the bottom-left corner (the count depends on `length`)."""
    if module <= 0 or length <= 0 or width <= 0:
        _fail("rack", "module, length and width must be positive")
    if pressure_angle <= 0 or pressure_angle >= 45:
        _fail("rack", "pressure_angle must be between 0 and 45 degrees (got %s)" % pressure_angle)
    h_f = (1.0 + clearance) * module
    if height <= h_f:
        _fail("rack", "height %s must exceed the tooth depth below the pitch line, %s" % (height, h_f))
    if fillet < 0 or backlash < 0:
        _fail("rack", "fillet and backlash must be >= 0")
    p = math.pi * module
    tan_a, sin_a, cos_a = math.tan(pressure_angle), math.sin(pressure_angle), math.cos(pressure_angle)
    rho = fillet * module
    half_top = p / 4.0 - module * tan_a - backlash / 2.0  # half the tooth at the tip
    half_root = p / 4.0 + h_f * tan_a - backlash / 2.0  # half the tooth at the root line
    if half_top <= 0:
        _fail("rack", "the teeth come to a point: %s backlash is too much for module %s" % (backlash, module))
    space_root = p / 2.0 - half_root  # half the space at the root line
    setback = rho * (1.0 - sin_a) / cos_a  # from the corner to the arc's start on the root line
    if setback >= space_root:
        _fail("rack", "fillet=%s does not fit in the tooth space: at most %s (in modules)" % (fillet, math.round(space_root * cos_a / (1.0 - sin_a) / module, 3)))
    n_arc = max(1, _arc_segments(rho, 90.0 - pressure_angle, tolerance)) if rho > 0 else 0

    def tooth(x0):
        """One tooth's upper outline from the middle of the space before it to
        the middle of the space after it, at y = -h_f between teeth."""
        pts = []
        # Right fillet of the previous space -> left flank.
        cx = x0 - half_root - setback  # fillet centre x; centre y = -h_f + rho
        if rho > 0:
            cy = -h_f + rho
            for k in range(n_arc + 1):
                ang = -90.0 + (90.0 - pressure_angle) * k / n_arc
                pts.append((cx + rho * math.cos(ang), cy + rho * math.sin(ang)))
        else:
            pts.append((x0 - half_root, -h_f))
        pts.append((x0 - half_top, module))
        pts.append((x0 + half_top, module))
        if rho > 0:
            cx = x0 + half_root + setback
            cy = -h_f + rho
            for k in range(n_arc + 1):
                ang = 270.0 - (90.0 - pressure_angle) * (n_arc - k) / n_arc
                pts.append((cx + rho * math.cos(ang), cy + rho * math.sin(ang)))
        else:
            pts.append((x0 + half_root, -h_f))
        return pts

    # Enough teeth to cover the length, then clip to it.
    n_side = int(math.ceil(length / 2.0 / p)) + 1
    top = []
    for k in range(-n_side, n_side + 1):
        top.extend(tooth(k * p))
    xl, xr = -length / 2.0, length / 2.0

    def clip(poly, keep_right_of, x_cut):
        out = []
        n = len(poly)
        for i in range(n):
            a, b = poly[i], poly[(i + 1) % n]
            ina = (a[0] >= x_cut) if keep_right_of else (a[0] <= x_cut)
            inb = (b[0] >= x_cut) if keep_right_of else (b[0] <= x_cut)
            if ina:
                out.append(a)
            if ina != inb:
                t = (x_cut - a[0]) / (b[0] - a[0])
                out.append((x_cut, a[1] + t * (b[1] - a[1])))
        return out

    poly = [(top[0][0], -height)] + top + [(top[-1][0], -height)]
    poly = clip(clip(poly, True, xl), False, xr)
    # Drop consecutive duplicates the clip can leave.
    cleaned = []
    for q in poly:
        if not cleaned or math.hypot(q[0] - cleaned[-1][0], q[1] - cleaned[-1][1]) > 1e-9:
            cleaned.append(q)
    if math.hypot(cleaned[0][0] - cleaned[-1][0], cleaned[0][1] - cleaned[-1][1]) <= 1e-9:
        cleaned.pop()
    s = sketch(XY, name=_sub(name, "_profile")).polygon(cleaned)
    return extrude(s, width, name=name)


def bevel(
    module,
    teeth,
    mate_teeth,
    *,
    face_width=None,
    shaft_angle=90.0,
    bore=0.0,
    pressure_angle=20.0,
    backlash=0.0,
    clearance=0.25,
    fillet=0.38,
    tolerance=0.02,
    name=None,
):
    """A straight bevel gear for a mate of `mate_teeth` at `shaft_angle`
    (Tredgold's back-cone approximation, see the module doc): pitch-cone apex
    at the origin, axis +z, the heel (large end, module `module`) at
    `z = r / tan(delta)` and the toe `face_width` closer to the apex along
    the pitch cone. `face_width` defaults to a third of the outer cone
    distance. The mate, built the same way with the counts swapped, meshes
    after `rotate(shaft_angle, axis=(0, 1, 0))` about the origin and a turn
    of `mesh_turn(mate_teeth)` about its own axis. What it is not: the
    spherical involute -- the flank is right at the heel and drifts by a
    few microns per millimetre toward the toe; and the back is a flat face at
    the heel, not a back cone.

    Names: `<name>` (a ruled loft), `<name>_heel`, `<name>_toe`; faces
    `<name>/side[0.*]` (see `layout`, computed for the virtual tooth count
    rounded to an int is *not* what is drawn -- use `layout(module, teeth,
    ...)`'s `parts` with this gear's own `segments` from the returned body's
    face count) and `<name>/side[1.*]` for the bore."""
    if type(teeth) != "int" or type(mate_teeth) != "int" or teeth < 3 or mate_teeth < 3:
        _fail("bevel", "teeth and mate_teeth must be ints of at least 3")
    if shaft_angle <= 0 or shaft_angle >= 180:
        _fail("bevel", "shaft_angle must be between 0 and 180 degrees (got %s)" % shaft_angle)
    delta = math.atan2(math.sin(shaft_angle), mate_teeth / teeth + math.cos(shaft_angle))  # pitch cone half-angle
    if delta <= 0 or delta >= 90:
        _fail("bevel", "no pitch cone for %d teeth against %d at %s degrees" % (teeth, mate_teeth, shaft_angle))
    cos_d, sin_d = math.cos(delta), math.sin(delta)
    r = module * teeth / 2.0
    cone = r / sin_d  # outer cone distance
    if face_width == None:
        face_width = cone / 3.0
    if face_width <= 0 or face_width >= cone:
        _fail("bevel", "face_width %s must be between 0 and the cone distance %s" % (face_width, math.round(cone, 3)))
    z_v = teeth / cos_d  # Tredgold's virtual spur gear
    _check_common("bevel", module, teeth, pressure_angle, 0.0, backlash, clearance, fillet, 0.0, tolerance)
    # The virtual gear may have a non-integer tooth count: the half profile
    # only needs it as a number.
    half = _external_half("bevel", module, z_v, pressure_angle, 0.0, backlash, clearance, fillet, 0.0, tolerance)
    seq, _ = _tooth_sequence(half, z_v, tolerance, False)
    scale = (cone - face_width) / cone
    z_heel = r / math.tan(delta)
    heel_pts = _outline(seq, teeth, 0.0, r_scale=cos_d, a_scale=1.0 / cos_d)
    toe_pts = [(x * scale, y * scale) for x, y in heel_pts]
    r_root_toe = half["r_root"] * cos_d * scale
    if bore < 0 or bore / 2.0 >= r_root_toe:
        _fail("bevel", "bore %s does not fit inside the toe's root circle (diameter %s)" % (bore, math.round(2 * r_root_toe, 3)))
    heel = _section(z_heel, heel_pts, bore / 2.0, _sub(name, "_heel"))
    toe = _section(z_heel * scale, toe_pts, bore / 2.0, _sub(name, "_toe"))
    return loft([toe, heel], ruled=True, name=name)


# ----------------------------------------------------------------- a planetary set


def _pfx(name, s):
    return s if name == None else name + "_" + s


def planetary_set(
    module,
    teeth_sun,
    teeth_planet,
    width,
    *,
    planets=3,
    helix_angle=30.0,
    pressure_angle=20.0,
    gap=0.35,
    layer=0.2,
    layers_up=2,
    bore_clearance=0.35,
    pin=8.0,
    plate=3.0,
    rim=4.0,
    rim_gap=0.5,
    socket=3.5,
    foot=0.3,
    pin_overlap=0.5,
    head_flare=1.0,
    head_rise=1.2,
    head_height=1.5,
    ring_trim=0.02,
    tolerance=0.01,
    sun_boss=None,
    ring_extra=None,
    ring_flutes=0,
    ring_flute_depth=0.8,
    ring_flute_radius=None,
    plate_extra=None,
    carrier_extra=None,
    ring_lift=0.0,
    name=None,
):
    """A print-in-place herringbone planetary: the sun, `planets` planets on
    headed carrier pins, the ring and the carrier, placed and phased so that
    every mesh runs at the horizontal gap `gap` and every vertical gap is
    `layers_up` whole layers (`examples/planetary.star`
    is the worked, measured example and `examples/gear_toy.star` builds a toy
    on it). Nothing here is assumed: the caller measures the set with
    `distance()` and `clearance_map()`, which is what the examples' tests do.

    The rules it builds by:

    * every external gear is thinned at its pitch circle by
      `b = gap / (cos alpha_t cos beta_b)`, so the flanks of a centred mesh
      stand `gap` apart along their common normal; the ring is *shifted*
      instead, by `x_r = b / (2 m_t tan alpha_n)`, which widens its spaces by
      the same `b` and lifts its tip clear of the planets' root fillets, and
      thinned by `ring_trim` on top (the loft's interpolation between its
      sections stands the internal mesh 0.009 mm closer than the external);
    * the root clearance coefficient is raised to `gap / module` when that
      exceeds 0.25, so every tip stands `gap` from the root land it faces;
    * the sun and the planets float `layers_up * layer` over the carrier
      plate; each pin's head starts as many layers over its planet and flares
      by `head_flare` over `head_rise` (39.8 degrees, under the overhang rule);
    * the carrier plate is `r_tip(ring) - rim_gap` in radius, its first
      `layers_up` layers inset by `foot` against elephant's foot, with a
      hexagonal key socket of circumradius `socket` through it (0 for none);
      the pins reach `pin_overlap` into it and are united on the exact kernel;
    * every body carries `tessellation(tolerance=)`, so a `distance()` between
      two of them is measured within twice that figure;
    * the sun's boss is glued on before the sun is moved (a moved body loses
      its face names in a boolean that follows).

    Hooks, for a set that is part of something larger, each called with
    the arithmetic so far as a dict (`P`, `a`, `teeth_ring`, `m_t`,
    `alpha_t`, `beta_b`, `backlash`, `ring_shift`, `clearance`, `lay_sun`,
    `lay_planet`, `lay_ring`, `z_gears`, `ring_height`, `r_plate`, `r_pin`,
    `r_bore`, `r_boss`, `pin_height`): `sun_boss(z_top, info)` returns the
    body to glue on the sun's top face (at `z = width`, in the sun's own
    frame; the default is a hexagonal boss of half the root radius, 6 mm
    tall); `ring_extra(ring, info)` takes the ring gear before it is phased
    and returns the body to use instead (a threaded collar united with it,
    say); `plate_extra(plate, info)` the same for the carrier plate before
    its pins are united (the place to glue raised figures onto its still
    pristine top face); `carrier_extra(carrier, info)` the same for the
    carrier after its pins.
    `ring_flutes` > 0 grooves the ring's outside for a grip (`internal`'s
    `flutes`, `flute_depth`, `flute_radius`: chevrons, on this herringbone).
    `ring_lift` builds the ring gear that much shorter and lifts it by as
    much, so its apex stays on the planets' mid-plane while something else
    (a foot) stands on the bed under it.

    Each body is built once and becomes a `part`; what comes back is placed
    `instance`s, which every measurement, `print_check` and `export`
    take as bodies, and which the caller may `pose` (the planetary example
    turns the sun through a tooth pitch) or group in an `assembly` and
    `explode` for a picture. Returns a dict: the instances `sun`, `planets`
    (a list), `ring`, `carrier` and `instances` (carrier, ring, the planets,
    the sun -- the order an `assembly` wants); the part bodies `sun_body`,
    `planet_body`, `ring_body`, `carrier_body`; the arithmetic `P`
    (`planetary()`'s dict), `a` (the centre distance), `teeth_ring`, `m_t`,
    `alpha_t`, `beta_b`, `backlash`, `ring_shift`, `clearance`; the layouts
    `lay_sun`, `lay_planet`, `lay_ring`; and the placement `z_gears`,
    `ring_height` (the ring gear's own height, before `ring_lift`),
    `ring_phase`, `r_plate`, `r_pin`, `r_bore`, `r_boss`, `pin_height`,
    `pin_centres`.

    Names (with `name=` a prefix, `<name>_` before each): the sun's
    `sun_gear`, `boss_profile`, `boss`, `sun_body`, `sun_part`, `sun`; the
    planets' `planet`, `planet_part`, `planet_<k>`; the ring's `ring_gear`,
    `ring_part`, `ring`; the carrier's `plate_section_<z>`, `plate`,
    `pin_<k>_foot/neck/flare/head`, `pin_<k>`, `carrier_with_pin_<k>`,
    `carrier_body`, `carrier_part`, `carrier`. Faces keep the gears' names
    (`layout`, `tooth_faces`), the plate's `plate/start`, `plate/end`,
    `plate/side[...]` and the pins' `pin_<k>/side[...]`."""
    helix = helix_angle
    P = planetary(teeth_sun, teeth_planet, planets=planets, module=module, helix_angle=helix)
    teeth_ring = P["ring"]
    a = P["centre_distance"]
    m_t = module / math.cos(helix)
    alpha_t = math.atan(math.tan(pressure_angle) / math.cos(helix))
    beta_b = math.atan(math.tan(helix) * math.cos(alpha_t))
    normal_factor = math.cos(alpha_t) * math.cos(beta_b)
    backlash = gap / normal_factor
    ring_shift = backlash / (2 * m_t * math.tan(pressure_angle))
    clearance = max(0.25, gap / module)
    limit = min_teeth(pressure_angle=pressure_angle, helix_angle=helix)
    if limit > min(teeth_sun, teeth_planet):
        _fail("planetary_set", "a gear under %s teeth is undercut at a %s degree helix, and a ring's tip corner passes an undercut neck closer than the flanks stand" % (math.round(limit, 1), helix))
    if ring_shift >= 0.9:
        _fail("planetary_set", "the ring's profile shift %s is outside what the library accepts" % ring_shift)
    z_gears = plate + layers_up * layer
    ring_height = 2 * (z_gears - ring_lift) + width
    r_ring_pitch = m_t * teeth_ring / 2
    lay_sun = layout(module, teeth_sun, pressure_angle=pressure_angle, backlash=backlash, clearance=clearance, helix_angle=helix)
    lay_planet = layout(module, teeth_planet, pressure_angle=pressure_angle, backlash=backlash, clearance=clearance, helix_angle=helix)
    lay_ring = layout(module, teeth_ring, pressure_angle=pressure_angle, backlash=ring_trim, clearance=clearance, helix_angle=helix, profile_shift=ring_shift, internal=True)
    r_pin = pin / 2.0
    r_bore = r_pin + bore_clearance
    if r_bore + 2.0 >= lay_planet["r_root"]:
        _fail("planetary_set", "the planet's bore (radius %s) leaves under 2 mm of web inside its root circle (radius %s)" % (math.round(r_bore, 3), math.round(lay_planet["r_root"], 3)))
    r_plate = lay_ring["r_tip"] - rim_gap
    r_boss = 0.5 * lay_sun["r_root"]
    pin_height = width + 2 * layers_up * layer
    info = {
        "P": P,
        "a": a,
        "teeth_ring": teeth_ring,
        "m_t": m_t,
        "alpha_t": alpha_t,
        "beta_b": beta_b,
        "backlash": backlash,
        "ring_shift": ring_shift,
        "clearance": clearance,
        "lay_sun": lay_sun,
        "lay_planet": lay_planet,
        "lay_ring": lay_ring,
        "z_gears": z_gears,
        "ring_height": ring_height,
        "r_plate": r_plate,
        "r_pin": r_pin,
        "r_bore": r_bore,
        "r_boss": r_boss,
        "pin_height": pin_height,
    }

    # The sun, glued to its boss before it becomes a part.
    sun_gear = herringbone(module, teeth_sun, width, helix, hand="right", pressure_angle=pressure_angle, backlash=backlash, clearance=clearance, name=_pfx(name, "sun_gear"))
    if sun_boss == None:
        boss = extrude(sketch(XY.offset(width), name=_pfx(name, "boss_profile")).ngon(6, r_boss), 6.0, name=_pfx(name, "boss"))
    else:
        boss = sun_boss(width, info)
    sun_body = glue(sun_gear, boss, name=_pfx(name, "sun_body"))
    sun_body.tessellation(tolerance=tolerance)
    sun = instance(part(sun_body, name=_pfx(name, "sun_part")), at=(0, 0, z_gears), name=_pfx(name, "sun"))

    # The planets: one part, `planets` instances, each turned to the phase
    # planetary() gives about its own axis and then moved onto its pin.
    planet_body = herringbone(module, teeth_planet, width, helix, hand="left", pressure_angle=pressure_angle, backlash=backlash, clearance=clearance, bore=2 * r_bore, name=_pfx(name, "planet"))
    planet_body.tessellation(tolerance=tolerance)
    planet_part = part(planet_body, name=_pfx(name, "planet_part"))
    planet_instances = []
    pin_centres = []
    for k in range(planets):
        cx, cy = _polar(a, P["planet_angles"][k])
        planet_instances.append(instance(planet_part, at=(cx, cy, z_gears), rotate=P["planet_turns"][k], name=_pfx(name, "planet_%d" % k)))
        pin_centres.append((cx, cy))

    # The ring, turned back by what a left-hand ring has turned by the height
    # where the gears start, so its section there is planetary()'s outline.
    ring_body = internal(module, teeth_ring, ring_height, rim, helix_angle=helix, hand="left", herringbone=True, pressure_angle=pressure_angle, backlash=ring_trim, clearance=clearance, profile_shift=ring_shift, flutes=ring_flutes, flute_depth=ring_flute_depth, flute_radius=ring_flute_radius, name=_pfx(name, "ring_gear"))
    ring_body.tessellation(tolerance=tolerance)
    if ring_extra != None:
        ring_body = ring_extra(ring_body, info)
        if ring_body.kind == "brep":  # a mesh body's triangles are its operands' own
            ring_body.tessellation(tolerance=tolerance)
    ring_phase = P["ring_turn"] + math.degrees((z_gears - ring_lift) * math.tan(helix) / r_ring_pitch)
    ring_part = part(ring_body, name=_pfx(name, "ring_part"))
    if ring_lift != 0.0:
        ring = instance(ring_part, at=(0, 0, ring_lift), rotate=ring_phase, name=_pfx(name, "ring"))
    else:
        ring = instance(ring_part, rotate=ring_phase, name=_pfx(name, "ring"))

    # The carrier: the plate with its inset foot and socket, and a pin per
    # planet reaching into it, united exactly.
    def plate_section(z, r):
        s = sketch(XY.offset(z), name=_pfx(name, "plate_section_%s" % math.round(z, 2))).circle(r)
        if socket > 0:
            s = s.ngon(6, socket)
        return s

    plate_body = loft([plate_section(0.0, r_plate - foot), plate_section(layers_up * layer, r_plate), plate_section(plate, r_plate)], ruled=True, name=_pfx(name, "plate"))
    if plate_extra != None:
        plate_body = plate_extra(plate_body, info)
    pins = []
    for k in range(planets):
        cx, cy = pin_centres[k]
        z0 = plate - pin_overlap
        z1 = plate + pin_height
        sections = [
            sketch(XY.offset(z0), name=_pfx(name, "pin_%d_foot" % k)).circle(r_pin, center=(cx, cy)),
            sketch(XY.offset(z1), name=_pfx(name, "pin_%d_neck" % k)).circle(r_pin, center=(cx, cy)),
            sketch(XY.offset(z1 + head_rise), name=_pfx(name, "pin_%d_flare" % k)).circle(r_pin + head_flare, center=(cx, cy)),
            sketch(XY.offset(z1 + head_rise + head_height), name=_pfx(name, "pin_%d_head" % k)).circle(r_pin + head_flare, center=(cx, cy)),
        ]
        pins.append(loft(sections, ruled=True, name=_pfx(name, "pin_%d" % k)))
    carrier_body = plate_body
    for k in range(planets):
        carrier_body = union(carrier_body, pins[k], kernel="brep", name=_pfx(name, "carrier_body") if k == planets - 1 else _pfx(name, "carrier_with_pin_%d" % k))
    if carrier_extra != None:
        carrier_body = carrier_extra(carrier_body, info)
    if carrier_body.kind == "brep":
        carrier_body.tessellation(tolerance=tolerance)
    carrier = instance(part(carrier_body, name=_pfx(name, "carrier_part")), name=_pfx(name, "carrier"))

    out = dict(info)
    out["sun"] = sun
    out["planets"] = planet_instances
    out["ring"] = ring
    out["carrier"] = carrier
    out["instances"] = [carrier, ring] + planet_instances + [sun]
    out["sun_body"] = sun_body
    out["planet_body"] = planet_body
    out["ring_body"] = ring_body
    out["carrier_body"] = carrier_body
    out["ring_phase"] = ring_phase
    out["pin_centres"] = pin_centres
    return out
