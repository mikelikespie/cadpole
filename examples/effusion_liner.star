# The combustor liner of a small turbojet, pierced by some six hundred
# effusion holes normal to its conical wall (`pattern_on_surface`).
#
# A can liner is a thin sheet-metal shell: a cylindrical primary zone, a
# conical transition and a short cylindrical exit, here one `revolve` of an
# annular profile. Effusion cooling drills the wall with hundreds of small
# holes so a film of compressor air bleeds through and lies along the hot
# side; every hole is drilled *normal to the surface it goes through*, and
# on a cone that is a different direction on every row.
#
# What this example demonstrates:
#
#   * `pattern_on_surface(hole, face, rows=, count=, merge=True)`: the hole
#     cutter is drawn once at the origin, straddling z = 0 with +z up, and the
#     pattern stands a copy on every point of a grid measured in millimetres
#     *on the cone* -- rows are circles about the axis, `count=` is the total
#     shared out among the rows in proportion to their length, so the pitch
#     is the same in mm on the wide rows and the narrow ones -- with each
#     copy's +z turned along the cone's outward normal. The rows sit
#     `margin` mm in from the cone's two ends;
#   * `merge=True` fuses the copies in one compose, so the liner
#     is pierced by one `subtract`, and the copies are one feature, `holes`;
#   * the outer cone is picked by name: a revolve names a face after the
#     profile segment it swept, `seg[0.1]`, and makes a full turn in three
#     pieces (`#0..#2`), which the pattern takes as one surface;
#   * an honest wall: the inner profile is the outer offset by `wall`
#     *perpendicular* to each segment (the corners are where the offset
#     lines meet), so a hole drilled normal to the cone is `wall` long through
#     the metal, and the volume test can say what 600 holes remove.
#
# The cut is the mesh kernel's (590 cylinders against a B-rep is routed
# there). At the defaults the liner is a clean solid. At some other hole
# counts the result has a few pairs of triangles crossing by a tenth of a
# micron along a cone chord, where a hole's rim falls on a long sliver
# triangle of the cone, and `assert_solid` reports it. The sweep below runs
# at counts that cut clean.
#
#   cadpole test examples/effusion_liner.star
#   cadpole inspect examples/effusion_liner.star
#   cadpole render examples/effusion_liner.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/effusion_liner.png

d_front = param("front_d", 60.0, unit="mm", min=30, max=120, doc="Outer diameter of the primary zone")
d_exit = param("exit_d", 40.0, unit="mm", min=20, max=100, doc="Outer diameter at the exit")
front_len = param("front_len", 20.0, unit="mm", min=5, max=60, doc="Length of the cylindrical primary zone")
cone_len = param("cone_len", 50.0, unit="mm", min=10, max=150, doc="Axial length of the conical section")
exit_len = param("exit_len", 15.0, unit="mm", min=3, max=60, doc="Length of the cylindrical exit")
wall = param("wall", 1.0, unit="mm", min=0.4, max=3, doc="Sheet thickness, measured through the surface")
rows = param("rows", 10, min=7, max=30, doc="Rows of effusion holes on the cone")
holes = param("holes", 590, min=1, max=1000, doc="Effusion holes in all, shared out among the rows by row length")
hole_d = param("hole_d", 0.6, unit="mm", min=0.2, max=1.3, doc="Effusion hole diameter")
margin = param("margin", 3.0, unit="mm", min=0, max=20, doc="Land left at each end of the cone, in mm along the slant")

assert_true(d_exit < d_front, msg="the liner narrows toward the exit")

r_front = d_front / 2.0
r_exit = d_exit / 2.0
z_cone = front_len
z_exit = front_len + cone_len
length = front_len + cone_len + exit_len

# --- the shell ---------------------------------------------------------------
# The outer profile is three segments, (r, z): the front cylinder, the cone,
# the exit cylinder. The inner profile is each segment moved `wall` along
# its inward normal, with the corners where neighbouring offset lines meet.
# On the cone the inward normal leans, so its horizontal offset is
# `wall / cos(lean)` and the corner with each cylinder sits a little short
# of the outer corner.
slant = math.sqrt(cone_len * cone_len + (r_front - r_exit) * (r_front - r_exit))
lean = math.atan((r_front - r_exit) / cone_len)      # the cone's angle from the axis
cone_dx = (r_exit - r_front) / slant                  # the cone's unit direction, (dr, dz)
cone_dz = cone_len / slant
in_x = -cone_dz                                       # its inward normal (toward the axis)
in_z = cone_dx
r_in_front = r_front - wall
r_in_exit = r_exit - wall


def on_inner_cone(x):
    """The z at which the inner cone's line reaches radius `x`."""
    x0 = r_front + wall * in_x
    z0 = z_cone + wall * in_z
    return z0 + (x - x0) / cone_dx * cone_dz


profile = sketch(XZ, name="profile").polygon([
    (r_front, 0),
    (r_front, z_cone),
    (r_exit, z_exit),
    (r_exit, length),
    (r_in_exit, length),
    (r_in_exit, on_inner_cone(r_in_exit)),
    (r_in_front, on_inner_cone(r_in_front)),
    (r_in_front, 0),
])
shell = revolve(profile, axis=(0, 0, 1), name="shell")
shell_cylinder_faces = shell.faces(kind=kind.cylinder).count()   # the two cylindrical sections, inside and out, in pieces

