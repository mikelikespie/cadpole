# A shaped text label standing proud of a rounded tag.
#
# `text(...)` shapes the string with the real kerning and ligatures of the
# bundled face and hands back the glyph outlines as a sketch, so letters
# extrude like any other profile. Pass `font=asset("MyFont.ttf")` to pin your
# own face by content hash instead.
#
#   cadpole render examples/text_label.star -o /tmp/label.png
#   cadpole render examples/text_label.star -p letters=v2.1 -o /tmp/v21.png

tag_w = param("tag_w", 54, unit="mm", min=30, max=120)
tag_h = param("tag_h", 22, unit="mm", min=12, max=60)
tag_t = param("tag_t", 3, unit="mm", min=1, max=10)
letters = param("letters", "POLE", doc="What the label says")
size = param("size", 9, unit="mm", min=3, max=30, doc="Em size of the type")
relief = param("relief", 0.8, unit="mm", min=0.2, max=3, doc="How far the letters stand up")

tag = extrude(sketch(XY).rounded_rect(tag_w, tag_h, 4), tag_t, name = "tag")
tag.material(preset = "pla_matte", color = "#2f3438")

# `baseline="middle"` centres the type on y = 0 rather than sitting it on the
# baseline, so it lines up with the tag without measuring the face by hand.
label = text(
    letters,
    size = size,
    weight = 700,
    align = "center",
    baseline = "middle",
    spacing = 0.4,
    plane = XY.offset(tag_t),
    name = "label",
)

# One body per letter, printed in a second colour: the tag and the type stay
# separate bodies rather than being unioned, which is what a two-material
# slicer wants anyway.
for glyph in extrude(label, relief, name = "raised"):
    glyph.material(preset = "pla_silk", color = "#e8c16a")
