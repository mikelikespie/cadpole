# Chamfer: selected edges cut back to a flat.
block = box(40, 24, 12, center=True, name="block")
# snippet
top = block.faces(normal=(0, 0, 1)).one()
part = chamfer(block, 3, edges=top.edges,
               name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
