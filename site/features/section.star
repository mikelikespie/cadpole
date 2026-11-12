# Section: cut a body with a plane and keep one side.
# A V-belt pulley: a hub with a keyed bore and a set-screw hole, a thin web,
# and a rim with one groove. Everything worth seeing is inside it.
pulley = revolve(sketch(XZ).polygon([
    (6.6, 0), (10.4, 0), (11, 0.6), (11, 9.5),       # bore and hub, lower end
    (19, 9.5), (19, 3.6), (19.6, 3),                 # web, then the rim's inside
    (26.4, 3), (27, 3.6), (27, 7.5),                 # rim, lower lip
    (22.5, 10.5), (22.5, 13.5), (27, 16.5),          # the V groove
    (27, 20.4), (26.4, 21), (19.6, 21), (19, 20.4),  # rim, upper lip
    (19, 14.5), (11, 14.5), (11, 25.4), (10.4, 26),  # web, hub
    (6.6, 26), (6, 25.4), (6, 0.6)]),                # the bore, chamfered
    name="pulley")
pulley.tessellation(tolerance=0.01)
body = pulley
# snippet
cut = plane(origin=(0, 0, 0), normal=(0, -1, 0))
part = split(body, cut, keep="below", name="part")
# end snippet
# The keyway and the set-screw hole lie in the cut, so the section shows
# them in profile.
key = box(2.8, 4, 28, at=(6.4, 0, -1), center=True, name="key")
screw_hole = rotate(cylinder(1.65, 8), 90, axis=(0, 1, 0), name="screw_hole")
screw_hole = translate(screw_hole, (-13, 0, 20.5), name="screw_hole_placed")
part = subtract(part, [key, screw_hole], name="pulley_half")
part.material(color="#6fbfa8", roughness=0.4)
paint(part, faces=part.faces(name="part/cut*"), color="#f0c987")


def one_half_with_its_cut_face():
    assert_solid(part)
    assert_bbox(part, (-27, 0, 0), (27, 27, 26))
    cut_faces = part.faces(name="part/cut*")
    assert_true(len(cut_faces) >= 1, msg="the cut face survives the details")
    for f in cut_faces:
        assert_close(f.normal[1], -1.0, tol=1e-6, msg="the cut face looks back along the cut plane's normal")


def the_details_show_in_the_cut():
    # The section of the half at the cut plane, a hair inside it: the
    # keyway widens the bore on +x and the set-screw hole breaks the hub
    # on -x, at their heights.
    s = part.section(plane(origin=(0, 0.05, 0), normal=(0, 1, 0)))
    whole = pulley_area()
    # The keyway takes 1.8 x 26 mm out of it, the hole 3.3 x 5 mm.
    assert_close(whole - s.area(), 1.8 * 26 + 3.3 * 5, tol=4, msg="the keyway and the set-screw hole are cut out of the section")


def pulley_area():
    # The profile's area on one side of the axis, times two.
    pts = [(6.6, 0), (10.4, 0), (11, 0.6), (11, 9.5), (19, 9.5), (19, 3.6), (19.6, 3),
           (26.4, 3), (27, 3.6), (27, 7.5), (22.5, 10.5), (22.5, 13.5), (27, 16.5),
           (27, 20.4), (26.4, 21), (19.6, 21), (19, 20.4), (19, 14.5), (11, 14.5),
           (11, 25.4), (10.4, 26), (6.6, 26), (6, 25.4), (6, 0.6)]
    a = 0.0
    for i in range(len(pts)):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % len(pts)]
        a += x0 * y1 - x1 * y0
    return abs(a)


test("one half, with its cut face", one_half_with_its_cut_face)
test("the keyway and the set-screw hole show in the cut", the_details_show_in_the_cut)
