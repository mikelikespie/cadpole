# A pillow block (a bearing housing), designed in inches: 4 x 1 1/2 x 2.
inch = 25.4
length = param("length", 4.0, unit="in", min=3.5, max=6.0)
bore_d = param("bore", 0.75, unit="in", min=0.375, max=1.0)

foot = extrude(sketch(XY).rounded_rect(length * inch, 1.5 * inch, 0.25 * inch),
               0.5 * inch, name="foot")
web = box(2 * inch, 1 * inch, 1 * inch,
          at=(-1 * inch, -0.5 * inch, 0.25 * inch), name="web")

def along_y(r, l, name):
    c = cylinder(r * inch, l * inch, at=(0, 0, -l * inch / 2))
    c = rotate(c, 90, axis=(1, 0, 0))
    return translate(c, (0, 0, 1.375 * inch), name=name)

barrel = along_y(0.625, 1.25, "barrel")
bore = along_y(bore_d / 2, 1.5, "bore")
x_hole = (length / 2 - 0.5) * inch
holes = [cylinder(0.14 * inch, 1 * inch, at=(x, 0, -0.25 * inch), name="hole_" + n)
         for (x, n) in [(-x_hole, "a"), (x_hole, "b")]]

block = subtract(union(foot, web, barrel, kernel="brep"), [bore] + holes,
                 kernel="brep", name="block")
block.material(color="#6fbfa8", roughness=0.4)


# Tests: `cadpole test examples/pillow_block.star`.

def the_block_is_one_solid():
    assert_solid(block)
    lo, hi = block.bbox()
    assert_true(abs(hi[0] - lo[0] - length * inch) < 1e-3, msg="the foot is `length` long")
    assert_true(abs(hi[2] - lo[2] - 2 * inch) < 0.05, msg="the barrel's crown is 2 in above the foot's underside")
    return block.volume()

test("one solid, 2 in tall, as long as `length`", the_block_is_one_solid, sweep={"length": [3.5, 4.0, 6.0]}, monotone=monotone.increasing)
test("a wider bore leaves less block", the_block_is_one_solid, sweep={"bore": [0.375, 0.75, 1.0]}, monotone=monotone.decreasing)
