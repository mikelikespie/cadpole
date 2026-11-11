# A peg board: a plate with a grid of pegs, showing patterns and load-free reuse.

cols = param("cols", 4, min=1, max=10)
rows = param("rows", 3, min=1, max=10)
pitch = param("pitch", 15, unit="mm", min=8, max=40)
peg_r = param("peg_r", 3, unit="mm", min=1, max=8)
peg_h = param("peg_h", 10, unit="mm", min=2, max=40)

plate_w = cols * pitch
plate_d = rows * pitch
plate = box(plate_w, plate_d, 4, name="plate")

# First column of pegs, then copy it across. Each pattern returns its bodies,
# the source included, so `pegs` ends up holding every peg on the board.
first = cylinder(peg_r, peg_h, at=(pitch / 2, pitch / 2, 4), name="peg")
column = pattern_linear(first, rows, (0, pitch, 0), name="peg_row")
pegs = []
for i, peg in enumerate(column):
    pegs.extend(pattern_linear(peg, cols, (pitch, 0, 0), name="peg_col_%d" % i))

plate.material(color="#c9c0a8")

# Tests: `cadpole test examples/peg_board.star`. The sweep rebuilds the board at
# every combination of the two counts and checks each one.

def every_peg_is_there():
    assert_eq(len(pegs), rows * cols)
    for peg in pegs:
        assert_solid(peg)
        assert_volume(peg, math.pi * peg_r * peg_r * peg_h)
    assert_bbox(plate, (0, 0, 0), (plate_w, plate_d, 4))
    # The last peg sits half a pitch inside the far corner, standing on the plate.
    cx = plate_w - pitch / 2
    cy = plate_d - pitch / 2
    assert_bbox(pegs[-1], (cx - peg_r, cy - peg_r, 4), (cx + peg_r, cy + peg_r, 4 + peg_h))

test("rows x cols pegs, each on the plate", every_peg_is_there, sweep={"cols": [2, 4, 10], "rows": [2, 3]})
