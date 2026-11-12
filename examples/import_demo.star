# Bring your existing files: three parts that arrive as files -- a sculpted
# mesh as STL, a CAD part as STEP, a laser-cut profile as DXF -- each read
# back into Cadpole and then worked on with ordinary native operations, side by
# side on labelled plinths. Each plinth's caption is what the importer found.
#
#   * STL -> `import_mesh`: an owl figurine (assets/owl.stl). The importer
#     measures and never mends; `import_report` says what it found (triangles,
#     shells, open and non-manifold edges, crossings, folds, genus, volume),
#     and the script reports those numbers. The owl is a closed solid, so it
#     takes a boolean like any body: it is welded onto a native round base and
#     three bolt holes are drilled through it -- a mesh fused with native
#     geometry.
#   * STEP -> `import_step`: a pillow block (assets/pillow_block.step). It
#     comes back as an exact B-rep -- planes and true cylinders, not
#     triangles -- with the volume the original had, so it can still be
#     filleted and drilled exactly: the bore's mouths are rounded (tori),
#     the drum's crown gets a spot face and a blind oil hole with a native
#     grease nipple seated in it, and the bolt holes' mouths are rounded too.
#   * DXF -> `import_dxf`: an involute spur-gear profile with a keyed bore
#     and six windows (assets/gear.dxf). Its closed outline becomes a sketch
#     with its holes, extruded `gear_depth` deep, with a native hub added and
#     one keyed bore cut through both; it stands in a cradle cut to its tip
#     circle.
#
# Every file is invented and written by Cadpole itself: tools/import_demo_assets.star
# builds the owl from fields, the block as `pillow_block` below does, and the
# gear from the gear library's involute outline, and exports them:
#
#   cadpole export tools/import_demo_assets.star --run-exports --out-dir assets
#   cadpole test import_demo.star
#   cadpole render import_demo.star --backend path --samples 256 --size 1600x1200 --view az=-35,el=30 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o import_demo.png

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
base_r = param("base_r", 21.0, unit = "mm", min = 18, max = 25, step = 0.5, doc = "Radius of the native base welded under the imported owl")
base_h = param("base_h", 4.0, unit = "mm", min = 2, max = 10, step = 0.5, doc = "Thickness of the owl's base")
bolt_d = param("bolt_d", 3.4, unit = "mm", min = 2, max = 5, step = 0.1, doc = "Bolt holes drilled through the owl's base")
bore_fillet = param("bore_fillet", 1.5, unit = "mm", min = 0.5, max = 3, step = 0.25, doc = "Fillet added to the imported block's bore mouths")
oil_d = param("oil_d", 3.0, unit = "mm", min = 1.5, max = 5, step = 0.5, doc = "Blind oil hole drilled into the imported block's crown")
gear_depth = param("gear_depth", 8.0, unit = "mm", min = 3, max = 20, step = 0.5, doc = "How deep the imported DXF profile is extruded")
hub_h = param("hub_h", 4.0, unit = "mm", min = 0, max = 10, step = 0.5, doc = "Height of the native hub on the gear (0: none)")
labels = param("labels", True, doc = "Label the plinths with the file each part came from")

SPACING = 76.0
PLINTH_W = 68.0
PLINTH_D = 56.0
PLINTH_H = 12.0
PLINTH_EDGE = 1.5      # the round on each plinth's top edge
GROOVE = 3.0           # how deep the gear sinks into its cradle
CRADLE_GAP = 0.3       # radial clearance between the gear's tips and its cradle

# The gear the DXF carries (tools/import_demo_assets.star): module 2.5, 24 teeth.
GEAR_R_TIP = 32.5
# The bore and key the plate's DXF carries, cut once more through plate and hub.
BORE_R = 6.0
KEY_W = 3.0
KEY_TOP = 9.0
HUB_R = 12.5

# Tones chosen for a dark page: a warm clay owl, a bead-blasted aluminium
# block, a sea-green nylon gear, on slate plinths that stay quieter than
# the parts.
COLORS = {"stl": "#d08c5f", "step": "#c4cacd", "dxf": "#4fbf95"}
LABEL_COLORS = {"stl": "#e6a273", "step": "#dde3e6", "dxf": "#6fd8ad"}
PLINTH_COLOR = "#59625f"
CAPTION_COLOR = "#b9c4c0"

