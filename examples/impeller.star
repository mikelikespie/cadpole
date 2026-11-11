# A closed centrifugal pump impeller: curved shroud, twisted vanes, hub nose,
# wear-ring lip, balance holes, and the shaft and cap nut it runs on.
#
# What this example demonstrates:
#
#   * a vane *lofted* between two different outlines -- the one it has where
#     it meets the hub and the one it has where it meets the shroud -- so the
#     blade twists, its leading edge leans into the eye, and its angle is
#     steeper at the hub than at the shroud, which is what a real impeller
#     vane does and what a flat extrude cannot;
#   * flanks drawn with `path().spline()`, so each flank is a run of exact
#     cubic faces that meet tangent, not a polyline of flat facets;
#   * the meridional shape (shroud and hub dome) as ruled lofts through many
#     circles along a quarter-ellipse: a cone per band, and the bands shade
#     as one surface;
#   * a thin curved shell (the shroud) as one loft through *annular* sections
#     -- a circle with a concentric hole -- whose inner and outer radii follow
#     the passage curve and its offset along the normal, `shroud_t` away. No
#     boolean: the shell is a closed solid the moment it is lofted;
#   * `pattern_circular(merge=True)`: seven vanes and seven balance holes in
#     one boolean each;
#   * an assembly of four bodies that keep their own materials, so the shroud
#     can be glass and show the vanes turning under it.
#
# What a real one has that this does not: fillets where the vanes meet the
# hub and shroud (castings have them), and a smooth meridional profile
# rather than one of conical bands.
#
# A blade angle is measured from the *tangent*, so a small angle is a long
# sweep. The vane centreline is a logarithmic spiral because that is the
# curve with a constant blade angle: r = r0 * exp(theta * tan(beta)), which
# reaches the tip after a wrap of ln(r_tip / r0) / tan(beta) radians. Hub and
# shroud outlines share that wrap, so the trailing edge is straight and axial
# and the hub, starting further in, gets the steeper angle.
#
#   cadpole render examples/impeller.star --backend path --samples 256 \
#       --size 1600x1200 --view product --lighting product --backdrop studio \
#       --floor gloss --tonemap neutral --edge-break 0.25 --denoise \
#       -o /tmp/impeller.png                          # docs/demo/impeller.png
#   cadpole render examples/impeller.star --param glass_shroud=false ...
#   cadpole export examples/impeller.star --format step -o /tmp/impeller.step

blades = param("blades", 7, min=3, max=16, doc="Number of vanes; odd counts avoid a resonance with the volute tongue")
r_eye = param("eye_radius", 26.0, unit="mm", min=8, max=80,
              doc="Inlet (eye) radius: where the shroud turns axial and the vane starts at the shroud")
r_tip = param("tip_radius", 60.0, unit="mm", min=20, max=200, doc="Outlet radius: the impeller's overall size")
beta = param("blade_angle", 24.0, unit="deg", min=10, max=60,
             doc="Vane angle from the tangent at the shroud streamline; smaller sweeps the vane further round")
vane_t = param("vane_thickness", 3.0, unit="mm", min=1, max=8)
b_out = param("outlet_width", 9.0, unit="mm", min=2, max=40, doc="Passage height at the tip")
z_eye = param("inlet_height", 26.0, unit="mm", min=6, max=80, doc="Height at which the shroud turns axial into the eye")
lip = param("wear_ring", 8.0, unit="mm", min=0, max=30, doc="Axial length of the shroud's eye lip, where the wear ring runs")
back_t = param("backplate", 6.0, unit="mm", min=2, max=20)
shroud_t = param("shroud", 3.5, unit="mm", min=1.5, max=10)
hub_r = param("hub_radius", 11.0, unit="mm", min=5, max=40, doc="Radius of the hub nose, where the dome ends")
dome_r = param("dome_radius", 32.0, unit="mm", min=8, max=150, doc="Radius at which the hub dome rises off the backplate")
nose_h = param("nose_height", 24.0, unit="mm", min=4, max=80, doc="Height of the hub nose above the backplate")
bore = param("bore", 12.0, unit="mm", min=5, max=30, doc="Shaft diameter (wider than the keyway)")
key = param("keyway", 4.0, unit="mm", min=0, max=10, doc="Keyway width; 0 for a plain bore")
holes = param("balance_holes", 4.0, unit="mm", min=0, max=12, doc="Balance hole diameter; 0 for none")
shaft_len = param("shaft_length", 70.0, unit="mm", min=0, max=300, doc="Shaft beyond the rear hub; 0 leaves shaft and nut out")
glass = param("glass_shroud", True, doc="Render the shroud as glass so the vanes show; off for the cast part")
sections = param("profile_steps", 12, min=4, max=40, doc="Conical bands per meridional curve")
steps = param("vane_steps", 12, min=4, max=40, doc="Spline points along each vane flank")

