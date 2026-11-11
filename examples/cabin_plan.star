# A two-bedroom cabin, written once and read three ways: a furnished
# floor plan with door swings, windows, dimensions and room areas; a
# section and an elevation; and the 3D model the same numbers build. The
# cabin is invented.
#
# Plan lengths are feet (u east, v north, inside faces of the rooms);
# the model is millimetres, and ft() turns one into the other.
#
# The kitchen, the bath and the two bedrooms are laid out by the room
# stage, `furnish()` in @cadpole/plan_furnish.star: a small MILP that
# packs a set of the catalog's pieces (@cadpole/plan_furniture.star)
# against the walls, keeps door swings, window zones and every piece's
# clear floor free, holds the kitchen's work triangle to NKBA's legs and
# walks to every use point from a door. Its answer is written out below
# (SOLVED), so the script evaluates without a solver -- the browser's
# engine has none -- and a test solves the four rooms again and compares.
# The living room and the breakfast table are placed by hand, and tests
# hold them to the same clearances.
#
# The 3D model is cut like a plan: walls, glass and tall cabinets above
# `plan_cut` are on the `above_cut` layer and the roof on `roof`, so
# switching both off shows the rooms from above.

load("@cadpole/plan.star", "area", "door", "floor", "ftin", "plan", "room", "room_local", "sqft", "validate", "window", "fixture")
load("@cadpole/plan_draw.star", "draw_floor")
load("@cadpole/plan_furnish.star", "furnish")
load("@cadpole/building.star", "deck", "ft", "gable_roof", "inch", "pitch", "stair")

units(units.ft_in)

P = 4 / 12    # a 4 in partition
EXT = 0.5     # a 6 in outside wall
KIND = {"Living": "living", "Kitchen": "kitchen", "Entry": "entry", "Bath": "bath", "Deck": "deck"}

def rm(key, name, rect, furnish = None):
    return room(key, name, floor = "L1", kind = KIND.get(name, "bed"), rect = rect, furnish = furnish)

# snippet
w = param("living_w", 14, unit="ft", min=12, max=20)
x0 = w + P             # east rooms start here
x1 = x0 + 14 + 8 / 12
xb, n = x0 + 5.5, 13.5 + P
TOP = 25.0             # the north wall's inside face
ROOMS = [
    rm("living", "Living", (0, w, 0, 13.5)),
    rm("kitchen", "Kitchen", (0, w, n, TOP), furnish = "kitchen_L"),
    rm("entry", "Entry", (x0, x1, 9.5 + P, 13.5)),
    rm("bed1", "Bedroom 1", (x0, x1, 0, 9.5), furnish = "full"),
    rm("bath", "Bath", (x0, xb, n, TOP), furnish = "tub"),
    rm("bed2", "Bedroom 2", (xb + P, x1, n, TOP), furnish = "full"),
]
# end snippet

glass_opacity = param("glass_opacity", 0.35, min = 0.1, max = 1, doc = "How see-through the window panes are")
roof_lift = param("roof_lift", 0.0, unit = "ft", min = 0, max = 16, step = 1, doc = "Raise the roof off the walls to look inside")
plan_cut = param("plan_cut", 4.0, unit = "ft", min = 3, max = 7.5, step = 0.5, doc = "Where the plan cuts the walls: everything above it is on the above_cut layer")

DECK = (1.0, w - 1.0, -8.5, -EXT)
ROOMS.append(rm("deck", "Deck", DECK))

DOORS = [
    door("front", a = "entry", b = "outside:street", wall = "E", at = (x1, 11.67), width = 3.0, swing = "a", hinge = "lo"),
    door("hall", a = "entry", b = "living", wall = "W", at = (x0, 11.67), width = 3.0, swing = "open"),
    door("bed1", a = "bed1", b = "entry", wall = "N", at = (x0 + 2.0, 9.5), width = 2.67, swing = "a", hinge = "lo"),
    door("bath", a = "bath", b = "entry", wall = "S", at = (x0 + 3.75, n), width = 2.33, swing = "a", hinge = "hi"),
    door("bed2", a = "bed2", b = "entry", wall = "S", at = (x0 + 7.75, n), width = 2.67, swing = "a", hinge = "lo"),
    door("cased", a = "living", b = "kitchen", wall = "N", at = (w / 2.0, 13.5), width = 6.0, swing = "open"),
    door("slider", a = "living", b = "outside:deck", wall = "S", at = (w / 2.0, 0.0), width = 6.0, swing = "slide"),
]

WINDOWS = [
    window("living_w", room = "living", wall = "W", at = (0.0, 6.75), width = 5.0, sill = 3.0, head = 6.67),
    window("kitchen_w", room = "kitchen", wall = "W", at = (0.0, 19.0), width = 3.0, sill = 3.5, head = 6.67),
    window("kitchen_n", room = "kitchen", wall = "N", at = (w / 2.0, TOP), width = 4.0, sill = 3.5, head = 6.67, source = "rear"),
    window("bath_n", room = "bath", wall = "N", at = (x0 + 2.75, TOP), width = 2.0, sill = 4.0, head = 6.67, source = "rear"),
    window("bed2_n", room = "bed2", wall = "N", at = (x1 - 4.375, TOP), width = 4.0, sill = 2.5, head = 6.67, source = "rear", egress = True),
    window("bed1_s", room = "bed1", wall = "S", at = (x0 + 8.5, 0.0), width = 4.0, sill = 2.5, head = 6.67, source = "street", egress = True),
    window("bed1_e", room = "bed1", wall = "E", at = (x1, 4.75), width = 3.0, sill = 2.5, head = 6.67),
    window("bed2_e", room = "bed2", wall = "E", at = (x1, 19.0), width = 3.0, sill = 2.5, head = 6.67),
]

