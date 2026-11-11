"""@cadpole/plan_spans.star -- span tables as data, and the rules that size what the tables do not.

    load("@cadpole/plan_spans.star", "joist_span", "rafter_span", "header_span", "header_for", "ijoist_span",
         "lvl_beam", "steel_beam", "joist_options", "ftin", "span_ft", "SAWN")

The tables @cadpole/plan_frame.star frames a plan with. Every length is in feet, as a plan is;
loads are psf and plf. Nothing here records a feature, so loading it is pure (probe: `joist_span`).

## What is a code table, and what is a rule of thumb

Read from the code (CRC 2022, which adopts the IRC 2021 with amendments -- an unverified reading until a plans examiner or an engineer has looked):

| Data | Table | What it covers |
| --- | --- | --- |
| `FLOOR_40` | R502.3.1(2) | floor joists, 40 psf live, L/360, dead 10 or 20 psf; DF-L, HF, SP, SPF; SS, #1, #2, #3; 2x6..2x12 at 12, 16, 19.2, 24 in |
| `FLOOR_30` | R502.3.1(1) | the same at 30 psf live: sleeping areas and attics with fixed stairs only |
| `RAFTER_20` | R802.4.1(2) | rafters, 20 psf roof live load, ceiling attached, L/240: a flat roof's joists with a ceiling under them |
| `HEADERS_EXT` | R602.7(1) | girders and headers in exterior bearing walls, by what they carry, ground snow load and building width |
| `HEADERS_INT` | R602.7(2) | girders and headers in interior bearing walls: one floor only, or two floors |

The rows are kept exactly as the code prints them ("16-6" is 16'-6"), so any row can be checked against the page
by eye. Two cross-checks: the City of Sacramento's handout CDD-0277 (rev. 2023-01-01) prints the same DF-L No. 2
rows of R502.3.1(2) and R802.4.1(2); and a DF-L No. 2 2x12 at 16 in spans 16'-6" at
20 psf dead, 18'-1" at 10.

Not read from any table, so marked `rule` wherever they appear (each is a planning aid, not engineering):
- **I-joists** (`ijoist_span`): span = depth (in) x K(spacing), capped. K was set at or under a mid-range series
  (2 5/16-in flanges) in a typical manufacturer's 40/20-psf, L/480 table. Use the maker's own table.
- **Roof and deck loads on a floor table**: an I-joist roof spans 1.15 x its floor span; a deck (Table R301.5:
  60 psf) spans 0.86 x the 40-psf floor span (deflection scales as (40/60)^(1/3) = 0.87, strength as
  (60/80)^(1/2) = 0.87).
- **LVL and steel beams** (`lvl_beam`, `steel_beam`): simple-span bending, shear and deflection (L/360 live,
  L/240 total) with nominal design values -- LVL 2.0E: Fb 2,600 psi, Fv 285 psi, E 2,000,000 psi; steel A992:
  0.6 Fy on Sx, E 29,000 ksi -- and section properties recalled from the AISC shapes database (`W_SHAPES`,
  unverified). No lateral bracing, bearing, connection, point-load or load-combination checks.

## Caveats that change numbers

- **Dead load.** R301.2.2.2 caps a floor assembly's average dead load at 10 psf in Seismic Design Categories D0,
  D1 and D2 (much of California is in them), and both floor tables' footnotes send those categories there. So the
  20-psf column (a finished ceiling, tile, gypsum concrete) is outside the prescriptive path there: such a floor
  is engineered. `@cadpole/plan_frame.star` plans at 20 psf by default anyway, because it is the shorter span and the
  real floor will be engineered; pass dead=10 for a light, prescriptive floor.
- **Headers.** R602.7(1) and (2) assume the header's top is braced by the framing over it; with cripple studs on
  it, 2x8, 2x10 and 2x12 spans are multiplied by 0.70 (`braced=False`). Building width is interpolated between
  the 12, 24 and 36-ft columns, as footnote c permits; below 12 ft the 12-ft column is used (conservative), and past
  36 ft the table has no answer.
- **R802.4.1(2)**: "Note b" (over 26 ft) is read as 26'-0".
- Where ground snow load is under 30 psf and the roof live load is 20 psf or less, R602.7(1) uses the 30-psf
  column (its footnote e): that is the low-snow case, and the default.
"""

# ---------------------------------------------------------------------------------------------------------- data

SAWN = {  # nominal -> actual (b, d) inches, for dimension lumber (dressed, dry)
    "2x4": (1.5, 3.5),
    "2x6": (1.5, 5.5),
    "2x8": (1.5, 7.25),
    "2x10": (1.5, 9.25),
    "2x12": (1.5, 11.25),
}

SPECIES = {"DF-L": "Douglas fir-larch", "HF": "Hem-fir", "SP": "Southern pine", "SPF": "Spruce-pine-fir"}
GRADES = ["SS", "#1", "#2", "#3"]
GRADE_NAMES = {"SS": "Select Structural", "#1": "No. 1", "#2": "No. 2", "#3": "No. 3"}
SPACINGS = [12, 16, 19.2, 24]  # inches on centre