# --- the effusion holes ----------------------------------------------------
# The cutter stands at the origin, straddling z = 0 with +z up; the pattern
# puts its origin on the cone and turns its +z along the outward normal,
# so it pierces the wall from 2 mm outside to 2 mm inside.
hole_len = 4.0
# The holes must stand a diameter apart, along a row and from row to row:
# holes that overlap leave the one mesh subtract a non-manifold result, after
# minutes, and holes that nearly touch leave slivers that cross.
# The pitch along a row is the same on every row (the count is shared out by
# row length), the total row length over the count.
row_pitch = rows * math.pi * (r_front + r_exit) / holes
row_spacing = (slant - 2.0 * margin) / rows
assert_true(row_pitch > 2.0 * hole_d,
            msg="the holes along a row are %s mm apart, too close for %s mm holes: fewer holes, more rows or smaller holes" % (int(row_pitch * 100) / 100.0, hole_d))
assert_true(row_spacing > 2.0 * hole_d,
            msg="the rows are %s mm apart, too close for %s mm holes: fewer rows, a smaller margin or smaller holes" % (int(row_spacing * 100) / 100.0, hole_d))
hole = cylinder(d=hole_d, height=hole_len).translate((0, 0, -hole_len / 2.0), name="hole")
cone_outer = shell.faces(name="seg[0.1]*")
drills = pattern_on_surface(hole, cone_outer, rows=rows, count=holes, margin=margin, normal=True, merge=True, name="holes")
liner = subtract(shell, drills, name="liner")

liner.material(preset=preset.steel)
export(liner, "effusion_liner.stl")


# ------------------------------------------------------------------ tests


def shell_volume():
    """The revolved shell by Pappus: 2 pi times the profile's area times
    the radius of its centroid."""
    pts = [
        (r_front, 0), (r_front, z_cone), (r_exit, z_exit), (r_exit, length),
        (r_in_exit, length), (r_in_exit, on_inner_cone(r_in_exit)),
        (r_in_front, on_inner_cone(r_in_front)), (r_in_front, 0),
    ]
    area2 = 0.0
    cx6 = 0.0
    for i in range(len(pts)):
        x0, z0 = pts[i]
        x1, z1 = pts[(i + 1) % len(pts)]
        cross = x0 * z1 - x1 * z0
        area2 += cross
        cx6 += (x0 + x1) * cross
    area = abs(area2) / 2.0
    cx = cx6 / (3.0 * area2)
    return 2.0 * math.pi * cx * area


def the_holes_are_drilled_through_the_wall():
    """The liner is a solid whose volume is the shell's less what `holes`
    holes of `hole_d` take out of a wall `wall` thick: each is a cylinder
    `wall` long through the metal, because the inner cone is the outer's
    perpendicular offset and every hole is normal to it. The chords of a
    0.6 mm hole undercut its circle, so the removed volume is bracketed,
    not matched."""
    assert_solid(liner)
    v_shell = shell_volume()
    v_hole = math.pi * (hole_d / 2.0) * (hole_d / 2.0) * wall
    removed = v_shell - liner.volume()
    assert_true(removed > 0.7 * holes * v_hole, msg="the holes were drilled through the wall")
    assert_true(removed < 1.3 * holes * v_hole, msg="the holes take no more than their own volume")
    return removed


test("the effusion holes go through the wall, and the liner stays a solid", the_holes_are_drilled_through_the_wall, sweep={"holes": [300, 800]})


def every_hole_is_there():
    """Each hole's wall is the cutter's cylinder, which the mesh kernel keeps
    as the two halves truck made it in: the liner has `2 * holes` cylinder
    faces beyond the shell's own, and no more, so every copy the pattern
    laid out was kept (none fell to the margin) and each cut once."""
    cyl = liner.faces(kind=kind.cylinder).count()
    assert_eq(cyl, shell_cylinder_faces + 2 * holes)
    return cyl


test("every one of the holes is drilled once", every_hole_is_there)


def the_holes_stay_on_the_cone():
    """Nothing is drilled through the cylindrical sections or the ends: the
    bounding box is the shell's, and every hole's wall sits between the
    cone's two ends."""
    lo, hi = liner.bbox()
    assert_close(lo[2], 0, tol=1e-6)
    assert_close(hi[2], length, tol=1e-6)
    assert_close(hi[0], r_front, tol=1e-3)
    holes_box = liner.faces(kind=kind.cylinder, above=XY.offset(z_cone), below=XY.offset(z_exit)).count()
    assert_eq(holes_box, 2 * holes)
    return holes_box


test("the holes stay on the cone and the liner keeps its envelope", the_holes_stay_on_the_cone)


# The ends of the declared ranges build, and quickly: the fewest rows and the
# widest holes (each used to put holes on top of one another).
def the_liner_is_solid():
    assert_solid(liner)


test("the fewest rows", the_liner_is_solid, sweep={"rows": [7]})
test("the widest holes", the_liner_is_solid, sweep={"hole_d": [1.3]})
test("the most holes", the_liner_is_solid, sweep={"holes": [1000]})
