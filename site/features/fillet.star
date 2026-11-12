# Fillet: the edges you select rounded, each set at its own radius. A
# machined pedestal: the base plate's four corners at R6 and the boss's top
# rim at R5; every edge left out of both sets stays sharp.
plate = box(52, 30, 6, at=(-26, -15, 0), name="plate")
boss = box(24, 20, 16, at=(-12, -10, 5), name="boss")   # 1 mm into the plate, so the union crosses it
holes = [cylinder(2.6, 8, at=(x, 0, -1)) for x in [-19, 19]]
block = subtract(union(plate, boss), holes, name="block")
# snippet
corners = block.edges(direction=(0, 0, 1), below=XY.offset(6.01),
                      min_dihedral=45)
part = fillet(block, 6, edges=corners, name="part")
top = part.faces(normal=(0, 0, 1)).max("z")
part = fillet(part, 5, edges=top.edges, name="soft")
# end snippet
part.material(color="#e0b48a", roughness=0.38, metallic=0.25)


# Tests: `cadpole test site/features/fillet.star`.

def only_the_selected_edges_are_round():
    assert_solid(part)
    blends = part.faces(name="*blend*")
    assert_true(len(blends) == 8, msg="four plate corners and four rim edges, got %d blends" % len(blends))
    # Edges left out stay sharp: the boss's own vertical corners, the plate's
    # top rim and the holes' mouths are still straight or circular creases
    # between two flat faces.
    sharp = part.edges(direction=(0, 0, 1), above=XY.offset(5.99), min_dihedral=45, allow_empty=True)
    assert_true(len(sharp) == 4, msg="the boss's four vertical corners are sharp, got %d" % len(sharp))
    plate_top = part.edges(on_plane=XY.offset(6), convex=True, kind="line", allow_empty=True)
    assert_true(len(plate_top) >= 4, msg="the plate's top edges are sharp")
    assert_true(abs(part.bbox()[1][2] - 21) < 1e-6, msg="the boss's top is 21 mm up")
    return len(blends)

test("two edge sets, two radii, the rest sharp", only_the_selected_edges_are_round)
