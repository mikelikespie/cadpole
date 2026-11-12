# Extrude: a flat outline pulled out into a solid. A length of 20 x 20
# T-slot framing: the square outline with a T-slot in each face and the
# centre bore, extruded 60 mm. The holes in the sketch run the full length.
size = 20.0
lip = 1.8        # the slot's lips
opening = 6.2    # the gap between the lips

def t_slot():
    # One slot, opening on the +y face (the sketch's +y, the model's +z).
    h, top = size / 2, size / 2 + 0.5   # carried past the face, so the cut crosses it
    return [(-opening / 2, top), (opening / 2, top), (opening / 2, h - lip),
            (5.5, h - lip), (5.5, 6.9), (2.9, 4.3), (-2.9, 4.3), (-5.5, 6.9),
            (-5.5, h - lip), (-opening / 2, h - lip)]

# snippet
outline = sketch(XZ).rounded_rect(20, 20, 1.5)
slots = sketch(XZ).polygon(t_slot()).pattern2d_circular(4)
outline = outline.subtract(slots).circle(2.1)
part = extrude(outline, 60, name="part")
# end snippet
part.tessellation(tolerance=0.008, angle=4)   # the 4.2 mm bore stays round seen end on
part = translate(part, (0, 30, 10), name="profile")
part.material(color="#c9d3db", roughness=0.32, metallic=0.75)


# Tests: `cadpole test site/features/extrude.star`.

def the_extrusion_is_the_outline_pulled_out():
    assert_solid(part)
    lo, hi = part.bbox()
    assert_true(abs(hi[1] - lo[1] - 60) < 1e-6 and abs(hi[0] - lo[0] - 20) < 1e-6 and abs(hi[2] - lo[2] - 20) < 1e-6,
                msg="20 x 20, 60 long")
    assert_true(abs(lo[2]) < 1e-6, msg="it lies on the ground")
    # Every cut runs the full length: the cross-section is the same at both
    # ends and in the middle, four slots open to the faces and one bore.
    for y in [-29.0, 0.0, 29.0]:
        s = part.section(XZ.offset(-y))
        assert_true(s.loops == 2, msg="the outline and the bore, got %d loops" % s.loops)
    s = part.section(XZ)
    assert_true(abs(s.area() - outline.area()) < 0.005 * outline.area(), msg="the section is the sketch: %s vs %s" % (s.area(), outline.area()))
    return s.area()

test("a T-slot outline, holes included, pulled into a solid", the_extrusion_is_the_outline_pulled_out)