assert_true(math.close(math.atan(1.0), 45.0), msg="math.atan answers in degrees")
assert_true(r_eye > hub_r + 4.0, msg="the eye has to clear the hub nose by a few mm, or there is no inlet")
assert_true(r_eye < dome_r, msg="the hub dome must leave the backplate outside the eye")
assert_true(dome_r < r_tip - 4.0, msg="the dome has to start well inside the tip")
assert_true(nose_h + 8.0 <= z_eye + lip or shaft_len <= 0,
            msg="the cap nut should sit inside the eye, below the shroud lip")


def polar(r, a):
    return (r * math.cos(a), r * math.sin(a))


# ---------------------------------------------------------- meridional curves
#
# Both the shroud and the hub dome are, in the (r, z) half-plane, a quarter of
# an ellipse: radial where they leave the flat part and axial where they meet
# the eye or the nose, bulging towards the corner between. The ellipse is
# entered a few degrees in from its radial end, so the curve has a small but
# finite slope at that end: `r` is then a proper function of `z`, which a loft
# of circles needs, and the curve can be extended below its start along its
# own tangent, which the trimming solids need.
PHI0 = 6.0


def quarter(r_start, z_start, r_end, z_end, n):
    """Points (r, z, nr, nz) from the radial end to the axial end, with the
    unit normal that points away from the corner (up at the start, outward at
    the end). `z` strictly increases along the list."""
    s0 = math.sin(PHI0)
    c0 = math.cos(PHI0)
    a = (r_start - r_end) / (1.0 - s0)
    b = (z_end - z_start) / c0
    rc = r_end + a
    zc = z_end
    pts = []
    for k in range(n + 1):
        phi = PHI0 + (90.0 - PHI0) * k / n
        r = rc - a * math.sin(phi)
        z = zc - b * math.cos(phi)
        nr = b * math.sin(phi)
        nz = a * math.cos(phi)
        m = math.hypot(nr, nz)
        pts.append((r, z, nr / m, nz / m))
    # The slope at the radial end, as dr per unit of z, for extending the curve.
    slope = (a * c0) / (b * s0)
    return pts, slope


def revolved(profile, name):
    """A solid of revolution as a ruled loft through one circle per (z, r)."""
    for i in range(1, len(profile)):
        assert_true(profile[i][0] > profile[i - 1][0] + 1e-6,
                    msg="loft sections must climb: %s then %s in %s" % (profile[i - 1], profile[i], name))
    return loft([sketch(XY.offset(z)).circle(r) for (z, r) in profile], ruled=True, name=name)


# The shroud's inner (passage) surface, from the tip to the eye.
shroud_in, shroud_slope = quarter(r_tip, b_out, r_eye, z_eye, sections)
assert_true(shroud_t * (z_eye - b_out) < (r_tip - r_eye) * (r_tip - r_eye),
            msg="shroud too thick for its curvature: the outer surface would fold back on itself")

# The shroud is one loft through annular sections: at each height the ring
# between the passage curve (the hole) and its offset by `shroud_t` along the
# normal (the outside). The offset curve's radius is read at the inner
# curve's heights by interpolation, and the first ring sits a hair above the
# tip -- at the tip itself the two curves meet at the rim edge and the ring
# would have no width. Above the turn both radii are constant: the lip.
#
# The vanes are trimmed by a second solid of revolution, TRIM: everything
# under the passage curve moved 0.3 mm along its inward normal, so the vane
# tops stop just short of the shroud and two bodies never share a face. Along
# the *normal*, not down in z: where the shroud turns axial a drop in z moves
# the surface by nothing radially, and the vane tops -- two meshes tessellated
# independently, each a few hundredths off its true surface -- would poke
# through the glass. It stands on a cylinder a hair inside the rim, because
# a vane's trailing corners (its flanks are offset square to the spiral, not
# radially) would otherwise stand proud of the backplate.
outer_curve = [(z + shroud_t * nz, r + shroud_t * nr) for (r, z, nr, nz) in shroud_in]


