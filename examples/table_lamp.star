# A bedside table lamp: a linen drum shade with a bound top edge, on a dark
# coarse-glazed ceramic base, lit from inside.
#
# **This is a product model, not a printable part.** Nothing here is checked
# against a nozzle or a bed. It is in `examples/` for two things the printable
# objects do not show: a shade that is genuinely a 1.2 mm wall rather than a
# solid cylinder, and a body that is a light.
#
# What this example demonstrates:
#
#   * **thin-walled and solid at the same time, with no boolean anywhere.**
#     The drum is one `loft` between two *annular* sections -- a circle with a
#     concentric circle inside it, which the sketch classifies as a hole
#     -- so `loft` pairs loop 0 to loop 0 and loop 1 to loop 1 and the
#     wall comes out as two exact conical surfaces closed by an annulus at
#     each rim. `cadpole check` reports it solid and watertight: the shade is
#     *hollow*, not *open*, and it needs neither the mesh kernel nor a
#     non-solid exemption;
#   * **a solid of revolution built as a loft of circles rather than a
#     `revolve`.** The base is four concentric discs lofted, which is the same
#     stack of exact conical frusta a lathe profile would give and is free of
#     two things a `revolve` is not: it needs no gap at the axis (a revolved
#     region touching the axis makes degenerate faces), and it does not
#     meet the tessellator's thin-band trouble -- see the two traps below;
#   * **`transmission`, `attenuation` and `scatter` doing what a colour
#     cannot.** Linen is not "white with an opacity": it absorbs over ~30 mm,
#     scatters forward over ~2 mm, and is lit from behind. Only
#     `--backend path` reads those fields, so this example reads flat under
#     the rasterizer and only becomes itself under the tracer. The notes at
#     the material say what matters;
#   * **an emissive body as an actual light.** The bulb is `preset="led"`, so
#     under the path tracer it lights the shade from inside, the harp and the
#     socket fall on the linen as silhouettes, and
#     the surface the lamp stands on is lit by the lamp;
#   * **`rotate(..., axis=(0,1,0), origin=)`** to splay one upright into two
#     harp legs without moving either foot.
#
# Two traps, both general:
#
#   * **two concentric `cone`s subtracted** is how `examples/lamp_shade.star`
#     builds its shell, and it works there because that shade *flares*: its
#     two cones cross transversally, which is what the exact kernel is for. A
#     straight-sided drum's walls are parallel and never cross, and the same
#     subtract fails ("the boundaries meet, but not transversally");
#   * **a thin ring revolved about the axis** -- the obvious way to turn the
#     bound edge -- can lose faces in tessellation, and so can a lathe profile
#     for the base, at some chamfer sizes and not at their neighbours. A model
#     that fails for one value of a parameter and not for its neighbours is
#     not a model, so nothing here is revolved.
#
#   cadpole render examples/table_lamp.star --backend path --samples 192 \
#       --size 1200x900 --view az=30,el=10 --environment -o /tmp/table_lamp.png
#   cadpole render examples/table_lamp.star --param bulb=false --param harp=false -o /tmp/lamp_off.png
#   cadpole check  examples/table_lamp.star

shade_d = param("shade_diameter", 300, unit="mm", min=120, max=600, doc="Outside diameter at the bottom rim")
shade_h = param("shade_height", 240, unit="mm", min=80, max=500, doc="Height of the drum")
shade_taper = param("shade_taper", 0, unit="mm", min=-60, max=60,
                    doc="How much narrower the top rim is than the bottom. 0 is straight-sided, a drum shade")
wall = param("wall", 1.2, unit="mm", min=0.4, max=6, doc="Linen thickness, measured through the surface")

base_d = param("base_diameter", 170, unit="mm", min=60, max=400, doc="Ceramic base, at the shoulder")
base_h = param("base_height", 85, unit="mm", min=30, max=250)
flare = param("base_flare", 2, unit="mm", min=0, max=30, doc="How much wider the base is at the floor than at its shoulder")
gap = param("shade_gap", 30, unit="mm", min=0, max=120, doc="Daylight between the base's shoulder and the shade's bottom rim")

