"""What render.py renders: every picture on the site, with the script and the
arguments that make it. Scripts are named from the repository root."""

FEATURES = {  # card: camera of its still (azimuth, elevation)
    "params": (-32, 28), "extrude": (-32, 28), "revolve": (40, 30), "sweep": (30, 20), "loft": (-32, 22), "boolean": (-8, 42),
    "thread": (-30, 20), "chamfer": (-32, 32), "fillet": (-32, 32), "mirror": (-20, 40), "linear-pattern": (-28, 30),
    "circular-pattern": (-30, 40), "constraints": (-30, 45), "offset": (-30, 40), "text": (-18, 42), "emboss": (-4, 16),
    "smooth-blend": (-35, 28), "twist": (-32, 20), "section": (-25, 22),
}

# name: (script, extra `cadpole render` arguments) -> static/media/<name>-{1200,720[,360]}.webp
STILLS = {
    "part": ("examples/part.star", ["--view", "az=-32,el=28"]),
    "planter": ("examples/planter.star", ["--view", "az=-30,el=24"]),
    "bracket": ("examples/bracket.star", ["--view", "az=-35,el=30"]),
    "knob": ("examples/knurled_knob.star", ["--view", "az=-30,el=26"]),
    "mascot": ("examples/desk_mascot.star", ["--view", "az=-30,el=24"]),
    "gridfinity": ("examples/gridfinity_showcase.star", []),
    # The lid lifted off the seated board, so the snap arms and the board show.
    "enclosure": ("examples/enclosure.star", ["-p", "lid_lift=26", "--view", "az=35,el=28"]),
    "thumbscrew": ("examples/thumbscrew.star", []),
    # Assembled, as it comes off the bed, the sun turned a little so the
    # planets' phases and the chevrons on every gear show.
    "planetary": ("examples/planetary.star", ["-p", "sun_angle=7.5", "--view", "az=-30,el=38"]),
    "wheel": ("examples/compressor_wheel.star", ["--view", "az=-30,el=30"]),
    # The lightbox's direction (az -35, el 30, 24 degrees), aimed at the row's middle
    # so the long, low scene sits centred in the square.
    "imports": ("examples/import_demo.star", ["--eye=-258,-369,272", "--target", "0,0,12", "--fov", "24"]),
}
STILLS.update({"feat-" + k: ("site/features/%s.star" % k, ["--view", "az=%d,el=%d" % v]) for k, v in FEATURES.items()})

# scene: (script with a `scroll_story` animation, frames, share of the picture the model fills)
FRAMES = {
    "iris": ("examples/iris.star", 60, 0.74),
    "mug": ("examples/embossed_mug.star", 72, 0.74),
    "radial": ("examples/radial_engine.star", 72, 0.74),
}

# The turbojet scene. framing: set -> (width, height, the model's share of the
# width in the first frame and in the last, the most of the height it may take).
JET = {
    "script": "examples/turbojet.star",
    "frames": 96,
    "framing": {"jet-wide": (1920, 1080, 0.66, 0.90, 0.84), "jet-narrow": (1800, 1350, 0.70, 0.93, 0.80)},
}

# seq/<name>: (framed set for wide screens, for phones, frames, a 1920 set too)
ENCODE = {
    "tj": ("jet-wide", "jet-narrow", 96, True),
    "ir": ("iris", "iris", 60, False),
    "mg": ("mug", "mug", 72, False),
    "ra": ("radial", "radial", 72, False),
}

# A screen recording of the web app: not something a script renders.
RECORDING = ["agent/agent.mp4", "agent/agent.webm"]

# static/draw/<name>.*: the file as the CLI writes it (<name>.raw.svg, and the other
# `formats`) and the copy the page sets inline (<name>.svg). `frame`: the square of
# the sheet, in its millimetres, that the inline copy is cut to (x, y, width,
# height). `pair`: (model, azimuth, elevation, tolerance, pixels) of a path-traced
# picture of exactly that square from the same parallel camera, <name>-<pixels>.webp;
# the frame is measured, not computed (README.md).
DRAWINGS = {
    # The drawings section's zoomable pair: the lines over the picture they were drawn from.
    "pair-thumbscrew": dict(
        script="site/features/pair-thumbscrew.star", view="sheet:V1", flags=["--tolerance", "0.02"], formats=("svg", "pdf", "dxf"),
        title="Thumbscrew and nut, hidden lines dashed",
        theme=["--only", "vis,hid", "--weights", "vis=2,hid=1.2", "--dash", "5,3.5", "--cut-dashes", "1", "--tiles", "4"],
        pair=("examples/thumbscrew.star", -30, 24, 0.02, 3000), frame="88.3538,119.3692,64.2926,64.2926"),
    # The feature cards'.
    "pair-pillow-block": dict(
        script="site/features/feat-hidden-line.star", view="sheet:V1", flags=["--tolerance", "0.005"],
        title="Pillow block, hidden lines dashed", theme=["--only", "vis,hid", "--weights", "vis=2.4,hid=1.3", "--dash", "9,6"],
        frame="50.1112,73.3114,140.7777,140.7777"),
    "sheet-pillow-block": dict(
        script="site/features/feat-sheet.star", view="sheet:D1", flags=["--tolerance", "0.005"],
        title="Pillow block, three views"),
    # The "Drawing" feature card: the card's own script, as the sheet its snippet makes.
    "sheet-flanged-boss": dict(
        script="site/features/drawing.star", view="three-view", flags=["--sheet", "A4", "--units", "in", "--title", "Flanged boss"],
        title="Flanged boss, three views"),
    "section-pillow-block": dict(
        script="site/features/feat-section.star", view="sheet:S1", flags=["--tolerance", "0.005"],
        title="Pillow block, section along the bore"),
}

PLAN = {"script": "examples/cabin_plan.star", "widths": [14, 14.5, 15, 15.5, 16]}

# The building sections' pictures: (script, `cadpole render` arguments) -> static/media/<name>-{1200,720}.webp.
# Raster renders on the page's colour. The shadow study's place is open ocean at the example's latitude.
BUILDINGS = {
    "lot": ("examples/hillside_lot.star", ["--size", "1600x1200", "--view", "az=-115,el=24"]),
    # The cabin cut like a plan: the roof and everything above the 4 ft plan cut switched off.
    "plan-cabin": ("examples/cabin_plan.star", ["--size", "1500x1500", "--view", "az=-35,el=55", "--layer", "roof=off", "--layer", "above_cut=off"]),
    "shadow": ("examples/shadow_study.star", ["--size", "640x480", "--view", "az=10,el=70", "--layer", "court_shadow=off", "--layer", "sightline=off", "--shadow-study", "--sun", "38,-140,2026-06-21,12:00,-9"]),
    "layout": ("examples/room_layout_opt.star", ["--size", "1200x1200", "--view", "az=-22,el=58"]),
}
# One sheet of the house set, themed for the page, and the whole set as the CLI exports it.
SET = {"name": "set-a3", "prefix": "seta3", "script": "examples/house_set.star", "view": "sheet:A3.0", "title": "A small house on a slope, section and elevation", "pdf": "house_set.pdf"}
LAYOUT = "examples/room_layout_opt.star"
