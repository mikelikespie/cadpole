# A storey kit: a small three-storey building printed one storey to
# a plate, plus its roof and its base, as one Bambu Studio project. Each
# piece is ONE object whose parts are its colour classes -- slab, walls,
# window frames, glass, door, deck -- and each class prints on its own
# filament slot: seven (an H2C's seven hotends, or two AMS units on one
# nozzle) or, with `slots=4`, merged down to the four one AMS holds.
#
# Every storey is modelled where it stands in the building, at its own
# height; the export sets each piece down on its plate's bed, lays the
# plates out as Bambu does and centres each plate's piece where every nozzle
# of the printer reaches.
#
#   cadpole export examples/storey_kit.star --run-exports --out-dir /tmp
#   cadpole export examples/storey_kit.star --run-exports --out-dir /tmp -p slots=4 -p printer=bambu_h2d
#
# With no template the project carries Cadpole's minimal settings: open it, and
# pick your printer, process and filament presets before slicing. To carry
# your own, save a project in Bambu Studio for your printer with your
# filaments loaded in this kit's slot order (wall, trim, stone, accent,
# glass, wood, ground) and put it in ./assets next to the script:
#
#   cadpole export examples/storey_kit.star --run-exports --out-dir /tmp -p template=h2c_7.3mf

slots = param("slots", 7, min = 4, max = 7, doc = "Filament slots the kit may use: 7 (an H2C's hotends, or two AMS units) or 4 (one AMS); below 7 the glass prints in the trim's filament, the deck and the ground in the stone's")
printer_name = param("printer", "bambu_h2c", choices = ["bambu_h2c", "bambu_h2d", "bambu_x1c", "bambu_p1s", "bambu_a1"], doc = "The machine the plates are laid out for and the pieces fitted to")
template_name = param("template", "", doc = "A Bambu Studio project saved for that printer, by its name under ./assets; empty for Cadpole's minimal settings")
storeys = param("storeys", 3, min = 1, max = 4, doc = "How many storeys the building has, one plate each")

# The colour classes, in slot order: with seven slots each is its own; with
# four, each merged class prints in the filament of the class it merges into.
CLASSES = ["wall", "trim", "stone", "accent", "glass", "wood", "ground"]
COLOURS = {
    "wall": "#C9C2B6",
    "trim": "#2B2D31",
    "stone": "#9A9890",
    "accent": "#C2462E",
    "glass": "#8FB3C9",
    "wood": "#A8835A",
    "ground": "#7C8F5A",
}
MERGE_TO_FOUR = {"glass": "trim", "wood": "stone", "ground": "stone"}

def kept(cls):
    """The class whose filament `cls` prints in."""
    return MERGE_TO_FOUR.get(cls, cls) if slots < 7 else cls

KEPT = [c for c in CLASSES if kept(c) == c]

def slot_of(cls):
    return KEPT.index(kept(cls)) + 1

def piece(body, cls):
    """A part of class `cls`, coloured as the filament it prints in."""
    body.material(color = COLOURS[kept(cls)])
    return (body, cls)

# The building, in millimetres at print scale (about 1:100).
W = 90.0       # across the front
D = 60.0       # front to back
FLOOR = 30.0   # floor to floor
SLAB = 2.4     # a whole number of 0.2 mm layers
WALL = 1.6     # two perimeters of a 0.4 mm nozzle
WIN_W = 12.0
WIN_H = 14.0
SILL = 8.0
FRAME = 1.2    # the thinnest a colour of its own prints with two perimeters
GLASS = 0.8
WINDOWS = [14.0, 64.0]   # x of each front window
DOOR_X = 39.0
DOOR_W = 12.0
DOOR_H = 20.0

def wall_box(tag, name, x0, x1, y0, y1, z0, z1):
    """A wall piece: a box from (x0, y0, z0) to (x1, y1, z1)."""
    return piece(box(x1 - x0, y1 - y0, z1 - z0, at = (x0, y0, z0), name = "%s_%s" % (tag, name)), "wall")

