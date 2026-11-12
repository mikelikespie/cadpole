# Sweep: a section carried along a path.
# A rectangular duct: in toward the back, up, then over to the right, the
# two bends in different planes. The flat sides stay flat through both,
# so you can see the section keep its orientation, and the amber ends are
# the section itself.
s = math.sqrt(0.5)
r = 14  # bend radius, on the duct's centre line
# snippet
route = curve((0, -26, 8)).line((0, -8, 8))
route = route.arc((0, -8 + r, 8 + r), via=(0, -8 + r * s, 8 + r - r * s))
route = route.line((0, 6, 26))
route = route.arc((r, 6, 26 + r), via=(r - r * s, 6, 26 + r * s))
route = route.line((40, 6, 26 + r))
duct = sketch(route.start_plane).rounded_rect(16, 10, 2.5)
duct = duct.rounded_rect(12.4, 6.4, 0.7)
part = sweep_along(duct, route, name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
paint(part, faces=part.faces(name="part/start") + part.faces(name="part/end"), color="#f0c987")


def one_hollow_duct():
    assert_solid(part)
    assert_true(part.kind == "brep", msg="lines and arcs sweep to an exact solid")
    wall = (16 * 10 - 12.4 * 6.4) * 30  # the section times the first leg's length, roughly
    assert_true(part.volume() > wall, msg="the duct runs the whole route")


def the_ends_are_the_section():
    # Each end is the ring the sketch draws: 16 x 10 outside, 12.4 x 6.4
    # inside, at its own end of the route.
    ring = 16 * 10 - (4 - math.pi) * 2.5 * 2.5 - (12.4 * 6.4 - (4 - math.pi) * 0.7 * 0.7)
    for name in ("part/start", "part/end"):
        assert_close(part.face(name).area, ring, rel=0.01, msg="%s is the section" % name)
    assert_close(part.face("part/start").normal[1], -1.0, tol=1e-6, msg="the start looks back along the first leg, toward -y")
    assert_close(part.face("part/end").normal[0], 1.0, tol=1e-6, msg="the end looks along the last leg, toward +x")


test("one hollow duct along the whole route", one_hollow_duct)
test("the ends are the section", the_ends_are_the_section)