# Table R502.3.1(2): floor joist spans, residential living areas, live load 40 psf, L/360. Each row: spacing (in),
# species, grade, the 2x6 2x8 2x10 2x12 spans at 10 psf dead, the same at 20 psf dead, as printed ("16-10a": its
# footnote a, end bearing increased to 2 in).
FLOOR_40 = {
    "table": "R502.3.1(2)",
    "title": "Floor joist spans for common lumber species (residential living areas, live load = 40 psf, L/360)",
    "live": 40,
    "deflection": 360,
    "sizes": ["2x6", "2x8", "2x10", "2x12"],
    "rows": [
        (12, "DF-L", "SS", "11-4 15-0 19-1 23-3", "11-4 15-0 19-1 23-3"),
        (12, "DF-L", "#1", "10-11 14-5 18-5 22-0", "10-11 14-2 17-4 20-1"),
        (12, "DF-L", "#2", "10-9 14-2 18-0 20-11", "10-8 13-6 16-5 19-1"),
        (12, "DF-L", "#3", "8-11 11-3 13-9 16-0", "8-1 10-3 12-7 14-7"),
        (12, "HF", "SS", "10-9 14-2 18-0 21-11", "10-9 14-2 18-0 21-11"),
        (12, "HF", "#1", "10-6 13-10 17-8 21-6", "10-6 13-10 17-1 19-10"),
        (12, "HF", "#2", "10-0 13-2 16-10 20-4", "10-0 13-1 16-0 18-6"),
        (12, "HF", "#3", "8-8 11-0 13-5 15-7", "7-11 10-0 12-3 14-3"),
        (12, "SP", "SS", "11-2 14-8 18-9 22-10", "11-2 14-8 18-9 22-10"),
        (12, "SP", "#1", "10-9 14-2 18-0 21-11", "10-9 14-2 16-11 20-1"),
        (12, "SP", "#2", "10-3 13-6 16-2 19-1", "9-10 12-6 14-9 17-5"),
        (12, "SP", "#3", "8-2 10-3 12-6 14-9", "7-5 9-5 11-5 13-6"),
        (12, "SPF", "SS", "10-6 13-10 17-8 21-6", "10-6 13-10 17-8 21-6"),
        (12, "SPF", "#1", "10-3 13-6 17-3 20-7", "10-3 13-3 16-3 18-10"),
        (12, "SPF", "#2", "10-3 13-6 17-3 20-7", "10-3 13-3 16-3 18-10"),
        (12, "SPF", "#3", "8-8 11-0 13-5 15-7", "7-11 10-0 12-3 14-3"),
        (16, "DF-L", "SS", "10-4 13-7 17-4 21-1", "10-4 13-7 17-4 21-1"),
        (16, "DF-L", "#1", "9-11 13-1 16-5 19-1", "9-8 12-4 15-0 17-5"),
        (16, "DF-L", "#2", "9-9 12-9 15-7 18-1", "9-3 11-8 14-3 16-6"),
        (16, "DF-L", "#3", "7-8 9-9 11-11 13-10", "7-0 8-11 10-11 12-7"),
        (16, "HF", "SS", "9-9 12-10 16-5 19-11", "9-9 12-10 16-5 19-11"),
        (16, "HF", "#1", "9-6 12-7 16-0 18-10", "9-6 12-2 14-10 17-2"),
        (16, "HF", "#2", "9-1 12-0 15-2 17-7", "8-11 11-4 13-10 16-1"),
        (16, "HF", "#3", "7-6 9-6 11-8 13-6", "6-10 8-8 10-7 12-4"),
        (16, "SP", "SS", "10-2 13-4 17-0 20-9", "10-2 13-4 17-0 20-9"),
        (16, "SP", "#1", "9-9 12-10 16-1 19-1", "9-9 12-7 14-8 17-5"),
        (16, "SP", "#2", "9-4 11-10 14-0 16-6", "8-6 10-10 12-10 15-1"),
        (16, "SP", "#3", "7-1 8-11 10-10 12-10", "6-5 8-2 9-10 11-8"),
        (16, "SPF", "SS", "9-6 12-7 16-0 19-6", "9-6 12-7 16-0 19-6"),
        (16, "SPF", "#1", "9-4 12-3 15-5 17-10", "9-1 11-6 14-1 16-3"),
        (16, "SPF", "#2", "9-4 12-3 15-5 17-10", "9-1 11-6 14-1 16-3"),
        (16, "SPF", "#3", "7-6 9-6 11-8 13-6", "6-10 8-8 10-7 12-4"),
        (19.2, "DF-L", "SS", "9-8 12-10 16-4 19-10", "9-8 12-10 16-4 19-6"),
        (19.2, "DF-L", "#1", "9-4 12-4 15-0 17-5", "8-10 11-3 13-8 15-11"),
        (19.2, "DF-L", "#2", "9-2 11-8 14-3 16-6", "8-5 10-8 13-0 15-1"),
        (19.2, "DF-L", "#3", "7-0 8-11 10-11 12-7", "6-5 8-2 9-11 11-6"),
        (19.2, "HF", "SS", "9-2 12-1 15-5 18-9", "9-2 12-1 15-5 18-9"),
        (19.2, "HF", "#1", "9-0 11-10 14-10 17-2", "8-9 11-1 13-6 15-8"),
        (19.2, "HF", "#2", "8-7 11-3 13-10 16-1", "8-2 10-4 12-8 14-8"),
        (19.2, "HF", "#3", "6-10 8-8 10-7 12-4", "6-3 7-11 9-8 11-3"),
        (19.2, "SP", "SS", "9-6 12-7 16-0 19-6", "9-6 12-7 16-0 19-6"),
        (19.2, "SP", "#1", "9-2 12-1 14-8 17-5", "9-0 11-5 13-5 15-11"),
        (19.2, "SP", "#2", "8-6 10-10 12-10 15-1", "7-9 9-10 11-8 13-9"),
        (19.2, "SP", "#3", "6-5 8-2 9-10 11-8", "5-11 7-5 9-0 10-8"),
        (19.2, "SPF", "SS", "9-0 11-10 15-1 18-4", "9-0 11-10 15-1 17-9"),
        (19.2, "SPF", "#1", "8-9 11-6 14-1 16-3", "8-3 10-6 12-10 14-10"),
        (19.2, "SPF", "#2", "8-9 11-6 14-1 16-3", "8-3 10-6 12-10 14-10"),
        (19.2, "SPF", "#3", "6-10 8-8 10-7 12-4", "6-3 7-11 9-8 11-3"),
        (24, "DF-L", "SS", "9-0 11-11 15-2 18-5", "9-0 11-11 15-0 17-5"),
        (24, "DF-L", "#1", "8-8 11-0 13-5 15-7", "7-11 10-0 12-3 14-3"),
        (24, "DF-L", "#2", "8-3 10-5 12-9 14-9", "7-6 9-6 11-8 13-6"),
        (24, "DF-L", "#3", "6-3 8-0 9-9 11-3", "5-9 7-3 8-11 10-4"),
        (24, "HF", "SS", "8-6 11-3 14-4 17-5", "8-6 11-3 14-4 16-10a"),
        (24, "HF", "#1", "8-4 10-10 13-3 15-5", "7-10 9-11 12-1 14-0"),
        (24, "HF", "#2", "7-11 10-2 12-5 14-4", "7-4 9-3 11-4 13-1"),
        (24, "HF", "#3", "6-2 7-9 9-6 11-0", "5-7 7-1 8-8 10-1"),
        (24, "SP", "SS", "8-10 11-8 14-11 18-1", "8-10 11-8 14-11 18-0"),
        (24, "SP", "#1", "8-6 11-3 13-1 15-7", "8-1 10-3 12-0 14-3"),
        (24, "SP", "#2", "7-7 9-8 11-5 13-6", "7-0 8-10 10-5 12-4"),
        (24, "SP", "#3", "5-9 7-3 8-10 10-5", "5-3 6-8 8-1 9-6"),
        (24, "SPF", "SS", "8-4 11-0 14-0 17-0", "8-4 11-0 13-8 15-11"),
        (24, "SPF", "#1", "8-1 10-3 12-7 14-7", "7-5 9-5 11-6 13-4"),
        (24, "SPF", "#2", "8-1 10-3 12-7 14-7", "7-5 9-5 11-6 13-4"),
        (24, "SPF", "#3", "6-2 7-9 9-6 11-0", "5-7 7-1 8-8 10-1"),
    ],
}

