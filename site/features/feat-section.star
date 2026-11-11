# Section: the pillow block cut along its bore, the cut face hatched,
# dimensioned between points of the model.
load("../../examples/pillow_block.star", "block")
inch = 25.4
set = sheet_set("pillow_block_section", size=(220, 165), units="in")
page = set.sheet("S1", "SECTION A-A", scale=2)
# snippet
cut = page.section_view(block, plane="x=0",
                        beyond=False)
cut.dimension((-0.75 * inch, 0), (0.75 * inch, 0),
              offset=-10)
# end snippet
cut.dimension((-0.625 * inch, 2 * inch), (0.625 * inch, 2 * inch), offset=8)
cut.dimension((0.75 * inch, 0), (0.75 * inch, 0.5 * inch), offset=-8)
cut.dimension((0.625 * inch, 1 * inch), (0.625 * inch, 1.75 * inch), offset=-18)
cut.dimension((-0.75 * inch, 0), (-0.75 * inch, 2 * inch), offset=10)
