# Display layers: a machined part, the envelope it may claim, and the
# fixture it sits in, each on a layer the web UI's Layers panel switches off
# and ghosts without re-evaluating -- and that `cadpole render --layer` controls
# the same way for an image:
#
#   cadpole render examples/layers_demo.star --view product --lighting product --backdrop studio \
#       --layer fixture=0.6 --size 1600x1200 -o docs/demo/layers_demo.png
#
# The envelope is declared at 30 % (its own default, so it renders ghosted
# with no flag at all); the fixture is declared hidden, and the command above
# ghosts it in at 60 %. `--layer envelope=off` would drop the envelope,
# `--layer other=off` everything on no layer (nothing here: every body is on
# one). The same three rows appear in the web UI's Layers panel, where the
# switch and the slider do this live.

length = param("length", 60, unit="mm", min=30, max=120, doc="Part length (X)")
width = param("width", 40, unit="mm", min=20, max=80, doc="Part width (Y)")
height = param("height", 24, unit="mm", min=10, max=60, doc="Part height (Z)")
bore = param("bore", 12, unit="mm", min=4, max=30, doc="Bore diameter (the boss is 4 mm wider)")
clearance = param("clearance", 6, unit="mm", min=0, max=20, doc="Envelope clearance around the part")

# The layers, in the order the panel lists them. A layer is metadata: nothing
# below changes shape because of one.
part_layer = layer("part", doc="The machined part")
envelope = layer("envelope", doc="The space the part may claim", opacity=0.3, color="#ff8800")
fixture = layer("fixture", doc="The plate and the pins the part is held by", visible=False, color="#4a90d9")

# The part: a block with a boss on top, both on the part layer by the
# keyword. (Primitives only, on purpose: a body that goes through a boolean
# tessellates a little differently on wasm, and every example must evaluate
# the same on both. A boolean's result takes its first operand's layer.)
part = box(length, width, height, center=True, name="part", layer=part_layer)
boss = cylinder(bore / 2 + 4, 6, at=(0, 0, height), name="boss", layer=part_layer)
part.material(preset="steel")
boss.material(preset="steel")

# The envelope: the part's box grown by the clearance, ghosted by its layer's
# own default opacity.
space = box(length + 2 * clearance, width + 2 * clearance, height + 6 + clearance, center=True, name="envelope", layer=envelope)

# The fixture: a plate under the part and two locating pins outside the
# envelope, tagged two ways -- the keyword on the plate, `Layer.add` on the
# pins.
plate = box(length + 40, width + 40, 6, at=(0, 0, -6), center=True, name="plate", layer=fixture)
pins = fixture.add(
    cylinder(3, 10, at=(-length / 2 - 8, 0, -6), name="pin_a"),
    cylinder(3, 10, at=(length / 2 + 8, 0, -6), name="pin_b"),
)


def a_part_inside_its_envelope():
    assert_solid(part)
    assert_solid(boss)
    assert_solid(space)
    assert_bbox(part, (-length / 2, -width / 2, 0), (length / 2, width / 2, height))
    assert_bbox(boss, (-bore / 2 - 4, -bore / 2 - 4, height), (bore / 2 + 4, bore / 2 + 4, height + 6))
    assert_true(space.volume() > part.volume() + boss.volume(), msg="the envelope encloses the part")
    assert_true(len(pins) == 2, msg="Layer.add hands the pins back")


test("the part is a solid inside its envelope, and the layers change nothing", a_part_inside_its_envelope)
test("at every clearance", a_part_inside_its_envelope, sweep={"clearance": [0, 6, 20]})
