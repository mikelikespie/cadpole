# A room scan as reference: a phone's LiDAR scan imported from glTF
# onto a ghosted display layer, and a parametric wall drawn over it with core
# operations -- the scan is the tracing paper, the walls are the model.
#
#   cadpole render examples/scan_underlay.star --view az=30,el=55 --layer walls=off -o /tmp/scan.png
#   cadpole render examples/scan_underlay.star --view az=30,el=55 --layer walls=0.4 -o /tmp/both.png
#   cadpole render examples/scan_underlay.star --layer scan=off -o /tmp/walls.png
#
# The scan is examples/assets/scan_room.glb: a synthetic 4 x 3 m room written
# by examples/tools/scan_room_glb.py in the layout a phone LiDAR scanning app
# writes -- one node, float positions, a colour per vertex, an unlit white
# material.
#
# glTF is +Y up in metres, and import_mesh turns it into Cadpole's +Z up in
# millimetres without being asked: (x, y, z) -> 1000 (x, -z, y), so the
# scan's back wall lands on y = 0 and its front wall on y = -3000. A file
# written some other way says so with up= and units=. The colours ride on
# the mesh, and the viewer and `cadpole render` draw them.
#
# A real scan is half a million to five million triangles, which wants
# repair=repair.off: the report's pairwise crossing search is skipped above
# 250 000 triangles anyway, and "off" says so up front; the census of open
# edges and shells still runs. Scans above 32 MB are read natively (a mesh
# may be 512 MB, `cadpole --max-asset-mb` to change it).

width = param("width", 4000, unit="mm", min=2400, max=8000, step=10, doc="Room width, inside (X): room for the doorway")
depth = param("depth", 3000, unit="mm", min=2000, max=8000, step=10, doc="Room depth, inside (Y)")
height = param("height", 2500, unit="mm", min=2100, max=4000, step=10, doc="Wall height: above the doorway")
t = param("wall", 120, unit="mm", min=60, max=300, step=5, doc="Wall thickness")
door_at = param("door_at", 1400, unit="mm", min=0, max=6000, step=10, doc="Doorway's left jamb, from the room's left wall")
door_w = param("door_w", 900, unit="mm", min=600, max=2000, step=10, doc="Doorway width")
door_h = param("door_h", 2050, unit="mm", min=1800, max=2500, step=10, doc="Doorway height")

# The layers, in the order the Layers panel lists them.
scan_layer = layer("scan", doc="The room as scanned: reference, not part of the model", opacity=0.5)
walls_layer = layer("walls", doc="The walls drawn over the scan")

# The scan: one body, named by the file's node, on the ghosted layer.
scan = scan_layer.add(import_mesh("scan_room.glb", name="scan", repair=repair.off))

# The doorway has to be in the front wall and under its top.
assert_true(door_at + door_w <= width, msg="the doorway runs past the room's right wall: move door_at left or narrow door_w")
assert_true(door_h < height, msg="the doorway is as tall as the walls or taller: lower door_h or raise height")


# The walls: boxes laid round the room's inside outline, `t` thick, the
# front wall in two pieces and a lintel over the doorway -- every number a
# parameter. (Boxes only, as layers_demo.star is: a body that goes through
# a boolean tessellates a hair differently on wasm, and every example must
# evaluate the same on both.)
def wall(name, x0, y0, x1, y1, z0=0):
    w = box(x1 - x0, y1 - y0, height - z0, at=(x0, y0, z0), name=name, layer=walls_layer)
    w.material(color=(0.86, 0.84, 0.79), roughness=0.9)
    return w

walls = [
    wall("back_wall", -t, 0, width + t, t),
    wall("left_wall", -t, -depth, 0, 0),
    wall("right_wall", width, -depth, width + t, 0),
    wall("front_left", -t, -depth - t, door_at, -depth),
    wall("front_right", door_at + door_w, -depth - t, width + t, -depth),
    wall("lintel", door_at, -depth - t, door_at + door_w, -depth, z0=door_h),
]


def the_scan_lands_in_millimetres_z_up_in_colour():
    r = import_report(scan)
    assert_eq(r.format, "glTF")
    assert_eq(r.object, "scan")
    assert_true(r.vertex_colors, msg="the scan's colours ride on its mesh")
    assert_true(r.pairs_skipped, msg="repair.off skips the crossing search")
    assert_true(not r.closed, msg="a scan is an open surface: no ceiling, a doorway")
    lo, hi = r.bbox
    # A scanner's jitter moves the surfaces a few millimetres; the room's
    # corners are where they were measured.
    assert_close(lo[0], 0, tol=5)
    assert_close(hi[0], 4000, tol=5)
    assert_close(lo[1], -3000, tol=5)
    assert_close(hi[1], 0, tol=5)
    assert_close(lo[2], 0, tol=5)
    assert_close(hi[2], 2500, tol=5)


test("the scan lands where the room is: millimetres, Z up, in its own colours", the_scan_lands_in_millimetres_z_up_in_colour)


def the_walls_stand_on_the_scan():
    slo, shi = import_report(scan).bbox
    lo = [1e9, 1e9, 1e9]
    hi = [-1e9, -1e9, -1e9]
    for w in walls:
        assert_solid(w)
        a, b = w.bbox()
        for k in range(3):
            lo[k] = min(lo[k], a[k])
            hi[k] = max(hi[k], b[k])
    # The walls' inside faces are the scan's surfaces, within the jitter.
    assert_close(lo[0], slo[0] - t, tol=5)
    assert_close(hi[0], shi[0] + t, tol=5)
    assert_close(lo[1], slo[1] - t, tol=5)
    assert_close(hi[1], shi[1] + t, tol=5)
    assert_close(hi[2], shi[2], tol=5)
    # The doorway the scan has is the one the walls leave.
    assert_close(walls[3].bbox()[1][0], 1400, tol=5)
    assert_close(walls[4].bbox()[0][0], 2300, tol=5)


test("the walls stand on the scan's outline", the_walls_stand_on_the_scan)



# The smallest room the sliders reach still has its doorway in the front wall.
def the_walls_are_solid():
    for w in walls:
        assert_solid(w)


test("the narrowest room", the_walls_are_solid, sweep={"width": [2400]})
test("the lowest room", the_walls_are_solid, sweep={"height": [2100]})