# ---- What the room stage placed: furnish(room_local(CABIN, key), set,
# wet = "any", keep_fixtures = False, without = ...), keyed as its records
# are (room.item.piece), in plan feet. The kitchen runs are NKBA's: a sink
# run (24 in of landing, the sink base, the dishwasher), a range between
# two 15 in landings, a counter-depth fridge with its 15 in landing.
D25 = 25 / 12  # a 25 in deep kitchen run
SOLVED = {
    "kitchen": {
        "kitchen.sinkrun.sink run": (4.25, 11.25, TOP - D25, TOP),
        "kitchen.rangerun.range run": (0.0, D25, n + 2 / 12, n + 2 / 12 + 5.0),
        "kitchen.fridge.fridge": (1.25, 4.25, TOP - 2.5, TOP),
        "kitchen.fridge.landing": (0.0, 1.25, TOP - D25, TOP),
    },
    "bath": {
        "bath.tub.tub": (x0, x0 + 2.5, n + 5.5, n + 10.5),
        "bath.wc.wc": (x0, x0 + 28 / 12, n, n + 2.5),
        "bath.lav.lav": (x0, x0 + 1.75, n + 2.5, n + 5.5),
    },
    "bed1": {
        "bed1.bed.bed": (x1 - 4.5, x1, 3.0, 9.5),
        "bed1.bed.nightstand": (x1 - 4.5 - 22 / 12, x1 - 4.5, 8.0, 9.5),
        "bed1.desk.desk": (x0 + 3.4183, x0 + 7.4183, 7.5, 9.5),
        "bed1.dresser.dresser": (x0, x0 + 1.5, 2.33, 5.33),
        "bed1.closet.closet": (x0 + 1.5, x0 + 6.5, 0.0, 2.0),
    },
    "bed2": {
        "bed2.bed.bed": (x1 - 4.5, x1, TOP - 6.5, TOP),
        "bed2.bed.nightstand": (x1 - 4.5 - 22 / 12, x1 - 4.5, TOP - 1.5, TOP),
        "bed2.dresser.dresser": (xb + P, xb + P + 1.5, n + 4.17, n + 7.17),
        "bed2.closet.closet": (x0 + 9.1683, x0 + 14.1683, n, n + 2.0),
    },
}
WITHOUT = {"bed2": ("desk",)}  # the small bedroom: bed, dresser and closet

# ---- Every piece, as plan records. `fixtures` get the plan's symbols
# (bowl, basin, tub, burners); `furniture` an outline and the details
# drawn below. h is the height in feet.
def pc(key, room, kind, rect, h, front, fix = False, solved = True):
    return {"key": key, "room": room, "kind": kind, "rect": rect, "h": h, "front": front, "fix": fix, "solved": solved}

K = SOLVED["kitchen"]
sr, rr, fr, ld = K["kitchen.sinkrun.sink run"], K["kitchen.rangerun.range run"], K["kitchen.fridge.fridge"], K["kitchen.fridge.landing"]
B1, B2, BA = SOLVED["bed1"], SOLVED["bed2"], SOLVED["bath"]
cm = (3.5 + w - 5.33) / 2.0  # the coffee table, midway between the sofa and the chairs
PIECES = [
    # kitchen: the runs split into what they hold
    pc("landing", "kitchen", "counter", ld, 3.0, "S"),
    pc("fridge", "kitchen", "fridge", fr, 70 / 12, "S"),
    pc("sink_l", "kitchen", "counter", (sr[0], sr[0] + 2.0, sr[2], sr[3]), 3.0, "S"),
    pc("sink", "kitchen", "sink", (sr[0] + 2.0, sr[0] + 5.0, sr[2], sr[3]), 3.0, "S", fix = True),
    pc("dw", "kitchen", "dishwasher", (sr[0] + 5.0, sr[1], sr[2], sr[3]), 3.0, "S"),
    pc("range_s", "kitchen", "counter", (rr[0], rr[1], rr[2], rr[2] + 1.25), 3.0, "E"),
    pc("range", "kitchen", "range", (rr[0], rr[1], rr[2] + 1.25, rr[2] + 3.75), 3.0, "E", fix = True),
    pc("range_n", "kitchen", "counter", (rr[0], rr[1], rr[2] + 3.75, rr[3]), 3.0, "E"),
    pc("table", "kitchen", "table", (w - 2.5, w, n + 17 / 12, n + 65 / 12), 2.5, "W", solved = False),
    pc("chair_k1", "kitchen", "chair", (w - 4.0, w - 2.5, n + 23 / 12, n + 41 / 12), 2.67, "E", solved = False),
    pc("chair_k2", "kitchen", "chair", (w - 4.0, w - 2.5, n + 44 / 12, n + 62 / 12), 2.67, "E", solved = False),
    # bath
    pc("tub", "bath", "tub", BA["bath.tub.tub"], 20 / 12, "E", fix = True),
    pc("wc", "bath", "wc", BA["bath.wc.wc"], 30 / 12, "E", fix = True),
    pc("lav", "bath", "lav", BA["bath.lav.lav"], 34 / 12, "E", fix = True),
    # bedrooms
    pc("bed1", "bed1", "bed", B1["bed1.bed.bed"], 2.0, "S"),
    pc("night1", "bed1", "nightstand", B1["bed1.bed.nightstand"], 2.0, "S"),
    pc("desk1", "bed1", "desk", B1["bed1.desk.desk"], 2.5, "S"),
    pc("dresser1", "bed1", "dresser", B1["bed1.dresser.dresser"], 34 / 12, "E"),
    pc("closet1", "bed1", "closet", B1["bed1.closet.closet"], 8.0, "N"),
    pc("bed2", "bed2", "bed", B2["bed2.bed.bed"], 2.0, "S"),
    pc("night2", "bed2", "nightstand", B2["bed2.bed.nightstand"], 2.0, "S"),
    pc("dresser2", "bed2", "dresser", B2["bed2.dresser.dresser"], 34 / 12, "E"),
    pc("closet2", "bed2", "closet", B2["bed2.closet.closet"], 8.0, "N"),
    # living: a sofa under the window facing two chairs across a low table
    pc("sofa", "living", "sofa", (0.33, 0.33 + 38 / 12, 3.0, 10.0), 34 / 12, "E", solved = False),
    pc("chair_a", "living", "armchair", (w - 5.33, w - 2.5, 3.0, 3.0 + 32 / 12), 34 / 12, "W", solved = False),
    pc("chair_b", "living", "armchair", (w - 5.33, w - 2.5, 10.0 - 32 / 12, 10.0), 34 / 12, "W", solved = False),
    pc("coffee", "living", "coffee table", (cm - 1.0, cm + 1.0, 4.5, 8.5), 16 / 12, "E", solved = False),
]

