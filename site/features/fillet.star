# Fillet: selected edges rounded at a constant radius.
block = box(40, 24, 12, center=True, name="block")
# snippet
sides = block.edges(direction=(0, 0, 1))
part = fillet(block, 6, edges=sides, name="part")
top = part.faces(normal=(0, 0, 1)).one()
part = fillet(part, 2, edges=top.edges, name="soft")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
