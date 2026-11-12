# Revolve: a profile turned about an axis.
# A flanged bushing: chamfered edges, a fillet where the boss meets the
# flange and a countersink at the top of the bore, all drawn in the one
# profile. Three quarters of a turn leave the profile showing in amber.
# snippet
profile = sketch(XZ).polygon(
    [(4.6, 0), (17.4, 0), (18, 0.6), (18, 3), (17, 4), (9, 4),
     (9, 15.2), (8.2, 16), (5.8, 16), (4, 14.2), (4, 0.6)])
profile = profile.fillet2d(1.5, corners=[5])
part = revolve(profile, angle=270, name="part")
# end snippet
part.tessellation(tolerance=0.004, angle=3)
part.material(color="#6fbfa8", roughness=0.35)
# The two planar ends of the turn, where the profile itself is the face.
cut = [f for n in [(0, -1, 0), (1, 0, 0), (0, 1, 0), (-1, 0, 0)]
       for f in part.faces(normal=n, allow_empty=True)]
paint(part, faces=cut, color="#f0c987")

def bushing():
    assert_solid(part)
    assert_eq(len(cut), 2, msg="two cut faces, both the profile")
    assert_bbox(part, (-18, -18, 0), (18, 18, 16), tol=0.05)
    # Three quarters of a turn of the profile: well under the full ring.
    ring = math.pi * (18 * 18 - 4 * 4) * 16
    assert_true(part.volume() < 0.75 * ring * 0.5, msg="a quarter is cut away")

test("a flanged bushing with its profile exposed", bushing)