CABIN = plan(
    "cabin",
    floors = [floor("L1", name = "Cabin", z = 0.0, ceiling = 8.0)],
    rooms = ROOMS,
    doors = DOORS,
    windows = WINDOWS,
    fixtures = [fixture(f["key"], room = f["room"], kind = f["kind"], rect = f["rect"], front = f["front"]) for f in PIECES if f["fix"]],
    furniture = [{"key": f["key"], "room": f["room"], "kind": f["kind"], "rect": f["rect"], "front": f["front"]} for f in PIECES if not f["fix"]],
)

INDOOR = [r for r in ROOMS if r["kind"] != "deck"]
for r in ROOMS:
    report("area_" + r["key"], area(r))
    print("room %s: %s, %s x %s" % (r["name"], sqft(area(r)), ftin(r["rects"][0][1] - r["rects"][0][0]), ftin(r["rects"][0][3] - r["rects"][0][2])))
TOTAL = 0.0
for r in INDOOR:
    TOTAL += area(r)
report("area_indoor", TOTAL)

# ---- The model, from the same plan value. Finished floor is z = 0.
CEILING = 8.0
GRADE = -1.5
CUT = min(plan_cut, CEILING - 0.5)
OUT = (-EXT, x1 + EXT, -EXT, TOP + EXT)  # outside faces
WALL_COLOR = "#d9d3c7"
above = layer("above_cut", doc = "Walls, glass and tall cabinets above the plan cut")

def fbox(r, z0, z1, name):
    """A box over plan rect r (feet) from z0 to z1 (feet)."""
    return box(ft(r[1] - r[0]), ft(r[3] - r[2]), ft(z1 - z0), at = (ft(r[0]), ft(r[2]), ft(z0)), name = name)

def opening(o, gap, z0, z1, name, pad = 0.1):
    """The hole a door or window makes: its width along the wall, through the wall's `gap` and `pad` past each face."""
    u, v = o["at"]
    h = o["width"] / 2.0
    s = o["wall"]
    if s == "N":
        r = (u - h, u + h, v - pad, v + gap + pad)
    elif s == "S":
        r = (u - h, u + h, v - gap - pad, v + pad)
    elif s == "E":
        r = (u - pad, u + gap + pad, v - h, v + h)
    else:
        r = (u - gap - pad, u + pad, v - h, v + h)
    return fbox(r, z0, z1, name)

HEAD = {"a": 6.67, "slide": 6.67, "open": 7.0}

def wall_band(z0, z1, name):
    """The walls between z0 and z1: the outside mass less the rooms, doors and windows that reach into the band."""
    holes = [fbox(r["rects"][0], z0 - 0.1, z1 + 0.1, "void_%s_%s" % (r["key"], name)) for r in INDOOR]
    for d in DOORS:
        gap = EXT if d["b"].startswith("outside") else P
        if HEAD[d["swing"]] > z0:
            holes.append(opening(d, gap, z0 - 0.1, min(HEAD[d["swing"]], z1 + 0.1), "door_%s_%s" % (d["key"], name)))
    for wd in WINDOWS:
        if wd["head"] > z0 and wd["sill"] < z1:
            holes.append(opening(wd, EXT, max(wd["sill"], z0 - 0.1), min(wd["head"], z1 + 0.1), "window_%s_%s" % (wd["key"], name)))
    b = subtract(*([fbox(OUT, z0, z1, "wall_mass_" + name)] + holes), **{"name": name})
    b.material(color = WALL_COLOR, roughness = 0.85)
    return b

walls = wall_band(0.0, CUT, "walls")
walls_above = above.add(wall_band(CUT, CEILING, "walls_above"))

