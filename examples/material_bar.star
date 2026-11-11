# A step wedge for judging a print material.
#
#     cadpole render examples/material_bar.star --backend path --samples 128 -o /tmp/bar.png
#
# Four square pads of the same footprint and four different thicknesses, plus
# a post and a ring, are enough to read a material off:
#
#   * an opaque filament shows the same colour on all four pads, and the
#     anisotropic layer term as a highlight smeared along the horizontal;
#   * a translucent one gets darker and more saturated from the 2 mm pad to
#     the 20 mm pad, because absorption is Beer-Lambert over the distance
#     light travels inside the solid, not a per-surface alpha;
#   * a scattering one (milky filament, frosted resin) also loses the sharp
#     edge between pad and ground;
#   * an emissive one lights the pads next to it.
#
# Every pad is its own body with a gap between it and the next.

pad = param("pad", 18, unit="mm", min=8, max=40, doc="Pad footprint")
gap = param("gap", 5, unit="mm", min=1, max=20, doc="Gap between pads")

THICKNESS = [2, 5, 10, 20]


def row_x(slot):
    """Centre of slot `slot` of a four-wide row centred on the origin."""
    return (slot - 1.5) * (pad + gap)


for i, t in enumerate(THICKNESS):
    box(pad, pad, t, center=True, at=(row_x(i), 0, 0), name="pad_%d" % i)

# A round wall, so the highlight has a curve to run along and a translucent
# material has a varying path length through it.
post = cylinder(pad / 2, 26, at=(row_x(1), pad + gap, 0), name="post")

# A ring: thin where you see through the side, thick through the corner.
ring = subtract(
    cylinder(pad / 2, 12, at=(row_x(2), pad + gap, 0), name="ring_outer"),
    cylinder(pad / 2 - 3, 14, at=(row_x(2), pad + gap, -1), name="ring_bore"),
    name="ring",
)
