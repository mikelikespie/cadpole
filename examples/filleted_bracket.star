# The mounting bracket of examples/bracket.star, with its top edges rounded.
#
# `fillet` is exact: every blend is the natural quadric tangent to
# both faces, wired into the B-rep, so `rounded.kind` is "brep" and the part
# exports to STEP as it is. One call rounds three kinds of edge here: the
# plate's four straight rim edges (cylinders that meet in elliptical mitres
# at the corners), the foot of the boss (a concave torus round an inner loop
# of the plate's top) and the two bolt-hole mouths (convex tori) -- the last
# two need no boolean at all: the loop's vertices are split and the wall's
# seams trimmed back to the contact circle.
#
# `kernel="field"` is the other path (docs/design.md): the body imported
# as a signed distance field, a blend added near the selected edges, the
# result contoured back to triangles -- a mesh body, said so in an info
# diagnostic with the resolution it used and the error it measured.
#
# Run it:
#   cadpole eval examples/filleted_bracket.star
#   cadpole render examples/filleted_bracket.star -o /tmp/b.png

w = param("width", 60, unit="mm", min=46, max=120, doc="Plate width (x)")
d = param("depth", 30, unit="mm", min=26, max=80, doc="Plate depth (y)")
t = param("thickness", 8, unit="mm", min=5, max=14)
bolt = param("bolt_d", 5.5, unit="mm", min=3, max=7, doc="Bolt hole diameter")
boss_r = param("boss_r", 8, unit="mm", min=4, max=10)
r = param("fillet_r", 2.0, unit="mm", min=0.5, max=2.5, doc="Radius on the top edges")

# The rounds round the boss, the holes and the plate's rim must not reach one
# another: a fillet that would is refused, so say which number to change.
assert_true(w / 2 - 8 - bolt / 2 - r > boss_r + r, msg="the bolt holes' rounds reach the boss's: widen the plate, or shrink boss_r, bolt_d or fillet_r")
assert_true(8 - bolt / 2 - r > r, msg="the bolt holes' rounds reach the rounds on the plate's ends (the holes are 8 mm in): shrink bolt_d or fillet_r")
assert_true(d / 2 - r > boss_r + r, msg="the boss's round reaches the plate's long edges: deepen the plate, or shrink boss_r or fillet_r")

plate = box(w, d, t, center=True, name="plate")
# The boss overlaps the plate by 1 mm: coplanar faces make B-rep booleans fail.
boss = cylinder(boss_r, 10, at=(0, 0, t - 1), name="boss")
body = union(plate, boss, name="body")

holes = [
    cylinder(bolt / 2, t + 2, at=(-w / 2 + 8, 0, -1), name="hole_left"),
    cylinder(bolt / 2, t + 2, at=(w / 2 - 8, 0, -1), name="hole_right"),
]
bracket = subtract(body, holes[0], holes[1], name="bracket")

# Round the top of the plate: the edges where its top face meets the sides,
# the bolt-hole rims and the foot of the boss. Selecting the face and taking
# its edges keeps the script readable and survives a change to the plate's
# size; `fillet` skips any of them that turn out to be tangent seams rather
# than real edges, and says which. A radius that would reach a hole, or is
# wider than a face, is an error naming the edge and the largest radius that
# fits.
tops = [f for f in bracket.faces(normal=(0, 0, 1)) if f.normal[2] > 0.99]
plate_top = [f for f in tops if f.centroid[2] < t + 0.01][0]
rounded = fillet(bracket, edges=plate_top.edges, radius=r, name="rounded")

rounded.material(color="#7fa6d9", roughness=0.5)

export(rounded, "filleted_bracket.stl")


# The ends of the declared ranges build: every round fits beside the others.
def the_bracket_is_solid():
    assert_solid(rounded)


test("the narrowest plate", the_bracket_is_solid, sweep={"width": [46]})
test("the shallowest plate", the_bracket_is_solid, sweep={"depth": [26]})
test("the thinnest plate", the_bracket_is_solid, sweep={"thickness": [5]})
test("the widest bolt", the_bracket_is_solid, sweep={"bolt_d": [7]})
test("the largest round", the_bracket_is_solid, sweep={"fillet_r": [2.5]})