glass = []
for wd in WINDOWS + [d for d in DOORS if d["swing"] == "slide"]:
    z0, z1 = (wd["sill"], wd["head"]) if "sill" in wd else (0.0, HEAD["slide"])
    pane = dict(wd, at = (wd["at"][0] - (0.2 if wd["wall"] == "W" else 0.0) + (0.2 if wd["wall"] == "E" else 0.0), wd["at"][1] - (0.2 if wd["wall"] == "S" else 0.0) + (0.2 if wd["wall"] == "N" else 0.0)))
    for lo, hi, tag in [(z0, min(z1, CUT), ""), (max(z0, CUT), z1, "_above")]:
        if hi - lo < 0.01:
            continue
        g = opening(pane, 0.1, lo, hi, "glass_" + wd["key"] + tag, pad = 0.0)
        g.material(color = "#9fc4d6", roughness = 0.1, opacity = glass_opacity)
        if tag:
            above.add(g)
        glass.append(g)

slab = fbox(OUT, -0.5, 0.0, "floor_slab")
slab.material(color = "#9b968d", roughness = 0.85)
base = fbox((OUT[0] + 0.25, OUT[1] - 0.25, OUT[2] + 0.25, OUT[3] - 0.25), GRADE, -0.5, "foundation")
base.material(color = "#8e8a84", roughness = 0.95)

# The floor finishes, thin on the slab: tile in the kitchen, the bath and
# the entry, wood elsewhere; the slab's edge outside reads as concrete.
FINISH = {"kitchen": "#a7a39a", "bath": "#9fb0b4", "entry": "#8f8c84", "living": "#b08a5e", "bed1": "#b08a5e", "bed2": "#b08a5e"}
finishes = []
for r in INDOOR:
    if r["key"] in FINISH:
        b = fbox(r["rects"][0], 0.0, 0.03, "tile_" + r["key"])
        b.material(color = FINISH[r["key"]], roughness = 0.6)
        finishes.append(b)

# ---- Furniture as quiet masses: one colour per material, nothing glossy.
WOOD, CAB, TOPC, WHITE, STEEL = "#9a7b58", "#8b8479", "#cfc8ba", "#f1efe9", "#b9bdbf"
SOFT, SAGE, LINEN, THROW = "#6f8089", "#7e8f7f", "#e3ddd0", "#8aa0a6"

def part(r, z0, z1, name, color, rough = 0.8):
    """A block of a piece; one that rises through the plan cut is two, the upper on the above_cut layer."""
    if z0 < CUT and z1 > CUT + 0.01:
        return part(r, z0, CUT, name, color, rough) + [above.add(x) for x in part(r, CUT, z1, name + "_above", color, rough)]
    b = fbox(r, z0, z1, name)
    b.material(color = color, roughness = rough)
    return [b]

def inset(r, d):
    return (r[0] + d, r[1] - d, r[2] + d, r[3] - d)

def strip(r, front, d0, d1, a0 = None, a1 = None):
    """The part of plan rect r from d0 to d1 feet out from its back (the side opposite `front`), and from a0 to a1 along it."""
    along = (r[1] - r[0]) if front in ("N", "S") else (r[3] - r[2])
    a0 = 0.0 if a0 == None else a0
    a1 = along if a1 == None else a1
    if front == "N":
        return (r[0] + a0, r[0] + a1, r[2] + d0, r[2] + d1)
    if front == "S":
        return (r[0] + a0, r[0] + a1, r[3] - d1, r[3] - d0)
    if front == "E":
        return (r[0] + d0, r[0] + d1, r[2] + a0, r[2] + a1)
    return (r[1] - d1, r[1] - d0, r[2] + a0, r[2] + a1)

def along_of(r, front):
    return (r[1] - r[0]) if front in ("N", "S") else (r[3] - r[2])

def depth_of(r, front):
    return (r[3] - r[2]) if front in ("N", "S") else (r[1] - r[0])

