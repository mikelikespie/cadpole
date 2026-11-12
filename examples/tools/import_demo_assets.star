# The files import_demo.star reads, made by Cadpole itself so every byte is
# invented: an organic owl figurine (fields, meshed) written as STL, a
# pillow block (exact B-rep) written as STEP, and an involute spur-gear profile
# with a keyed bore and lightening windows written as DXF.
#
#   cadpole export examples/tools/import_demo_assets.star --run-exports --out-dir examples/assets
#
# The owl stands in for "a mesh from somewhere else" (a scan, a sculpt, a
# download), the block for "a part from another CAD tool", and the gear for
# "a profile from a laser-cutter or a 2D drafting package".
#
# The owl's numbers are tuned so the field mesher returns a clean solid with
# no folds (cadpole check: "solid"): close concave creases -- an eyeball sunk
# in a socket, toes side by side -- leave hair-thin crossings in the mesh, so
# every feature stands proud of the surface and the toes splay apart.
# `owl_res` and `owl_noise` are there to explore that.

# The same block import_demo.star's tests rebuild natively: keep the two alike.

BLOCK = dict(w = 64.0, d = 24.0, t = 9.0, boss_r = 15.0, boss_h = 30.0, bore_r = 7.0, bolt_r = 3.3, bolt_x = 24.0)

def pillow_block(name = "pillow_block"):
    b = BLOCK
    # One profile in the XZ plane -- foot, pedestal and drum -- with the bore
    # cut in 2D, extruded through the depth, then the two bolt holes drilled.
    foot = sketch(XZ, name = name + "_foot").rect(b["w"], b["t"], center = (0, b["t"] / 2))
    post = sketch(XZ, name = name + "_post").rect(2 * b["boss_r"], b["boss_h"], center = (0, b["boss_h"] / 2))
    drum = sketch(XZ, name = name + "_drum").circle(b["boss_r"], center = (0, b["boss_h"]))
    bore = sketch(XZ, name = name + "_bore").circle(b["bore_r"], center = (0, b["boss_h"]))
    profile = foot.union(post, name = name + "_fp").union(drum, name = name + "_outline").subtract(bore, name = name + "_profile")
    slab = extrude(profile, b["d"], name = name + "_slab")
    lo, hi = slab.bbox()
    slab = translate(slab, (0, -(lo[1] + hi[1]) / 2, 0), name = name + "_centered")
    bolts = [cylinder(b["bolt_r"], b["t"] + 2, at = (x, 0, -1), name = "%s_bolt_%d" % (name, i)) for i, x in enumerate([-b["bolt_x"], b["bolt_x"]])]
    return subtract(slab, bolts, name = name, kernel = "brep")

# ---- STL: an owl, sculpted with fields --------------------------------------
#
# A perched owl about 50 mm tall facing -y: an egg of a body, a broad head,
# raised eye discs with pupils, a hooked beak, swept ear tufts, folded wings
# standing proud of the flanks and two clawed feet on the perch line. Every
# feature is added (raised), not carved: a smooth blend over a convex bump
# meshes cleanly where a close concave crease would not.

body = field.sphere(14.0).translate((0, 0.5, 13.6)).smooth_union(field.capsule((0, 0.8, 18.0), (0, 0.4, 27.0), 11.8), 6.0)
head = field.sphere(11.5).translate((0, 0.5, 37.0))
owl = body.smooth_union(head, 5.0)

# The facial disc: a shallow raised dish the eyes sit in.
disc = field.cylinder(7.0, 2.4, round = 1.1).rotate(90, axis = (1, 0, 0))
owl = owl.smooth_union(disc.translate((-4.6, -9.6, 37.2)), 3.0).smooth_union(disc.translate((4.6, -9.6, 37.2)), 3.0)
# Eye rings proud of the disc, then the pupils proud of the rings.
for sx in [-4.9, 4.9]:
    owl = owl.smooth_union(field.cylinder(3.9, 1.6, round = 0.7).rotate(90, axis = (1, 0, 0)).translate((sx, -11.0, 37.6)), 0.9)
    owl = owl.smooth_union(field.sphere(2.1).translate((sx, -11.6, 37.6)), 0.5)
# A hooked beak between and below the eyes.
beak = field.cone(2.7, 0.35, 6.5).rotate(150, axis = (1, 0, 0)).translate((0, -11.6, 33.6))
owl = owl.smooth_union(beak, 1.2)

# Breast feathers: rows of shallow chevrons laid on the body's surface,
# each arm a thin capsule standing about half a millimetre proud.
def breast_y(x, z):
    # The front of the body at (x, z): the nearer of the sphere and the capsule.
    ys = 0.5 - math.sqrt(max(196.0 - x * x - (z - 13.6) * (z - 13.6), 0.0))
    yc = 0.6 - math.sqrt(max(139.2 - x * x, 0.0)) if z > 18.0 else 99.0
    return min(ys, yc)

