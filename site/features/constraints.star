# Sketch constraints: an angle bracket's profile solved from dimensions, not
# coordinates. The amber outline is the rough sketch the solver started from.
s = constrained_sketch(XZ, name="profile")
heel = s.point(0, 0, name="heel", fix=True)
toe = s.point(47, 3, name="toe")
toe_top = s.point(51, 9, name="toe_top")
bend_lo = s.point(15, 10, name="bend_lo")
bend_hi = s.point(9, 16, name="bend_hi")
tip_in = s.point(10, 38, name="tip_in")
tip_out = s.point(-3, 41, name="tip_out")
foot = s.line(heel, toe, name="foot")
toe_face = s.line(toe, toe_top, name="toe_face")
shelf = s.line(toe_top, bend_lo, name="shelf")
web = s.line(bend_hi, tip_in, name="web")
cap = s.line(tip_in, tip_out, name="cap")
back = s.line(tip_out, heel, name="back")
bend = s.arc(s.point(15, 16, name="bend_c"), 6, start=180, end=270, name="bend")
s.constrain("coincident", bend_lo, bend.end)
s.constrain("coincident", bend_hi, bend.start)
s.constrain("tangent", shelf, bend, bend_lo)
s.constrain("tangent", web, bend, bend_hi)
s.constrain("radius", bend, 6, name="bend radius")
s.constrain("parallel", foot, shelf)
s.constrain("parallel", back, web)
s.constrain("parallel", foot, cap)
s.constrain("distance", heel, tip_out, 40, name="height")
# snippet
s.constrain("horizontal", foot)
s.constrain("vertical", back)
s.constrain("perpendicular", foot, toe_face)
s.constrain("equal", toe_face, cap)
s.constrain("distance", heel, toe, 50, name="length")
s.constrain("distance", toe, toe_top, 6, name="thick")
# end snippet
assert_eq(s.dof(), 0)
width = 36
bracket = extrude(s, width, name="bracket")

# Finished as a part: rounded outer corners and two clearance holes in each leg,
# placed from the solved points.
corners = bracket.edges(direction=(0, 0, 1), on_plane=plane(origin=(50, 0, 0), normal=(1, 0, 0))) + \
          bracket.edges(direction=(1, 0, 0), on_plane=plane(origin=(0, 0, 40), normal=(0, 0, 1)))
bracket = fillet(bracket, 5, edges=corners, name="rounded")
x_foot = (s.at(bend_lo)[0] + s.at(toe)[0]) / 2
z_web = (s.at(bend_hi)[1] + s.at(tip_in)[1]) / 2
holes = []
for y in (-width / 4, -3 * width / 4):
    holes.append(cylinder(2.75, 12, at=(x_foot, y, -3)))
    holes.append(cylinder(2.75, 12, at=(0, 0, -3)).rotate(90, axis=(0, 1, 0)).translate((-3, y, z_web)))
part = subtract(bracket, holes, name="part")
part.tessellation(angle=8)
part.material(preset="aluminum", color="#9fd8c8", roughness=0.35)

# The starting guess, drawn as a line just off the end face.
rough = sketch(XZ).polygon([(0, 0), (47, 3), (51, 9), (15, 10), (9, 16), (10, 38), (-3, 41)])
line = subtract2d(rough.offset(0.5, join=join.miter), rough.offset(-0.5, join=join.miter))
guess = extrude(line, 0.4, name="guess").translate((0, -width - 3, 0.5))
guess.material(color="#f0c987", roughness=0.6, shadow=False)

def solved():
    assert_eq(s.dof(), 0)
    assert_close(s.at(toe), (50, 0), tol=1e-6)
    assert_close(s.at(toe_top), (50, 6), tol=1e-6)
    assert_close(s.at(tip_out), (0, 40), tol=1e-6)
    assert_close(s.at(tip_in), (6, 40), tol=1e-6)
    assert_close(s.at("bend_c"), (12, 12), tol=1e-6)

test("the solver squares the rough guess to the dimensions", solved)

def one_part():
    assert_solid(part)
    assert_eq(part.kind, "brep")
    assert_bbox(part, (0, -width, 0), (50, 0, 40))
    holes = part.edges(min_radius=2.7, max_radius=2.8)
    assert_eq(len({(math.round(e.center[0]), math.round(e.center[1] / 9)): 1 for e in holes if e.center[0] > 10}), 2)
    assert_eq(len({(math.round(e.center[2]), math.round(e.center[1] / 9)): 1 for e in holes if e.center[0] < 7}), 2)
    c = distance(part, guess)
    assert_true(not c.overlap and c.distance > 2, msg="the guess outline floats clear of the part")

test("one exact bracket, two holes in each leg", one_part)