# Table R502.3.1(1): the same at 30 psf live -- sleeping areas, and attics reached by a fixed stair (R311.7).
FLOOR_30 = {
    "table": "R502.3.1(1)",
    "title": "Floor joist spans for common lumber species (residential sleeping areas, live load = 30 psf, L/360)",
    "live": 30,
    "deflection": 360,
    "sizes": ["2x6", "2x8", "2x10", "2x12"],
    "rows": [
        (12, "DF-L", "SS", "12-6 16-6 21-0 25-7", "12-6 16-6 21-0 25-7"),
        (12, "DF-L", "#1", "12-0 15-10 20-3 24-8", "12-0 15-7 19-0 22-0"),
        (12, "DF-L", "#2", "11-10 15-7 19-10 23-4", "11-8 14-9 18-0 20-11"),
        (12, "DF-L", "#3", "9-11 12-7 15-5 17-10", "8-11 11-3 13-9 16-0"),
        (12, "HF", "SS", "11-10 15-7 19-10 24-2", "11-10 15-7 19-10 24-2"),
        (12, "HF", "#1", "11-7 15-3 19-5 23-7", "11-7 15-3 18-9 21-9"),
        (12, "HF", "#2", "11-0 14-6 18-6 22-6", "11-0 14-4 17-6 20-4"),
        (12, "HF", "#3", "9-8 12-4 15-0 17-5", "8-8 11-0 13-5 15-7"),
        (12, "SP", "SS", "12-3 16-2 20-8 25-1", "12-3 16-2 20-8 25-1"),
        (12, "SP", "#1", "11-10 15-7 19-10 24-2", "11-10 15-7 18-7 22-0"),
        (12, "SP", "#2", "11-3 14-11 18-1 21-4", "10-9 13-8 16-2 19-1"),
        (12, "SP", "#3", "9-2 11-6 14-0 16-6", "8-2 10-3 12-6 14-9"),
        (12, "SPF", "SS", "11-7 15-3 19-5 23-7", "11-7 15-3 19-5 23-7"),
        (12, "SPF", "#1", "11-3 14-11 19-0 23-0", "11-3 14-7 17-9 20-7"),
        (12, "SPF", "#2", "11-3 14-11 19-0 23-0", "11-3 14-7 17-9 20-7"),
        (12, "SPF", "#3", "9-8 12-4 15-0 17-5", "8-8 11-0 13-5 15-7"),
        (16, "DF-L", "SS", "11-4 15-0 19-1 23-3", "11-4 15-0 19-1 23-3"),
        (16, "DF-L", "#1", "10-11 14-5 18-5 21-4", "10-8 13-6 16-5 19-1"),
        (16, "DF-L", "#2", "10-9 14-2 17-5 20-3", "10-1 12-9 15-7 18-1"),
        (16, "DF-L", "#3", "8-7 10-11 13-4 15-5", "7-8 9-9 11-11 13-10"),
        (16, "HF", "SS", "10-9 14-2 18-0 21-11", "10-9 14-2 18-0 21-11"),
        (16, "HF", "#1", "10-6 13-10 17-8 21-1", "10-6 13-4 16-3 18-10"),
        (16, "HF", "#2", "10-0 13-2 16-10 19-8", "9-10 12-5 15-2 17-7"),
        (16, "HF", "#3", "8-5 10-8 13-0 15-1", "7-6 9-6 11-8 13-6"),
        (16, "SP", "SS", "11-2 14-8 18-9 22-10", "11-2 14-8 18-9 22-10"),
        (16, "SP", "#1", "10-9 14-2 18-0 21-4", "10-9 13-9 16-1 19-1"),
        (16, "SP", "#2", "10-3 13-3 15-8 18-6", "9-4 11-10 14-0 16-6"),
        (16, "SP", "#3", "7-11 10-0 11-1 14-4", "7-1 8-11 10-10 12-10"),
        (16, "SPF", "SS", "10-6 13-10 17-8 21-6", "10-6 13-10 17-8 21-4"),
        (16, "SPF", "#1", "10-3 13-6 17-2 19-11", "9-11 12-7 15-5 17-10"),
        (16, "SPF", "#2", "10-3 13-6 17-2 19-11", "9-11 12-7 15-5 17-10"),
        (16, "SPF", "#3", "8-5 10-8 13-0 15-1", "7-6 9-6 11-8 13-6"),
        (19.2, "DF-L", "SS", "10-8 14-1 18-0 21-10", "10-8 14-1 18-0 21-4"),
        (19.2, "DF-L", "#1", "10-4 13-7 16-9 19-6", "9-8 12-4 15-0 17-5"),
        (19.2, "DF-L", "#2", "10-1 13-0 15-11 18-6", "9-3 11-8 14-3 16-6"),
        (19.2, "DF-L", "#3", "7-10 10-0 12-2 14-1", "7-0 8-11 10-11 12-7"),
        (19.2, "HF", "SS", "10-1 13-4 17-0 20-8", "10-1 13-4 17-0 20-7"),
        (19.2, "HF", "#1", "9-10 13-0 16-7 19-3", "9-7 12-2 14-10 17-2"),
        (19.2, "HF", "#2", "9-5 12-5 15-6 17-1", "8-11 11-4 13-10 16-1"),
        (19.2, "HF", "#3", "7-8 9-9 11-10 13-9", "6-10 8-8 10-7 12-4"),
        (19.2, "SP", "SS", "10-6 13-10 17-8 21-6", "10-6 13-10 17-8 21-6"),
        (19.2, "SP", "#1", "10-1 13-4 16-5 19-6", "9-11 12-7 14-8 17-5"),
        (19.2, "SP", "#2", "9-6 12-1 14-4 16-10", "8-6 10-10 12-10 15-1"),
        (19.2, "SP", "#3", "7-3 9-1 11-0 13-1", "6-5 8-2 9-10 11-8"),
        (19.2, "SPF", "SS", "9-10 13-0 16-7 20-2", "9-10 13-0 16-7 19-6"),
        (19.2, "SPF", "#1", "9-8 12-9 15-8 18-3", "9-1 11-6 14-1 16-3"),
        (19.2, "SPF", "#2", "9-8 12-9 15-8 18-3", "9-1 11-6 14-1 16-3"),
        (19.2, "SPF", "#3", "7-8 9-9 11-10 13-9", "6-10 8-8 10-7 12-4"),
        (24, "DF-L", "SS", "9-11 13-1 16-8 20-3", "9-11 13-1 16-5 19-1"),
        (24, "DF-L", "#1", "9-7 12-4 15-0 17-5", "8-8 11-0 13-5 15-7"),
        (24, "DF-L", "#2", "9-3 11-8 14-3 16-6", "8-3 10-5 12-9 14-9"),
        (24, "DF-L", "#3", "7-0 8-11 10-11 12-7", "6-3 8-0 9-9 11-3"),
        (24, "HF", "SS", "9-4 12-4 15-9 19-2", "9-4 12-4 15-9 18-5"),
        (24, "HF", "#1", "9-2 12-1 14-10 17-2", "8-7 10-10 13-3 15-5"),
        (24, "HF", "#2", "8-9 11-4 13-10 16-1", "8-0 10-2 12-5 14-4"),
        (24, "HF", "#3", "6-10 8-8 10-7 12-4", "6-2 7-9 9-6 11-0"),
        (24, "SP", "SS", "9-9 12-10 16-5 19-11", "9-9 12-10 16-5 19-8"),
        (24, "SP", "#1", "9-4 12-4 14-8 17-5", "8-10 11-3 13-1 15-7"),
        (24, "SP", "#2", "8-6 10-10 12-10 15-1", "7-7 9-8 11-5 13-6"),
        (24, "SP", "#3", "6-5 8-2 9-10 11-8", "5-9 7-3 8-10 10-5"),
        (24, "SPF", "SS", "9-2 12-1 15-5 18-9", "9-2 12-1 15-0 17-5"),
        (24, "SPF", "#1", "8-11 11-6 14-1 16-3", "8-1 10-3 12-7 14-7"),
        (24, "SPF", "#2", "8-11 11-6 14-1 16-3", "8-1 10-3 12-7 14-7"),
        (24, "SPF", "#3", "6-10 8-8 10-7 12-4", "6-2 7-9 9-6 11-0"),
    ],
}