def r3(v):
    return math.round(v * 1000.0) / 1000.0

def grouped(n):
    # 114968 -> "114,968", for the captions.
    s = str(n)
    out = ""
    for i in range(len(s)):
        if i > 0 and (len(s) - i) % 3 == 0:
            out += ","
        out += s[i]
    return out

slots = ["stl", "step", "dxf"] if show == "all" else [show]

def slot_x(kind):
    # Each part keeps its place in the row whatever `show` says, so a
    # single-part view is the same geometry as the full scene.
    return (["stl", "step", "dxf"].index(kind) - 1) * SPACING

# ---- The plinths, each labelled with the format, the file and what the -----
# ---- importer found in it ---------------------------------------------------

plinths = {}

def plinth(kind, title, caption, cut = None):
    x = slot_x(kind)
    p = extrude(sketch(XY.offset(-PLINTH_H), name = kind + "_plinth_outline").rounded_rect(PLINTH_W, PLINTH_D, 6.0, center = (x, 0)), PLINTH_H, name = kind + "_plinth_block")
    p = fillet(p, PLINTH_EDGE, edges = p.edges(on_plane = XY), name = kind + "_plinth_edged")
    if cut != None:
        p = subtract(p, cut, name = kind + "_plinth_cut", kernel = "brep")
    p.material(preset = preset.pla_matte, color = PLINTH_COLOR, roughness = 0.75)
    plinths[kind] = p
    if labels:
        # The labels are bodies of their own, raised off the front face, so
        # `cadpole render` (one colour per body) shows them in their own colour.
        face = XZ.offset(PLINTH_D / 2.0 - 0.1)
        t = extrude(text(title, size = 5.5, align = "center", baseline = "middle", weight = 700, plane = face, at = (x, -4.2), name = kind + "_title"), 0.6, name = kind + "_label")
        for g in (t if type(t) == "list" else [t]):
            g.material(preset = preset.pla_matte, color = LABEL_COLORS[kind])
        f = extrude(text(caption, size = 2.9, align = "center", baseline = "middle", plane = face, at = (x, -9.0), name = kind + "_caption"), 0.3, name = kind + "_filename")
        for g in (f if type(f) == "list" else [f]):
            g.material(preset = preset.pla_matte, color = CAPTION_COLOR)
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
    report("stl_folds", owl_report.folds)
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
    owl.material(preset = preset.pla_matte, color = COLORS["stl"], roughness = 0.6)
    plinth("stl", "STL", "owl.stl  -  %s triangles, %s" % (
        grouped(owl_report.triangles), "watertight" if owl_report.solid else "open"))

# ---- STEP: the pillow block, imported exact, filleted and drilled ----------

block_src = None
block = None
nipple = None
if "step" in slots:
    block_src = import_step("pillow_block.step", name = "block")
    report("step_faces", block_src.faces().count())
    report("step_cylinders", block_src.faces(kind = kind.cylinder).count())
    report("step_planes", block_src.faces(kind = kind.plane).count())
    report("step_volume", block_src.volume())
    print("import_step(\"pillow_block.step\") -> %s: %d faces (%d plane, %d cylinder), %d edges, volume %s mm3" % (
        block_src.kind, block_src.faces().count(), block_src.faces(kind = kind.plane).count(), block_src.faces(kind = kind.cylinder).count(),
        block_src.edges().count(), r3(block_src.volume())))

    x = slot_x("step")
    t = BLOCK["t"]
    moved = translate(block_src, (x, 0, 0), name = "block_placed")
    mouths = moved.edges(min_radius = BLOCK["bore_r"] - 0.01, max_radius = BLOCK["bore_r"] + 0.01)
    rounded = fillet(moved, bore_fillet, edges = mouths, name = "block_rounded")
    # A spot face on the drum's crown for a grease fitting, a blind oil hole
    # down from it, and the bolt holes' mouths rounded -- all exact.
    crown = BLOCK["boss_h"] + BLOCK["boss_r"]
    spot = cylinder(5.0, 5.0, at = (x, 0, crown - 1.5), name = "spot_face")
    faced = subtract(rounded, spot, name = "block_spotfaced", kernel = "brep")
    oil = cylinder(oil_d / 2.0, 6.0, at = (x, 0, BLOCK["boss_h"] + BLOCK["bore_r"] + 2.5), name = "oil_hole")
    drilled = subtract(faced, oil, name = "block_drilled", kernel = "brep")
    block = fillet(drilled, 0.8, edges = drilled.edges(min_radius = BLOCK["bolt_r"] - 0.01, max_radius = BLOCK["bolt_r"] + 0.01, above = XY.offset(t - 0.5)), name = "block_finished")
    block.material(preset = preset.aluminum, color = COLORS["step"], metallic = 0.85, roughness = 0.42)
    # A native grease nipple in brass, seated on the spot face with its shank
    # in the oil hole: a hex, a neck and a ball head.
    floor = crown - 1.5
    hexagon = sketch(XY.offset(floor), name = "nipple_hex_outline").polygon([(x + 4.0 * math.cos(30.0 + 60.0 * i), 4.0 * math.sin(30.0 + 60.0 * i)) for i in range(6)])
    nipple = union([
        extrude(hexagon, 3.5, name = "nipple_hex"),
        cylinder(oil_d / 2.0 - 0.15, 3.0, at = (x, 0, floor - 2.9), name = "nipple_shank"),
        cylinder(1.5, 2.6, at = (x, 0, floor + 3.4), name = "nipple_neck"),
        sphere(2.3, center = (x, 0, floor + 6.6), name = "nipple_head"),
    ], name = "grease_nipple")
    nipple.material(preset = preset.brass, roughness = 0.3)
    plinth("step", "STEP", "pillow_block.step  -  %d faces, exact B-rep" % block_src.faces().count())