def masses(f):
    """A piece's bodies, its plan rect raised to its height in a few blocks."""
    r, k, nm, fr = f["rect"], f["kind"], "furn_" + f["key"], f["front"]
    L, D = along_of(r, fr), depth_of(r, fr)
    if k in ("counter", "dishwasher"):
        return [part(strip(r, fr, 0.0, D - 0.1), 0.0, 2.9, nm, CAB), part(r, 2.9, 3.0, nm + "_top", TOPC, 0.5)]
    if k == "sink":
        return [part(strip(r, fr, 0.0, D - 0.1), 0.0, 2.9, nm, CAB), part(r, 2.9, 3.0, nm + "_top", TOPC, 0.5),
                part(strip(r, fr, 0.35, D - 0.35, 0.25, L - 0.25), 3.0, 3.02, nm + "_basin", STEEL, 0.3)]
    if k == "range":
        return [part(r, 0.0, 3.0, nm, "#7c7f81", 0.5)] + [part(strip(r, fr, d, d + 0.6, a, a + 0.6), 3.0, 3.03, "%s_burner%d" % (nm, i), "#5f6264", 0.6)
                                                          for i, (d, a) in enumerate([(0.3, 0.3), (0.3, 1.6), (1.1, 0.3), (1.1, 1.6)])]
    if k == "fridge":
        return [part(r, 0.0, f["h"], nm, STEEL, 0.4)]
    if k == "tub":
        shell = subtract(fbox(r, 0.0, f["h"], nm + "_block"), fbox(inset(r, 0.25), 0.45, f["h"] + 0.1, nm + "_basin"), name = nm)
        shell.material(color = WHITE, roughness = 0.35)
        return [[shell]]
    if k == "wc":
        tank = part(strip(r, fr, 0.0, 0.7, 0.3, L - 0.3), 0.0, f["h"], nm + "_tank", WHITE, 0.35)
        bowl = part(strip(r, fr, 0.7, D - 0.1, 0.55, L - 0.55), 0.0, 1.35, nm, WHITE, 0.35)
        return [tank, bowl]
    if k == "lav":
        top = subtract(fbox(r, 2.75, f["h"], nm + "_slab"), fbox(strip(r, fr, 0.4, D - 0.35, 0.6, L - 0.6), 2.5, f["h"] + 0.1, nm + "_bowl"), name = nm + "_top")
        top.material(color = WHITE, roughness = 0.35)
        return [part(strip(r, fr, 0.0, D - 0.1), 0.0, 2.75, nm, CAB), [top]]
    if k == "bed":
        return [part(r, 0.0, 1.1, nm, WOOD), part(inset(r, 0.1), 1.1, 1.9, nm + "_mattress", LINEN, 0.9),
                part(strip(inset(r, 0.1), fr, 2.2, D - 0.2), 1.9, 1.98, nm + "_throw", THROW, 0.9),
                part(strip(r, fr, 0.0, 0.25), 1.1, 3.0, nm + "_head", WOOD)]
    if k in ("nightstand", "dresser", "desk"):
        return [part(r, 0.0, f["h"], nm, WOOD)]
    if k == "closet":
        return [part(r, 0.0, f["h"], nm, "#b8b0a2", 0.8)]
    if k == "table":
        return [part(r, 2.35, 2.5, nm, WOOD)] + [part((r[0] + a, r[0] + a + 0.15, r[2] + b, r[2] + b + 0.15), 0.0, 2.35, "%s_leg%d" % (nm, i), WOOD)
                                                for i, (a, b) in enumerate([(0.1, 0.1), (r[1] - r[0] - 0.25, 0.1), (0.1, r[3] - r[2] - 0.25), (r[1] - r[0] - 0.25, r[3] - r[2] - 0.25)])]
    if k == "coffee table":
        return [part(r, 0.0, f["h"], nm, WOOD)]
    if k == "chair":
        return [part(r, 0.0, 1.5, nm, WOOD), part(strip(r, fr, 0.0, 0.15), 1.5, f["h"], nm + "_back", WOOD)]
    if k in ("sofa", "armchair"):
        c = SOFT if k == "sofa" else SAGE
        return [part(strip(r, fr, 0.0, D, 0.45, L - 0.45), 0.0, 1.45, nm, c, 0.95),
                part(strip(r, fr, 0.0, 0.7), 1.45, f["h"], nm + "_back", c, 0.95),
                part(strip(r, fr, 0.0, D, 0.0, 0.45), 0.0, 2.1, nm + "_arm0", c, 0.95),
                part(strip(r, fr, 0.0, D, L - 0.45, L), 0.0, 2.1, nm + "_arm1", c, 0.95)]
    fail("no mass for a %s" % k)

furniture3d = []
for f in PIECES:
    for parts in masses(f):
        furniture3d.extend(parts)

footprint = [(ft(OUT[0]), ft(OUT[2])), (ft(OUT[1]), ft(OUT[2])), (ft(OUT[1]), ft(OUT[3])), (ft(OUT[0]), ft(OUT[3]))]
# The roof is a 9 in shell, open underneath so its soffit shows, and the
# gable ends are wall: the space under the shell, within the walls.
ROOF_T = 0.75
roof_block = gable_roof(footprint, pitch = pitch("6:12"), ridge_axis = "x", overhang = ft(1.5), eave_z = ft(CEILING + roof_lift), fascia = inch(6), name = "roof_block")
roof = subtract(roof_block, translate(copy(roof_block, name = "roof_underside"), (0, 0, -ft(ROOF_T))), name = "roof")
roof.material(color = "#4f5a63", roughness = 0.55)
INSET = 0.05
gable_fill = gable_roof([(ft(OUT[0] + INSET), ft(OUT[2] + INSET)), (ft(OUT[1] - INSET), ft(OUT[2] + INSET)), (ft(OUT[1] - INSET), ft(OUT[3] - INSET)), (ft(OUT[0] + INSET), ft(OUT[3] - INSET))],
                        pitch = pitch("6:12"), ridge_axis = "x", eave_z = ft(CEILING), name = "gable_block")
gables = intersect(gable_fill, translate(copy(roof_block, name = "roof_inside"), (0, 0, -ft(ROOF_T) - inch(1))), name = "gables")
gables.material(color = WALL_COLOR, roughness = 0.85)
layer("roof", doc = "Switch the roof off to look into the rooms").add(roof, gables)

# The doors that swing, open square to their wall as the plan draws them,
# and a mullion in the slider: 1 3/4 in leaves of wood, cut at the plan cut too.
LEAF_T = 1.75 / 12
DOORLEAF = "#8a6f52"
leaves = []
for d in [d for d in DOORS if d["swing"] == "a"]:
    u, v = d["at"]
    h = d["width"] / 2.0
    ww = d["width"] - 2 / 12
    if d["wall"] in ("N", "S"):
        a0 = u - h if d["hinge"] == "lo" else u + h - LEAF_T
        r = (a0, a0 + LEAF_T, v - ww, v) if d["wall"] == "N" else (a0, a0 + LEAF_T, v, v + ww)
    else:
        b0 = v - h if d["hinge"] == "lo" else v + h - LEAF_T
        r = (u - ww, u, b0, b0 + LEAF_T) if d["wall"] == "E" else (u, u + ww, b0, b0 + LEAF_T)
    leaves.extend(part(r, 0.0, HEAD["a"] - 0.05, "leaf_" + d["key"], DOORLEAF, 0.7))
for d in [d for d in DOORS if d["swing"] == "slide"]:
    u, v = d["at"]
    leaves.extend(part((u - 1 / 12, u + 1 / 12, v - EXT, v), 0.0, HEAD["slide"], "mullion_" + d["key"], "#5b5650", 0.6))

