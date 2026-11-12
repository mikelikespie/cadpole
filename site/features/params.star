# Parameters: named numbers the model is rebuilt from.
# The plate the parameters ask for, in front, and behind it the same
# function at two other settings, so the picture shows what a change does.
# snippet
w = param("width", 40, unit="mm", min=20, max=80)
holes = param("holes", 3, min=1, max=6)

def plate(w, holes, name):
    r = min(3, 0.3 * w / holes)          # holes shrink to fit a narrow plate
    s = sketch(XY).rounded_rect(w, 20, 4)
    for i in range(holes):
        s = s.circle(r, center=((i + 0.5) * w / holes - w / 2, 0))
    return extrude(s, 6, name=name)
# end snippet

def finished(w, holes, name, y):
    # The snippet's plate with the outer edge of its top rounded: the blank
    # rounded first and the same holes drilled after, so a web between close
    # holes is left whole. The test below holds it to the snippet's plate.
    blank = extrude(sketch(XY).rounded_rect(w, 20, 4), 6, name=name + "_blank")
    top = blank.faces(normal=(0, 0, 1)).one()
    blank = fillet(blank, 0.8, edges=top.edges, name=name + "_soft")
    r = min(3, 0.3 * w / holes)
    drills = sketch(XY.offset(-1))
    for i in range(holes):
        drills = drills.circle(r, center=((i + 0.5) * w / holes - w / 2, 0))
    p = subtract(blank, extrude(drills, 8), name=name + "_drilled")
    return p.translate((0, y, 0), name=name)

part = finished(w, holes, "part", 0)
part.material(color="#6fbfa8", roughness=0.35)
# Two more settings in deeper teal, each a step wider with more holes, or
# narrower with fewer where a step up would pass the limit.
def step(v, d, lo, hi):
    return v + d if v + d <= hi else max(lo, v - d)

others = [(step(w, 16, 20, 80), step(holes, 1, 1, 6), "#4f9e8a"),
          (step(w, 32, 20, 80), step(holes, 3, 1, 6), "#3f8574")]
for k, (ow, oh, color) in enumerate(others):
    b = finished(ow, oh, "variant_%d" % (k + 1), 28 * (k + 1))
    b.material(color=color, roughness=0.35)

def every_setting_builds():
    assert_solid(part)
    assert_bbox(part, (-w / 2, -10, 0), (w / 2, 10, 6), tol=0.01)
    # The rounded plate is the one the snippet's function draws, less the
    # little the rounding takes off.
    drawn = plate(w, holes, "check").volume()
    assert_true(part.volume() < drawn and part.volume() > 0.97 * drawn, msg="the plate the snippet draws")
    assert_true(part.volume() < w * 20 * 6 - holes * 6, msg="the holes are cut")

test("every setting inside the limits builds a plate", every_setting_builds,
     sweep={"width": [20, 30, 40, 60, 80], "holes": [1, 3, 6]})