# ---- DXF: the gear profile, imported, extruded, hubbed, stood upright ------

gear_dxf = None
gear = None
cradle = None
if "dxf" in slots:
    gear_dxf = import_dxf("gear.dxf", name = "gear_dxf")
    profile = gear_dxf.sketch(layer = "gear_profile", closed = True)
    report("dxf_area", profile.area())
    print("import_dxf(\"gear.dxf\") -> layers %s; sketch of %d loops, area %s mm2, bbox %s" % (
        [l[0] for l in gear_dxf.layers()], len(profile.points()), r3(profile.area()), gear_dxf.bbox()))
    flat = extrude(profile, gear_depth, name = "gear_plate")
    # The mesh kernel below freezes its operands' triangles, so give them a
    # fine chord now: the windows and bore stay round in any view.
    flat.tessellation(tolerance = 0.02)
    if hub_h > 0:
        # A native hub on the plate's front face, overlapping it by 0.5 mm,
        # its rim chamfered; then one keyed bore, the DXF's own size, cut
        # through plate and hub together so the two keyways are one. Both
        # run on the mesh kernel: the exact kernel does not yet join a solid
        # to an outline of several hundred short flank segments.
        hub = cylinder(HUB_R, hub_h + 0.5, at = (0, 0, gear_depth - 0.5), name = "hub")
        hub = chamfer(hub, 0.8, edges = hub.edges(on_plane = XY.offset(gear_depth + hub_h)), name = "hub_chamfered")
        hub.tessellation(tolerance = 0.02)
        flat = union(flat, hub, name = "gear_hubbed", kernel = "mesh")
        span = gear_depth + hub_h + 2.0
        keyed = sketch(XY.offset(-1.0), name = "bore_through").circle(BORE_R).union(
            sketch(XY.offset(-1.0), name = "key_through").rect(KEY_W, KEY_TOP - BORE_R + 1.0, center = (0, (KEY_TOP + BORE_R - 1.0) / 2.0)), name = "bore_keyed")
        cut = extrude(keyed, span, name = "bore_cut")
        cut.tessellation(tolerance = 0.02)
        flat = subtract(flat, cut, name = "gear_bored", kernel = "mesh")
    # Stand it up: the gear's +z (the hub side) turns to face the camera (-y),
    # the plate centred on y = 0 over its cradle.
    upright = rotate(flat, 90, axis = (1, 0, 0), name = "gear_upright")
    x = slot_x("dxf")
    cz = GEAR_R_TIP + CRADLE_GAP - GROOVE
    gear = translate(upright, (x, gear_depth / 2.0, cz), name = "gear")
    gear.material(preset = preset.petg_glossy, color = COLORS["dxf"], roughness = 0.38)
    # The cradle: an arc of the tip circle, CRADLE_GAP larger, cut GROOVE deep
    # and a little wider than the plate, so the gear sits in a matching seat.
    seat = rotate(cylinder(GEAR_R_TIP + CRADLE_GAP, gear_depth + 0.8, name = "gear_seat"), -90, axis = (1, 0, 0), name = "gear_seat_turned")
    cradle = translate(seat, (x, -(gear_depth + 0.8) / 2.0, cz), name = "gear_cradle")
    plinth("dxf", "DXF", "gear.dxf  -  %d loops, one profile" % len(profile.points()), cut = cradle)

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
    assert_eq(r.overlaps, 0)
    assert_eq(r.folds, 0, msg = "no doubled-back sheets anywhere on the sculpt")
    assert_eq(r.genus, 0)
    assert_solid(owl_src)
    assert_close(owl_src.volume(), r.volume, rel = 1e-6)