def outer_r_at(z):
    """Radius of the offset curve at height `z`, by linear interpolation."""
    if z >= outer_curve[-1][0]:
        return r_eye + shroud_t
    for i in range(1, len(outer_curve)):
        (z0, r0), (z1, r1) = outer_curve[i - 1], outer_curve[i]
        if z <= z1:
            return r0 + (r1 - r0) * (z - z0) / (z1 - z0) if z >= z0 else r_tip
    return r_eye + shroud_t


rings = [(b_out + 0.15, r_tip, shroud_in[0][0] - 0.1)]
for (r, z, _, _) in shroud_in[1:]:
    rings.append((z, outer_r_at(z), r))
rings.append((z_eye + lip, r_eye + shroud_t, r_eye))
for i in range(1, len(rings)):
    assert_true(rings[i][0] > rings[i - 1][0] + 1e-6, msg="shroud rings must climb")
    assert_true(rings[i][1] > rings[i][2] + 0.2, msg="every shroud ring has width: %s" % (rings[i],))
shroud = loft([sketch(XY.offset(z)).circle(ro).circle(ri) for (z, ro, ri) in rings], ruled=True, name="shroud")

GAP = 0.3
trim_profile = [(-2.0, r_tip - 0.3), (b_out - GAP, r_tip - 0.3)]
trim_profile += [(z - GAP * nz, r - GAP * nr) for (r, z, nr, nz) in shroud_in[1:]]
trim_profile += [(z_eye + lip + 0.7, r_eye - GAP)]

# ---------------------------------------------------------------- the vanes
#
# One outline per streamline. Both run from their own start radius to the
# tip through the same wrap, so the corresponding points of the two sections
# sit at the same angle and the loft's ruling lines are radial-axial: the
# trailing edge is straight, the leading edge leans from the hub out to the
# shroud, and the blade angle is steeper at the hub, exactly as designed.
r_end = r_tip - 0.5      # the trailing edge sits a hair inside the rim
r0_shroud = r_eye + 4.0  # where the vane meets the shroud: out where the shroud still slopes.
                         # Started at the turn, the vane's top corner would stand up under
                         # the near-vertical shroud as a tall thin sail
r0_hub = hub_r + 1.0     # where it meets the hub, buried in the dome
wrap = math.degrees(math.log(r_end / r0_shroud) / math.tan(beta))
beta_hub = math.atan(math.log(r_end / r0_hub) / math.radians(wrap))
assert_true(wrap > 20.0, msg="a vane that barely wraps is a paddle, not an impeller: lower blade_angle")
assert_true(beta_hub < 70.0, msg="the hub streamline's blade angle is %s deg; move the hub out or the eye in" % int(beta_hub))

Z_BOTTOM = -1.0          # vane sections start inside the backplate...
Z_TOP = z_eye + 0.5      # ...and end above the shroud, which trims them


def centreline(r0):
    pts = []
    for i in range(steps + 1):
        t = i / steps
        pts.append(polar(r0 * math.exp(t * math.log(r_end / r0)), -wrap * (1.0 - t)))
    return pts


def vane_outline(r0):
    """A closed path: square trailing edge, spline flanks, round leading edge."""
    c = centreline(r0)
    half = vane_t / 2.0
    left = []
    right = []
    for i in range(len(c)):
        j0 = max(0, i - 1)
        j1 = min(len(c) - 1, i + 1)
        dx = c[j1][0] - c[j0][0]
        dy = c[j1][1] - c[j0][1]
        n = math.hypot(dx, dy)
        assert_true(n > 1e-9, msg="two vane points coincide; raise vane_steps")
        left.append((c[i][0] - dy / n * half, c[i][1] + dx / n * half))
        right.append((c[i][0] + dy / n * half, c[i][1] - dx / n * half))
        if i == 0:
            nose = (c[0][0] - dx / n * half, c[0][1] - dy / n * half)
    last = len(c) - 1
    p = path(left[last]).line(right[last])
    p = p.spline([right[i] for i in range(last - 1, -1, -1)])
    p = p.arc(left[0], via=nose)
    p = p.spline([left[i] for i in range(1, last + 1)])
    return p


