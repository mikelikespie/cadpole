# The files import_demo.star reads, made by Cadpole itself so every byte is
# invented: an organic owl figurine (fields, meshed) written as STL, a
# pillow block (exact B-rep) written as STEP, and a spur-gear profile with
# a keyed bore and lightening holes written as DXF.
#
#   cadpole export examples/tools/import_demo_assets.star --run-exports --out-dir examples/assets
#
# The owl stands in for "a mesh from somewhere else" (a scan, a sculpt, a
# download), the block for "a part from another CAD tool", and the gear for
# "a profile from a laser-cutter or a 2D drafting package".
#
# The owl's numbers are tuned so the field mesher returns a clean solid
# (cadpole check: "solid"): close concave creases -- an eyeball in its socket,
# toes side by side, a two-sphere head -- left hair-thin crossings in the
# mesh, so the face is kept simple and every blend is generous. `owl_res`
# and `owl_noise` are there to explore that; 0.4 / 0.35 is the clean pair.

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

body = field.capsule((0, 0, 11), (0, 0, 20), 13.0)
head = field.sphere(10.5).translate((0, 0, 34))
tuft_l = field.capsule((-6, 0, 40), (-9.5, 0.5, 47.0), 3.0)
tuft_r = field.capsule((6, 0, 40), (9.5, 0.5, 47.0), 3.0)
wing_l = field.capsule((-12.5, 2, 25), (-11.0, 4, 7), 3.8)
wing_r = field.capsule((12.5, 2, 25), (11.0, 4, 7), 3.8)
feet = field.capsule((-4.2, -11.0, 2.8), (4.2, -11.0, 2.8), 2.6)
owl = (body.smooth_union(head, 6.0)
           .smooth_union(tuft_l, 3.5).smooth_union(tuft_r, 3.5)
           .smooth_union(wing_l, 3.5).smooth_union(wing_r, 3.5)
           .smooth_union(feet, 4.0))
# Eye sockets and a beak.
for sx in [-5.3, 5.3]:
    owl = owl.smooth_subtract(field.sphere(3.9).translate((sx, -9.9, 35.5)), 2.0)
beak = field.cone(2.4, 0.6, 4.5).rotate(90, axis=(1, 0, 0)).translate((0, -9.3, 31.5))
owl = owl.smooth_union(beak, 1.4)
# A whisper of noise so it reads as sculpted, then a flat bottom at z = 0.
owl = owl.displace(field.noise(scale=6.0, amplitude=1.0, seed=7), strength=param("owl_noise", 0.35, doc="Sculpted-surface noise on the owl"))
owl = owl.intersect(field.half_space(origin=(0, 0, 0.6), normal=(0, 0, -1)))
owl = owl.translate((0, 0, -0.6))
owl_body = mesh(owl, resolution=param("owl_res", 0.4, unit="mm", doc="Mesher cell size for the owl"), simplify=False, name="owl")
export(owl_body, "owl.stl")

# ---- STEP: a pillow block, exact surfaces ----------------------------------

block = pillow_block()
export(block, "pillow_block.step")
report("block_volume", block.volume())

# ---- DXF: a spur-gear outline with a keyed bore and four windows -----------

teeth = 18
r_root = 27.0
r_tip = 32.0
pts = []
for i in range(teeth):
    a0 = 360.0 * i / teeth
    step = 360.0 / teeth
    for (da, r) in [(0.00, r_root), (0.18, r_root), (0.32, r_tip), (0.62, r_tip), (0.76, r_root)]:
        a = a0 + da * step  # Cadpole's math.cos/sin take degrees
        pts.append((r * math.cos(a), r * math.sin(a)))
gear = sketch(XY, name="gear").polygon(pts)
bore2d = sketch(XY, name="gear_bore").circle(6.0).union(sketch(XY, name="key").rect(3.0, 4.0, center=(0, 7.0)), name="keyed_bore")
windows = sketch(XY, name="window").slot(13.0, 7.0, center=(17.0, 0)).pattern2d_circular(4, merge=True, name="windows")
gear_profile = gear.subtract(bore2d, name="gear_bored").subtract(windows, name="gear_profile")
export_dxf(gear_profile, "gear.dxf")
report("gear_area", gear_profile.area())
print("gear profile area: %s mm2" % gear_profile.area())
print("pillow block volume: %s mm3" % block.volume())
