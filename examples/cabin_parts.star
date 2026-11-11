# The parts of `cabin_site.star`: a room, a deck and a shed roof, each a
# function of its size. Drawn in feet, as a house is (one unit is one foot;
# `cadpole publish --model-unit ft` scales it once).
#
# A library file that also stands on its own: evaluated by itself it builds
# the existing shed the cabin replaces, which the site script puts on a
# hidden layer.

def room(name, w, d, h, at, layer, wall=0.5):
    """Four walls and a floor slab, `w` x `d` outside, `h` tall, at `at`."""
    x, y = at
    floor = box(w, d, wall, at=(x, y, 0), name=name + "_floor", layer=layer)
    south = box(w, wall, h, at=(x, y, wall), name=name + "_south", layer=layer)
    north = box(w, wall, h, at=(x, y + d - wall, wall), name=name + "_north", layer=layer)
    west = box(wall, d - 2 * wall, h, at=(x, y + wall, wall), name=name + "_west", layer=layer)
    east = box(wall, d - 2 * wall, h, at=(x + w - wall, y + wall, wall), name=name + "_east", layer=layer)
    for b in [south, north, west, east]:
        b.material(color="#c9b79c", roughness=0.8)
    floor.material(color="#8a8f96", roughness=0.9)
    return [floor, south, north, west, east]

def shed_roof(name, w, d, at, z, layer, overhang=1.5, t=0.75):
    """A flat roof plate with an overhang all round."""
    x, y = at
    r = box(w + 2 * overhang, d + 2 * overhang, t, at=(x - overhang, y - overhang, z), name=name, layer=layer)
    r.material(color="#3b3f45", roughness=0.6)
    return [r]

def deck(name, w, d, at, z, layer):
    """Boards on a frame: one plate, `z` up."""
    x, y = at
    b = box(w, d, 0.5, at=(x, y, z), name=name, layer=layer)
    b.material(preset="wood_oak")
    return [b]

EXISTING_SHED = room("shed", 10, 8, 7, (-14, 2), None)