rim_h = param("rim_height", 7, unit="mm", min=0, max=30, doc="Height of the dark bound edge at the top of the shade")
rim_t = param("rim_thickness", 2.2, unit="mm", min=0.6, max=8, doc="How far that edge stands proud of the linen")

bulb = param("bulb", True, doc="Include the emissive bulb. Only `--backend path` lights with it")
harp = param("harp", True, doc="Include the harp, seen in silhouette through the lit linen")

r_bot = shade_d / 2.0
r_top = r_bot - shade_taper / 2.0
base_r = base_d / 2.0
shade_z = base_h + gap
shade_top = shade_z + shade_h

assert_true(r_top > 0 and r_bot > 0, msg="the shade taper is larger than the shade")
assert_true(base_r < r_bot, msg="the base has to stand inside the shade's footprint")
assert_true(2 * wall < base_r, msg="the wall is thicker than the base is wide")


def disc(r, z, name):
    """One section of a turned solid: a circle at height `z`."""
    return sketch(XY.offset(z), name=name).circle(r)


def ring(r, thick, z, name):
    """One section of a thin-walled tube: a circle with a concentric hole."""
    return sketch(XY.offset(z), name=name).circle(r).circle(r - thick)


def tube(r0, r1, thick, z0, z1, name):
    """A thin-walled tube from `z0` to `z1`, outer radius `r0` rising to `r1`."""
    return loft([ring(r0, thick, z0, name + "_lower"), ring(r1, thick, z1, name + "_upper")], name=name)


# --- the shade ----------------------------------------------------------------
# A wall `wall` thick measured *through* a leaning surface is `wall / cos(lean)`
# measured horizontally, which is the offset the inner ring needs. At the
# default taper of 0 that is exactly `wall`; the division is here so the shade
# stays honest about its own thickness if the taper is turned up.
lean = math.atan((r_bot - r_top) / shade_h)
h_wall = wall / math.cos(lean)
assert_true(h_wall >= wall - 1e-9, msg="a leaning wall is never thinner than its own thickness")

shade = tube(r_bot, r_top, h_wall, shade_z, shade_top, "shade")
assert_solid(shade, msg="a hollow shade is still a closed solid")

# Linen, read from behind. Two things to know:
#
#   * **`transmission` has to be spelled out.** `opacity` alone reads as "how
#     much of the background shows through", and with only that set the shade
#     is a flat off-white wall with no glow; `transmission` is what carries
#     light out of a lit interior;
#   * **`scatter` and a 1.2 mm wall do not mix.** `scatter` is a mean free
#     path, so to scatter even once inside 1.2 mm of linen it has to be about
#     a millimetre, and at that length the glow disappears. A wall this thin
#     cannot have a weave *and* be lit through, so the weave is left off and
#     `attenuation` does the warming on its own.
shade.material(color="#f2e5cd", roughness=0.9, opacity=0.42, transmission=0.9,
               attenuation=30, ior=1.45)

# The bound top edge: a short ring of rectangular section
# straddling the rim, `rim_t` proud of the linen on both faces.
parts = [shade]
if rim_h > 0:
    band = tube(r_top + rim_t, r_top + rim_t, h_wall + 2 * rim_t,
                shade_top - rim_h, shade_top, "shade_rim")
    band.material(color="#26211d", roughness=0.55)
    rlo, rhi = band.bbox()
    assert_true(math.close(rhi[2], shade_top, tol=0.01), msg="the bound edge finishes flush with the top rim")
    assert_true(rhi[0] - rlo[0] > 2 * r_top, msg="the bound edge stands proud of the linen")
    parts.append(band)

# --- the base -----------------------------------------------------------------
# Four concentric discs, lofted: the floor, the top of the foot chamfer, the
# bottom of the shoulder chamfer, and the shoulder itself. Consecutive
# sections of equal radius give a cylinder and unequal ones an exact conical
# frustum, so this is a turned profile by another name -- with flat caps
# instead of a hole down the axis.
foot = 3.0
shoulder = 4.0
assert_true(shoulder < base_h / 2, msg="the shoulder chamfer is half the base")
assert_true(flare + foot < base_r, msg="that much flare leaves no straight wall on the base")
base = loft([
    disc(base_r + flare - foot, 0.0, "base_floor"),
    disc(base_r + flare, foot, "base_foot"),
    disc(base_r, base_h - shoulder, "base_wall"),
    disc(base_r - shoulder, base_h, "base_shoulder"),
], name="base")
# Dark glaze, coarse. `noise` at a 4 mm blob is the grain, and
# it also breaks up the highlight, which is what stops a dark cylinder reading
# as a length of black plastic pipe.
base.material(color="#332b26", roughness=0.72, texture="noise", scale=4)

