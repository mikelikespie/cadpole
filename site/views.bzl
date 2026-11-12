"""The viewer pages the site publishes, one per model: `cadpole publish` runs
once for each, and the page's own 3D draws a mesh-only copy of the same
published model.

name:   the page is view/<name>/ and the packed model models/<name>.cpmesh
back:   the section of the site the page's home link returns to
script: the document, under //examples or //site/features
args:   further `cadpole publish` arguments (tolerance, parameters)
units:  how the page writes a length: inches for parts, feet and inches for buildings
"""

def _v(name, back, title, script, args = [], units = "in"):
    return struct(name = name, back = back, title = title, script = script, args = args, units = units)

def _building(name, back, title, script, args = []):
    return _v(name, back, title, "//examples:" + script, args, units = "ft-in")

def _feature(id, title):
    return _v("feat-" + id, "can", title, "features/%s.star" % id, ["--tolerance", "0.05"])

def _play(name, title, script):
    return _v(name, "play", title, "//examples:" + script, ["--tolerance", "0.05", "--show-params", "all"])

VIEWS = [
    _v("turbojet_cut", "turbojet", "Turbojet, cut away", "//examples:turbojet.star", ["--tolerance", "0.12", "-p", "cutaway=true"]),
    _v("turbojet_open", "turbojet", "Turbojet, apart", "//examples:turbojet.star", ["--tolerance", "0.12", "-p", "explode=18", "-p", "cutaway=true"]),
    _v("iris", "iris", "Iris diaphragm", "//examples:iris.star", ["--tolerance", "0.1", "-p", "opening=0.55"]),
    _v("mug", "mug", "Embossed mug", "//examples:embossed_mug.star", ["--tolerance", "0.15", "-p", "scene=single", "-p", "finish=raised"]),
    _v("radial", "radial", "Radial engine", "//examples:radial_engine.star", ["--tolerance", "0.25"]),
    _v("radial_open", "radial", "Radial engine, apart", "//examples:radial_engine.star", ["--tolerance", "0.25", "-p", "explode=30", "-p", "cutaway=true"]),
    _v("mascot", "gallery", "Desk mascot", "//examples:desk_mascot.star", ["--tolerance", "0.1"]),
    _v("gridfinity", "gallery", "Gridfinity set", "//examples:gridfinity_showcase.star", ["--tolerance", "0.15"]),
    _v("enclosure", "gallery", "Enclosure for a Raspberry Pi 5", "//examples:enclosure.star", ["--tolerance", "0.1", "-p", "lid_lift=26", "--eye", "112,-159,125", "--target", "0,0,22", "--show-params", "wall,clearance,closure,clip_len,barb,fastener,cooling,gpio,button,shell,lid_lift"]),
    _v("thumbscrew", "gallery", "Thumbscrew and nut", "//examples:thumbscrew.star", ["--tolerance", "0.1"]),
    _v("imports", "gallery", "Imported files", "//examples:import_demo.star", ["--tolerance", "0.15"]),
    _v("planetary", "gallery", "Herringbone planetary", "//examples:planetary.star", ["--tolerance", "0.15", "-p", "sun_angle=7.5", "-p", "chord=0.05"]),
    _v("wheel", "gallery", "Compressor wheel", "//examples:compressor_wheel.star", ["--tolerance", "0.15"]),
    _play("part", "Mounting plate", "part.star"),
    _play("planter", "Planter", "planter.star"),
    _play("bracket", "Pivot bracket", "bracket.star"),
    _play("knob", "Knurled knob", "knurled_knob.star"),
    # Opens cut like a plan, from the preview's camera (az -35, el 55): the
    # roof and the walls above the 4 ft cut hidden, both a switch away in Layers.
    _building("plan-cabin", "plan", "Cabin", "cabin_plan.star", ["--hide-layer", "roof", "--hide-layer", "above_cut", "--eye=-3602,-9247,20279", "--target=4420,2210,305"]),
    # From the downhill side (az -115, el 24), where the levels step down the slope.
    _building("lot", "site", "Hillside lot", "hillside_lot.star", ["--eye=-41243,34551,21117", "--target=6706,12192,-2438"]),
    # From the south-south-west (az 20, el 55), down the slot to the light court and the lantern.
    _building("shadow", "sun", "Shadow study", "shadow_study.star", ["--eye=17727,-25474,53405", "--target=4572,10668,-1524"]),
    _building("layout", "layout", "Rooms placed by the solver", "room_layout_opt.star", ["--eye=-578,-6198,18094", "--target=3658,4285,0"]),
    _feature("params", "Parameters"),
    _feature("extrude", "Extrude"),
    _feature("revolve", "Revolve"),
    _feature("sweep", "Sweep"),
    _feature("loft", "Loft"),
    _feature("boolean", "Booleans"),
    _feature("thread", "Threads"),
    _feature("chamfer", "Chamfer"),
    _feature("fillet", "Fillet"),
    _feature("mirror", "Mirror"),
    _feature("linear-pattern", "Linear pattern"),
    _feature("circular-pattern", "Circular pattern"),
    _feature("constraints", "Sketch constraints"),
    _feature("offset", "Offset"),
    _feature("text", "Text"),
    _feature("emboss", "Emboss"),
    _feature("smooth-blend", "Smooth blend"),
    _feature("twist", "Twist"),
    _feature("section", "Section"),
    _feature("drawing", "Drawing"),
]

# The gallery's print files: `cadpole plate` on the model as written, so
# every piece is oriented, packed and coloured by the planner
# (docs/plating.md). A gallery card whose name is here gets a download of
# print/<name>.3mf. Laid out for the X1 Carbon's bed, which the P1 and A1
# share; a slicer moves the plates to another printer.
def _print(name, script, args = []):
    return struct(name = name, script = "//examples:" + script, args = ["--printer", "bambu_x1c"] + args)

PRINTS = [
    _print("mascot", "desk_mascot.star"),
    # The two printed parts; the board is the thing the case is for.
    _print("enclosure", "enclosure.star", ["--layer", "base", "--layer", "lid"]),
    # The thread form that prints standing up without support.
    _print("thumbscrew", "thumbscrew.star", ["-p", "thread_form=print"]),
]