vane = loft([
    sketch(XY.offset(Z_BOTTOM)).add(vane_outline(r0_hub)),
    sketch(XY.offset(Z_TOP)).add(vane_outline(r0_shroud)),
], ruled=True, name="vane")
ring = pattern_circular(vane, blades, angle=360.0, merge=True, name="vanes")
vanes = intersect(ring, revolved(trim_profile, "passage"), name="vanes_trimmed")

# ------------------------------------------------------ backplate, hub, bore
#
# Everything that meets the backplate starts *inside* it by a millimetre.
# Parts that merely touch on a shared plane are the tangent case: the exact
# kernel refuses them and the mesh kernel fuses them into a body with a wall
# of doubled faces.
back = extrude(sketch(XY.offset(-back_t)).circle(r_tip), back_t, name="backplate")
# The dome starts a millimetre down inside the backplate and *crosses* its
# top at `dome_r`, at a real angle. Started on the surface it would leave it
# tangentially, and a curve leaving a plane tangentially is a touch, not a
# crossing: the tangent case again.
phi_plate = math.acos(nose_h * math.cos(PHI0) / (nose_h + 1.0))
dome_start = hub_r + (dome_r - hub_r) / (1.0 - math.sin(phi_plate)) * (1.0 - math.sin(PHI0))
dome_curve, _ = quarter(dome_start, -1.0, hub_r, nose_h, sections)
# No section within half a millimetre of the backplate's top: the union cuts
# the dome along that plane, and a cut that lands a tenth of a millimetre from
# a band's edge leaves a sliver face the tessellator cannot close.
dome_profile = [dome_curve[0]] + [p for p in dome_curve[1:] if p[1] > 0.5]
dome = revolved([(z, r) for (r, z, _, _) in dome_profile], "hub_dome")
rear_boss = extrude(sketch(XY.offset(-back_t - 8.0)).circle(hub_r + 2.0), 9.0, name="rear_hub")
impeller = union([back, dome, rear_boss, vanes], name="wheel")


assert_true(key < bore, msg="the keyway is as wide as the bore or wider: make keyway smaller than bore, or 0 for a plain bore")


def bore_outline(r):
    if key <= 0:
        return None
    a0 = math.asin(key / 2.0 / r)
    n = 48
    pts = [polar(r, a0 + (360.0 - 2 * a0) * i / n) for i in range(n + 1)]
    pts.append((r + key / 2.0, -key / 2.0))
    pts.append((r + key / 2.0, key / 2.0))
    return pts


bore_sketch = sketch(XY.offset(-back_t - 10.0))
loop = bore_outline(bore / 2.0)
bore_sketch = bore_sketch.polygon(loop) if loop else bore_sketch.circle(bore / 2.0)
cuts = [extrude(bore_sketch, back_t + 10.0 + nose_h + 2.0, name="bore_cut")]


# Balance holes let the pressure behind the backplate bleed into the eye, so
# the wheel is not pushed into its housing. They go through backplate and dome
# just outside the rear hub, in the gap between two vanes -- and because the
# vane twists, "between" has to be checked over the hole's whole depth: the
# vane at that radius moves several degrees between the backplate and where
# the dome surface is.
def vane_angle(r, z):
    """Angle of the vane centreline at (r, z), or None if no vane is there."""
    f = (z - Z_BOTTOM) / (Z_TOP - Z_BOTTOM)
    lo = 0.0
    hi = 1.0

    def r_at(t):
        return (1.0 - f) * r0_hub * math.exp(t * math.log(r_end / r0_hub)) + f * r0_shroud * math.exp(t * math.log(r_end / r0_shroud))

    if r < r_at(0.0) or r > r_at(1.0):
        return None
    for _ in range(40):
        mid = (lo + hi) / 2.0
        if r_at(mid) < r:
            lo = mid
        else:
            hi = mid
    return -wrap * (1.0 - (lo + hi) / 2.0)