# Table R802.4.1(2): rafter spans, roof live load 20 psf, ceiling attached to the rafters, L/240. "NoteB" is the
# table's note b, "span exceeds 26 feet in length" (read as 26'-0"). Spans are horizontal projections.
RAFTER_20 = {
    "table": "R802.4.1(2)",
    "title": "Rafter spans for common lumber species (roof live load = 20 psf, ceiling attached to rafters, L/240)",
    "live": 20,
    "deflection": 240,
    "sizes": ["2x4", "2x6", "2x8", "2x10", "2x12"],
    "rows": [
        (12, "DF-L", "SS", "10-5 16-4 21-7 NoteB NoteB", "10-5 16-4 21-7 NoteB NoteB"),
        (12, "DF-L", "#1", "10-0 15-9 20-10 NoteB NoteB", "10-0 15-4 19-5 23-9 NoteB"),
        (12, "DF-L", "#2", "9-10 15-6 20-5 26-0 NoteB", "9-10 14-7 18-5 22-6 26-0"),
        (12, "DF-L", "#3", "8-9 12-10 16-3 19-10 23-0", "7-7 11-1 14-1 17-2 19-11"),
        (12, "HF", "SS", "9-10 15-6 20-5 NoteB NoteB", "9-10 15-6 20-5 NoteB NoteB"),
        (12, "HF", "#1", "9-8 15-2 19-11 25-5 NoteB", "9-8 15-2 19-2 23-5 NoteB"),
        (12, "HF", "#2", "9-2 14-5 19-0 24-3 NoteB", "9-2 14-2 17-11 21-11 25-5"),
        (12, "HF", "#3", "8-7 12-6 15-10 19-5 22-6", "7-5 10-10 13-9 16-9 19-6"),
        (12, "SP", "SS", "10-3 16-1 21-2 NoteB NoteB", "10-3 16-1 21-2 NoteB NoteB"),
        (12, "SP", "#1", "9-10 15-6 20-5 NoteB NoteB", "9-10 15-6 19-10 23-2 NoteB"),
        (12, "SP", "#2", "9-5 14-9 19-6 23-5 NoteB", "9-0 13-6 17-1 20-3 23-10"),
        (12, "SP", "#3", "8-0 11-9 14-10 18-0 21-4", "6-11 10-2 12-10 15-7 18-6"),
        (12, "SPF", "SS", "9-8 15-2 19-11 25-5 NoteB", "9-8 15-2 19-11 25-5 NoteB"),
        (12, "SPF", "#1", "9-5 14-9 19-6 24-10 NoteB", "9-5 14-4 18-2 22-3 25-9"),
        (12, "SPF", "#2", "9-5 14-9 19-6 24-10 NoteB", "9-5 14-4 18-2 22-3 25-9"),
        (12, "SPF", "#3", "8-7 12-6 15-10 19-5 22-6", "7-5 10-10 13-9 16-9 19-6"),
        (16, "DF-L", "SS", "9-6 14-11 19-7 25-0 NoteB", "9-6 14-11 19-7 25-0 NoteB"),
        (16, "DF-L", "#1", "9-1 14-4 18-11 23-9 NoteB", "9-1 13-3 16-10 20-7 23-10"),
        (16, "DF-L", "#2", "8-11 14-1 18-5 22-6 26-0", "8-7 12-7 16-0 19-6 22-7"),
        (16, "DF-L", "#3", "7-7 11-1 14-1 17-2 19-11", "6-7 9-8 12-2 14-11 17-3"),
        (16, "HF", "SS", "8-11 14-1 18-6 23-8 NoteB", "8-11 14-1 18-6 23-8 NoteB"),
        (16, "HF", "#1", "8-9 13-9 18-1 23-1 NoteB", "8-9 13-1 16-7 20-4 23-7"),
        (16, "HF", "#2", "8-4 13-1 17-3 21-11 25-5", "8-4 12-3 15-6 18-11 22-0"),
        (16, "HF", "#3", "7-5 10-10 13-9 16-9 19-6", "6-5 9-5 11-11 14-6 16-10"),
        (16, "SP", "SS", "9-4 14-7 19-3 24-7 NoteB", "9-4 14-7 19-3 24-7 NoteB"),
        (16, "SP", "#1", "8-11 14-1 18-6 23-2 NoteB", "8-11 13-7 17-2 20-1 23-10"),
        (16, "SP", "#2", "8-7 13-5 17-1 20-3 23-10", "7-9 11-8 14-9 17-6 20-8"),
        (16, "SP", "#3", "6-11 10-2 12-10 15-7 18-6", "6-0 8-10 11-2 13-6 16-0"),
        (16, "SPF", "SS", "8-9 13-9 18-1 23-1 NoteB", "8-9 13-9 18-1 23-0 NoteB"),
        (16, "SPF", "#1", "8-7 13-5 17-9 22-3 25-9", "8-6 12-5 15-9 19-3 22-4"),
        (16, "SPF", "#2", "8-7 13-5 17-9 22-3 25-9", "8-6 12-5 15-9 19-3 22-4"),
        (16, "SPF", "#3", "7-5 10-10 13-9 16-9 19-6", "6-5 9-5 11-11 14-6 16-10"),
        (19.2, "DF-L", "SS", "8-11 14-0 18-5 23-7 NoteB", "8-11 14-0 18-5 23-0 NoteB"),
        (19.2, "DF-L", "#1", "8-7 13-6 17-9 21-8 25-2", "8-4 12-2 15-4 18-9 21-9"),
        (19.2, "DF-L", "#2", "8-5 13-3 16-10 20-7 23-10", "7-10 11-6 14-7 17-10 20-8"),
        (19.2, "DF-L", "#3", "6-11 10-2 12-10 15-8 18-3", "6-0 8-9 11-2 13-7 15-9"),
        (19.2, "HF", "SS", "8-5 13-3 17-5 22-3 NoteB", "8-5 13-3 17-5 22-3 25-9"),
        (19.2, "HF", "#1", "8-3 12-11 17-1 21-5 24-10", "8-2 12-0 15-2 18-6 21-6"),
        (19.2, "HF", "#2", "7-10 12-4 16-3 20-0 23-2", "7-8 11-2 14-2 17-4 20-1"),
        (19.2, "HF", "#3", "6-9 9-11 12-7 15-4 17-9", "5-10 8-7 10-10 13-3 15-5"),
        (19.2, "SP", "SS", "8-9 13-9 18-2 23-1 NoteB", "8-9 13-9 18-2 23-1 NoteB"),
        (19.2, "SP", "#1", "8-5 13-3 17-5 21-2 25-2", "8-4 12-4 15-8 18-4 21-9"),
        (19.2, "SP", "#2", "8-1 12-3 15-7 18-6 21-9", "7-1 10-8 13-6 16-0 18-10"),
        (19.2, "SP", "#3", "6-4 9-4 11-9 14-3 16-10", "5-6 8-1 10-2 12-4 14-7"),
        (19.2, "SPF", "SS", "8-3 12-11 17-1 21-9 NoteB", "8-3 12-11 17-1 21-0 24-4"),
        (19.2, "SPF", "#1", "8-1 12-8 16-7 20-3 23-6", "7-9 11-4 14-4 17-7 20-4"),
        (19.2, "SPF", "#2", "8-1 12-8 16-7 20-3 23-6", "7-9 11-4 14-4 17-7 20-4"),
        (19.2, "SPF", "#3", "6-9 9-11 12-7 15-4 17-9", "5-10 8-7 10-10 13-3 15-5"),
        (24, "DF-L", "SS", "8-3 13-0 17-2 21-10 NoteB", "8-3 13-0 16-10 20-7 23-10"),
        (24, "DF-L", "#1", "8-0 12-6 15-10 19-5 22-6", "7-5 10-10 13-9 16-9 19-6"),
        (24, "DF-L", "#2", "7-10 11-11 15-1 18-5 21-4", "7-0 10-4 13-0 15-11 18-6"),
        (24, "DF-L", "#3", "6-2 9-1 11-6 14-1 16-3", "5-4 7-10 10-0 12-2 14-1"),
        (24, "HF", "SS", "7-10 12-3 16-2 20-8 25-1", "7-10 12-3 16-2 19-10 23-0"),
        (24, "HF", "#1", "7-8 12-0 15-8 19-2 22-2", "7-4 10-9 13-7 16-7 19-3"),
        (24, "HF", "#2", "7-3 11-5 14-8 17-10 20-9", "6-10 10-0 12-8 15-6 17-11"),
        (24, "HF", "#3", "6-1 8-10 11-3 13-8 15-11", "5-3 7-8 9-9 11-10 13-9"),
        (24, "SP", "SS", "8-1 12-9 16-10 21-6 NoteB", "8-1 12-9 16-10 20-10 24-8"),
        (24, "SP", "#1", "7-10 12-3 16-2 18-11 22-6", "7-5 11-1 14-0 16-5 19-6"),
        (24, "SP", "#2", "7-4 11-0 13-11 16-6 19-6", "6-4 9-6 12-1 14-4 16-10"),
        (24, "SP", "#3", "5-8 8-4 10-6 12-9 15-1", "4-11 7-3 9-1 11-0 13-1"),
        (24, "SPF", "SS", "7-8 12-0 15-10 20-2 24-7", "7-8 12-0 15-4 18-9 21-9"),
        (24, "SPF", "#1", "7-6 11-9 14-10 18-2 21-0", "6-11 10-2 12-10 15-8 18-3"),
        (24, "SPF", "#2", "7-6 11-9 14-10 18-2 21-0", "6-11 10-2 12-10 15-8 18-3"),
        (24, "SPF", "#3", "6-1 8-10 11-3 13-8 15-11", "5-3 7-8 9-9 11-10 13-9"),
    ],
}

