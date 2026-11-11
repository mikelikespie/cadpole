# Bring your existing files: three parts that arrive as files -- a sculpted
# mesh as STL, a CAD part as STEP, a laser-cut profile as DXF -- each read
# back into Cadpole and then worked on with ordinary native operations, side by
# side on labelled plinths.
#
#   * STL -> `import_mesh`: an owl figurine (assets/owl.stl). The importer
#     measures and never mends; `import_report` says what it found (triangles,
#     shells, open and non-manifold edges, crossings, genus, volume), and the
#     script reports those numbers. The owl is a closed solid, so it takes a
#     boolean like any body: it is welded onto a native round base and three
#     bolt holes are drilled through it -- a mesh fused with native geometry.
#   * STEP -> `import_step`: a pillow block (assets/pillow_block.step). It
#     comes back as an exact B-rep -- planes and true cylinders, not
#     triangles -- with the volume the original had, so it can still be
#     filleted and drilled exactly: the bore's mouths are rounded (tori),
#     the drum's crown gets a spot face and a blind oil hole, and the bolt
#     holes' mouths are rounded too.
#   * DXF -> `import_dxf`: a spur-gear profile with a keyed bore and four
#     windows (assets/gear.dxf). Its closed polyline becomes a sketch with
#     its holes, extruded `gear_depth` deep, with a native keyed hub added;
#     it stands in a groove cut in its plinth.
#
# Every file is invented and written by Cadpole itself: tools/import_demo_assets.star
# builds the owl from fields, the block as `pillow_block` below does, and the
# gear from a sketch, and exports them:
#
#   cadpole export tools/import_demo_assets.star --run-exports --out-dir assets
#   cadpole test import_demo.star
#   cadpole render import_demo.star --backend path --samples 256 --size 1600x1200 --view az=-30,el=18 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o import_demo.png

# The pillow block tools/import_demo_assets.star writes as STEP, as a function,
# so the tests can build the native original beside the import.

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

def pillow_block_volume():
    # Exact: the profile's area (foot + pedestal + the drum's upper half,
    # less the bore) times the depth, less the two bolt holes through the foot.
    b = BLOCK
    area = b["w"] * b["t"] + 2 * b["boss_r"] * (b["boss_h"] - b["t"]) + math.pi * b["boss_r"] * b["boss_r"] / 2 - math.pi * b["bore_r"] * b["bore_r"]
    return area * b["d"] - 2 * math.pi * b["bolt_r"] * b["bolt_r"] * b["t"]

show = param("show", "all", choices = ["all", "stl", "step", "dxf"], doc = "Which imported part to show")
base_r = param("base_r", 21.0, unit = "mm", min = 18, max = 30, step = 0.5, doc = "Radius of the native base welded under the imported owl")
base_h = param("base_h", 4.0, unit = "mm", min = 2, max = 10, step = 0.5, doc = "Thickness of the owl's base")
bolt_d = param("bolt_d", 3.4, unit = "mm", min = 2, max = 5, step = 0.1, doc = "Bolt holes drilled through the owl's base")
bore_fillet = param("bore_fillet", 1.5, unit = "mm", min = 0.5, max = 3, step = 0.25, doc = "Fillet added to the imported block's bore mouths")
oil_d = param("oil_d", 3.0, unit = "mm", min = 1.5, max = 5, step = 0.5, doc = "Blind oil hole drilled into the imported block's crown")
gear_depth = param("gear_depth", 8.0, unit = "mm", min = 3, max = 20, step = 0.5, doc = "How deep the imported DXF profile is extruded")
hub_h = param("hub_h", 4.0, unit = "mm", min = 0, max = 10, step = 0.5, doc = "Height of the native hub on the gear (0: none)")
labels = param("labels", True, doc = "Label the plinths with the file each part came from")

SPACING = 82.0
PLINTH_W = 70.0
PLINTH_D = 54.0
PLINTH_H = 16.0
GROOVE = 3.0

COLORS = {"stl": "#d9773f", "step": "#3f78b5", "dxf": "#4e9a5b"}
PLINTH_COLOR = "#e9e5df"

def r3(v):
    return math.round(v * 1000.0) / 1000.0

