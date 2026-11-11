# A knurled thumbscrew and its nut as a line drawing from the camera its render
# uses: the edges in sight solid, the ones behind dashed, the thread inside the
# nut among them.
load("../../examples/thumbscrew.star", "screw", "nut")
set = sheet_set("thumbscrew_lines", size=(300, 300), units="in")
page = set.sheet("V1", "PICTORIAL")
view = page.axon_view([screw, nut], az=-30, el=24, hidden="dashed")
