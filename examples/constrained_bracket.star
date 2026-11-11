# A mounting plate whose holes are *dimensioned*, not placed.
#
# The literal version of this plate (examples/bracket.star) writes the hole
# centres as `(-w / 2 + 8, 0)`: the author did the algebra, and the algebra has
# to be redone every time the drawing changes. Here the holes are 10 mm in from
# the ends and on the plate's centreline, and the solver works out where that
# is — change `width` and the holes move with the edges they were measured
# from, because that is what the sketch actually says.
#
# Units are millimetres.

w = param("width", 80, unit="mm", min=40, max=160, doc="Plate width (x)")
d = param("depth", 40, unit="mm", min=25, max=90, doc="Plate depth (y)")
t = param("thickness", 6, unit="mm", min=3, max=12)
margin = param("edge_margin", 10, unit="mm", min=5, max=25, doc="Bolt centres from the ends")
bolt = param("bolt_d", 5.5, unit="mm", min=3, max=10, doc="Bolt hole diameter")

s = constrained_sketch(XY, name="plate_profile")

# The four corners. Only the first is pinned; every other coordinate below is a
# starting guess, deliberately a little off, so it is obvious that the
# dimensions and not the numbers decide the answer. A guess only has to be on
# the right side of things — a hole that starts outside the edge it is
# dimensioned from would be solved to the outside of it.
bl = s.point(0, 0, name="bl", fix=True)
br = s.point(w, 1, name="br")
tr = s.point(w + 1, d, name="tr")
tl = s.point(-1, d - 1, name="tl")

bottom = s.line(bl, br, name="bottom")
right = s.line(br, tr, name="right")
top = s.line(tr, tl, name="top")
left = s.line(tl, bl, name="left")

# Four lines through four shared points are already a closed outline; these
# say it is a rectangle, and how big.
s.constrain("horizontal", bottom)
s.constrain("horizontal", top)
s.constrain("vertical", left)
s.constrain("vertical", right)
s.constrain("distance", bl, br, w, name="width")
s.constrain("distance", br, tr, d, name="depth")

# A centreline between the two ends. Construction geometry: it constrains the
# holes and never becomes material.
ml = s.point(0, d / 2, name="mid_left", construction=True)
mr = s.point(w, d / 2, name="mid_right", construction=True)
axis = s.line(ml, mr, name="centreline", construction=True)
s.constrain("midpoint", ml, left)
s.constrain("midpoint", mr, right)

# Bolt holes: on the centreline, `margin` in from the end nearest each.
hl = s.point(margin, d / 2, name="hole_left_center")
hole_left = s.circle(hl, bolt / 2, name="hole_left")
s.constrain("on_curve", hl, axis, name="left hole on the centreline")
s.constrain("distance", hl, left, margin, name="left hole from the left edge")
s.constrain("radius", hole_left, bolt / 2, name="bolt radius")

hr = s.point(w - margin, d / 2, name="hole_right_center")
hole_right = s.circle(hr, bolt / 2, name="hole_right")
s.constrain("on_curve", hr, axis, name="right hole on the centreline")
s.constrain("distance", hr, right, margin, name="right hole from the right edge")
s.constrain("equal", hole_right, hole_left, name="the bolts match")

# Fully dimensioned: nothing is left to chance, and the solver says so.
assert_eq(s.dof(), 0)

# And the drawing is true at every width, which is the point of writing it this
# way: `cadpole sweep examples/constrained_bracket.star --param width=60,100,150`
# moves the right-hand hole with the right-hand edge.
def near(a, b):
    return a - b < 1e-6 and b - a < 1e-6

assert_true(near(s.at(br)[0], w), msg="the plate is `width` wide")
assert_true(near(s.at(hr)[0], w - margin), msg="the right hole follows the right edge")
assert_true(near(s.at(hl)[1], d / 2), msg="both holes sit on the centreline")

plate = extrude(s, t, name="plate")
plate.material(color="#7fa6d9", roughness=0.5)

export(plate, "constrained_bracket.stl")