# Table R602.7(1): girder and header spans for exterior bearing walls (No. 2 DF-L, HF, SP and SPF). Per row: span and
# NJ (jack studs at each end) for building widths 12, 24, 36 ft, at ground snow loads 30, 50 and 70 psf in turn.
HEADERS_EXT = {
    "table": "R602.7(1)",
    "title": "Girder spans and header spans for exterior bearing walls",
    "snow": [30, 50, 70],
    "widths": [12, 24, 36],
    "supporting": {
        "Roof and ceiling": [
            ("1-2x6", "4-0 1 3-1 2 2-7 2 3-5 1 2-8 2 2-3 2 3-0 2 2-4 2 2-0 2"),
            ("1-2x8", "5-1 2 3-11 2 3-3 2 4-4 2 3-4 2 2-10 2 3-10 2 3-0 2 2-6 3"),
            ("1-2x10", "6-0 2 4-8 2 3-11 2 5-2 2 4-0 2 3-4 3 4-7 2 3-6 3 3-0 3"),
            ("1-2x12", "7-1 2 5-5 2 4-7 3 6-1 2 4-8 3 3-11 3 5-5 2 4-2 3 3-6 3"),
            ("2-2x4", "4-0 1 3-1 1 2-7 1 3-5 1 2-7 1 2-2 1 3-0 1 2-4 1 2-0 1"),
            ("2-2x6", "6-0 1 4-7 1 3-10 1 5-1 1 3-11 1 3-3 2 4-6 1 3-6 2 2-11 2"),
            ("2-2x8", "7-7 1 5-9 1 4-10 2 6-5 1 5-0 2 4-2 2 5-9 1 4-5 2 3-9 2"),
            ("2-2x10", "9-0 1 6-10 2 5-9 2 7-8 2 5-11 2 4-11 2 6-9 2 5-3 2 4-5 2"),
            ("2-2x12", "10-7 2 8-1 2 6-10 2 9-0 2 6-11 2 5-10 2 8-0 2 6-2 2 5-2 3"),
            ("3-2x8", "9-5 1 7-3 1 6-1 1 8-1 1 6-3 1 5-3 2 7-2 1 5-6 2 4-8 2"),
            ("3-2x10", "11-3 1 8-7 1 7-3 2 9-7 1 7-4 2 6-2 2 8-6 1 6-7 2 5-6 2"),
            ("3-2x12", "13-2 1 10-1 2 8-6 2 11-3 2 8-8 2 7-4 2 10-0 2 7-9 2 6-6 2"),
            ("4-2x8", "10-11 1 8-4 1 7-0 1 9-4 1 7-2 1 6-0 1 8-3 1 6-4 1 5-4 2"),
            ("4-2x10", "12-11 1 9-11 1 8-4 1 11-1 1 8-6 1 7-2 2 9-10 1 7-7 2 6-4 2"),
            ("4-2x12", "15-3 1 11-8 1 9-10 2 13-0 1 10-0 2 8-5 2 11-7 1 8-11 2 7-6 2"),
        ],
        "Roof, ceiling and one center bearing floor": [
            ("1-2x6", "3-3 1 2-7 2 2-2 2 3-0 2 2-4 2 2-0 2 2-9 2 2-2 2 1-10 2"),
            ("1-2x8", "4-1 2 3-3 2 2-9 2 3-9 2 3-0 2 2-6 3 3-6 2 2-9 2 2-4 3"),
            ("1-2x10", "4-11 2 3-10 2 3-3 3 4-6 2 3-6 3 3-0 3 4-1 2 3-3 3 2-9 3"),
            ("1-2x12", "5-9 2 4-6 3 3-10 3 5-3 2 4-2 3 3-6 3 4-10 3 3-10 3 3-3 4"),
            ("2-2x4", "3-3 1 2-6 1 2-2 1 3-0 1 2-4 1 2-0 1 2-8 1 2-2 1 1-10 1"),
            ("2-2x6", "4-10 1 3-9 1 3-3 2 4-5 1 3-6 2 3-0 2 4-1 1 3-3 2 2-9 2"),
            ("2-2x8", "6-1 1 4-10 2 4-1 2 5-7 2 4-5 2 3-9 2 5-2 2 4-1 2 3-6 2"),
            ("2-2x10", "7-3 2 5-8 2 4-10 2 6-8 2 5-3 2 4-5 2 6-1 2 4-10 2 4-1 2"),
            ("2-2x12", "8-6 2 6-8 2 5-8 2 7-10 2 6-2 2 5-3 3 7-2 2 5-8 2 4-10 3"),
            ("3-2x8", "7-8 1 6-0 1 5-1 2 7-0 1 5-6 2 4-8 2 6-5 1 5-1 2 4-4 2"),
            ("3-2x10", "9-1 1 7-2 2 6-1 2 8-4 1 6-7 2 5-7 2 7-8 2 6-1 2 5-2 2"),
            ("3-2x12", "10-8 2 8-5 2 7-2 2 9-10 2 7-8 2 6-7 2 9-0 2 7-1 2 6-1 2"),
            ("4-2x8", "8-10 1 6-11 1 5-11 1 8-1 1 6-4 1 5-5 2 7-5 1 5-11 1 5-0 2"),
            ("4-2x10", "10-6 1 8-3 2 7-0 2 9-8 1 7-7 2 6-5 2 8-10 1 7-0 2 6-0 2"),
            ("4-2x12", "12-4 1 9-8 2 8-3 2 11-4 2 8-11 2 7-7 2 10-4 2 8-3 2 7-0 2"),
        ],
        "Roof, ceiling and one clear span floor": [
            ("1-2x6", "2-11 2 2-3 2 1-11 2 2-9 2 2-1 2 1-9 2 2-7 2 2-0 2 1-8 2"),
            ("1-2x8", "3-9 2 2-10 2 2-5 3 3-6 2 2-8 2 2-3 3 3-3 2 2-6 3 2-2 3"),
            ("1-2x10", "4-5 2 3-5 3 2-10 3 4-2 2 3-2 3 2-8 3 3-11 2 3-0 3 2-6 3"),
            ("1-2x12", "5-2 2 4-0 3 3-4 3 4-10 3 3-9 3 3-2 4 4-7 3 3-6 3 3-0 4"),
            ("2-2x4", "2-11 1 2-3 1 1-10 1 2-9 1 2-1 1 1-9 1 2-7 1 2-0 1 1-8 1"),
            ("2-2x6", "4-4 1 3-4 2 2-10 2 4-1 1 3-2 2 2-8 2 3-10 1 3-0 2 2-6 2"),
            ("2-2x8", "5-6 2 4-3 2 3-7 2 5-2 2 4-0 2 3-4 2 4-10 2 3-9 2 3-2 2"),
            ("2-2x10", "6-7 2 5-0 2 4-2 2 6-1 2 4-9 2 4-0 2 5-9 2 4-5 2 3-9 3"),
            ("2-2x12", "7-9 2 5-11 2 4-11 3 7-2 2 5-7 2 4-8 3 6-9 2 5-3 3 4-5 3"),
            ("3-2x8", "6-11 1 5-3 2 4-5 2 6-5 1 5-0 2 4-2 2 6-1 1 4-8 2 4-0 2"),
            ("3-2x10", "8-3 2 6-3 2 5-3 2 7-8 2 5-11 2 5-0 2 7-3 2 5-7 2 4-8 2"),
            ("3-2x12", "9-8 2 7-5 2 6-2 2 9-0 2 7-0 2 5-10 2 8-6 2 6-7 2 5-6 3"),
            ("4-2x8", "8-0 1 6-1 1 5-1 2 7-5 1 5-9 2 4-10 2 7-0 1 5-5 2 4-7 2"),
            ("4-2x10", "9-6 1 7-3 2 6-1 2 8-10 1 6-10 2 5-9 2 8-4 1 6-5 2 5-5 2"),
            ("4-2x12", "11-2 2 8-6 2 7-2 2 10-5 2 8-0 2 6-9 2 9-10 2 7-7 2 6-5 2"),
        ],
        "Roof, ceiling and two center bearing floors": [
            ("1-2x6", "2-8 2 2-1 2 1-10 2 2-7 2 2-0 2 1-9 2 2-5 2 1-11 2 1-8 2"),
            ("1-2x8", "3-5 2 2-8 2 2-4 3 3-3 2 2-7 2 2-2 3 3-1 2 2-5 3 2-1 3"),
            ("1-2x10", "4-0 2 3-2 3 2-9 3 3-10 2 3-1 3 2-7 3 3-8 2 2-11 3 2-5 3"),
            ("1-2x12", "4-9 3 3-9 3 3-2 4 4-6 3 3-7 3 3-1 4 4-3 3 3-5 3 2-11 4"),
            ("2-2x4", "2-8 1 2-1 1 1-9 1 2-6 1 2-0 1 1-8 1 2-5 1 1-11 1 1-7 1"),
            ("2-2x6", "4-0 1 3-2 2 2-8 2 3-9 1 3-0 2 2-7 2 3-7 1 2-10 2 2-5 2"),
            ("2-2x8", "5-0 2 4-0 2 3-5 2 4-10 2 3-10 2 3-3 2 4-7 2 3-7 2 3-1 2"),
            ("2-2x10", "6-0 2 4-9 2 4-0 2 5-8 2 4-6 2 3-10 3 5-5 2 4-3 2 3-8 3"),
            ("2-2x12", "7-0 2 5-7 2 4-9 3 6-8 2 5-4 3 4-6 3 6-4 2 5-0 3 4-3 3"),
            ("3-2x8", "6-4 1 5-0 2 4-3 2 6-0 1 4-9 2 4-1 2 5-8 2 4-6 2 3-10 2"),
            ("3-2x10", "7-6 2 5-11 2 5-1 2 7-1 2 5-8 2 4-10 2 6-9 2 5-4 2 4-7 2"),
            ("3-2x12", "8-10 2 7-0 2 5-11 2 8-5 2 6-8 2 5-8 3 8-0 2 6-4 2 5-4 3"),
            ("4-2x8", "7-3 1 5-9 1 4-11 2 6-11 1 5-6 2 4-8 2 6-7 1 5-2 2 4-5 2"),
            ("4-2x10", "8-8 1 6-10 2 5-10 2 8-3 2 6-6 2 5-7 2 7-10 2 6-2 2 5-3 2"),
            ("4-2x12", "10-2 2 8-1 2 6-10 2 9-8 2 7-8 2 6-7 2 9-2 2 7-3 2 6-2 2"),
        ],
        "Roof, ceiling, and two clear span floors": [
            ("1-2x6", "2-3 2 1-9 2 1-5 2 2-3 2 1-9 2 1-5 3 2-2 2 1-8 2 1-5 3"),
            ("1-2x8", "2-10 2 2-2 3 1-10 3 2-10 2 2-2 3 1-10 3 2-9 2 2-1 3 1-10 3"),
            ("1-2x10", "3-4 2 2-7 3 2-2 3 3-4 3 2-7 3 2-2 4 3-3 3 2-6 3 2-2 4"),
            ("1-2x12", "4-0 3 3-0 3 2-7 4 4-0 3 3-0 4 2-7 4 3-10 3 3-0 4 2-6 4"),
            ("2-2x4", "2-3 1 1-8 1 1-4 1 2-3 1 1-8 1 1-4 1 2-2 1 1-8 1 1-4 2"),
            ("2-2x6", "3-4 1 2-6 2 2-2 2 3-4 2 2-6 2 2-2 2 3-3 2 2-6 2 2-1 2"),
            ("2-2x8", "4-3 2 3-3 2 2-8 2 4-3 2 3-3 2 2-8 2 4-1 2 3-2 2 2-8 3"),
            ("2-2x10", "5-0 2 3-10 2 3-2 3 5-0 2 3-10 2 3-2 3 4-10 2 3-9 3 3-2 3"),
            ("2-2x12", "5-11 2 4-6 3 3-9 3 5-11 2 4-6 3 3-9 3 5-8 2 4-5 3 3-9 3"),
            ("3-2x8", "5-3 1 4-0 2 3-5 2 5-3 2 4-0 2 3-5 2 5-1 2 3-11 2 3-4 2"),
            ("3-2x10", "6-3 2 4-9 2 4-0 2 6-3 2 4-9 2 4-0 2 6-1 2 4-8 2 4-0 3"),
            ("3-2x12", "7-5 2 5-8 2 4-9 3 7-5 2 5-8 2 4-9 3 7-2 2 5-6 3 4-8 3"),
            ("4-2x8", "6-1 1 4-8 2 3-11 2 6-1 1 4-8 2 3-11 2 5-11 1 4-7 2 3-10 2"),
            ("4-2x10", "7-3 2 5-6 2 4-8 2 7-3 2 5-6 2 4-8 2 7-0 2 5-5 2 4-7 2"),
            ("4-2x12", "8-6 2 6-6 2 5-6 2 8-6 2 6-6 2 5-6 2 8-3 2 6-4 2 5-4 3"),
        ],
    },
}

