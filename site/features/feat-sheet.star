# Dimensioned sheet: front, top, right and a pictorial view of the pillow
# block on one letter-size page, dimensioned in inches.
load("../../examples/pillow_block.star", "block")
# snippet
sheet = drawing(block, view="three-view",
                sheet="ANSI_A", units="in",
                title="Pillow block")
export_pdf(sheet, "pillow_block.pdf")
export_dxf(sheet, "pillow_block.dxf")
# end snippet
