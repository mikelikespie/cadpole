# Hidden-line view: the pillow block as a line drawing from the same
# camera a render uses, with the edges behind the part dashed.
load("../../examples/pillow_block.star", "block")
set = sheet_set("pillow_block_lines", size=(300, 300), units="in")
page = set.sheet("V1", "PICTORIAL")
# snippet
view = page.axon_view(block, az=-35, el=28,
                      hidden="dashed")
# end snippet