# Table R602.7(2): girder and header spans for interior bearing walls. Per row: span and NJ for building widths 12,
# 24 and 36 ft.
HEADERS_INT = {
    "table": "R602.7(2)",
    "title": "Girder spans and header spans for interior bearing walls",
    "widths": [12, 24, 36],
    "supporting": {
        "One floor only": [
            ("2-2x4", "4-1 1 2-10 1 2-4 1"),
            ("2-2x6", "6-1 1 4-4 1 3-6 1"),
            ("2-2x8", "7-9 1 5-5 1 4-5 2"),
            ("2-2x10", "9-2 1 6-6 2 5-3 2"),
            ("2-2x12", "10-9 1 7-7 2 6-3 2"),
            ("3-2x8", "9-8 1 6-10 1 5-7 1"),
            ("3-2x10", "11-5 1 8-1 1 6-7 2"),
            ("3-2x12", "13-6 1 9-6 2 7-9 2"),
            ("4-2x8", "11-2 1 7-11 1 6-5 1"),
            ("4-2x10", "13-3 1 9-4 1 7-8 1"),
            ("4-2x12", "15-7 1 11-0 1 9-0 2"),
        ],
        "Two floors": [
            ("2-2x4", "2-7 1 1-11 1 1-7 1"),
            ("2-2x6", "3-11 1 2-11 2 2-5 2"),
            ("2-2x8", "5-0 1 3-8 2 3-1 2"),
            ("2-2x10", "5-11 2 4-4 2 3-7 2"),
            ("2-2x12", "6-11 2 5-2 2 4-3 3"),
            ("3-2x8", "6-3 1 4-7 2 3-10 2"),
            ("3-2x10", "7-5 1 5-6 2 4-6 2"),
            ("3-2x12", "8-8 2 6-5 2 5-4 2"),
            ("4-2x8", "7-2 1 5-4 1 4-5 2"),
            ("4-2x10", "8-6 1 6-4 2 5-3 2"),
            ("4-2x12", "10-1 1 7-5 2 6-2 2"),
        ],
    },
}

# What a header in an exterior wall carries, in R602.7(1)'s words, by (floors over it, center bearing or not).
EXT_CASES = {
    (0, True): "Roof and ceiling",
    (0, False): "Roof and ceiling",
    (1, True): "Roof, ceiling and one center bearing floor",
    (1, False): "Roof, ceiling and one clear span floor",
    (2, True): "Roof, ceiling and two center bearing floors",
    (2, False): "Roof, ceiling, and two clear span floors",
}