slots = ["stl", "step", "dxf"] if show == "all" else [show]

def slot_x(kind):
    # Each part keeps its place in the row whatever `show` says, so a
    # single-part view is the same geometry as the full scene.
    return (["stl", "step", "dxf"].index(kind) - 1) * SPACING

# ---- The plinths, each labelled with the format and the file ---------------

def plinth(kind, title, filename, groove = None):
    x = slot_x(kind)
    p = extrude(sketch(XY.offset(-PLINTH_H), name = kind + "_plinth_outline").rounded_rect(PLINTH_W, PLINTH_D, 5.0, center = (x, 0)), PLINTH_H, name = kind + "_plinth")
    if groove != None:
        p = subtract(p, groove, name = kind + "_plinth_grooved", kernel = "brep")
    p.material(preset = preset.pla_matte, color = PLINTH_COLOR)
    if labels:
        # The labels are bodies of their own, raised off the front face, so
        # `cadpole render` (one colour per body) shows them in the part's colour.
        face = XZ.offset(PLINTH_D / 2.0 - 0.1)
        t = extrude(text(title, size = 8.0, align = "center", baseline = "middle", weight = 700, plane = face, at = (x, -PLINTH_H / 2.0 + 2.6), name = kind + "_title"), 0.9, name = kind + "_label")
        for g in (t if type(t) == "list" else [t]):
            g.material(preset = preset.pla_matte, color = COLORS[kind])
        f = extrude(text(filename, size = 4.0, align = "center", baseline = "middle", plane = face, at = (x, -PLINTH_H + 3.6), name = kind + "_file"), 0.35, name = kind + "_filename")
        for g in (f if type(f) == "list" else [f]):
            g.material(preset = preset.pla_matte, color = "#5d5852")
    return p

# ---- STL: the owl, imported and fused with native geometry -----------------

owl_src = None
owl_report = None
owl = None
if "stl" in slots:
    owl_src = import_mesh("owl.stl", name = "owl")
    owl_report = import_report(owl_src)
    print("import_mesh(\"owl.stl\") -> import_report:")
    for k in ["format", "object", "triangles", "vertices", "groups", "shells", "open_edges", "flipped_edges", "non_manifold_edges",
              "crossings", "overlaps", "folds", "degenerate", "welded", "closed", "oriented", "inverted", "solid", "genus", "volume", "area", "bbox", "chord_error"]:
        print("  " + (k + ":" + " " * 20)[:20] + str(getattr(owl_report, k)))
    print("  summary             " + owl_report.summary)
    report("stl_triangles", owl_report.triangles)
    report("stl_shells", owl_report.shells)
    report("stl_open_edges", owl_report.open_edges)
    report("stl_non_manifold_edges", owl_report.non_manifold_edges)
    report("stl_crossings", owl_report.crossings)
    report("stl_solid", owl_report.solid)
    report("stl_genus", owl_report.genus)
    report("stl_volume", owl_report.volume)

    x = slot_x("stl")
    lifted = translate(owl_src, (x, 0, base_h), name = "owl_on_base")
    disc = cylinder(base_r, base_h + 0.6, at = (x, 0, 0), name = "owl_base_disc")
    bolts = []
    for i in range(3):
        a = 90.0 + 120.0 * i
        rr = base_r - bolt_d / 2.0 - 2.2
        bolts.append(cylinder(bolt_d / 2.0, base_h + 2.0, at = (x + rr * math.cos(a), rr * math.sin(a), -1.0), name = "owl_bolt_%d" % i))
    owl = subtract(union(lifted, disc, name = "owl_fused"), bolts, name = "owl_mounted")
    owl.material(preset = preset.pla_matte, color = COLORS["stl"])
    plinth("stl", "STL", "owl.stl")

# ---- STEP: the pillow block, imported exact, filleted and drilled ----------