blo, bhi = base.bbox()
assert_true(math.close(bhi[2], base_h, tol=0.01), msg="the base's shoulder is `base_height` up")
assert_true(math.close(bhi[0] - blo[0], base_d + 2 * flare, tol=0.5), msg="the base is widest just above the floor")

# --- the fittings -------------------------------------------------------------
# Everything from here up is inside the shade, and shows as dark shapes
# through the lit linen.
neck_r = 13.0
socket = cylinder(neck_r, gap + 46, at=(0, 0, base_h), name="socket")
socket.material(color="#2b2b2d", roughness=0.35, metallic=0.4)
parts += [base, socket]

if bulb:
    # Clear of the linen everywhere, and tall rather than stubby: a short
    # emitter puts a hot spot on the shade, and a tall one spreads into the
    # soft vertical band. `led` carries the emissive
    # material; the `emissive` after it is the wattage, and a preset's fields
    # are overridden by whatever follows it.
    bulb_r = min(34.0, r_top - h_wall - 18.0)
    assert_true(bulb_r > 6.0, msg="no room for a bulb inside a shade this narrow")
    parts.append(cylinder(bulb_r, shade_h * 0.72, at=(0, 0, shade_z + shade_h * 0.10), name="bulb").material(
        preset="led", color="#fff0d0", emissive=(14.0, 11.5, 8.1)))

if harp:
    # One upright, splayed both ways: `rotate` about Y through the foot of the
    # leg leans a vertical rod without moving where it stands.
    leg_h = shade_h * 0.86
    splay = 9.0
    for i, side in enumerate([-1.0, 1.0]):
        foot_at = (side * neck_r, 0, base_h + 8)
        rod = cylinder(2.6, leg_h, at=foot_at, name="harp_%d" % i)
        rod = rotate(rod, -side * splay, axis=(0, 1, 0), origin=foot_at, name="harp_splay_%d" % i)
        rod.material(color="#2b2b2d", roughness=0.3, metallic=0.5)
        parts.append(rod)

# --- checks ---------------------------------------------------------------------
slo, shi = shade.bbox()
assert_true(math.close(shi[2], shade_top, tol=0.02), msg="the shade's top rim is where the arithmetic put it")
assert_true(math.close(shi[0] - slo[0], shade_d, tol=0.5), msg="the bottom rim is the widest part of the shade")
assert_true(shi[0] - slo[0] > shi[2] - slo[2], msg="the shade is wider than it is tall")
assert_true(math.close((shi[0] - slo[0]) / (shi[2] - slo[2]), 1.25, tol=0.06),
            msg="the shade is about 1.22 wide to tall")
assert_true(math.close(base_d / shade_d, 0.57, tol=0.05),
            msg="the base is a little over half the shade's diameter")
assert_true(math.close(base_h / shade_h, 0.354, tol=0.05),
            msg="the base is about a third of the shade's height")

# And the shade holds only the material of its own wall, which is the one
# claim a picture of it cannot make.
ring_area = math.pi * (r_bot * r_bot - (r_bot - h_wall) * (r_bot - h_wall))
assert_true(math.close(shade.volume(), ring_area * shade_h, tol=0.05 * ring_area * shade_h),
            msg="the shade is hollow, not a solid cylinder with a colour on it")

# Every body of the lamp, in one list, so a scene script can `load()` this
# file and stand the lamp on something: `translate` consumes what it moves, so
# a caller needs a handle on all of them or the originals stay at the origin.
LAMP = parts
LAMP_HEIGHT = shade_top
LAMP_WIDTH = shade_d

export([shade, base], "table_lamp.step")