# Wide-flange shapes: (name, depth in, weight plf, Ix in^4, Sx in^3), recalled from the AISC shapes database --
# UNVERIFIED, check against the Manual before any use beyond planning.
W_SHAPES = [
    ("W8x18", 8.14, 18.0, 61.9, 15.2),
    ("W8x24", 7.93, 24.0, 82.7, 20.9),
    ("W8x31", 8.00, 31.0, 110.0, 27.5),
    ("W10x22", 10.2, 22.0, 118.0, 23.2),
    ("W10x26", 10.3, 26.0, 144.0, 27.9),
    ("W10x33", 9.73, 33.0, 171.0, 35.0),
    ("W10x45", 10.1, 45.0, 248.0, 49.1),
    ("W12x26", 12.2, 26.0, 204.0, 33.4),
    ("W12x35", 12.5, 35.0, 285.0, 45.6),
    ("W12x50", 12.2, 50.0, 391.0, 64.2),
    ("W14x30", 13.8, 30.0, 291.0, 42.0),
    ("W14x38", 14.1, 38.0, 385.0, 54.6),
    ("W16x31", 15.9, 31.0, 375.0, 47.2),
    ("W16x40", 16.0, 40.0, 518.0, 64.7),
    ("W18x35", 17.7, 35.0, 510.0, 57.6),
    ("W18x50", 18.0, 50.0, 800.0, 88.9),
    ("W21x44", 20.7, 44.0, 843.0, 81.6),
    ("W21x62", 21.0, 62.0, 1330.0, 127.0),
]

LVL_DEPTHS = [9.25, 11.25, 11.875, 14.0, 16.0, 18.0, 20.0, 24.0]  # inches; plies are 1 3/4 in
LVL = {"Fb": 2600.0, "Fv": 285.0, "E": 2.0e6, "ply": 1.75, "pcf": 42.0, "grade": "2.0E LVL"}
STEEL = {"Fb": 30000.0, "E": 29.0e6, "grade": "A992 (Fy 50 ksi)"}

# I-joists by rule: span (ft) = depth (in) x K[spacing], at most CAP[spacing] (40 psf live, 20 dead, L/480).
IJOIST_DEPTHS = [9.5, 11.875, 14.0, 16.0]
IJOIST_K = {12: 1.70, 16: 1.55, 19.2: 1.44, 24: 1.25}
IJOIST_CAP = {12: 27.0, 16: 23.5, 19.2: 21.5, 24: 17.5}
ROOF_FACTOR = 1.15  # an I-joist roof (20 live, 20 dead) against its floor span: a rule
DECK_FACTOR = 0.86  # a 60-psf deck against a 40-psf floor table: a rule (see the module's notes)

# ------------------------------------------------------------------------------------------------------ helpers


def _fail(where, msg):
    fail("plan_spans.%s: %s" % (where, msg))


def span_ft(s):
    """A table entry as printed ("16-6", "16-10a", "NoteB") -> feet (16.5, 16.8333, 26.0). The probe of the module's
    purity test is `joist_span`; this is its parser."""
    t = s.rstrip("ab")
    if t == "NoteB":
        return 26.0
    p = t.split("-")
    if len(p) != 2:
        _fail("span_ft", "not a span as the tables print one: %r" % s)
    return int(p[0]) + int(p[1]) / 12.0