block_src = None
block = None
if "step" in slots:
    block_src = import_step("pillow_block.step", name = "block")
    report("step_faces", block_src.faces().count())
    report("step_cylinders", block_src.faces(kind = kind.cylinder).count())
    report("step_planes", block_src.faces(kind = kind.plane).count())
    report("step_volume", block_src.volume())
    print("import_step(\"pillow_block.step\") -> %s: %d faces (%d plane, %d cylinder), %d edges, volume %s mm3 (exact %s)" % (
        block_src.kind, block_src.faces().count(), block_src.faces(kind = kind.plane).count(), block_src.faces(kind = kind.cylinder).count(),
        block_src.edges().count(), r3(block_src.volume()), r3(pillow_block_volume())))

    x = slot_x("step")
    moved = translate(block_src, (x, 0, 0), name = "block_placed")
    mouths = moved.edges(min_radius = BLOCK["bore_r"] - 0.01, max_radius = BLOCK["bore_r"] + 0.01)
    rounded = fillet(moved, bore_fillet, edges = mouths, name = "block_rounded")
    # A spot face on the drum's crown for a grease fitting, a blind oil hole
    # down from it, and the bolt holes' mouths rounded -- all exact.
    spot = cylinder(5.0, 5.0, at = (x, 0, BLOCK["boss_h"] + BLOCK["boss_r"] - 1.5), name = "spot_face")
    faced = subtract(rounded, spot, name = "block_spotfaced", kernel = "brep")
    oil = cylinder(oil_d / 2.0, 6.0, at = (x, 0, BLOCK["boss_h"] + BLOCK["bore_r"] + 2.5), name = "oil_hole")
    drilled = subtract(faced, oil, name = "block_drilled", kernel = "brep")
    block = fillet(drilled, 0.8, edges = drilled.edges(min_radius = BLOCK["bolt_r"] - 0.01, max_radius = BLOCK["bolt_r"] + 0.01, above = XY.offset(BLOCK["t"] - 0.5)), name = "block_finished")
    block.material(preset = preset.aluminum, color = COLORS["step"], metallic = 0.6, roughness = 0.35)
    plinth("step", "STEP", "pillow_block.step")

# ---- DXF: the gear profile, imported, extruded, hubbed, stood upright ------

gear_dxf = None
gear = None
hub_profile = None
if "dxf" in slots:
    gear_dxf = import_dxf("gear.dxf", name = "gear_dxf")
    profile = gear_dxf.sketch(layer = "gear_profile", closed = True)
    report("dxf_area", profile.area())
    print("import_dxf(\"gear.dxf\") -> layers %s; sketch of %d loops, area %s mm2, bbox %s" % (
        [l[0] for l in gear_dxf.layers()], len(profile.points()), r3(profile.area()), gear_dxf.bbox()))
    flat = extrude(profile, gear_depth, name = "gear_plate")
    if hub_h > 0:
        # The hub overlaps the plate by 0.5 mm and its keyed bore is a
        # touch larger than the plate's, so the union is a clean crossing.
        hp = XY.offset(gear_depth - 0.5)
        hub_ring = sketch(hp, name = "hub_ring").circle(9.5)
        hub_hole = sketch(hp, name = "hub_hole").circle(6.4).union(sketch(hp, name = "hub_key").rect(3.6, 4.0, center = (0, 7.4)), name = "hub_keyed")
        hub_profile = hub_ring.subtract(hub_hole, name = "hub_profile")
        hub = extrude(hub_profile, hub_h + 0.5, name = "hub")
        flat = union(flat, hub, name = "gear_hubbed", kernel = "brep")
    # Stand it up: the gear's +z (the hub side) turns to face the camera (-y).
    upright = rotate(flat, 90, axis = (1, 0, 0), name = "gear_upright")
    lo, hi = upright.bbox()
    x = slot_x("dxf")
    gear = translate(upright, (x, -(lo[1] + hi[1]) / 2.0, -lo[2] - GROOVE), name = "gear")
    gear.material(preset = preset.petg_glossy, color = COLORS["dxf"])
    groove = box(52.0, gear_depth + 0.8, GROOVE + 1.0, at = (x - 26.0, -(gear_depth + 0.8) / 2.0, -GROOVE), name = "gear_groove")
    plinth("dxf", "DXF", "gear.dxf", groove = groove)

# ---- Tests -----------------------------------------------------------------