# A landing and three steps at the front door, which is a foot and a half above grade.
STOOP = (x1 + EXT, x1 + EXT + 4.0, 11.67 - 2.0, 11.67 + 2.0)
stoop = deck([(ft(STOOP[0]), ft(STOOP[2])), (ft(STOOP[1]), ft(STOOP[2])), (ft(STOOP[1]), ft(STOOP[3])), (ft(STOOP[0]), ft(STOOP[3]))], 0.0, inch(6), posts = (ft(4), inch(4)), ground = ft(GRADE), name = "stoop")
stoop_steps = stair(ft(GRADE), 0.0, ft(3), direction = (-1, 0), at = (ft(STOOP[1]) + inch(30), ft(11.67 + 1.5)), name = "stoop_steps")

dk = deck([(ft(DECK[0]), ft(DECK[2])), (ft(DECK[1]), ft(DECK[2])), (ft(DECK[1]), ft(DECK[3])), (ft(DECK[0]), ft(DECK[3]))], 0.0, inch(6), posts = (ft(6), inch(6)), ground = ft(GRADE), name = "deck")
steps = stair(ft(GRADE), 0.0, ft(4), direction = (0, 1), at = (ft(w / 2.0 + 2.0), ft(DECK[2]) - inch(30)), name = "steps")
for b in dk["bodies"] + [steps["body"]] + stoop["bodies"] + [stoop_steps["body"]]:
    b.material(color = "#8f7a62", roughness = 0.85)

HOUSE = [base, slab, walls, walls_above, roof, gables] + glass + finishes + furniture3d + leaves + dk["bodies"] + [steps["body"]] + stoop["bodies"] + [stoop_steps["body"]]

# ---- The drawings: a plan, a section and an elevation at
# 1/4" = 1'-0", every length written in feet and inches.
S = 6.35  # paper mm to the foot
PLAN_W = (OUT[1] - OUT[0]) * S + 50.0
PLAN_H = (OUT[3] - DECK[2]) * S + 44.0
PAPER_H = PLAN_H + 20.0
SET = sheet_set("cabin", size = (PLAN_W + 110.0, PAPER_H), units = "ft-in", scale = "1/4\" = 1'-0\"", margin = 5)
a1 = SET.sheet("A1", "PLAN")
AX, AY = a1.area()[0] + 16.0, a1.area()[1] + 6.0
lay = draw_floor(a1, CABIN, "L1", at = (AX, AY), scale = S, dims = False)

def paper(u, v):
    """The paper point of plan point (u, v)."""
    return (AX + lay["margin"][0] + (u - lay["origin"][0]) * S, AY + lay["margin"][1] + (v - lay["origin"][1]) * S)

# The pieces' own lines, in the plan's furniture ink: pillows and the
# top of the sheet on a bed, a sofa's back and arms, a closet's rod and
# sliding doors, a chair at the desk.
INK, NOTE = "#9a948a", "#5d6670"

def line(pts, dash = None):
    a1.polyline([paper(u, v) for u, v in pts], weight = 0.13, color = INK, dash = dash)

def outline(r, dash = None):
    line([(r[0], r[2]), (r[1], r[2]), (r[1], r[3]), (r[0], r[3]), (r[0], r[2])], dash)

def seg(r, front, d0, a0, d1, a1_):
    """A line from (d0 out from the back, a0 along) to (d1, a1_) in rect r's own frame."""
    p = strip(r, front, d0, d0, a0, a0)
    q = strip(r, front, d1, d1, a1_, a1_)
    line([(p[0], p[2]), (q[0], q[2])])

def tag(r, s):
    a1.text(s, at = paper((r[0] + r[1]) / 2.0, (r[2] + r[3]) / 2.0), size = 1.3, color = NOTE, align = "center", valign = "middle")

for f in PIECES:
    r, k, fr = f["rect"], f["kind"], f["front"]
    L, D = along_of(r, fr), depth_of(r, fr)
    if k == "bed":
        pw = (L - 0.9) / 2.0 if L >= 4.0 else L - 0.6
        for a in ([0.3, 0.6 + pw] if L >= 4.0 else [0.3]):
            outline(strip(r, fr, 0.4, 1.4, a, a + pw))
        seg(r, fr, 2.2, 0.0, 2.2, L)
    elif k in ("sofa", "armchair"):
        outline(strip(r, fr, 0.0, 0.7, 0.45, L - 0.45))
        seg(r, fr, 0.0, 0.45, D, 0.45)
        seg(r, fr, 0.0, L - 0.45, D, L - 0.45)
        if k == "sofa":
            for t in [1.0 / 3, 2.0 / 3]:
                a = 0.45 + t * (L - 0.9)
                seg(r, fr, 0.7, a, D, a)
    elif k == "closet":
        seg(r, fr, D / 2.0, 0.2, D / 2.0, L - 0.2)
        seg(r, fr, D - 0.12, 0.0, D - 0.12, L / 2.0 + 0.3)
        seg(r, fr, D - 0.3, L / 2.0 - 0.3, D - 0.3, L)
    elif k == "desk":
        outline(strip(r, fr, D + 0.25, D + 1.75, L / 2.0 - 0.75, L / 2.0 + 0.75))
    elif k == "chair":
        seg(r, fr, 0.2, 0.15, 0.2, L - 0.15)
    elif k == "coffee table":
        outline(inset(r, 0.2))
    elif k == "fridge":
        seg(r, fr, D - 0.15, 0.0, D - 0.15, L)
        tag(r, "REF")
    elif k == "dishwasher":
        tag(r, "DW")
    elif k == "nightstand":
        c = paper((r[0] + r[1]) / 2.0, (r[2] + r[3]) / 2.0)
        a1.circle(center = c, r = 0.35 * S, weight = 0.13)

