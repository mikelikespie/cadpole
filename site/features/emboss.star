# Emboss: artwork wrapped onto a curved face and raised: a pen cup with its
# number on the side.
r = 17
h = 30
wall = 2
blank = subtract(cylinder(r, h, name="outer"),
                 cylinder(r - wall, h, at=(0, 0, 3), name="inner"), name="blank")
rim = blank.edges(on_plane=XY.offset(h))
cup = fillet(blank, 0.8, edges=rim, name="cup")
cup = chamfer(cup, 1, edges=cup.edges(on_plane=XY, min_radius=r - 0.1), name="foot")
side = cup.faces(kind=kind.cylinder).nearest((0, -r, h / 2))
# snippet
art = text("42", size=14, weight=800,
           align="center", baseline="middle")
part = emboss(cup, side, art, 1.2, wrap=wrap.unroll,
              origin=(0, -r, 16), up=(0, 0, 1), name="part")
# end snippet
part.material(preset="pla_silk", color="#8fd3c0", roughness=0.35)

def raised_on_a_hollow_cup():
    assert_solid(part)
    assert_eq(part.kind, "brep")
    # Hollow: well under the solid cylinder's volume.
    assert_true(part.volume() < 0.4 * math.pi * r * r * h, msg="the cup is hollow")
    # The figures stand 1.2 proud of the wall, on the front only.
    lo, hi = part.bbox()
    assert_close(lo[1], -(r + 1.2), tol=0.05)
    assert_close(hi[1], r, tol=0.05)
    assert_true(len(part.faces(name="part/end")) >= 2, msg="both figures are raised")

test("figures raised on a hollow cup", raised_on_a_hollow_cup)