def ftin(x, frac=1):
    """Feet as feet-inches, rounded to 1/frac in: ftin(16.5) = 16'-6", ftin(3.3125, 2) = 3'-3 1/2"."""
    neg = x < 0
    n = int(math.round(abs(x) * 12.0 * frac))
    f, r = n // (12 * frac), n % (12 * frac)
    whole, part = r // frac, r % frac
    s = "%d'-%d" % (f, whole)
    if part:
        g = _gcd(part, frac)
        s += " %d/%d" % (part // g, frac // g)
    return ("-" if neg else "") + s + '"'


def _gcd(a, b):
    for _ in range(64):
        if b == 0:
            break
        a, b = b, a % b
    return a


def _spacing_key(spacing):
    for s in SPACINGS:
        if abs(float(spacing) - s) < 1e-6:
            return s
    _fail("spacing", "%s in is not a tabulated spacing (12, 16, 19.2, 24)" % spacing)


def _row(table, species, grade, spacing):
    sp = _spacing_key(spacing)
    for r in table["rows"]:
        if r[1] == species and r[2] == grade and abs(r[0] - sp) < 1e-6:
            return r
    _fail("row", "%s has no row for %s %s at %s in" % (table["table"], species, grade, spacing))


def _pick(table, row, size, dead):
    if size not in table["sizes"]:
        _fail("size", "%s has no %s (it lists %s)" % (table["table"], size, ", ".join(table["sizes"])))
    if dead not in (10, 20):
        _fail("dead", "%s lists dead loads of 10 and 20 psf, not %s" % (table["table"], dead))
    vals = (row[3] if dead == 10 else row[4]).split(" ")
    return span_ft(vals[table["sizes"].index(size)])


# --------------------------------------------------------------------------------------------------- the tables


def joist_span(size, spacing, species="DF-L", grade="#2", dead=20, live=40):
    """The largest clear span (ft) of a sawn floor joist: Table R502.3.1(2) at live=40, R502.3.1(1) at live=30.
    joist_span("2x12", 16) is 16.5 (16'-6": DF-L No. 2, 20 psf dead); dead=10 gives 18'-1"."""
    table = FLOOR_40 if live == 40 else (FLOOR_30 if live == 30 else None)
    if table == None:
        _fail("joist_span", "the floor tables are for 40 psf (R502.3.1(2)) or 30 psf (R502.3.1(1)) live load, not %s" % live)
    return _pick(table, _row(table, species, grade, spacing), size, dead)


def rafter_span(size, spacing, species="DF-L", grade="#2", dead=20):
    """The largest horizontal span (ft) of a rafter or flat-roof joist with the ceiling attached, 20 psf roof live
    load: Table R802.4.1(2). rafter_span("2x10", 16) is 19.5 (19'-6")."""
    return _pick(RAFTER_20, _row(RAFTER_20, species, grade, spacing), size, dead)


def ijoist_span(depth_in, spacing, use="floor"):
    """A RULE OF THUMB, not a table: an I-joist's simple span (ft) at 40 psf live, 20 dead, L/480 -- depth (in) x K,
    capped -- set at or under a mid-range series of a typical maker's table. use="roof" x1.15, "deck" x0.86."""
    sp = _spacing_key(spacing)
    s = min(depth_in * IJOIST_K[sp], IJOIST_CAP[sp])
    if use == "roof":
        return s * ROOF_FACTOR
    if use == "deck":
        return s * DECK_FACTOR
    return s


def _interp_width(cols, widths, width):
    """Footnote c of Tables R602.7(1) and (2): spans may be interpolated between building widths. Below the first
    column the first is used (a narrower building carries less: conservative); past the last there is no answer."""
    if width <= widths[0]:
        return cols[0]
    for k in range(len(widths) - 1):
        a, b = widths[k], widths[k + 1]
        if width <= b:
            t = (width - a) / float(b - a)
            lo, hi = cols[k], cols[k + 1]
            return (lo[0] + t * (hi[0] - lo[0]), max(lo[1], hi[1]))
    return None


def _cells(s):
    v = s.split(" ")
    return [(span_ft(v[2 * k]), int(v[2 * k + 1])) for k in range(len(v) // 2)]


def header_span(size, wall="interior", supporting="One floor only", width=24.0, snow=30, braced=True):
    """The largest span (ft) of a sawn girder or header ("2-2x10": two 2x10s) and its jack studs at each end, from
    Table R602.7(2) (wall="interior": supporting "One floor only" or "Two floors") or R602.7(1) (wall="exterior":
    supporting one of its five cases, see EXT_CASES), at a building width (ft) interpolated per footnote c, and for
    R602.7(1) a ground snow load column (30 for under 30 psf with a 20-psf roof). braced=False applies the 0.70
    factor for a header with cripple studs on it. Returns {"span", "jacks", "table"} or None past the table."""
    if wall == "interior":
        t = HEADERS_INT
        rows = t["supporting"].get(supporting)
        if rows == None:
            _fail("header_span", "R602.7(2) supports %s, not %r" % (" or ".join(['"%s"' % k for k in t["supporting"]]), supporting))
        off = 0
    elif wall == "exterior":
        t = HEADERS_EXT
        rows = t["supporting"].get(supporting)
        if rows == None:
            _fail("header_span", "R602.7(1) supports %s, not %r" % (", ".join(['"%s"' % k for k in t["supporting"]]), supporting))
        if snow not in t["snow"]:
            _fail("header_span", "R602.7(1)'s ground snow columns are 30, 50 and 70 psf, not %s" % snow)
        off = t["snow"].index(snow) * 3
    else:
        _fail("header_span", 'wall is "interior" or "exterior", not %r' % wall)
    for r in rows:
        if r[0] == size:
            cells = _cells(r[1])[off:off + 3]
            got = _interp_width(cells, t["widths"], width)
            if got == None:
                return None
            span = got[0]
            if not braced and size.split("-")[1] in ("2x8", "2x10", "2x12"):
                span = span * 0.70
            return {"span": span, "jacks": got[1], "table": t["table"]}
    _fail("header_span", "%s has no %s" % (t["table"], size))


HEADER_SIZES = ["2-2x4", "2-2x6", "2-2x8", "2-2x10", "2-2x12", "3-2x8", "3-2x10", "3-2x12", "4-2x8", "4-2x10", "4-2x12"]


def _header_rank(size):
    n, s = size.split("-")
    b, d = SAWN[s]
    return int(n) * b * d  # lumber in the section, in^2: the cheapest first


def header_for(span, wall="interior", supporting="One floor only", width=24.0, snow=30, braced=True):
    """The smallest sawn girder or header (least lumber, then fewest jacks) whose R602.7 span covers `span` ft, as
    {"size", "span", "jacks", "table"}; None when nothing in the table does (an LVL or steel beam, by rule)."""
    best = None
    for size in HEADER_SIZES:
        got = header_span(size, wall, supporting, width, snow, braced)
        if got == None or got["span"] + 1e-9 < span:
            continue
        key = (_header_rank(size), got["jacks"])
        if best == None or key < best["key"]:
            best = {"size": size, "span": got["span"], "jacks": got["jacks"], "table": got["table"], "key": key}
    if best != None:
        best.pop("key")
    return best


# ------------------------------------------------------------------------------------ beams by rule of thumb


def _beam_ok(L, w_total, w_live, S, I, Fb, E, area=None, Fv=None):
    """Simple span L (ft), uniform loads (plf). Returns (ok, utilisation, what governs)."""
    Lin = L * 12.0
    M = w_total * L * L / 8.0 * 12.0  # lb-in
    fb = M / S
    u_b = fb / Fb
    d_live = 5.0 * (w_live / 12.0) * Lin * Lin * Lin * Lin / (384.0 * E * I)
    d_tot = 5.0 * (w_total / 12.0) * Lin * Lin * Lin * Lin / (384.0 * E * I)
    u_dl = d_live / (Lin / 360.0)
    u_dt = d_tot / (Lin / 240.0)
    u_v = 0.0
    if area != None and Fv != None:
        V = w_total * L / 2.0
        u_v = 1.5 * V / area / Fv
    u = max(u_b, u_dl, u_dt, u_v)
    gov = "bending"
    if u == u_dl:
        gov = "live-load deflection (L/360)"
    elif u == u_dt:
        gov = "total deflection (L/240)"
    elif u == u_v:
        gov = "shear"
    return (u <= 1.0, u, gov)


def lvl_beam(span, plf, live_plf=None, max_depth=None, max_plies=4):
    """A RULE OF THUMB, not engineering: the least LVL (2.0E, 1 3/4-in plies) for a simple span (ft) under a uniform
    load (plf, total; live_plf its live part, 2/3 of it if not given), self weight added. Returns {"size", "depth",
    "plies", "util", "governs", "rule"} or None when four plies at the deepest depth will not do."""
    live = live_plf if live_plf != None else plf * 2.0 / 3.0
    best = None
    for d in LVL_DEPTHS:
        if max_depth != None and d > max_depth + 1e-9:
            continue
        for n in range(2, max_plies + 1):
            b = n * LVL["ply"]
            sw = b * d / 144.0 * LVL["pcf"]
            Fb = LVL["Fb"] * min(1.0, math.pow(12.0 / d, 1.0 / 9.0))
            ok, u, gov = _beam_ok(span, plf + sw, live, b * d * d / 6.0, b * d * d * d / 12.0, Fb, LVL["E"], b * d, LVL["Fv"])
            if not ok:
                continue
            if best == None or b * d < best["b"] * best["depth"] - 1e-9:
                best = {"size": "%d-ply 1 3/4 x %s LVL" % (n, _inch(d)), "depth": d, "plies": n, "b": b,
                        "util": u, "governs": gov, "rule": "rule of thumb (LVL 2.0E, simple span)"}
    return best


def steel_beam(span, plf, live_plf=None, max_depth=None):
    """A RULE OF THUMB, not engineering: the lightest wide-flange (of W_SHAPES, unverified properties) for a simple
    span (ft) under a uniform load (plf, total), self weight added, 0.6 Fy on Sx with the compression flange taken as
    braced, deflection L/360 live and L/240 total. Returns {"size", "depth", "util", "governs", "rule"} or None."""
    live = live_plf if live_plf != None else plf * 2.0 / 3.0
    best = None
    for (name, d, wt, Ix, Sx) in W_SHAPES:
        if max_depth != None and d > max_depth + 1e-9:
            continue
        ok, u, gov = _beam_ok(span, plf + wt, live, Sx, Ix, STEEL["Fb"], STEEL["E"])
        if not ok:
            continue
        if best == None or wt < best["weight"] - 1e-9:
            best = {"size": name, "depth": d, "weight": wt, "util": u, "governs": gov,
                    "rule": "rule of thumb (A992, simple span, braced)"}
    return best


def _inch(d):
    whole = int(d + 1e-9)
    part = d - whole
    for (num, den) in ((1, 4), (1, 2), (3, 4), (1, 8), (3, 8), (5, 8), (7, 8)):
        if abs(part - num / float(den)) < 1e-6:
            return "%d %d/%d" % (whole, num, den)
    return "%d" % whole


# ------------------------------------------------------------------------------------ joist options for a menu


def joist_options(species="DF-L", grades=None, spacings=None, sawn=None, ijoists=None, dead=20, live=40):
    """Every joist a frame may use, as dicts {"family", "size", "depth" (in), "spacing", "grade", "spans": {"floor",
    "roof", "deck"} (ft), "rule"}: sawn from R502.3.1(2) (floor), R802.4.1(2) (roof) and the deck rule; I-joists by
    rule. Defaults: DF-L No. 2 (the West Coast default), every tabulated spacing, 2x8..2x12, 9 1/2..16-in I-joists."""
    grades = grades if grades != None else ["#2"]
    spacings = spacings if spacings != None else SPACINGS
    sawn = sawn if sawn != None else ["2x8", "2x10", "2x12"]
    ijoists = ijoists if ijoists != None else IJOIST_DEPTHS
    out = []
    for g in grades:
        for s in sawn:
            for sp in spacings:
                f = joist_span(s, sp, species, g, dead, live)
                r = rafter_span(s, sp, species, g, dead) if s in RAFTER_20["sizes"] else f
                out.append({"family": "sawn", "size": s, "depth": SAWN[s][1], "b": SAWN[s][0], "spacing": sp,
                            "grade": "%s %s" % (species, GRADE_NAMES[g]),
                            "spans": {"floor": f, "roof": max(f, r), "deck": f * DECK_FACTOR},
                            "rule": "%s; roof %s; deck: rule" % (FLOOR_40["table"] if live == 40 else FLOOR_30["table"], RAFTER_20["table"])})
    for d in ijoists:
        for sp in spacings:
            f = ijoist_span(d, sp)
            out.append({"family": "ijoist", "size": "%s-in I-joist" % _inch(d), "depth": d, "b": 2.3125, "spacing": sp,
                        "grade": "I-joist (by rule)",
                        "spans": {"floor": f, "roof": f * ROOF_FACTOR, "deck": f * DECK_FACTOR},
                        "rule": "rule of thumb"})
    return out