# Clear room widths along the north wall and room depths along the east wall, then the outside faces overall.
for a, b in [(0.0, w), (x0, x0 + 5.5), (x0 + 5.5 + P, x1)]:
    a1.dimension(paper(a, OUT[3]), paper(b, OUT[3]), offset = 6.0, size = 2.2)
for a, b in [(0.0, 9.5), (9.5 + P, 13.5), (n, TOP)]:
    a1.dimension(paper(OUT[1], a), paper(OUT[1], b), offset = -6.0, size = 2.2)
a1.dimension(paper(OUT[0], OUT[3]), paper(OUT[1], OUT[3]), offset = 13.0, size = 2.2)
a1.dimension(paper(OUT[0], OUT[2]), paper(OUT[0], OUT[3]), offset = 7.0, size = 2.2)

a2 = SET.sheet("A2", "SECTION")
BX, BY = a2.area()[0], a2.area()[1]
sec = a2.section_view(HOUSE, plane = "x=%s" % ft(w / 2.0 - 4.0), at = (BX + 80.0, BY + 50.0), origin = (ft(OUT[2]), 0.0), hatch = "solid", name = "section")
sec.dimension((ft(OUT[3]), 0.0), (ft(OUT[3]), ft(CEILING)), offset = -16.0, size = 2.2)
sec.dimension((ft(OUT[2]), ft(GRADE)), (ft(OUT[3]), ft(GRADE)), offset = -8.0, size = 2.2)
a2.text("SECTION THROUGH LIVING AND KITCHEN", at = (BX + 80.0, BY + 22.0), size = 2.6, bold = True)

a3 = SET.sheet("A3", "SOUTH ELEVATION")
RIDGE = roof.bbox()[1][2]  # the roof's own top, whatever its pitch and overhang
el = a3.elevation_view(HOUSE, direction = "south", at = (BX + 30.0, BY + 50.0), origin = (ft(OUT[0]), 0.0), name = "elevation")
el.dimension((ft(OUT[0]), ft(GRADE)), (ft(OUT[1]), ft(GRADE)), offset = -8.0, size = 2.2)
el.dimension((ft(OUT[1]), 0.0), (ft(OUT[1]), RIDGE), offset = -18.0, size = 2.2)
a3.text("SOUTH ELEVATION", at = (BX + 30.0, BY + 22.0), size = 2.6, bold = True)

export(SET, "cabin.pdf")
export(SET, "cabin.dxf")
export(SET, "cabin.svg")

# ---- Tests: `cadpole test cabin.star`.
def the_plan_is_clean():
    assert_eq(validate(CABIN), [])
    assert_eq(lay["overlaps"], [])

def the_living_room_is_its_width():
    assert_close(area(ROOMS[0]), w * 13.5, tol = 1e-9)
    return area(ROOMS[0])

test("the plan validates and every label has a place", the_plan_is_clean)
test("the living room grows with its width", the_living_room_is_its_width, sweep = {"living_w": [12, 14, 16, 18]}, monotone = monotone.increasing)

def meets(a, b):
    """True when plan rects a and b overlap by more than a hair."""
    e = 0.01
    return a[0] < b[1] - e and b[0] < a[1] - e and a[2] < b[3] - e and b[2] < a[3] - e

ROOM_OF = {r["key"]: r["rects"][0] for r in ROOMS}

def every_room_is_furnished():
    """Each room holds what it is for, every piece inside its room and clear of every other."""
    need = {"kitchen": ["sink", "range", "fridge", "dishwasher", "table"], "bath": ["wc", "lav", "tub"],
            "bed1": ["bed", "closet", "dresser", "desk"], "bed2": ["bed", "closet", "dresser"], "living": ["sofa", "armchair", "coffee table"]}
    for key, kinds in need.items():
        have = [f["kind"] for f in PIECES if f["room"] == key]
        for k in kinds:
            assert_true(k in have, msg = "%s has no %s" % (key, k))
    for f in PIECES:
        r, q = f["rect"], ROOM_OF[f["room"]]
        assert_true(r[0] >= q[0] - 1e-6 and r[1] <= q[1] + 1e-6 and r[2] >= q[2] - 1e-6 and r[3] <= q[3] + 1e-6, msg = "%s leaves its room" % f["key"])
        for g in PIECES:
            if f["key"] < g["key"]:
                assert_true(not meets(r, g["rect"]), msg = "%s and %s overlap" % (f["key"], g["key"]))
    return len(PIECES)

def clear_strip(f, depth):
    """The clear floor `depth` ft deep along a piece's front."""
    r, fr = f["rect"], f["front"]
    D = depth_of(r, fr)
    return strip(r, fr, D, D + depth)

def free(zone, but = ()):
    """No piece's solid enters zone, other than those named."""
    return [f["key"] for f in PIECES if f["key"] not in but and meets(zone, f["rect"])]

def BY(key):
    return [f for f in PIECES if f["key"] == key][0]

