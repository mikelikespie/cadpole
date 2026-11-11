# Threads: a real helical thread from the standard library.
# snippet
load("@cadpole/mechanical.star", "bolt", "nut")
screw = bolt(12, 2, 26, 19, 7, name="screw")
ring = nut(12, 2, 19, 8, start=(0, 0, 9),
           name="ring")
# end snippet
screw.material(color="#6fbfa8", roughness=0.4)
ring.material(color="#f0c987", roughness=0.4)
