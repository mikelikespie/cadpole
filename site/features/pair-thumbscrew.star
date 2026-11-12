# A knurled thumbscrew and its nut as a line drawing from the camera its render
# uses: the edges in sight solid, the ones behind dashed, the thread inside the
# nut among them. The view fills the drawing area of a portrait A4 sheet at a
# standard enlargement.
load("../../examples/thumbscrew.star", "screw", "nut")
set = sheet_set("Thumbscrew and nut", size=(210, 297), units="mm")
page = set.sheet("V1", "PICTORIAL", scale="4:1")
view = page.axon_view([screw, nut], az=-30, el=24, hidden="dashed", box=set.area())