def mesh_import_is_watertight():
    if owl_report == None:
        return
    r = owl_report
    assert_eq(r.format, "STL")
    assert_true(r.closed, msg = "the STL is closed")
    assert_true(r.oriented, msg = "every edge is wound consistently")
    assert_true(r.solid, msg = "the STL is a solid")
    assert_eq(r.shells, 1)
    assert_eq(r.open_edges, 0)
    assert_eq(r.non_manifold_edges, 0)
    assert_eq(r.crossings, 0)
    assert_eq(r.genus, 0)
    assert_solid(owl_src)
    assert_close(owl_src.volume(), r.volume, rel = 1e-6)

test("the STL comes back watertight: one closed, oriented shell", mesh_import_is_watertight)

def mesh_takes_native_booleans():
    if owl == None:
        return 0.0
    assert_solid(owl)
    # Base and owl fused: more than the owl, less than owl + full disc.
    disc_v = math.pi * base_r * base_r * (base_h + 0.6)
    holes_v = 3 * math.pi * (bolt_d / 2.0) * (bolt_d / 2.0) * base_h
    assert_true(owl.volume() > owl_report.volume + disc_v - holes_v - 0.6 * math.pi * 16 * 16 - 1.0, msg = "the base is fused on")
    assert_true(owl.volume() < owl_report.volume + disc_v, msg = "the bolt holes are drilled")
    return owl.volume()

test("the imported owl takes a native base and bolt holes", mesh_takes_native_booleans, sweep = {"base_r": [19.0, 21.0, 24.0, 28.0]}, monotone = "increasing")

def step_round_trip_keeps_volume():
    if block_src == None:
        return
    assert_eq(block_src.kind, "brep")
    assert_solid(block_src)
    native = pillow_block(name = "native_block")
    assert_close(block_src.volume(), native.volume(), rel = 1e-4)
    assert_close(block_src.volume(), pillow_block_volume(), rel = 0.005)
    assert_eq(block_src.faces().count(), native.faces().count())
    assert_eq(block_src.faces(kind = kind.cylinder).count(), native.faces(kind = kind.cylinder).count())
    assert_eq(block_src.faces(kind = kind.plane).count(), native.faces(kind = kind.plane).count())
    assert_true(block_src.faces(kind = kind.cylinder).count() >= 4, msg = "bore, drum and bolt holes stay true cylinders")

test("the STEP round trip keeps the volume and every face's kind", step_round_trip_keeps_volume)

def step_part_still_edits_exactly():
    if block == None:
        return 0.0
    assert_eq(block.kind, "brep")
    assert_solid(block)
    assert_true(block.faces(kind = kind.torus).count() > 0, msg = "the bore fillets are tori")
    return block.volume()

test("the imported block fillets and drills exactly", step_part_still_edits_exactly, sweep = {"bore_fillet": [0.5, 1.0, 1.5, 2.5]}, monotone = "decreasing")

def dxf_extrudes_with_its_holes():
    if gear == None:
        return 0.0
    assert_solid(gear)
    area = gear_dxf.sketch(layer = "gear_profile", closed = True).area()
    # Outline, keyed bore and four windows: six loops, one region.
    assert_eq(len(gear_dxf.sketch(layer = "gear_profile", closed = True).points()), 6)
    # The area import_demo_assets.star measured on the native sketch it exported:
    # the slots' arcs travel as polyline bulges, so the import is exact.
    assert_close(area, 2239.800192230167, rel = 1e-9)
    hub_v = 0.0
    if hub_h > 0:
        hub_v = hub_profile.area() * hub_h
    assert_close(gear.volume(), area * gear_depth + hub_v, rel = 0.01)
    return gear.volume()

test("the DXF profile extrudes with its bore and windows", dxf_extrudes_with_its_holes, sweep = {"gear_depth": [4.0, 8.0, 12.0, 16.0]}, monotone = "increasing")

def every_view_builds():
    shown = [b for b in [owl, block, gear] if b != None]
    assert_eq(len(shown), 3 if show == "all" else 1)
    for b in shown:
        assert_solid(b)

test("each `show` choice builds its parts as solids", every_view_builds, sweep = {"show": ["all", "stl", "step", "dxf"]})