def the_kitchen_works():
    """NKBA, as @cadpole/plan_furniture.star reads it: a 42 in work aisle
    in front of every run, each leg of the triangle between the sink, the
    range and the fridge 4 to 9 ft and all three at most 26 ft, and 24 in,
    15 in and 15 in of landing beside them."""
    for key in ["landing", "fridge", "sink_l", "sink", "dw", "range_s", "range", "range_n"]:
        zone = clear_strip(BY(key), 42 / 12)
        q = ROOM_OF["kitchen"]
        assert_true(zone[0] >= q[0] - 1e-6 and zone[1] <= q[1] + 1e-6 and zone[2] >= q[2] - 1e-6 and zone[3] <= q[3] + 1e-6, msg = "the aisle at %s leaves the kitchen" % key)
        assert_eq(free(zone), [], msg = "in the aisle in front of %s" % key)

    def use(key):
        f = BY(key)
        z = clear_strip(f, 0.0)
        return ((z[0] + z[1]) / 2.0, (z[2] + z[3]) / 2.0)
    pts = [use("sink"), use("range"), use("fridge")]
    total = 0.0
    for i in range(3):
        a, b = pts[i], pts[(i + 1) % 3]
        leg = math.sqrt((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]))
        assert_true(leg >= 4.0 and leg <= 9.0, msg = "a triangle leg of %s" % ftin(leg))
        total += leg
    assert_true(total <= 26.0, msg = "the triangle is %s" % ftin(total))
    assert_close(along_of(BY("sink_l")["rect"], "S"), 2.0, tol = 1e-6)
    assert_true(along_of(BY("range_s")["rect"], "E") >= 1.25 - 1e-6 and along_of(BY("range_n")["rect"], "E") >= 1.25 - 1e-6)
    assert_close(along_of(BY("landing")["rect"], "S"), 1.25, tol = 1e-6)
    return total

def the_bath_has_its_clearances():
    """24 in clear in front of the WC and the lavatory (CPC 402.5), 30 in along the tub (NKBA), all inside the bath."""
    q = ROOM_OF["bath"]
    for key, depth in [("wc", 2.0), ("lav", 2.0), ("tub", 2.5)]:
        zone = clear_strip(BY(key), depth)
        assert_true(zone[1] <= q[1] + 1e-6, msg = "the clear floor at the %s leaves the bath" % key)
        assert_eq(free(zone), [], msg = "in front of the %s" % key)
    assert_true(BY("wc")["rect"][3] - BY("wc")["rect"][2] >= 2.5 - 1e-6, msg = "the WC's 30 in space")

def swing_zone(d):
    """What a door keeps clear on the side it swings into: the leaf's square, or a 36 in landing at a cased opening."""
    u, v = d["at"]
    h = d["width"] / 2.0
    out = []
    reach = d["width"] if d["swing"] == "a" else 3.0
    sides = [d["a"]] if d["swing"] == "a" else [d["a"], d["b"]]
    for key in sides:
        if key not in ROOM_OF:
            continue
        q = ROOM_OF[key]
        if d["wall"] in ("N", "S"):
            into_north = q[2] >= v - P - 1e-6
            out.append((u - h, u + h, v + (P if d["wall"] == "S" and into_north else 0.0), v + reach) if into_north else (u - h, u + h, v - reach, v))
        else:
            into_east = q[0] >= u - P - 1e-6
            out.append((u, u + reach + P, v - h, v + h) if into_east else (u - reach, u, v - h, v + h))
    return out

def no_piece_is_in_a_doorway():
    for d in DOORS:
        if d["swing"] not in ("a", "open"):
            continue
        for zone in swing_zone(d):
            assert_eq(free(zone), [], msg = "in the way of door %s" % d["key"])

def the_plan_cut_cuts_every_wall():
    """The walls stop at the plan cut and the band above starts there; between them they are the whole wall."""
    assert_close(walls.bbox()[1][2], ft(CUT), tol = 0.01)
    assert_close(walls_above.bbox()[0][2], ft(CUT), tol = 0.01)
    assert_close(walls_above.bbox()[1][2], ft(CEILING), tol = 0.01)
    assert_eq(walls_above.layer(), "above_cut")
    whole = wall_band(0.0, CEILING, "walls_whole")
    assert_close(walls.volume() + walls_above.volume(), whole.volume(), tol = 1e-6 * whole.volume())
    for b in furniture3d + glass:
        if b.layer() != "above_cut":
            assert_true(b.bbox()[1][2] <= ft(CUT) + 1.0, msg = "%s stands above the cut" % b.name)
    return CUT

def the_room_stage_still_places_them():
    """Solve the four rooms again (needs the native solver) and compare with SOLVED."""
    for key, placed in SOLVED.items():
        r = furnish(room_local(CABIN, key), wet = "any", keep_fixtures = False, without = WITHOUT.get(key, ()), name = "cabin_" + key)
        assert_true(r["ok"], msg = "%s: %s" % (key, r["reason"]))
        got = {x["key"]: x["rect"] for x in r["furniture"] + r["fixtures"]}
        assert_eq(sorted(got.keys()), sorted(placed.keys()))
        for k, rect in placed.items():
            for i in range(4):
                assert_close(got[k][i], rect[i], tol = 0.01, msg = "%s[%d]" % (k, i))

test("every room has its furniture, inside it and clear of the rest", every_room_is_furnished)
test("the kitchen meets the work aisle, triangle and landing rules", the_kitchen_works)
test("the bath keeps its clear floor", the_bath_has_its_clearances)
test("nothing stands in a door's swing or a cased opening's landing", no_piece_is_in_a_doorway)
test("the 3D plan cut cuts every wall", the_plan_cut_cuts_every_wall)
test("the room stage still places the kitchen, the bath and the bedrooms", the_room_stage_still_places_them)