if holes > 0:
    r_hole = hub_r + 2.0 + holes / 2.0 + 1.5
    z_dome = None
    for (r, z, _, _) in dome_curve:
        if z_dome == None and r <= r_hole:
            z_dome = z
    z_dome = z_dome if z_dome != None else nose_h
    seen = [a for a in [vane_angle(r_hole, z_dome * k / 5.0) for k in range(6)] if a != None]
    lo_a = min(seen) if seen else 0.0
    hi_a = max(seen) if seen else 0.0
    pitch = 360.0 / blades
    centre = (lo_a + hi_a) / 2.0 + pitch / 2.0
    clearance = pitch / 2.0 - (hi_a - lo_a) / 2.0 - math.degrees((vane_t / 2.0 + holes / 2.0) / r_hole)
    assert_true(clearance > 1.0,
                msg="a %s mm balance hole at r=%s would cut a vane (%s deg short); make it smaller" % (holes, r_hole, int(-clearance)))
    hole = extrude(sketch(XY.offset(-back_t - 1.0)).circle(holes / 2.0, center=polar(r_hole, centre)),
                   back_t + 1.0 + z_dome + 2.0, name="balance_hole")
    cuts.append(pattern_circular(hole, blades, angle=360.0, merge=True, name="balance_holes"))

impeller = subtract(impeller, union(cuts, name="cuts") if len(cuts) > 1 else cuts[0], name="impeller")

# ------------------------------------------------------------ shaft and nut
bodies = [impeller, shroud]
if shaft_len > 0:
    shaft = extrude(sketch(XY.offset(-back_t - 8.0 - shaft_len)).circle(bore / 2.0 - 0.05),
                    shaft_len + 8.0 + back_t + nose_h + 1.0, name="shaft")
    across_flats = 2.0 * hub_r - 3.0
    hex_body = extrude(sketch(XY.offset(nose_h)).polygon([polar(across_flats / 2.0 / math.cos(30.0), 30.0 + 60.0 * i) for i in range(6)]),
                  8.0, name="nut_hex")
    # A chamfer on the top corners: the hex intersected with a cylinder that
    # tapers over its last third.
    r_c = across_flats / 2.0 / math.cos(30.0)
    chamfer = loft([
        sketch(XY.offset(nose_h - 1.0)).circle(r_c + 0.5),
        sketch(XY.offset(nose_h + 5.5)).circle(r_c + 0.5),
        sketch(XY.offset(nose_h + 8.1)).circle(across_flats / 2.0 - 0.3),
    ], ruled=True, name="nut_chamfer")
    nut = intersect(hex_body, chamfer, name="nut")
    bodies += [shaft, nut]

# ------------------------------------------------------------- materials
#
# For a metal, `color` is *reflectance*, not a diffuse tint: polished aluminium
# returns about 0.9 of what hits it, so the hex has to be near white. A mid
# grey like #8d8f92 decodes (correctly, as sRGB) to 0.28 linear and renders a
# wheel that looks cast in graphite.
aluminium = dict(color="#eceef1", roughness=0.42, metallic=1.0)
# The wheel is billet: turned on the backplate, dome and bore, flank-milled
# along the rulings on the vanes (`texture="machined"` picks per face).
impeller.material(texture="machined", **aluminium)
if glass:
    shroud.material(color="#9fd4d8", roughness=0.14, opacity=0.10, ior=1.52, attenuation=140.0)
else:
    shroud.material(**aluminium)
if shaft_len > 0:
    shaft.material(color="#c9ccd2", roughness=0.18, metallic=1.0)
    nut.material(color="#b7bac0", roughness=0.32, metallic=1.0)

for b in bodies:
    assert_solid(b)

lo, hi = impeller.bbox()
assert_true(math.close(hi[0], r_tip, tol=0.5), msg="the wheel reaches the tip radius")
assert_true(math.close(hi[2], nose_h, tol=0.05),
            msg="the hub nose is the highest point of the wheel: the vanes meet the shroud below its turn, top is %s" % hi[2])
assert_true(math.close(lo[2], -back_t - 8.0, tol=0.05), msg="the rear hub is the lowest")
# The wheel keeps its faces through four mesh booleans: a union that lost
# face provenance would come back with one face per triangle.
assert_true(len(impeller.faces()) < 2000, msg="the wheel has a few hundred faces, not one per triangle")


# The smallest bore still takes the default keyway.
def the_impeller_is_solid():
    assert_solid(impeller)


test("the smallest bore takes the keyway", the_impeller_is_solid, sweep={"bore": [5]})