# `lift` makes up for the blend between the sphere and the capsule, which
# stands proud of both.
for row, (z, lift) in enumerate([(11.0, 0.25), (15.5, -0.3), (20.0, -0.7), (24.5, -0.4)]):
    for cx in ([-5.2, 0.0, 5.2] if row % 2 == 0 else [-2.6, 2.6]):
        for side in [-1.0, 1.0]:
            a = (cx, breast_y(cx, z) + lift, z)
            b = (cx + 2.4 * side, breast_y(cx + 2.4 * side, z + 1.6) + lift, z + 1.6)
            owl = owl.smooth_union(field.capsule(a, b, 0.75), 0.6)
# Ear tufts: tapered cones swept up and out.
for sgn in [-1, 1]:
    # The cone is centred on its axis's midpoint: tilt it, then stand its
    # base on the crown.
    tuft = field.cone(3.8, 0.45, 11.0).rotate(30 * sgn, axis = (0, 1, 0)).translate(((6.0 + 5.5 * math.sin(30.0)) * sgn, 0.8, 43.0 + 5.5 * math.cos(30.0)))
    owl = owl.smooth_union(tuft, 2.2)
# Folded wings: a long flattened capsule on each flank, blended lightly so
# their leading edge stays a ridge, and a second, shorter primary-feather
# capsule below it.
for sgn in [-1, 1]:
    wing = field.capsule((12.0 * sgn, 2.8, 28.0), (10.6 * sgn, 7.0, 7.0), 4.0)
    owl = owl.smooth_union(wing, 1.6)
    prim = field.capsule((13.2 * sgn, 6.0, 17.0), (10.0 * sgn, 10.5, 4.5), 2.4)
    owl = owl.smooth_union(prim, 1.0)
# Two feet, each three toes splayed apart so no two toes form a crease.
for sx in [-5.0, 5.0]:
    for da in [-30.0, 0.0, 30.0]:
        a = -90.0 + da
        owl = owl.smooth_union(field.capsule((sx, -8.0, 2.0), (sx + 6.0 * math.cos(a), -8.0 + 6.0 * math.sin(a), 1.3), 1.3), 1.0)

# A whisper of noise so it reads as sculpted, then a flat bottom at z = 0.
owl = owl.displace(field.noise(scale = 5.0, amplitude = 1.0, seed = 7), strength = param("owl_noise", 0.18, doc = "Sculpted-surface noise on the owl"))
owl = owl.intersect(field.half_space(origin = (0, 0, 0.6), normal = (0, 0, -1)))
owl = owl.translate((0, 0, -0.6))
owl_body = mesh(owl, resolution = param("owl_res", 0.35, unit = "mm", doc = "Mesher cell size for the owl"), simplify = False, name = "owl")
export(owl_body, "owl.stl")

# ---- STEP: a pillow block, exact surfaces ----------------------------------

block = pillow_block()
export(block, "pillow_block.step")
report("block_volume", block.volume())

# ---- DXF: a spur-gear outline with a keyed bore and six windows -----------
#
# The library's involute outline (true involute flanks and the trochoid root
# fillet, module 2.5, 24 teeth), a keyed bore for a 12 mm shaft and six round
# lightening windows that leave a full rim under the roots and a full web
# round the hub.

load("@cadpole/gears.star", "involute_profile", "layout")

GEAR_MODULE = 2.5
GEAR_TEETH = 24
lay = layout(GEAR_MODULE, GEAR_TEETH)
# Tooth 0 points along +x; with an even count a tooth space, not a tooth,
# then lies on -y, so the gear stands in its cradle on two teeth.
gear = involute_profile(GEAR_MODULE, GEAR_TEETH, name = "gear")
bore2d = sketch(XY, name = "gear_bore").circle(6.0).union(sketch(XY, name = "key").rect(3.0, 4.0, center = (0, 7.0)), name = "keyed_bore")
# The windows sit between the spokes' directions, clear of the keyway at +y.
windows = sketch(XY, name = "window").circle(4.0, center = (19.5 * math.cos(30.0), 19.5 * math.sin(30.0))).pattern2d_circular(6, merge = True, name = "windows")
gear_profile = gear.subtract(bore2d, name = "gear_bored").subtract(windows, name = "gear_profile")
export_dxf(gear_profile, "gear.dxf")
report("gear_area", gear_profile.area())
report("gear_r_root", lay["r_root"])
report("gear_r_tip", lay["r_tip"])
print("gear profile area: %s mm2 (root r %s, tip r %s)" % (gear_profile.area(), lay["r_root"], lay["r_tip"]))
print("pillow block volume: %s mm3" % block.volume())