test("the STL comes back watertight: one closed, oriented shell, no folds", mesh_import_is_watertight)

def owl_reads_as_an_owl():
    if owl_src == None:
        return
    lo, hi = owl_src.bbox()
    # A perched figure about 50 mm tall, wider across than deep, standing on
    # its flat bottom at z = 0 with its ear tufts the highest point.
    assert_close(lo[2], 0.0, tol = 0.05)
    assert_true(hi[2] > 48.0 and hi[2] < 56.0, msg = "about 50 mm tall, tufts included")
    assert_true(hi[0] - lo[0] > hi[1] - lo[1] - 2.0, msg = "the wings make it about as wide as it is deep")
    # The face is on -y: the beak and eye discs stand further forward than the breast.
    assert_true(-lo[1] > hi[1], msg = "the face, beak and feet are on the -y side")

test("the sculpt is an upright owl facing -y", owl_reads_as_an_owl)

def mesh_takes_native_booleans():
    if owl == None:
        return 0.0
    assert_solid(owl)
    # Base and owl fused: more than the owl, less than owl + full disc.
    disc_v = math.pi * base_r * base_r * (base_h + 0.6)
    holes_v = 3 * math.pi * (bolt_d / 2.0) * (bolt_d / 2.0) * base_h
    assert_true(owl.volume() > owl_report.volume + disc_v - holes_v - 0.6 * math.pi * 16 * 16 - 1.0, msg = "the base is fused on")
    assert_true(owl.volume() < owl_report.volume + disc_v, msg = "the bolt holes are drilled")
    # The base stays on the plinth's top, inside its chamfered edge.
    lo, hi = owl.bbox()
    assert_true(hi[1] - lo[1] <= PLINTH_D - 2 * PLINTH_EDGE, msg = "the base fits on the plinth")
    return owl.volume()

test("the imported owl takes a native base and bolt holes", mesh_takes_native_booleans, sweep = {"base_r": [18.0, 21.0, 23.0, 25.0]}, monotone = "increasing")

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
    assert_true(block.volume() < block_src.volume(), msg = "the edits only remove material: fillets, spot face, oil hole")
    return block.volume()

test("the imported block fillets and drills exactly", step_part_still_edits_exactly, sweep = {"bore_fillet": [0.5, 1.0, 1.5, 2.5]}, monotone = "decreasing")

def dxf_extrudes_with_its_holes():
    if gear == None:
        return 0.0
    assert_solid(gear)
    sk = gear_dxf.sketch(layer = "gear_profile", closed = True)
    area = sk.area()
    # Outline, keyed bore and six windows: eight loops, one region.
    assert_eq(len(sk.points()), 8)
    # The area import_demo_assets.star measured on the native sketch it exported.
    assert_close(area, 2366.779062984984, rel = 1e-6)
    hub_v = 0.0
    if hub_h > 0:
        # The hub's ring less the keyed bore, plus the 0.5 mm it overlaps the
        # plate, which the union does not count twice.
        bore_a = math.pi * BORE_R * BORE_R + KEY_W * (KEY_TOP - BORE_R)
        hub_v = (math.pi * HUB_R * HUB_R - bore_a) * hub_h
    assert_close(gear.volume(), area * gear_depth + hub_v, rel = 0.01)
    return gear.volume()

test("the DXF profile extrudes with its bore and windows", dxf_extrudes_with_its_holes, sweep = {"gear_depth": [4.0, 8.0, 12.0, 16.0]}, monotone = "increasing")

