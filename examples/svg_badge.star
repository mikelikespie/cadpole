# A badge cut from an imported SVG.
#
# `asset("badge.svg")` names a file the document carries, not a path: the CLI
# picks it up from ./assets next to this script, a packed document embeds it,
# and the browser gets it from IndexedDB. Nested subpaths in the drawing come
# in as holes, and its circles and arcs stay real arcs, so the rivet holes
# extrude to cylinders rather than faceted splines.
#
#   cadpole render examples/svg_badge.star -o /tmp/badge.png

thickness = param("thickness", 2.4, unit="mm", min=0.8, max=10, doc="Badge thickness")
scale = param("scale", 1.0, min=0.4, max=3, doc="Enlarge or shrink the artwork")

# `ids=` takes the artwork and its detail and leaves the stroked guide line
# out; that one is unfilled, so it would have come in as a construction path
# (`art.paths`) rather than a face. `align="center"` puts the drawing's
# bounding-box centre at `at`.
art = import_svg(
    asset("badge.svg"),
    ids = ["badge", "marks"],
    align = "center",
    at = (0, 0),
    scale = scale,
    name = "art",
)

badge = extrude(art, thickness, name = "badge")
badge.material(preset = "pla_matte", color = "#3d6ea8")