def storey(i):
    """Storey `i` (0 at the bottom) as (body, class) pairs, at its own height.

    Every piece is a box: the walls are laid around the openings rather than
    a ring with the openings subtracted (an exact boolean's edge order
    differs in its last bits between native and wasm), and a window frame is
    four bars."""
    z0 = i * FLOOR
    tag = "L%d" % i
    zs = z0 + SLAB
    top = z0 + FLOOR
    parts = [
        piece(box(W, D, SLAB, at = (0, 0, z0), name = tag + "_slab"), "stone"),
        wall_box(tag, "back", 0, W, D - WALL, D, zs, top),
        wall_box(tag, "left", 0, WALL, WALL, D - WALL, zs, top),
        wall_box(tag, "right", W - WALL, W, WALL, D - WALL, zs, top),
    ]

    # The front wall: piers between the openings, the wall under and over each.
    openings = sorted([(x, x + WIN_W, zs + SILL, zs + SILL + WIN_H) for x in WINDOWS] + [(DOOR_X, DOOR_X + DOOR_W, zs, zs + DOOR_H)])
    x = 0.0
    for k, (a, b, za, zb) in enumerate(openings):
        if a > x:
            parts.append(wall_box(tag, "pier%d" % k, x, a, 0, WALL, zs, top))
        if za > zs:
            parts.append(wall_box(tag, "under%d" % k, a, b, 0, WALL, zs, za))
        if zb < top:
            parts.append(wall_box(tag, "over%d" % k, a, b, 0, WALL, zb, top))
        x = b
    if x < W:
        parts.append(wall_box(tag, "pier%d" % len(openings), x, W, 0, WALL, zs, top))

    # Each window: a frame of four bars FRAME wide through the wall, and the
    # glass inside it.
    for k, x in enumerate(WINDOWS):
        zw = zs + SILL
        bars = [
            (x, x + WIN_W, zw, zw + FRAME),
            (x, x + WIN_W, zw + WIN_H - FRAME, zw + WIN_H),
            (x, x + FRAME, zw + FRAME, zw + WIN_H - FRAME),
            (x + WIN_W - FRAME, x + WIN_W, zw + FRAME, zw + WIN_H - FRAME),
        ]
        for j, (a, b, za, zb) in enumerate(bars):
            parts.append(piece(box(b - a, WALL, zb - za, at = (a, 0, za), name = "%s_frame%d_%d" % (tag, k, j)), "trim"))
        glass = box(WIN_W - 2 * FRAME, GLASS, WIN_H - 2 * FRAME, at = (x + FRAME, (WALL - GLASS) / 2, zw + FRAME), name = "%s_glass%d" % (tag, k))
        parts.append(piece(glass, "glass"))
    door = box(DOOR_W, WALL, DOOR_H, at = (DOOR_X, 0, zs), name = tag + "_door")
    deck = box(30, 8, SLAB, at = (30, -8, z0), name = tag + "_deck")
    parts += [piece(door, "accent"), piece(deck, "wood")]
    return parts

def roof():
    top = storeys * FLOOR
    lid = box(W + 4, D + 4, 3, at = (-2, -2, top), name = "roof")
    chimney = box(8, 8, 10, at = (70, 40, top + 3), name = "chimney")
    return [piece(lid, "trim"), piece(chimney, "wall")]

def base():
    ground = box(W + 40, D + 40, 6, at = (-20, -20, -6), name = "ground")
    path = box(DOOR_W, 12, 0.6, at = (DOOR_X, -20, 0), name = "path")
    return [piece(ground, "ground"), piece(path, "stone")]

template = template_name if template_name else None

def plate(parts, plate_name):
    export(
        [b for b, _ in parts],
        "storey_kit.3mf",
        flavor = flavor.bambu,
        plate = plate_name,
        name = plate_name,
        extruder = [slot_of(c) for _, c in parts],
        printer = printer_name,
        template = template,
    )

for i in range(storeys):
    plate(storey(i), "Storey %d" % (i + 1))
plate(roof(), "Roof")
plate(base(), "Base")
plate_images("storey_kit.3mf")
