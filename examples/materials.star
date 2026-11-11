# A row of material samples, one body per material, so
# `cadpole render examples/materials.star` shows what each preset and each
# procedural texture actually looks like. The Materials section of
# docs/starlark-api.md lists them all.
#
# Every sample is placed with translate() and nothing touches anything else,
# so there is only one boolean in the whole file (the bored block).

pitch = param("pitch", 26, unit="mm", min=18, max=60, doc="Spacing between samples")
size = param("size", 20, unit="mm", min=8, max=40, doc="Sample cube side")
h = param("height", 28, unit="mm", min=10, max=60, doc="Sample height")


def place(body, slot, name):
    """Put a sample in slot `slot` of the row, which is centred on the origin."""
    return body.translate(((slot - 3.5) * pitch, 0, 0), name=name)


def cube():
    return box(size, size, h, center=True)


def post():
    return cylinder(size / 2, h)


# 0 — matte PLA: the printer default, a diffuse grey.
place(cube(), 0, "pla_matte").material(preset="pla_matte")

# 1 — silk PLA: the same plastic with a much tighter highlight.
place(post(), 1, "pla_silk").material(preset="pla_silk")

# 2 — brass: a metal, so its colour comes from the reflection, not the diffuse.
place(post(), 2, "brass").material(preset="brass")

# 3 — steel, bored through. The bore sticks out 1 mm at both ends: coplanar
# faces make booleans fail in the current kernel, so overlap instead of touch.
bored = subtract(cube(), cylinder(size / 4, h + 2, at=(0, 0, -1)))
place(bored, 3, "steel").material(preset="steel")

# 4 — oak: procedural growth rings around Z, so they follow the shape and
# cross from face to face without a seam.
place(cube(), 4, "wood_oak").material(preset="wood_oak", scale=16)

# 5 — a painted checker: any preset takes a texture and a repeat length.
place(cube(), 5, "checker").material(preset="pla_matte", color="#c9543d", texture="checker", scale=4.5)

# 6 — clear acrylic. Translucency is composited by the renderer's
# order-independent pass.
place(cube(), 6, "acrylic").material(preset="acrylic_clear")

# 7 — an LED lens: emissive, so it stays bright wherever the lights are.
place(post(), 7, "led").material(preset="led")
