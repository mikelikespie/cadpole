# A cabin on its lot, to publish as a static web page:
#
#   cadpole publish examples/cabin_site.star --model-unit ft --out-dir /tmp/cabin
#   python3 -m http.server -d /tmp/cabin 8000     # then open http://127.0.0.1:8000/
#
# The page is Cadpole's own web app opening the model `cadpole publish` evaluated
# ahead of time -- no Starlark runs in the browser, and none is shipped: the
# bake carries the geometry, the names, the layers and the materials, and
# not this comment, the parts file it loads or what it prints.
#
# What it shows:
#
#   * **a document of more than one file.** The rooms, the deck and the roof
#     are functions in `cabin_parts.star`, loaded beside this one; `cadpole
#     publish` evaluates them together, exactly as `cadpole eval` does;
#   * **a model drawn in feet** with no `units(...)` of its own, the way a
#     house sketch often starts. `--model-unit ft` scales it to millimetres
#     once, at the bake, and the page reads in feet and inches;
#   * **layers a page respects.** The shed the cabin replaces is on a layer
#     declared hidden: the published page opens without it, and its Layers
#     panel brings it back without evaluating anything.
#
# A published page carries the bodies and never the script: this comment
# (PUBLISH-PRIVACY-MARKER) and the line the script prints below are in no
# file `cadpole publish` writes.

load("cabin_parts.star", "room", "shed_roof", "deck", "EXISTING_SHED")

width = param("width", 24, min=16, max=40, doc="Cabin width (ft)")
depth = param("depth", 16, min=12, max=24, doc="Cabin depth (ft)")
height = param("height", 9, min=8, max=12, doc="Wall height (ft)")

cabin = layer("cabin", doc="The new cabin")
site = layer("site", doc="The lot and the deck", color="#6f8f5a")
existing = layer("existing", doc="The shed it replaces", visible=False, color="#b05030")

walls = room("cabin", width, depth, height, (0, 0), cabin)
roof = shed_roof("cabin_roof", width, depth, (0, 0), height + 0.5, cabin)
porch = deck("deck", width, 8, (0, -8), 0, site)
lot = box(60, 50, 0.1, at=(-20, -15, -0.1), name="lot", layer=site)
lot.material(color="#5d7a4a", roughness=1.0)
existing.add(*EXISTING_SHED)

print("PUBLISH-PRIVACY-MARKER: the budget is a secret")

def the_cabin_stands_on_its_lot():
    lo, hi = lot.bbox()
    for b in walls + roof + porch:
        blo, bhi = b.bbox()
        assert_true(blo[0] >= lo[0] and bhi[0] <= hi[0] and blo[1] >= lo[1] and bhi[1] <= hi[1], msg=b.name + " is on the lot")
    assert_true(len(walls) == 5, msg="a floor and four walls")

test("the cabin stands on its lot", the_cabin_stands_on_its_lot)