def gear_hub_has_one_keyway():
    if gear == None or hub_h <= 0:
        return
    # The key's flat top is KEY_TOP from the axis and the hub's rim HUB_R:
    # at least three millimetres of wall, not a sliver.
    assert_true(HUB_R - KEY_TOP >= 3.0, msg = "a full wall over the keyway")
    # A 12 mm shaft and its key, a little under size, slide through plate and
    # hub without touching either: one bore and one keyway, the same in both,
    # with no ledge or step between them.
    lo, hi = gear.bbox()
    x = slot_x("dxf")
    cz = GEAR_R_TIP + CRADLE_GAP - GROOVE
    length = gear_depth + hub_h + 4.0
    y0 = gear_depth / 2.0 + 2.0
    shaft = translate(rotate(cylinder(BORE_R - 0.2, length, name = "probe_shaft"), 90, axis = (1, 0, 0), name = "probe_shaft_turned"), (x, y0, cz), name = "probe_shaft_placed")
    key = box(KEY_W - 0.2, length, KEY_TOP - BORE_R + 0.6, at = (x - (KEY_W - 0.2) / 2.0, y0 - length, cz + BORE_R - 0.8), name = "probe_key")
    for what, probe in [("shaft", shaft), ("key", key)]:
        c = distance(probe, gear)
        assert_true(not c.overlap and c.distance > 0.02, msg = "the %s slides through the bore: %s" % (what, c))

test("the hub carries the plate's keyway with a full wall over it", gear_hub_has_one_keyway, sweep = {"hub_h": [2.0, 4.0, 8.0]})

def gear_sits_in_its_cradle():
    if gear == None:
        return
    p = plinths["dxf"]
    assert_solid(p)
    # No interference: the gear clears its cradle by about CRADLE_GAP.
    c = distance(gear, p)
    assert_true(not c.overlap, msg = "the gear does not cut into its plinth: %s" % c)
    assert_true(c.distance > 0.05 and c.distance < CRADLE_GAP + 0.2, msg = "the gear rests just clear of its seat: %s" % c)
    lo, hi = gear.bbox()
    # It sinks into the cradle, below the plinth's top...
    assert_true(lo[2] < -GROOVE + 0.5, msg = "the gear sits down in its cradle")
    # ...and the cradle is no longer than the arc the gear fills.
    clo, chi = cradle.bbox()
    chord = 2.0 * math.sqrt((GEAR_R_TIP + CRADLE_GAP) * (GEAR_R_TIP + CRADLE_GAP) - (GEAR_R_TIP + CRADLE_GAP - GROOVE) * (GEAR_R_TIP + CRADLE_GAP - GROOVE))
    assert_true(chord < 32.0, msg = "the seat is a short arc, not a long slot")
    # The gear stands within the plinth's footprint.
    assert_true(hi[0] - lo[0] < PLINTH_W, msg = "the gear is narrower than its plinth")

test("the gear stands in a matching cradle with no interference", gear_sits_in_its_cradle, sweep = {"gear_depth": [4.0, 8.0, 16.0]})

def every_view_builds():
    shown = [b for b in [owl, block, gear] if b != None]
    assert_eq(len(shown), 3 if show == "all" else 1)
    for b in shown:
        assert_solid(b)

test("each `show` choice builds its parts as solids", every_view_builds, sweep = {"show": ["all", "stl", "step", "dxf"]})

def parts_rest_on_their_plinths():
    # Each part stands on (or, the gear, in) its plinth and clear of the
    # next one: nothing floats and nothing overlaps a neighbour.
    for k, b in [("stl", owl), ("step", block)]:
        if b == None:
            continue
        lo, hi = b.bbox()
        assert_close(lo[2], 0.0, tol = 0.05)
        assert_true(lo[0] > slot_x(k) - PLINTH_W / 2.0 and hi[0] < slot_x(k) + PLINTH_W / 2.0, msg = k + " stands within its plinth")
        c = distance(b, plinths[k])
        assert_true(not c.overlap and c.distance < 0.05, msg = k + " sits on its plinth without sinking into it: %s" % c)
    if nipple != None:
        # The grease nipple's hex sits on the spot face and its shank stands
        # clear inside the oil hole.
        c = distance(nipple, block)
        assert_true(not c.overlap and c.distance < 0.05, msg = "the grease nipple is seated, not buried: %s" % c)
    assert_true(SPACING > PLINTH_W, msg = "a gap between the plinths")

test("every part rests on its own plinth, inside its footprint", parts_rest_on_their_plinths)
