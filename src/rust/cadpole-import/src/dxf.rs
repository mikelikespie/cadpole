//! ASCII DXF into a [`Drawing`]: layers, entities in millimetres, texts and
//! points — what a surveyor's site plan carries.
//!
//! A DXF is a text file of group-code/value pairs, one per line, and the
//! entity section has barely changed between R12 and 2018, so this reader
//! is Cadpole's own: no crate, one pass over the pairs after the byte-level
//! [`crate::prescan::dxf`], under [`crate::bounded::run`]. What it reads:
//! `LINE`, `LWPOLYLINE` and `POLYLINE`/`VERTEX` (bulges kept as arcs),
//! `ARC`, `CIRCLE`, `ELLIPSE`, `SPLINE` (control points and knots, or fit
//! points), `POINT`, `TEXT`, `MTEXT` (formatting codes stripped), `ATTRIB`
//! (the values a block's tags carry), `SOLID` (how a DXF draws an
//! arrowhead) and `HATCH` boundaries; `INSERT`s of `BLOCK`s are expanded in
//! place, with their scale, rotation and arrays, nested to
//! [`MAX_INSERT_DEPTH`] and never through a cycle. Layers come with their
//! colours (`$INSUNITS`, the ACI index → RGB), coordinates are scaled to
//! millimetres by `$INSUNITS` or the caller's `units`, and `$EXTMIN`/
//! `$EXTMAX` are kept as [`Drawing::extents`].
//!
//! **Exact where the file is exact.** A bulge is an arc, an `ARC` is an
//! arc, a rotated or uniformly scaled `INSERT` keeps an arc an arc (the
//! centre moves, the angles shift, a mirror reverses them); a non-uniform
//! scale turns a circle into the parametrised ellipse it really is,
//! `c + u cos t + v sin t`, which is closed under every affine map. Only a
//! bulge inside a non-uniformly scaled block is flattened, within
//! tolerance and counted.
//!
//! **What is refused, by name.** A DWG (the `AC10xx` stamp) and a binary
//! DXF are refused in the pre-scan with the way out. `3DSOLID`, `REGION`,
//! `BODY` and `SURFACE` carry ACIS SAT data Cadpole does not read;
//! `ACAD_PROXY_ENTITY` is a placeholder for an application object;
//! `DIMENSION` and `LEADER` are re-derived by every reader from its own
//! style, so only an *exploded* dimension is read. Each is counted under
//! its DXF name and said once.
//!
//! The derived views ([`Drawing::sketch`], [`Drawing::curves`],
//! [`Drawing::points`], [`Drawing::texts`], [`Drawing::spot_elevations`],
//! [`Drawing::contours`], [`Drawing::bbox`]) are pure functions of the
//! parsed drawing, so a script can ask the same memoised drawing several
//! questions without parsing twice.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadpole_core::fmath;
use cadpole_core::sketch::{self, Loop, P2, Path, Segment};
use cadpole_import_2d::curve;

use crate::prescan::{self, MAX_DXF_EXPANDED, MAX_DXF_SPLINE_POINTS, MAX_DXF_TEXT_BYTES, MAX_INSERT_DEPTH};
use crate::{Note, bounded, label};

const TAU: f64 = std::f64::consts::TAU;
const PI: f64 = std::f64::consts::PI;
/// Below this a chord is a point, in mm.
const EPS: f64 = 1e-9;

/// The flattening and joining tolerance when the caller gives none, in mm.
pub const DEFAULT_TOLERANCE: f64 = 0.01;

/// A point of the drawing, in millimetres once it leaves the reader.
pub type P3 = [f64; 3];

// ---------------------------------------------------------------- units

/// A length unit `import_dxf(units=)` names, or `$INSUNITS` declares. The
/// members are the `dxf_units` vocabulary, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Unit {
    /// Read `$INSUNITS`; millimetres with a warning when it says nothing.
    #[default]
    Auto,
    Mm,
    Cm,
    M,
    In,
    Ft,
    Yd,
    /// The US survey foot, 1200/3937 m: two parts per million longer than
    /// the international foot, which is a millimetre across a 500 m site.
    SurveyFt,
}

impl Unit {
    /// Every member, in vocabulary order.
    pub const ALL: [Unit; 8] = [Unit::Auto, Unit::Mm, Unit::Cm, Unit::M, Unit::In, Unit::Ft, Unit::Yd, Unit::SurveyFt];

    pub fn name(self) -> &'static str {
        match self {
            Unit::Auto => "auto",
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::M => "m",
            // `inch`, not `in`: `in` is a Starlark keyword, so `dxf_units.in`
            // cannot be written (the `ease_in` precedent); the string
            // `"in"` still parses.
            Unit::In => "inch",
            Unit::Ft => "ft",
            Unit::Yd => "yd",
            Unit::SurveyFt => "survey_ft",
        }
    }

    /// The vocabulary's spellings, a few long forms, and nothing else.
    /// Empty means `auto`.
    pub fn parse(s: &str) -> Result<Unit, String> {
        Ok(match s.trim() {
            "" | "auto" => Unit::Auto,
            "mm" | "millimeter" | "millimetre" | "millimeters" | "millimetres" => Unit::Mm,
            "cm" | "centimeter" | "centimetre" => Unit::Cm,
            "m" | "meter" | "metre" | "meters" | "metres" => Unit::M,
            "in" | "inch" | "inches" => Unit::In,
            "ft" | "foot" | "feet" => Unit::Ft,
            "yd" | "yard" | "yards" => Unit::Yd,
            "survey_ft" | "us_survey_ft" | "usft" | "us-ft" => Unit::SurveyFt,
            other => {
                let valid: Vec<&str> = Unit::ALL.iter().map(|u| u.name()).collect();
                return Err(format!("unknown unit {other:?}; valid: {}", valid.join(", ")));
            }
        })
    }

    /// Millimetres per unit. `Auto` is 1: the caller resolves it first.
    pub fn mm(self) -> f64 {
        match self {
            Unit::Auto | Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::In => 25.4,
            Unit::Ft => 304.8,
            Unit::Yd => 914.4,
            Unit::SurveyFt => 1200.0 / 3937.0 * 1000.0,
        }
    }

    /// The unit a `$INSUNITS` code names, when Cadpole converts it.
    pub fn from_insunits(code: i64) -> Option<Unit> {
        Some(match code {
            1 => Unit::In,
            2 => Unit::Ft,
            4 => Unit::Mm,
            5 => Unit::Cm,
            6 => Unit::M,
            10 => Unit::Yd,
            21 => Unit::SurveyFt,
            _ => return None,
        })
    }

    /// What a `$INSUNITS` code means, for the diagnostic when it is not
    /// one Cadpole converts.
    pub fn insunits_name(code: i64) -> &'static str {
        match code {
            0 => "unitless",
            1 => "inches",
            2 => "feet",
            3 => "miles",
            4 => "millimetres",
            5 => "centimetres",
            6 => "metres",
            7 => "kilometres",
            8 => "microinches",
            9 => "mils",
            10 => "yards",
            11 => "angstroms",
            12 => "nanometres",
            13 => "microns",
            14 => "decimetres",
            15 => "decametres",
            16 => "hectometres",
            17 => "gigametres",
            18 => "astronomical units",
            19 => "light years",
            20 => "parsecs",
            21 => "US survey feet",
            22 => "US survey inches",
            23 => "US survey yards",
            24 => "US survey miles",
            _ => "an unknown unit code",
        }
    }
}

/// Where the drawing's unit came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnitSource {
    /// `$INSUNITS` in the header.
    Header,
    /// `units=` from the caller.
    Option,
    /// Nothing said; millimetres, with a warning.
    #[default]
    Assumed,
}

impl UnitSource {
    pub fn name(self) -> &'static str {
        match self {
            UnitSource::Header => "$INSUNITS",
            UnitSource::Option => "units=",
            UnitSource::Assumed => "assumed",
        }
    }
}

// ---------------------------------------------------------------- options

/// What [`import`] takes beside the bytes.
#[derive(Clone, Debug)]
pub struct Options {
    /// Layer patterns to keep (case-insensitive, `*` and `?`), matched
    /// against the file's own names. Empty keeps every layer; a pattern
    /// that matches nothing is an error listing what there is.
    pub layers: Vec<String>,
    /// The unit; `Auto` reads `$INSUNITS`.
    pub units: Unit,
    /// Flattening tolerance for what has to be flattened (an ellipse's
    /// Béziers, a bulge inside a non-uniformly scaled block), in mm.
    pub tolerance: f64,
    /// The wall-clock budget for the parse, in milliseconds
    /// ([`crate::bounded`]).
    pub budget_ms: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options { layers: Vec::new(), units: Unit::Auto, tolerance: DEFAULT_TOLERANCE, budget_ms: crate::IMPORT_BUDGET_MS }
    }
}

// ---------------------------------------------------------------- the drawing

/// The DXF entity types read, by their DXF names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Line,
    LwPolyline,
    Polyline,
    Arc,
    Circle,
    Ellipse,
    Spline,
    Point,
    Text,
    MText,
    Attrib,
    Solid,
    Hatch,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Line,
        Kind::LwPolyline,
        Kind::Polyline,
        Kind::Arc,
        Kind::Circle,
        Kind::Ellipse,
        Kind::Spline,
        Kind::Point,
        Kind::Text,
        Kind::MText,
        Kind::Attrib,
        Kind::Solid,
        Kind::Hatch,
    ];

    /// The DXF name, lower-cased: what a curve is named by
    /// (`<layer>/lwpolyline[3]`).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Line => "line",
            Kind::LwPolyline => "lwpolyline",
            Kind::Polyline => "polyline",
            Kind::Arc => "arc",
            Kind::Circle => "circle",
            Kind::Ellipse => "ellipse",
            Kind::Spline => "spline",
            Kind::Point => "point",
            Kind::Text => "text",
            Kind::MText => "mtext",
            Kind::Attrib => "attrib",
            Kind::Solid => "solid",
            Kind::Hatch => "hatch",
        }
    }

    /// Whether the entity is a label rather than geometry.
    pub fn is_text(self) -> bool {
        matches!(self, Kind::Text | Kind::MText | Kind::Attrib)
    }
}

/// An entity's geometry, in millimetres, in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Line {
        a: P3,
        b: P3,
    },
    /// Vertices with, per vertex, the bulge of the edge to the next one:
    /// `tan(θ/4)` of the arc's included angle, positive counter-clockwise,
    /// zero for a straight edge. The last bulge is the closing edge's.
    Polyline {
        vertices: Vec<P3>,
        bulges: Vec<f64>,
        closed: bool,
    },
    /// Counter-clockwise from `start` to `end`, radians.
    Arc {
        center: P3,
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: P3,
        radius: f64,
    },
    /// `center + u cos t + v sin t` for `t` from `start` to `end` (radians).
    /// `u` and `v` are the file's major and minor axes for an `ELLIPSE`,
    /// and any pair of conjugate diameters once a block's transform has
    /// been through — the form is closed under affine maps, so a scaled
    /// circle is exact.
    Ellipse {
        center: P3,
        u: P3,
        v: P3,
        start: f64,
        end: f64,
    },
    /// A B-spline by control points, knots and weights, or by fit points
    /// when the file gives only those.
    Spline {
        degree: usize,
        control: Vec<P3>,
        knots: Vec<f64>,
        weights: Vec<f64>,
        fit: Vec<P3>,
        closed: bool,
    },
    Point {
        at: P3,
    },
    /// A label: its placement point (the alignment point for a justified
    /// `TEXT`, the insertion point otherwise), its plain text, its height in
    /// mm and its rotation in degrees.
    Text {
        at: P3,
        text: String,
        height: f64,
        rotation: f64,
    },
}

/// One entity of the drawing, after unit scaling and `INSERT` expansion.
#[derive(Clone, Debug)]
pub struct Entity {
    pub kind: Kind,
    /// Index into [`Drawing::layers`].
    pub layer: usize,
    /// The ACI colour index the file gave (256 = by layer, 0 = by block).
    pub aci: i16,
    /// The colour resolved through the layer and the block: what a viewer
    /// would draw it in.
    pub color: [u8; 3],
    /// The block definition it came out of, sanitised; `None` at top level.
    pub block: Option<String>,
    pub geometry: Geometry,
}

/// One layer, from the `LAYER` table or from the first entity that named it.
#[derive(Clone, Debug)]
pub struct Layer {
    /// The sanitised name: what loops are named by (`<name>/loop[k]`) and
    /// what a script passes to `layer=`.
    pub name: String,
    /// The file's own spelling, only ever shown quoted.
    pub raw: String,
    pub aci: i16,
    pub color: [u8; 3],
    /// A negative colour index in the table: the layer is switched off.
    pub off: bool,
    /// Bit 1 of the table flags.
    pub frozen: bool,
    /// Every entity on it in the file, expanded, selected or not.
    pub entities: usize,
    /// Whether `layers=` kept it.
    pub selected: bool,
}

/// What [`import`] produces.
#[derive(Clone, Debug, Default)]
pub struct Drawing {
    /// `$ACADVER`, as written (`AC1015`), and the release it names.
    pub version: String,
    /// The unit the coordinates were scaled from.
    pub unit: Unit,
    pub unit_source: UnitSource,
    /// Millimetres per drawing unit.
    pub scale: f64,
    pub layers: Vec<Layer>,
    /// In file order, `INSERT`s expanded where they stood, only the
    /// selected layers.
    pub entities: Vec<Entity>,
    /// `$EXTMIN` / `$EXTMAX`, scaled, when the header has both.
    pub extents: Option<(P3, P3)>,
    /// Block definitions in the file.
    pub blocks: usize,
    /// `INSERT`s expanded (an array counts once).
    pub inserts: usize,
    /// Entity types not read, by DXF name.
    pub skipped: BTreeMap<String, usize>,
    /// Entities whose extrusion direction is not ±Z.
    pub tilted: usize,
    /// Entities with a missing or non-finite number.
    pub malformed: usize,
    /// Entities on layers `layers=` did not keep.
    pub unselected: usize,
    /// Bulges flattened inside non-uniformly scaled blocks.
    pub flattened: usize,
    pub notes: Vec<Note>,
    /// Group-code/value pairs read.
    pub pairs: usize,
}

/// The ACI (AutoCAD Color Index) table, 1–255, as RGB. 1–9 and 250–255
/// are named entries; 10–249 follow the table's own rule (24 hues 15°
/// apart, five values, full and half saturation), reproduced here rather
/// than tabulated.
pub fn aci_rgb(i: i16) -> [u8; 3] {
    match i {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        7 => [255, 255, 255],
        8 => [65, 65, 65],
        9 => [128, 128, 128],
        10..=249 => {
            let hue = ((i - 10) / 10) as f64 * 15.0;
            let k = ((i - 10) % 10) as usize;
            let v: f64 = [255.0, 204.0, 153.0, 127.0, 76.0][k / 2];
            let m = if k % 2 == 1 { (v / 2.0).floor() } else { 0.0 };
            let sector = (hue / 60.0) as usize % 6;
            let f = (hue % 60.0) / 60.0;
            let (r, g, b) = match sector {
                0 => (v, v * f, 0.0),
                1 => (v * (1.0 - f), v, 0.0),
                2 => (0.0, v, v * f),
                3 => (0.0, v * (1.0 - f), v),
                4 => (v * f, 0.0, v),
                _ => (v, 0.0, v * (1.0 - f)),
            };
            let ch = |c: f64| (m + c * (v - m) / v).floor() as u8;
            [ch(r), ch(g), ch(b)]
        }
        250 => [51, 51, 51],
        251 => [91, 91, 91],
        252 => [132, 132, 132],
        253 => [173, 173, 173],
        254 => [214, 214, 214],
        _ => [255, 255, 255],
    }
}

/// The release an `$ACADVER` code names.
pub fn release_of(acadver: &str) -> &'static str {
    match acadver.trim() {
        "AC1006" => "R10",
        "AC1009" => "R11/R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------- the pair reader

/// Group-code/value pairs, one at a time, with one pair of lookahead.
/// `999` comments are skipped; blank code lines (a trailing newline) are
/// skipped, as the pre-scan skipped them.
struct Reader<'a> {
    lines: std::str::Split<'a, char>,
    line: usize,
    pairs: usize,
    peeked: Option<Option<(i32, &'a str)>>,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Reader<'a> {
        Reader { lines: text.split('\n'), line: 0, pairs: 0, peeked: None }
    }

    fn read_pair(&mut self) -> Option<(i32, &'a str)> {
        loop {
            let code_line = self.lines.next()?;
            self.line += 1;
            let code = code_line.trim();
            if code.is_empty() {
                continue;
            }
            // The pre-scan checked every code line parses; a difference
            // here can only come from lossy UTF-8 decoding, and ends the
            // file.
            let code: i32 = code.parse::<i64>().ok()?.clamp(-1, 9999) as i32;
            let value = self.lines.next().unwrap_or("");
            self.line += 1;
            let value = value.strip_suffix('\r').unwrap_or(value);
            self.pairs += 1;
            if code == 999 {
                continue;
            }
            return Some((code, value));
        }
    }

    fn next(&mut self) -> Option<(i32, &'a str)> {
        match self.peeked.take() {
            Some(p) => p,
            None => self.read_pair(),
        }
    }

    fn peek(&mut self) -> Option<(i32, &'a str)> {
        if self.peeked.is_none() {
            let p = self.read_pair();
            self.peeked = Some(p);
        }
        self.peeked.unwrap_or(None)
    }
}

/// One entity's pairs, after its `0` / name pair, up to the next `0`.
struct Raw<'a> {
    pairs: Vec<(i32, &'a str)>,
}

impl<'a> Raw<'a> {
    fn first(&self, code: i32) -> Option<&'a str> {
        self.pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }
    fn s(&self, code: i32) -> Option<&'a str> {
        self.first(code).map(str::trim)
    }
    fn f(&self, code: i32) -> Option<f64> {
        self.first(code).and_then(num)
    }
    fn i(&self, code: i32) -> Option<i64> {
        self.first(code).and_then(int)
    }
    fn all_f(&self, code: i32) -> Vec<f64> {
        self.pairs.iter().filter(|(c, _)| *c == code).filter_map(|(_, v)| num(v)).collect()
    }
    /// `10/20/30` (or `11/21/31`, …) as a point; `None` when x or y is
    /// missing or not finite. z defaults to 0.
    fn point(&self, x_code: i32) -> Option<P3> {
        Some([self.f(x_code)?, self.f(x_code + 10)?, self.f(x_code + 20).unwrap_or(0.0)])
    }
}

fn num(v: &str) -> Option<f64> {
    let x: f64 = v.trim().parse().ok()?;
    x.is_finite().then_some(x)
}

fn int(v: &str) -> Option<i64> {
    let t = v.trim();
    t.parse::<i64>().ok().or_else(|| num(t).map(|x| x as i64))
}

fn finite3(p: P3) -> bool {
    p.iter().all(|c| c.is_finite())
}

/// The largest coordinate an entity may end up at, in mm: a million
/// kilometres. Past it the joiner's spatial hash cannot key a point, and
/// no drawing means it.
const MAX_COORD_MM: f64 = 1e12;

fn sane3(p: P3) -> bool {
    p.iter().all(|c| c.is_finite() && c.abs() < MAX_COORD_MM)
}

// ---------------------------------------------------------------- transforms

/// An affine map of the plane plus a scale and shift of z: what an
/// `INSERT` is, what the unit scale is, and what an entity's own OCS is
/// when its extrusion direction is −Z. `x' = m·(x, y) + t`, `z' = zs·z + zt`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Xf {
    m: [[f64; 2]; 2],
    t: [f64; 2],
    zs: f64,
    zt: f64,
}

impl Xf {
    const ID: Xf = Xf { m: [[1.0, 0.0], [0.0, 1.0]], t: [0.0, 0.0], zs: 1.0, zt: 0.0 };
    /// The OCS→WCS map of a 2D entity whose extrusion is (0, 0, −1): the
    /// arbitrary-axis algorithm gives `Ax = (−1, 0, 0)`, `Ay = (0, 1, 0)`.
    const FLIP_X: Xf = Xf { m: [[-1.0, 0.0], [0.0, 1.0]], t: [0.0, 0.0], zs: -1.0, zt: 0.0 };

    fn scale(s: f64) -> Xf {
        Xf { m: [[s, 0.0], [0.0, s]], t: [0.0, 0.0], zs: s, zt: 0.0 }
    }

    fn apply(&self, p: P3) -> P3 {
        [
            self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.t[0],
            self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.t[1],
            self.zs * p[2] + self.zt,
        ]
    }

    /// The linear part on a vector (a direction, an ellipse axis).
    fn linear(&self, v: P3) -> P3 {
        [self.m[0][0] * v[0] + self.m[0][1] * v[1], self.m[1][0] * v[0] + self.m[1][1] * v[1], self.zs * v[2]]
    }

    /// `self ∘ inner`: apply `inner` first.
    fn then(&self, inner: &Xf) -> Xf {
        let a = &self.m;
        let b = &inner.m;
        Xf {
            m: [
                [a[0][0] * b[0][0] + a[0][1] * b[1][0], a[0][0] * b[0][1] + a[0][1] * b[1][1]],
                [a[1][0] * b[0][0] + a[1][1] * b[1][0], a[1][0] * b[0][1] + a[1][1] * b[1][1]],
            ],
            t: [
                a[0][0] * inner.t[0] + a[0][1] * inner.t[1] + self.t[0],
                a[1][0] * inner.t[0] + a[1][1] * inner.t[1] + self.t[1],
            ],
            zs: self.zs * inner.zs,
            zt: self.zs * inner.zt + self.zt,
        }
    }

    fn det(&self) -> f64 {
        self.m[0][0] * self.m[1][1] - self.m[0][1] * self.m[1][0]
    }

    /// The largest stretch of any direction, bounded by the column norms.
    fn max_stretch(&self) -> f64 {
        let c0 = (self.m[0][0] * self.m[0][0] + self.m[1][0] * self.m[1][0]).sqrt();
        let c1 = (self.m[0][1] * self.m[0][1] + self.m[1][1] * self.m[1][1]).sqrt();
        (c0 + c1).max(EPS)
    }

    /// A uniform scale `s`, a rotation `rot` (the angle the image of +x
    /// makes) and whether it is mirrored; `None` for a non-uniform scale
    /// or a shear, which is where an arc stops being an arc.
    fn uniform(&self) -> Option<(f64, f64, bool)> {
        let (a, c) = (self.m[0][0], self.m[1][0]);
        let (b, d) = (self.m[0][1], self.m[1][1]);
        let l0 = (a * a + c * c).sqrt();
        let l1 = (b * b + d * d).sqrt();
        if l0 < EPS || (l0 - l1).abs() > 1e-9 * l0 || (a * b + c * d).abs() > 1e-9 * l0 * l0 {
            return None;
        }
        Some((l0, fmath::atan2(c, a), self.det() < 0.0))
    }
}

/// An arc's angles with the start in `[0, 2π)` and the end past it by the
/// sweep in `(0, 2π]`: what a transform's `atan2` of a negative zero, or a
/// file's `350°..10°`, both settle into.
fn normalise_arc(start: f64, end: f64) -> (f64, f64) {
    let sweep = sweep_ccw(start, end);
    let mut s = start % TAU;
    if s < 0.0 {
        s += TAU;
    }
    if s >= TAU || s == 0.0 {
        s = 0.0;
    }
    (s, s + sweep)
}

/// The counter-clockwise sweep from `start` to `end`, in `(0, 2π]`. By
/// remainder, not by a loop: a file can write an angle of 1e308 degrees,
/// and adding 2π to that never gets anywhere.
fn sweep_ccw(start: f64, end: f64) -> f64 {
    let s = (end - start) % TAU;
    if s <= 0.0 { s + TAU } else { s }
}

/// Whether an angle is one a drawing could mean: finite and within a
/// million radians (a hundred and sixty thousand turns).
fn sane_angle(a: f64) -> bool {
    a.is_finite() && a.abs() < 1e6
}

/// Points along a circular arc of the plane, dense enough for `tol`.
fn arc_points(center: P2, r: f64, start: f64, sweep: f64, tol: f64) -> Vec<P2> {
    let per = if r > tol { 2.0 * fmath::acos((1.0 - tol / r).clamp(-1.0, 1.0)) } else { PI / 2.0 };
    let n = if per > 1e-6 { ((sweep.abs() / per).ceil() as usize).clamp(1, 4096) } else { 4096 };
    (0..=n)
        .map(|i| {
            let t = start + sweep * (i as f64 / n as f64);
            let (sn, cs) = fmath::sin_cos(t);
            P2::new(center.x + r * cs, center.y + r * sn)
        })
        .collect()
}

/// Every `base + k * period` inside the sweep from `start` by `sweep`
/// (either sign), ends included. At most 64, which no circle or ellipse
/// sweep of a sane angle reaches.
fn angles_within(base: f64, period: f64, start: f64, sweep: f64) -> Vec<f64> {
    let (lo, hi) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
    let mut out = Vec::new();
    if !(lo.is_finite() && hi.is_finite()) {
        return out;
    }
    let mut k = ((lo - base) / period).ceil();
    while out.len() < 64 {
        let t = base + k * period;
        if t > hi {
            break;
        }
        out.push(t);
        k += 1.0;
    }
    out
}

/// The points that bound a circular arc: both ends and each axis crossing
/// its sweep passes, so the box needs no sampling.
fn arc_extremes(center: P2, r: f64, start: f64, sweep: f64) -> Vec<P2> {
    let at = |t: f64| {
        let (sn, cs) = fmath::sin_cos(t);
        P2::new(center.x + r * cs, center.y + r * sn)
    };
    let mut out = vec![at(start), at(start + sweep)];
    out.extend(angles_within(0.0, PI / 2.0, start, sweep).into_iter().map(at));
    out
}

/// The points that bound a segment: its ends, an arc's axis crossings, and a
/// quadratic or cubic Bézier's turning points in x and y. A Bézier of higher
/// degree falls back to its control points, which contain it.
fn segment_extremes(s: &Segment) -> Vec<P2> {
    match s {
        Segment::Line { a, b } => vec![*a, *b],
        Segment::Arc { a, b, via } => match sketch::arc_params(*a, *b, *via) {
            Some((c, r, t0, t1)) => arc_extremes(c, r, t0, t1 - t0),
            None => vec![*a, *b, *via],
        },
        Segment::Bezier { a, b, ctrl } => {
            let mut pts = vec![*a];
            pts.extend(ctrl.iter().copied());
            pts.push(*b);
            if pts.len() > 4 {
                return pts;
            }
            let eval = |t: f64| -> P2 {
                // de Casteljau.
                let mut q = pts.clone();
                for n in (1..q.len()).rev() {
                    for i in 0..n {
                        q[i] = P2::new(q[i].x + t * (q[i + 1].x - q[i].x), q[i].y + t * (q[i + 1].y - q[i].y));
                    }
                }
                q[0]
            };
            let mut out = vec![*a, *b];
            for axis in 0..2 {
                let c: Vec<f64> = pts.iter().map(|p| if axis == 0 { p.x } else { p.y }).collect();
                for t in bezier_turning_params(&c) {
                    out.push(eval(t));
                }
            }
            out
        }
    }
}

/// Parameters in (0, 1) where a quadratic or cubic Bézier coordinate with
/// control values `c` has zero derivative.
fn bezier_turning_params(c: &[f64]) -> Vec<f64> {
    let inside = |t: f64| t > 0.0 && t < 1.0;
    match c.len() {
        3 => {
            let den = c[0] - 2.0 * c[1] + c[2];
            if den.abs() < 1e-300 {
                return Vec::new();
            }
            let t = (c[0] - c[1]) / den;
            if inside(t) { vec![t] } else { Vec::new() }
        }
        4 => {
            // B'(t)/3 = A t² + B t + C.
            let a = -c[0] + 3.0 * c[1] - 3.0 * c[2] + c[3];
            let b = 2.0 * (c[0] - 2.0 * c[1] + c[2]);
            let cc = c[1] - c[0];
            let scale = c.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
            let mut roots = Vec::new();
            if a.abs() <= 1e-12 * scale {
                if b.abs() > 1e-300 {
                    roots.push(-cc / b);
                }
            } else {
                let disc = b * b - 4.0 * a * cc;
                if disc >= 0.0 {
                    let sq = disc.sqrt();
                    roots.push((-b + sq) / (2.0 * a));
                    roots.push((-b - sq) / (2.0 * a));
                }
            }
            roots.into_iter().filter(|t| inside(*t)).collect()
        }
        _ => Vec::new(),
    }
}

/// A bulged edge `a → b` as the arc it encodes: centre, radius, start
/// angle and signed sweep. `None` for a straight edge.
fn bulge_arc(a: P2, b: P2, bulge: f64) -> Option<(P2, f64, f64, f64)> {
    if bulge.abs() < 1e-12 {
        return None;
    }
    let seg = curve::bulge_segment(a, b, bulge);
    let Segment::Arc { a, b, via } = seg else { return None };
    let (c, r, t0, t1) = sketch::arc_params(a, b, via)?;
    Some((c, r, t0, t1 - t0))
}

/// `g` through `xf`, exactly where that is possible. `None` when a
/// coordinate stops being finite. `flattened` counts the bulges that had to
/// become chords.
fn transform(g: &Geometry, xf: &Xf, tol: f64, flattened: &mut usize) -> Option<Geometry> {
    let uniform = xf.uniform();
    let out = match g {
        Geometry::Line { a, b } => Geometry::Line { a: xf.apply(*a), b: xf.apply(*b) },
        Geometry::Point { at } => Geometry::Point { at: xf.apply(*at) },
        Geometry::Polyline { vertices, bulges, closed } => {
            let has_bulge = bulges.iter().any(|b| b.abs() >= 1e-12);
            match uniform {
                Some((_, _, mirrored)) => Geometry::Polyline {
                    vertices: vertices.iter().map(|p| xf.apply(*p)).collect(),
                    bulges: bulges.iter().map(|b| if mirrored { -b } else { *b }).collect(),
                    closed: *closed,
                },
                None if !has_bulge => Geometry::Polyline {
                    vertices: vertices.iter().map(|p| xf.apply(*p)).collect(),
                    bulges: bulges.clone(),
                    closed: *closed,
                },
                None => {
                    // A bulge under a non-uniform scale is an elliptical
                    // arc, which a polyline cannot hold: chords, within
                    // tolerance measured after the stretch.
                    let n = vertices.len();
                    let edges = if *closed { n } else { n.saturating_sub(1) };
                    let src_tol = tol / xf.max_stretch();
                    let mut out: Vec<P3> = Vec::with_capacity(n);
                    for i in 0..edges {
                        let a = vertices[i];
                        let b = vertices[(i + 1) % n];
                        out.push(xf.apply(a));
                        if let Some((c, r, t0, sweep)) = bulge_arc(P2::new(a[0], a[1]), P2::new(b[0], b[1]), bulges[i]) {
                            *flattened += 1;
                            let pts = arc_points(c, r, t0, sweep, src_tol);
                            for p in pts.iter().skip(1).take(pts.len().saturating_sub(2)) {
                                out.push(xf.apply([p.x, p.y, a[2]]));
                            }
                        }
                    }
                    if !*closed && n > 0 {
                        out.push(xf.apply(vertices[n - 1]));
                    }
                    let m = out.len();
                    Geometry::Polyline { vertices: out, bulges: vec![0.0; m], closed: *closed }
                }
            }
        }
        Geometry::Arc { center, radius, start, end } => match uniform {
            Some((s, rot, false)) => {
                let (start, end) = normalise_arc(start + rot, end + rot);
                Geometry::Arc { center: xf.apply(*center), radius: radius * s, start, end }
            }
            Some((s, rot, true)) => {
                let (start, end) = normalise_arc(rot - end, rot - start);
                Geometry::Arc { center: xf.apply(*center), radius: radius * s, start, end }
            }
            None => Geometry::Ellipse {
                center: xf.apply(*center),
                u: xf.linear([*radius, 0.0, 0.0]),
                v: xf.linear([0.0, *radius, 0.0]),
                start: *start,
                end: *end,
            },
        },
        Geometry::Circle { center, radius } => match uniform {
            Some((s, _, _)) => Geometry::Circle { center: xf.apply(*center), radius: radius * s },
            None => Geometry::Ellipse {
                center: xf.apply(*center),
                u: xf.linear([*radius, 0.0, 0.0]),
                v: xf.linear([0.0, *radius, 0.0]),
                start: 0.0,
                end: TAU,
            },
        },
        Geometry::Ellipse { center, u, v, start, end } => {
            Geometry::Ellipse { center: xf.apply(*center), u: xf.linear(*u), v: xf.linear(*v), start: *start, end: *end }
        }
        Geometry::Spline { degree, control, knots, weights, fit, closed } => Geometry::Spline {
            degree: *degree,
            control: control.iter().map(|p| xf.apply(*p)).collect(),
            knots: knots.clone(),
            weights: weights.clone(),
            fit: fit.iter().map(|p| xf.apply(*p)).collect(),
            closed: *closed,
        },
        Geometry::Text { at, text, height, rotation } => {
            let rotation = match uniform {
                Some((_, rot, false)) => rotation + rot.to_degrees(),
                Some((_, rot, true)) => rot.to_degrees() - rotation,
                None => {
                    let (sn, cs) = fmath::sin_cos(rotation.to_radians());
                    let d = xf.linear([cs, sn, 0.0]);
                    fmath::atan2(d[1], d[0]).to_degrees()
                }
            };
            Geometry::Text { at: xf.apply(*at), text: text.clone(), height: height * xf.det().abs().sqrt(), rotation }
        }
    };
    let sane = |x: f64| x.is_finite() && x.abs() < MAX_COORD_MM;
    let ok = match &out {
        Geometry::Line { a, b } => sane3(*a) && sane3(*b),
        Geometry::Point { at } => sane3(*at),
        Geometry::Polyline { vertices, bulges, .. } => vertices.iter().all(|p| sane3(*p)) && bulges.iter().all(|b| sane(*b)),
        Geometry::Arc { center, radius, start, end } => sane3(*center) && sane(*radius) && sane_angle(*start) && sane_angle(*end),
        Geometry::Circle { center, radius } => sane3(*center) && sane(*radius),
        Geometry::Ellipse { center, u, v, start, end } => sane3(*center) && sane3(*u) && sane3(*v) && sane_angle(*start) && sane_angle(*end),
        Geometry::Spline { control, fit, .. } => control.iter().chain(fit).all(|p| sane3(*p)),
        Geometry::Text { at, height, rotation, .. } => sane3(*at) && sane(*height) && rotation.is_finite(),
    };
    ok.then_some(out)
}

// ---------------------------------------------------------------- parsing

/// An entity as parsed, in drawing units, world coordinates, before any
/// `INSERT` transform.
struct Parsed {
    kind: Kind,
    layer: String,
    aci: i16,
    true_color: Option<[u8; 3]>,
    geometry: Geometry,
}

/// An `INSERT` as parsed.
struct InsertRec {
    block: String,
    layer: String,
    aci: i16,
    true_color: Option<[u8; 3]>,
    at: P3,
    scale: [f64; 3],
    rotation_deg: f64,
    cols: usize,
    rows: usize,
    col_spacing: f64,
    row_spacing: f64,
    flip: bool,
}

enum Item {
    Entity(Parsed),
    Insert(InsertRec),
}

struct Block {
    raw: String,
    base: P3,
    items: Vec<Item>,
}

#[derive(Default)]
struct LayerRec {
    raw: String,
    aci: i16,
    true_color: Option<[u8; 3]>,
    flags: i64,
}

struct Parser<'a> {
    r: Reader<'a>,
    tolerance: f64,
    insunits: Option<i64>,
    extmin: [Option<f64>; 3],
    extmax: [Option<f64>; 3],
    version: String,
    layer_table: Vec<LayerRec>,
    blocks: HashMap<String, Block>,
    block_count: usize,
    items: Vec<Item>,
    skipped: BTreeMap<String, usize>,
    tilted: usize,
    malformed: usize,
    text_cut: usize,
    spline_over_cap: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, tolerance: f64) -> Parser<'a> {
        Parser {
            r: Reader::new(text),
            tolerance,
            insunits: None,
            extmin: [None; 3],
            extmax: [None; 3],
            version: String::new(),
            layer_table: Vec::new(),
            blocks: HashMap::new(),
            block_count: 0,
            items: Vec::new(),
            skipped: BTreeMap::new(),
            tilted: 0,
            malformed: 0,
            text_cut: 0,
            spline_over_cap: 0,
        }
    }

    fn read_raw(&mut self) -> Raw<'a> {
        let mut pairs = Vec::new();
        while let Some((code, value)) = self.r.peek() {
            if code == 0 {
                break;
            }
            self.r.next();
            pairs.push((code, value));
        }
        Raw { pairs }
    }

    fn run(&mut self) {
        while let Some((code, value)) = self.r.next() {
            if code != 0 {
                continue;
            }
            match value.trim() {
                "SECTION" => {
                    let name = match self.r.next() {
                        Some((2, n)) => n.trim().to_owned(),
                        _ => String::new(),
                    };
                    match name.as_str() {
                        "HEADER" => self.header(),
                        "TABLES" => self.tables(),
                        "BLOCKS" => self.blocks_section(),
                        "ENTITIES" => {
                            let items = self.entities("ENDSEC");
                            self.items.extend(items);
                        }
                        _ => self.skip_section(),
                    }
                }
                "EOF" => break,
                _ => {}
            }
        }
    }

    fn skip_section(&mut self) {
        while let Some((code, value)) = self.r.next() {
            if code == 0 && matches!(value.trim(), "ENDSEC" | "EOF") {
                break;
            }
        }
    }

    fn header(&mut self) {
        let mut var = "";
        while let Some((code, value)) = self.r.next() {
            match code {
                0 if matches!(value.trim(), "ENDSEC" | "EOF") => break,
                9 => var = value.trim(),
                70 if var == "$INSUNITS" => self.insunits = int(value),
                1 if var == "$ACADVER" => self.version = value.trim().to_owned(),
                10 | 20 | 30 if var == "$EXTMIN" => self.extmin[(code / 10 - 1) as usize] = num(value),
                10 | 20 | 30 if var == "$EXTMAX" => self.extmax[(code / 10 - 1) as usize] = num(value),
                _ => {}
            }
        }
    }

    fn tables(&mut self) {
        while let Some((code, value)) = self.r.next() {
            if code != 0 {
                continue;
            }
            match value.trim() {
                "ENDSEC" | "EOF" => break,
                "TABLE" => {
                    let name = match self.r.peek() {
                        Some((2, n)) => {
                            self.r.next();
                            n.trim().to_owned()
                        }
                        _ => String::new(),
                    };
                    if name == "LAYER" {
                        self.layer_table();
                    } else {
                        while let Some((c, v)) = self.r.next() {
                            if c == 0 && matches!(v.trim(), "ENDTAB" | "ENDSEC" | "EOF") {
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn layer_table(&mut self) {
        while let Some((code, value)) = self.r.next() {
            if code != 0 {
                continue;
            }
            match value.trim() {
                "ENDTAB" | "ENDSEC" | "EOF" => break,
                "LAYER" => {
                    let raw = self.read_raw();
                    let name = raw.s(2).unwrap_or("").to_owned();
                    let aci = raw.i(62).unwrap_or(7).clamp(-255, 256) as i16;
                    self.layer_table.push(LayerRec { raw: name, aci, true_color: true_color(&raw), flags: raw.i(70).unwrap_or(0) });
                }
                _ => {
                    self.read_raw();
                }
            }
        }
    }

    fn blocks_section(&mut self) {
        while let Some((code, value)) = self.r.next() {
            if code != 0 {
                continue;
            }
            match value.trim() {
                "ENDSEC" | "EOF" => break,
                "BLOCK" => {
                    let raw = self.read_raw();
                    let name = raw.s(2).or(raw.s(3)).unwrap_or("").to_owned();
                    let base = [raw.f(10).unwrap_or(0.0), raw.f(20).unwrap_or(0.0), raw.f(30).unwrap_or(0.0)];
                    let items = self.entities("ENDBLK");
                    self.block_count += 1;
                    self.blocks.insert(name.to_uppercase(), Block { raw: name, base, items });
                }
                _ => {}
            }
        }
    }

    /// Entity records up to `until` (`ENDSEC` or `ENDBLK`), each
    /// interpreted; `POLYLINE` and an attributed `INSERT` take their
    /// trailing records too.
    fn entities(&mut self, until: &str) -> Vec<Item> {
        let mut items = Vec::new();
        while let Some((code, value)) = self.r.next() {
            if code != 0 {
                continue;
            }
            let name = value.trim();
            if name == until || matches!(name, "ENDSEC" | "ENDBLK" | "EOF") {
                break;
            }
            let raw = self.read_raw();
            match name {
                "POLYLINE" => {
                    let verts = self.read_vertices();
                    if let Some(e) = self.polyline(&raw, verts) {
                        items.push(Item::Entity(e));
                    }
                }
                "INSERT" => {
                    if raw.i(66) == Some(1) {
                        let attribs = self.read_attribs();
                        items.extend(attribs.into_iter().map(Item::Entity));
                    }
                    if let Some(rec) = self.insert(&raw) {
                        items.push(Item::Insert(rec));
                    }
                }
                _ => items.extend(self.entity(name, &raw).into_iter().map(Item::Entity)),
            }
        }
        items
    }

    /// The `VERTEX` records after a `POLYLINE`, up to `SEQEND`.
    fn read_vertices(&mut self) -> Vec<(P3, f64, i64)> {
        let mut out = Vec::new();
        loop {
            match self.r.peek() {
                Some((0, v)) if v.trim() == "VERTEX" => {
                    self.r.next();
                    let raw = self.read_raw();
                    if let Some(p) = raw.point(10) {
                        out.push((p, raw.f(42).unwrap_or(0.0), raw.i(70).unwrap_or(0)));
                    } else {
                        self.malformed += 1;
                    }
                }
                Some((0, v)) if v.trim() == "SEQEND" => {
                    self.r.next();
                    self.read_raw();
                    break;
                }
                _ => break,
            }
        }
        out
    }

    /// The `ATTRIB` records after an `INSERT` with attributes, up to
    /// `SEQEND`: each is a label in world coordinates.
    fn read_attribs(&mut self) -> Vec<Parsed> {
        let mut out = Vec::new();
        loop {
            match self.r.peek() {
                Some((0, v)) if v.trim() == "ATTRIB" => {
                    self.r.next();
                    let raw = self.read_raw();
                    out.extend(self.text_entity(Kind::Attrib, &raw));
                }
                Some((0, v)) if v.trim() == "SEQEND" => {
                    self.r.next();
                    self.read_raw();
                    break;
                }
                _ => break,
            }
        }
        out
    }

    /// The entity's extrusion direction as the OCS map it implies: identity
    /// for +Z (or none), the x flip for −Z, `None` for anything tilted.
    fn ocs(&mut self, raw: &Raw<'a>) -> Option<Xf> {
        let n = [raw.f(210).unwrap_or(0.0), raw.f(220).unwrap_or(0.0), raw.f(230).unwrap_or(1.0)];
        if n[0].abs() < 1e-9 && n[1].abs() < 1e-9 {
            if n[2] > 0.0 {
                return Some(Xf::ID);
            }
            if n[2] < 0.0 {
                return Some(Xf::FLIP_X);
            }
        }
        self.tilted += 1;
        None
    }

    fn common(&self, raw: &Raw<'a>) -> (String, i16, Option<[u8; 3]>) {
        let layer = raw.s(8).unwrap_or("0");
        let layer = if layer.is_empty() { "0" } else { layer };
        (layer.to_owned(), raw.i(62).unwrap_or(256).clamp(-255, 256) as i16, true_color(raw))
    }

    fn parsed(&mut self, kind: Kind, raw: &Raw<'a>, ocs: &Xf, geometry: Geometry) -> Option<Parsed> {
        let (layer, aci, true_color) = self.common(raw);
        let mut flattened = 0;
        let geometry = if *ocs == Xf::ID { Some(geometry) } else { transform(&geometry, ocs, self.tolerance, &mut flattened) };
        match geometry {
            Some(geometry) => Some(Parsed { kind, layer, aci, true_color, geometry }),
            None => {
                self.malformed += 1;
                None
            }
        }
    }

    fn entity(&mut self, name: &str, raw: &Raw<'a>) -> Vec<Parsed> {
        match name {
            "LINE" => {
                let (Some(a), Some(b)) = (raw.point(10), raw.point(11)) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                self.parsed(Kind::Line, raw, &Xf::ID, Geometry::Line { a, b }).into_iter().collect()
            }
            "POINT" => {
                let Some(at) = raw.point(10) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                self.parsed(Kind::Point, raw, &Xf::ID, Geometry::Point { at }).into_iter().collect()
            }
            "CIRCLE" => {
                let Some(ocs) = self.ocs(raw) else { return Vec::new() };
                let (Some(center), Some(radius)) = (raw.point(10), raw.f(40)) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                if radius <= 0.0 {
                    self.malformed += 1;
                    return Vec::new();
                }
                self.parsed(Kind::Circle, raw, &ocs, Geometry::Circle { center, radius }).into_iter().collect()
            }
            "ARC" => {
                let Some(ocs) = self.ocs(raw) else { return Vec::new() };
                let (Some(center), Some(radius), Some(start), Some(end)) = (raw.point(10), raw.f(40), raw.f(50), raw.f(51)) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                if radius <= 0.0 || !sane_angle(start.to_radians()) || !sane_angle(end.to_radians()) {
                    self.malformed += 1;
                    return Vec::new();
                }
                let (start, end) = normalise_arc(start.to_radians(), end.to_radians());
                let g = Geometry::Arc { center, radius, start, end };
                self.parsed(Kind::Arc, raw, &ocs, g).into_iter().collect()
            }
            "ELLIPSE" => {
                let Some(ocs) = self.ocs(raw) else { return Vec::new() };
                let (Some(center), Some(major), Some(ratio)) = (raw.point(10), raw.point(11), raw.f(40)) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                let start = raw.f(41).unwrap_or(0.0);
                let end = raw.f(42).unwrap_or(TAU);
                let len = (major[0] * major[0] + major[1] * major[1]).sqrt();
                if len < EPS || ratio <= 0.0 || !sane_angle(start) || !sane_angle(end) {
                    self.malformed += 1;
                    return Vec::new();
                }
                // The minor axis is the major turned a quarter turn about
                // the extrusion direction; the flip below then mirrors it.
                let v = [-major[1] * ratio, major[0] * ratio, 0.0];
                let g = Geometry::Ellipse { center, u: major, v, start, end };
                // The centre and axes are WCS; only the sense depends on the
                // extrusion, which is what the flip of `v` alone expresses.
                let g = if ocs == Xf::FLIP_X {
                    if let Geometry::Ellipse { center, u, v, start, end } = g {
                        Geometry::Ellipse { center, u, v: [-v[0], -v[1], -v[2]], start, end }
                    } else {
                        g
                    }
                } else {
                    g
                };
                self.parsed(Kind::Ellipse, raw, &Xf::ID, g).into_iter().collect()
            }
            "LWPOLYLINE" => {
                let Some(ocs) = self.ocs(raw) else { return Vec::new() };
                let mut vertices: Vec<P3> = Vec::new();
                let mut bulges: Vec<f64> = Vec::new();
                let mut bad = false;
                let elevation = raw.f(38).unwrap_or(0.0);
                for (code, value) in &raw.pairs {
                    match code {
                        10 => match num(value) {
                            Some(x) => {
                                vertices.push([x, 0.0, elevation]);
                                bulges.push(0.0);
                            }
                            None => bad = true,
                        },
                        20 => match (vertices.last_mut(), num(value)) {
                            (Some(last), Some(y)) => last[1] = y,
                            _ => bad = true,
                        },
                        42 => match (bulges.last_mut(), num(value)) {
                            (Some(last), Some(b)) => *last = b,
                            _ => bad = true,
                        },
                        _ => {}
                    }
                }
                if bad || vertices.is_empty() {
                    self.malformed += 1;
                    return Vec::new();
                }
                let closed = raw.i(70).unwrap_or(0) & 1 == 1;
                let g = Geometry::Polyline { vertices, bulges, closed };
                self.parsed(Kind::LwPolyline, raw, &ocs, g).into_iter().collect()
            }
            "SPLINE" => {
                let flags = raw.i(70).unwrap_or(0);
                let degree = raw.i(71).unwrap_or(3).clamp(1, 11) as usize;
                let mut control: Vec<P3> = Vec::new();
                let mut fit: Vec<P3> = Vec::new();
                let mut bad = false;
                for (code, value) in &raw.pairs {
                    match code {
                        10 | 11 => match num(value) {
                            Some(x) => (if *code == 10 { &mut control } else { &mut fit }).push([x, 0.0, 0.0]),
                            None => bad = true,
                        },
                        20 | 21 => {
                            let list = if *code == 20 { &mut control } else { &mut fit };
                            match (list.last_mut(), num(value)) {
                                (Some(last), Some(y)) => last[1] = y,
                                _ => bad = true,
                            }
                        }
                        30 | 31 => {
                            let list = if *code == 30 { &mut control } else { &mut fit };
                            match (list.last_mut(), num(value)) {
                                (Some(last), Some(z)) => last[2] = z,
                                _ => bad = true,
                            }
                        }
                        _ => {}
                    }
                }
                if control.len() > MAX_DXF_SPLINE_POINTS || fit.len() > MAX_DXF_SPLINE_POINTS {
                    self.spline_over_cap += 1;
                    return Vec::new();
                }
                if bad || (control.len() < 2 && fit.len() < 2) {
                    self.malformed += 1;
                    return Vec::new();
                }
                let g = Geometry::Spline {
                    degree,
                    control,
                    knots: raw.all_f(40),
                    weights: raw.all_f(41),
                    fit,
                    closed: flags & 1 == 1,
                };
                self.parsed(Kind::Spline, raw, &Xf::ID, g).into_iter().collect()
            }
            "TEXT" => self.text_entity(Kind::Text, raw).into_iter().collect(),
            "MTEXT" => self.text_entity(Kind::MText, raw).into_iter().collect(),
            "ATTRIB" => self.text_entity(Kind::Attrib, raw).into_iter().collect(),
            "SOLID" | "TRACE" => {
                let Some(ocs) = self.ocs(raw) else { return Vec::new() };
                let (Some(p0), Some(p1), Some(p2)) = (raw.point(10), raw.point(11), raw.point(12)) else {
                    self.malformed += 1;
                    return Vec::new();
                };
                // A SOLID's corners are written in bow-tie order: 1, 2, 4, 3.
                let mut vertices = vec![p0, p1];
                if let Some(p3) = raw.point(13)
                    && (p3[0] - p2[0]).abs() + (p3[1] - p2[1]).abs() > 1e-12
                {
                    vertices.push(p3);
                }
                vertices.push(p2);
                let n = vertices.len();
                let g = Geometry::Polyline { vertices, bulges: vec![0.0; n], closed: true };
                self.parsed(Kind::Solid, raw, &ocs, g).into_iter().collect()
            }
            "HATCH" => self.hatch(raw),
            "VERTEX" | "SEQEND" => Vec::new(),
            other => {
                let key: String = other.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(32).collect();
                *self.skipped.entry(if key.is_empty() { "?".into() } else { key }).or_insert(0) += 1;
                Vec::new()
            }
        }
    }

    fn polyline(&mut self, raw: &Raw<'a>, verts: Vec<(P3, f64, i64)>) -> Option<Parsed> {
        let flags = raw.i(70).unwrap_or(0);
        if flags & (16 | 64) != 0 {
            *self.skipped.entry("POLYLINE_MESH".into()).or_insert(0) += 1;
            return None;
        }
        let ocs = self.ocs(raw)?;
        let elevation = raw.f(30).unwrap_or(0.0);
        let spline_fit = flags & 4 != 0;
        let is_3d = flags & 8 != 0;
        let mut vertices = Vec::with_capacity(verts.len());
        let mut bulges = Vec::with_capacity(verts.len());
        for (p, bulge, vflags) in verts {
            // A spline-fit polyline carries its frame (bit 16) beside the
            // fitted vertices (bit 8); the curve is the fitted ones.
            if spline_fit && vflags & 16 != 0 {
                continue;
            }
            let z = if is_3d { p[2] } else { elevation };
            vertices.push([p[0], p[1], z]);
            bulges.push(if is_3d { 0.0 } else { bulge });
        }
        if vertices.is_empty() {
            self.malformed += 1;
            return None;
        }
        let g = Geometry::Polyline { vertices, bulges, closed: flags & 1 == 1 };
        self.parsed(Kind::Polyline, raw, &ocs, g)
    }

    fn text_entity(&mut self, kind: Kind, raw: &Raw<'a>) -> Option<Parsed> {
        let ocs = if kind == Kind::MText { Xf::ID } else { self.ocs(raw)? };
        let mut text = String::new();
        if kind == Kind::MText {
            for (code, value) in &raw.pairs {
                if *code == 3 {
                    text.push_str(value);
                }
            }
        }
        text.push_str(raw.first(1).unwrap_or(""));
        if text.len() > MAX_DXF_TEXT_BYTES {
            let mut cut = MAX_DXF_TEXT_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            self.text_cut += 1;
        }
        let text = if kind == Kind::MText { mtext_plain(&text) } else { text_plain(&text) };
        let justified = raw.i(72).unwrap_or(0) != 0 || raw.i(73).unwrap_or(0) != 0;
        let at = if kind != Kind::MText && justified && raw.f(11).is_some() { raw.point(11) } else { raw.point(10) };
        let Some(at) = at else {
            self.malformed += 1;
            return None;
        };
        let height = raw.f(40).unwrap_or(0.0).abs();
        let rotation = match raw.f(50) {
            Some(r) => r,
            None if kind == Kind::MText => match (raw.f(11), raw.f(21)) {
                (Some(dx), Some(dy)) if dx.abs() + dy.abs() > EPS => fmath::atan2(dy, dx).to_degrees(),
                _ => 0.0,
            },
            None => 0.0,
        };
        self.parsed(kind, raw, &ocs, Geometry::Text { at, text, height, rotation })
    }

    fn insert(&mut self, raw: &Raw<'a>) -> Option<InsertRec> {
        let Some(block) = raw.s(2) else {
            self.malformed += 1;
            return None;
        };
        let flip = match self.ocs(raw) {
            Some(x) => x == Xf::FLIP_X,
            None => return None,
        };
        let Some(at) = raw.point(10) else {
            self.malformed += 1;
            return None;
        };
        let scale = [raw.f(41).unwrap_or(1.0), raw.f(42).unwrap_or(1.0), raw.f(43).unwrap_or(1.0)];
        if scale.iter().any(|s| s.abs() < 1e-12) {
            self.malformed += 1;
            return None;
        }
        let (layer, aci, true_color) = self.common(raw);
        Some(InsertRec {
            block: block.to_owned(),
            layer,
            aci,
            true_color,
            at,
            scale,
            rotation_deg: raw.f(50).unwrap_or(0.0),
            cols: raw.i(70).unwrap_or(1).clamp(1, 1 << 20) as usize,
            rows: raw.i(71).unwrap_or(1).clamp(1, 1 << 20) as usize,
            col_spacing: raw.f(44).unwrap_or(0.0),
            row_spacing: raw.f(45).unwrap_or(0.0),
            flip,
        })
    }

    /// The boundary paths of a `HATCH`, each as a closed polyline (arcs as
    /// bulges, elliptical and spline edges as chords within tolerance).
    fn hatch(&mut self, raw: &Raw<'a>) -> Vec<Parsed> {
        let Some(ocs) = self.ocs(raw) else { return Vec::new() };
        let Some(start) = raw.pairs.iter().position(|(c, _)| *c == 91) else {
            self.malformed += 1;
            return Vec::new();
        };
        let n_loops = int(raw.pairs[start].1).unwrap_or(0).clamp(0, 100_000) as usize;
        let pairs = &raw.pairs;
        let mut i = start + 1;
        let mut out = Vec::new();
        let mut bad = false;
        let take = |i: &mut usize, code: i32| -> Option<f64> {
            if *i < pairs.len() && pairs[*i].0 == code {
                let v = num(pairs[*i].1);
                *i += 1;
                v
            } else {
                None
            }
        };
        let tol = self.tolerance;
        'loops: for _ in 0..n_loops {
            let Some(flags) = take(&mut i, 92) else {
                bad = true;
                break;
            };
            let flags = flags as i64;
            let mut vertices: Vec<P3> = Vec::new();
            let mut bulges: Vec<f64> = Vec::new();
            if flags & 2 != 0 {
                // A polyline boundary: 72 has-bulge, 73 closed, 93 count.
                let _has_bulge = take(&mut i, 72);
                let _closed = take(&mut i, 73);
                let Some(count) = take(&mut i, 93) else {
                    bad = true;
                    break;
                };
                for _ in 0..(count.max(0.0) as usize) {
                    let (Some(x), Some(y)) = (take(&mut i, 10), take(&mut i, 20)) else {
                        bad = true;
                        break 'loops;
                    };
                    vertices.push([x, y, 0.0]);
                    bulges.push(take(&mut i, 42).unwrap_or(0.0));
                }
            } else {
                let Some(count) = take(&mut i, 93) else {
                    bad = true;
                    break;
                };
                for _ in 0..(count.max(0.0) as usize) {
                    let Some(edge) = take(&mut i, 72) else {
                        bad = true;
                        break 'loops;
                    };
                    match edge as i64 {
                        1 => {
                            let (Some(x0), Some(y0), Some(x1), Some(y1)) = (take(&mut i, 10), take(&mut i, 20), take(&mut i, 11), take(&mut i, 21)) else {
                                bad = true;
                                break 'loops;
                            };
                            vertices.push([x0, y0, 0.0]);
                            bulges.push(0.0);
                            vertices.push([x1, y1, 0.0]);
                            bulges.push(0.0);
                        }
                        2 => {
                            let (Some(cx), Some(cy), Some(r), Some(a0), Some(a1)) =
                                (take(&mut i, 10), take(&mut i, 20), take(&mut i, 40), take(&mut i, 50), take(&mut i, 51))
                            else {
                                bad = true;
                                break 'loops;
                            };
                            let ccw = take(&mut i, 73).unwrap_or(1.0) != 0.0;
                            let (a0, a1) = (a0.to_radians(), a1.to_radians());
                            if !(sane_angle(a0) && sane_angle(a1) && r > 0.0 && r.is_finite()) {
                                bad = true;
                                break 'loops;
                            }
                            let sweep = if ccw { sweep_ccw(a0, a1) } else { sweep_ccw(a1, a0) };
                            let sign = if ccw { 1.0 } else { -1.0 };
                            // Three points on a circle fix an arc only up to a
                            // half turn; split a longer one.
                            let pieces = if sweep > PI * 0.999 { 2 } else { 1 };
                            let step = sweep / pieces as f64;
                            for k in 0..pieces {
                                let t = a0 + sign * step * k as f64;
                                let (sn, cs) = fmath::sin_cos(t);
                                vertices.push([cx + r * cs, cy + r * sn, 0.0]);
                                bulges.push(sign * fmath::tan(step / 4.0));
                            }
                            let t = a0 + sign * sweep;
                            let (sn, cs) = fmath::sin_cos(t);
                            vertices.push([cx + r * cs, cy + r * sn, 0.0]);
                            bulges.push(0.0);
                        }
                        3 => {
                            let (Some(cx), Some(cy), Some(mx), Some(my), Some(ratio), Some(a0), Some(a1)) = (
                                take(&mut i, 10),
                                take(&mut i, 20),
                                take(&mut i, 11),
                                take(&mut i, 21),
                                take(&mut i, 40),
                                take(&mut i, 50),
                                take(&mut i, 51),
                            ) else {
                                bad = true;
                                break 'loops;
                            };
                            let ccw = take(&mut i, 73).unwrap_or(1.0) != 0.0;
                            let (a0, a1) = (a0.to_radians(), a1.to_radians());
                            if !(sane_angle(a0) && sane_angle(a1) && ratio > 0.0 && ratio.is_finite()) {
                                bad = true;
                                break 'loops;
                            }
                            let sweep = if ccw { sweep_ccw(a0, a1) } else { sweep_ccw(a1, a0) };
                            let sign = if ccw { 1.0 } else { -1.0 };
                            let u = [mx, my, 0.0];
                            let v = [-my * ratio, mx * ratio, 0.0];
                            let g = Geometry::Ellipse { center: [cx, cy, 0.0], u, v, start: a0, end: a0 + sign * sweep };
                            for p in ellipse_points(&g, tol) {
                                vertices.push([p.x, p.y, 0.0]);
                                bulges.push(0.0);
                            }
                        }
                        4 => {
                            let degree = take(&mut i, 94).unwrap_or(3.0).clamp(1.0, 11.0) as usize;
                            let _rational = take(&mut i, 73);
                            let _periodic = take(&mut i, 74);
                            let nk = take(&mut i, 95).unwrap_or(0.0).clamp(0.0, MAX_DXF_SPLINE_POINTS as f64) as usize;
                            let nc = take(&mut i, 96).unwrap_or(0.0).clamp(0.0, MAX_DXF_SPLINE_POINTS as f64) as usize;
                            let mut knots = Vec::with_capacity(nk);
                            for _ in 0..nk {
                                let Some(k) = take(&mut i, 40) else {
                                    bad = true;
                                    break 'loops;
                                };
                                knots.push(k);
                            }
                            let mut control = Vec::with_capacity(nc);
                            let mut weights = Vec::with_capacity(nc);
                            for _ in 0..nc {
                                let (Some(x), Some(y)) = (take(&mut i, 10), take(&mut i, 20)) else {
                                    bad = true;
                                    break 'loops;
                                };
                                control.push(P2::new(x, y));
                                weights.push(take(&mut i, 42).unwrap_or(1.0));
                            }
                            let nf = take(&mut i, 97).unwrap_or(0.0).clamp(0.0, MAX_DXF_SPLINE_POINTS as f64) as usize;
                            let mut fit = Vec::with_capacity(nf);
                            for _ in 0..nf {
                                let (Some(x), Some(y)) = (take(&mut i, 11), take(&mut i, 21)) else {
                                    bad = true;
                                    break 'loops;
                                };
                                fit.push(P2::new(x, y));
                            }
                            // Tangents, when written.
                            take(&mut i, 12);
                            take(&mut i, 22);
                            take(&mut i, 13);
                            take(&mut i, 23);
                            let segs = if control.len() >= 2 {
                                curve::bspline_segments(degree, &control, &knots, &weights, tol)
                            } else if fit.len() >= 2 {
                                sketch::catmull_rom(&fit, false, 0.5)
                            } else {
                                Vec::new()
                            };
                            for p in flatten(&segs, tol) {
                                vertices.push([p.x, p.y, 0.0]);
                                bulges.push(0.0);
                            }
                        }
                        _ => {
                            bad = true;
                            break 'loops;
                        }
                    }
                }
            }
            // The loop's source objects: 97 count, then 330 handles.
            if let Some(n) = take(&mut i, 97) {
                for _ in 0..(n.max(0.0) as usize) {
                    if i < pairs.len() && pairs[i].0 == 330 {
                        i += 1;
                    }
                }
            }
            // A text-box loop (bit 8) is not a boundary of the fill.
            if flags & 8 != 0 || vertices.len() < 2 {
                continue;
            }
            let n = vertices.len();
            bulges.truncate(n);
            let g = Geometry::Polyline { vertices, bulges, closed: true };
            if let Some(p) = self.parsed(Kind::Hatch, raw, &ocs, g) {
                out.push(p);
            }
        }
        if bad {
            self.malformed += 1;
        }
        out
    }
}

fn true_color(raw: &Raw<'_>) -> Option<[u8; 3]> {
    raw.i(420).map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A `TEXT` value as plain text: the `%%` codes decoded, control
/// characters out.
pub fn text_plain(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '%' && b.get(i + 1) == Some(&'%') {
            match b.get(i + 2) {
                Some('d') | Some('D') => out.push('°'),
                Some('p') | Some('P') => out.push('±'),
                Some('c') | Some('C') => out.push('Ø'),
                Some('%') => out.push('%'),
                Some('u') | Some('U') | Some('o') | Some('O') => {}
                Some(d) if d.is_ascii_digit() => {
                    let code: String = b[i + 2..(i + 5).min(b.len())].iter().take_while(|c| c.is_ascii_digit()).collect();
                    if let Some(ch) = code.parse::<u32>().ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                    i += 2 + code.len();
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
            i += 3;
            continue;
        }
        if c == '\n' || !c.is_control() {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// An `MTEXT` value as plain text: paragraphs to newlines, the inline
/// formatting codes (`\A1;`, `\C1;`, `\H2.5x;`, `\fArial|b0;`, `\L`…`\l`,
/// `{}` groups, `\S1^2;` stacking as `1/2`, `\U+00B0`) removed or decoded,
/// then the `TEXT` codes.
pub fn mtext_plain(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            '\\' if i + 1 < b.len() => {
                let n = b[i + 1];
                match n {
                    'P' => {
                        out.push('\n');
                        i += 2;
                    }
                    '~' => {
                        out.push(' ');
                        i += 2;
                    }
                    '\\' | '{' | '}' => {
                        out.push(n);
                        i += 2;
                    }
                    'L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'N' | 'X' => i += 2,
                    'A' | 'C' | 'c' | 'H' | 'W' | 'Q' | 'T' | 'F' | 'f' | 'p' => {
                        let mut j = i + 2;
                        while j < b.len() && b[j] != ';' {
                            j += 1;
                        }
                        i = j + 1;
                    }
                    'S' => {
                        let mut j = i + 2;
                        let mut frag = String::new();
                        while j < b.len() && b[j] != ';' {
                            frag.push(b[j]);
                            j += 1;
                        }
                        let frag = frag.replace(['^', '#'], "/");
                        out.push_str(frag.trim());
                        i = j + 1;
                    }
                    'U' if b.get(i + 2) == Some(&'+') => {
                        let hex: String = b[i + 3..(i + 7).min(b.len())].iter().collect();
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(ch) if hex.len() == 4 => {
                                out.push(ch);
                                i += 7;
                            }
                            _ => i += 2,
                        }
                    }
                    'M' if b.get(i + 2) == Some(&'+') => i += 8,
                    _ => i += 2,
                }
            }
            '{' | '}' => i += 1,
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    text_plain(&out)
}

/// Points along an ellipse arc, dense enough for `tol`, endpoints included.
fn ellipse_points(g: &Geometry, tol: f64) -> Vec<P2> {
    let Geometry::Ellipse { center, u, v, start, end } = g else { return Vec::new() };
    let r = (u[0] * u[0] + u[1] * u[1]).sqrt().max((v[0] * v[0] + v[1] * v[1]).sqrt());
    let sweep = end - start;
    let per = if r > tol { 2.0 * fmath::acos((1.0 - tol / r).clamp(-1.0, 1.0)) } else { PI / 2.0 };
    let n = if per > 1e-6 { ((sweep.abs() / per).ceil() as usize).clamp(2, 4096) } else { 4096 };
    (0..=n)
        .map(|i| {
            let t = start + sweep * (i as f64 / n as f64);
            let (sn, cs) = fmath::sin_cos(t);
            P2::new(center[0] + u[0] * cs + v[0] * sn, center[1] + u[1] * cs + v[1] * sn)
        })
        .collect()
}

/// A chain of segments as a polyline within `tol`, endpoints included.
fn flatten(segs: &[Segment], tol: f64) -> Vec<P2> {
    if segs.is_empty() {
        return Vec::new();
    }
    let mut pts = Loop { segments: segs.to_vec() }.sample_tolerance(tol);
    pts.push(segs[segs.len() - 1].end());
    pts
}

/// Cubic Béziers along an ellipse arc: pieces of at most a quarter turn,
/// each with the classic `4/3 tan(Δ/4)` handles, subdivided until the
/// approximation is inside `tol`. Exact under affine maps, which is why the
/// ellipse is kept in its parametrised form.
fn ellipse_beziers(g: &Geometry, tol: f64) -> Vec<Segment> {
    let Geometry::Ellipse { center, u, v, start, end } = g else { return Vec::new() };
    let sweep = end - start;
    if sweep.abs() < 1e-12 {
        return Vec::new();
    }
    let r = (u[0] * u[0] + u[1] * u[1]).sqrt().max((v[0] * v[0] + v[1] * v[1]).sqrt());
    // The quarter-turn cubic is off by 2.7e-4 r; the error falls as Δ^6.
    let mut per = PI / 2.0;
    if r > 0.0 {
        let ratio = (tol / (2.7e-4 * r)).max(1e-12);
        per = (per * fmath::powf(ratio, 1.0 / 6.0)).clamp(PI / 64.0, PI / 2.0);
    }
    let n = ((sweep.abs() / per).ceil() as usize).clamp(1, 256);
    let step = sweep / n as f64;
    let point = |t: f64| {
        let (sn, cs) = fmath::sin_cos(t);
        P2::new(center[0] + u[0] * cs + v[0] * sn, center[1] + u[1] * cs + v[1] * sn)
    };
    let deriv = |t: f64| {
        let (sn, cs) = fmath::sin_cos(t);
        P2::new(-u[0] * sn + v[0] * cs, -u[1] * sn + v[1] * cs)
    };
    let k = 4.0 / 3.0 * fmath::tan(step / 4.0);
    (0..n)
        .map(|i| {
            let t0 = start + step * i as f64;
            let t1 = t0 + step;
            let (a, b) = (point(t0), point(t1));
            let (da, db) = (deriv(t0), deriv(t1));
            Segment::Bezier { a, b, ctrl: vec![P2::new(a.x + k * da.x, a.y + k * da.y), P2::new(b.x - k * db.x, b.y - k * db.y)] }
        })
        .collect()
}

/// A circular arc from its centre as one or two [`Segment::Arc`]s (three
/// points on a circle fix an arc only up to a half turn).
fn arc_segments(c: P2, r: f64, start: f64, end: f64) -> Vec<Segment> {
    let sweep = sweep_ccw(start, end);
    let pieces = if sweep > PI * 0.999 { 2 } else { 1 };
    let step = sweep / pieces as f64;
    let on = |t: f64| {
        let (sn, cs) = fmath::sin_cos(t);
        P2::new(c.x + r * cs, c.y + r * sn)
    };
    (0..pieces)
        .map(|i| {
            let t0 = start + step * i as f64;
            Segment::Arc { a: on(t0), b: on(t0 + step), via: on(t0 + step / 2.0) }
        })
        .collect()
}

/// Case-insensitive glob with `*` and `?`.
pub fn glob(pattern: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some('?') => !s.is_empty() && go(&p[1..], &s[1..]),
            Some(c) => !s.is_empty() && s[0] == *c && go(&p[1..], &s[1..]),
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let s: Vec<char> = s.to_lowercase().chars().collect();
    go(&p, &s)
}

// ---------------------------------------------------------------- expansion

struct Emit<'p> {
    blocks: &'p HashMap<String, Block>,
    drawing: Drawing,
    layer_index: HashMap<String, usize>,
    /// The sanitised names already given out, so a new layer finds a free
    /// name without scanning every layer before it.
    layer_names: HashSet<String>,
    patterns: &'p [String],
    tol: f64,
    stack: Vec<String>,
    dropped: usize,
    depth_hits: usize,
    cycles: usize,
    missing: BTreeMap<String, usize>,
}

/// Where an entity is being placed: the `INSERT`'s layer (for entities on
/// layer `0`), its colour (for BYBLOCK), and its block's name.
struct Placement {
    layer: Option<usize>,
    color: [u8; 3],
    block: Option<String>,
}

impl<'p> Emit<'p> {
    fn layer_index(&mut self, raw: &str) -> usize {
        let key = raw.to_uppercase();
        if let Some(&i) = self.layer_index.get(&key) {
            return i;
        }
        let mut name = label::sanitise(raw);
        if name.is_empty() {
            name = "unnamed".into();
        }
        let base = name.clone();
        let mut k = 1;
        while self.layer_names.contains(&name) {
            name = format!("{base}#{k}");
            k += 1;
        }
        let selected = self.patterns.is_empty() || self.patterns.iter().any(|p| glob(p, raw) || glob(p, &name));
        let i = self.drawing.layers.len();
        self.layer_names.insert(name.clone());
        self.drawing.layers.push(Layer {
            name,
            raw: raw.to_owned(),
            aci: 7,
            color: aci_rgb(7),
            off: false,
            frozen: false,
            entities: 0,
            selected,
        });
        self.layer_index.insert(key, i);
        i
    }

    fn resolve_layer(&mut self, raw: &str, at: &Placement) -> usize {
        if raw == "0" && let Some(l) = at.layer {
            return l;
        }
        self.layer_index(raw)
    }

    fn resolve_color(&self, aci: i16, true_color: Option<[u8; 3]>, layer: usize, at: &Placement) -> [u8; 3] {
        if let Some(c) = true_color {
            return c;
        }
        match aci {
            0 => at.color,
            256 => self.drawing.layers[layer].color,
            n => aci_rgb(n.abs()),
        }
    }

    fn emit(&mut self, item: &Item, xf: &Xf, at: &Placement) -> Result<(), String> {
        match item {
            Item::Entity(e) => {
                let layer = self.resolve_layer(&e.layer, at);
                let color = self.resolve_color(e.aci, e.true_color, layer, at);
                let Some(geometry) = transform(&e.geometry, xf, self.tol, &mut self.drawing.flattened) else {
                    self.dropped += 1;
                    return Ok(());
                };
                self.drawing.layers[layer].entities += 1;
                if !self.drawing.layers[layer].selected {
                    self.drawing.unselected += 1;
                    return Ok(());
                }
                if self.drawing.entities.len() >= MAX_DXF_EXPANDED {
                    return Err(format!("the drawing expands to more than {MAX_DXF_EXPANDED} entities (INSERT arrays multiply)"));
                }
                self.drawing.entities.push(Entity { kind: e.kind, layer, aci: e.aci, color, block: at.block.clone(), geometry });
                Ok(())
            }
            Item::Insert(rec) => {
                let key = rec.block.to_uppercase();
                let Some(block) = self.blocks.get(&key) else {
                    *self.missing.entry(rec.block.clone()).or_insert(0) += 1;
                    return Ok(());
                };
                if self.stack.len() >= MAX_INSERT_DEPTH {
                    self.depth_hits += 1;
                    return Ok(());
                }
                if self.stack.contains(&key) {
                    self.cycles += 1;
                    return Ok(());
                }
                // An array multiplies: refuse what cannot fit before placing
                // any of it (nested INSERTs count once here; the per-entity
                // check below is the backstop).
                let want = rec.cols.saturating_mul(rec.rows).saturating_mul(block.items.len());
                if self.drawing.entities.len().saturating_add(want) > MAX_DXF_EXPANDED {
                    return Err(format!(
                        "the drawing expands to more than {MAX_DXF_EXPANDED} entities (an INSERT of {} placed {} × {} times)",
                        label::quoted(&block.raw),
                        rec.cols,
                        rec.rows
                    ));
                }
                self.drawing.inserts += 1;
                let layer = self.resolve_layer(&rec.layer, at);
                let color = self.resolve_color(rec.aci, rec.true_color, layer, at);
                let inner = Placement { layer: Some(layer), color, block: Some(label::sanitise(&block.raw)) };
                let (sn, cs) = fmath::sin_cos(rec.rotation_deg.to_radians());
                let [sx, sy, sz] = rec.scale;
                let ocs = if rec.flip { Xf::FLIP_X } else { Xf::ID };
                self.stack.push(key);
                let mut result = Ok(());
                'rows: for j in 0..rec.rows {
                    for i in 0..rec.cols {
                        let (ox, oy) = (i as f64 * rec.col_spacing, j as f64 * rec.row_spacing);
                        let tx = rec.at[0] + cs * ox - sn * oy;
                        let ty = rec.at[1] + sn * ox + cs * oy;
                        let m = [[cs * sx, -sn * sy], [sn * sx, cs * sy]];
                        let b = block.base;
                        let local = Xf {
                            m,
                            t: [tx - (m[0][0] * b[0] + m[0][1] * b[1]), ty - (m[1][0] * b[0] + m[1][1] * b[1])],
                            zs: sz,
                            zt: rec.at[2] - sz * b[2],
                        };
                        let total = xf.then(&ocs).then(&local);
                        for item in &block.items {
                            if let Err(e) = self.emit(item, &total, &inner) {
                                result = Err(e);
                                break 'rows;
                            }
                        }
                    }
                }
                self.stack.pop();
                result
            }
        }
    }
}

/// The parse and the expansion, unbounded: for tests that want a panic to
/// be a failure with a backtrace.
pub fn import_unbounded(bytes: &[u8], opts: &Options) -> Result<Drawing, String> {
    let pairs = prescan::dxf(bytes)?;
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let tol = opts.tolerance.max(1e-6);
    let mut p = Parser::new(text, tol);
    p.run();

    // The unit rule: `units=` wins; else `$INSUNITS` when it is one Cadpole
    // converts; else millimetres with a warning.
    let mut notes = Vec::new();
    let (unit, source) = match (opts.units, p.insunits) {
        (Unit::Auto, Some(code)) if code != 0 => match Unit::from_insunits(code) {
            Some(u) => (u, UnitSource::Header),
            None => {
                return Err(format!(
                    "$INSUNITS {code} ({}) is not a unit Cadpole converts; pass units= (one of {})",
                    Unit::insunits_name(code),
                    Unit::ALL.iter().skip(1).map(|u| u.name()).collect::<Vec<_>>().join(", ")
                ));
            }
        },
        (Unit::Auto, _) => (Unit::Mm, UnitSource::Assumed),
        (u, _) => (u, UnitSource::Option),
    };
    match source {
        UnitSource::Header => notes.push(Note::info(format!(
            "$INSUNITS says {}; coordinates scaled by {} to millimetres",
            Unit::insunits_name(p.insunits.unwrap_or(0)),
            cadpole_core::context::fmt_num(unit.mm())
        ))),
        UnitSource::Assumed => notes.push(Note::warning(
            "the file declares no unit ($INSUNITS is 0 or absent): coordinates read as millimetres; pass \
             units=dxf_units.ft (or inch, m, …) to say otherwise",
        )),
        UnitSource::Option => {
            if let Some(code) = p.insunits.filter(|c| *c != 0)
                && Unit::from_insunits(code) != Some(unit)
            {
                notes.push(Note::info(format!(
                    "units={} overrides the file's $INSUNITS ({})",
                    unit.name(),
                    Unit::insunits_name(code)
                )));
            }
        }
    }
    let scale = unit.mm();

    let mut emit = Emit {
        blocks: &p.blocks,
        drawing: Drawing {
            version: if p.version.is_empty() { String::new() } else { format!("{} ({})", p.version, release_of(&p.version)) },
            unit,
            unit_source: source,
            scale,
            layers: Vec::new(),
            entities: Vec::new(),
            extents: None,
            blocks: p.block_count,
            inserts: 0,
            skipped: std::mem::take(&mut p.skipped),
            tilted: p.tilted,
            malformed: p.malformed,
            unselected: 0,
            flattened: 0,
            notes: Vec::new(),
            pairs,
        },
        layer_index: HashMap::new(),
        layer_names: HashSet::new(),
        patterns: &opts.layers,
        tol,
        stack: Vec::new(),
        dropped: 0,
        depth_hits: 0,
        cycles: 0,
        missing: BTreeMap::new(),
    };
    // The table's layers first, in table order, with their colours.
    for rec in &p.layer_table {
        let i = emit.layer_index(&rec.raw);
        let l = &mut emit.drawing.layers[i];
        l.off = rec.aci < 0;
        l.aci = rec.aci.abs();
        l.color = rec.true_color.unwrap_or_else(|| aci_rgb(rec.aci.abs()));
        l.frozen = rec.flags & 1 != 0;
    }
    let root = Xf::scale(scale);
    let top = Placement { layer: None, color: aci_rgb(7), block: None };
    for item in &p.items {
        emit.emit(item, &root, &top)?;
    }
    let mut d = emit.drawing;
    d.malformed += emit.dropped;

    // A pattern that matched nothing is a typo, and the diagnostic lists
    // what there is.
    for pat in &opts.layers {
        if !d.layers.iter().any(|l| glob(pat, &l.raw) || glob(pat, &l.name)) {
            let have: Vec<String> = d.layers.iter().map(|l| label::quoted(&l.raw)).collect();
            return Err(format!(
                "layers={:?} matches no layer; the file has: {}",
                pat,
                if have.is_empty() { "none".to_owned() } else { have.join(", ") }
            ));
        }
    }

    if let ([Some(x0), Some(y0), z0], [Some(x1), Some(y1), z1]) = (p.extmin, p.extmax) {
        let lo = [x0 * scale, y0 * scale, z0.unwrap_or(0.0) * scale];
        let hi = [x1 * scale, y1 * scale, z1.unwrap_or(0.0) * scale];
        // AutoCAD writes ±1e20 for a drawing with no extents.
        if finite3(lo) && finite3(hi) && lo.iter().chain(&hi).all(|c| c.abs() < 1e15) && x0 <= x1 && y0 <= y1 {
            d.extents = Some((lo, hi));
        }
    }

    // What was not read, said once each.
    if !d.skipped.is_empty() {
        let parts: Vec<String> = d
            .skipped
            .iter()
            .map(|(k, n)| {
                let why = match k.as_str() {
                    "DIMENSION" | "LEADER" | "MLEADER" | "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => {
                        "every reader re-derives these from its own style; explode them to lines and TEXT to keep what was drawn"
                    }
                    "3DSOLID" | "REGION" | "BODY" | "SURFACE" | "PLANESURFACE" | "EXTRUDEDSURFACE" | "REVOLVEDSURFACE"
                    | "SWEPTSURFACE" | "LOFTEDSURFACE" | "NURBSURFACE" | "MESH" | "3DFACE" | "POLYLINE_MESH" | "HELIX" => {
                        "3D geometry (ACIS SAT data or a mesh): export the part as STEP for import_step"
                    }
                    "ACAD_PROXY_ENTITY" | "OLE2FRAME" => "a proxy for an application object: open it in the authoring program and explode it",
                    "ATTDEF" => "an attribute template; the ATTRIB values are read",
                    "IMAGE" | "WIPEOUT" | "VIEWPORT" | "LIGHT" | "SECTION" | "UNDERLAY" | "PDFUNDERLAY" | "DWFUNDERLAY" | "DGNUNDERLAY" => {
                        "not drawing geometry"
                    }
                    "XLINE" | "RAY" => "infinite construction lines",
                    "MLINE" | "TOLERANCE" | "SHAPE" => "not a 2D entity Cadpole reads",
                    _ => "not a 2D entity Cadpole reads",
                };
                format!("{k} ×{n} ({why})")
            })
            .collect();
        d.notes.push(Note::info(format!("not read: {}", parts.join("; "))));
    }
    if d.tilted > 0 {
        d.notes.push(Note::warning(format!(
            "{} entit{} lie{} outside the XY plane (an extrusion direction that is not ±Z) and {} skipped",
            d.tilted,
            if d.tilted == 1 { "y" } else { "ies" },
            if d.tilted == 1 { "s" } else { "" },
            if d.tilted == 1 { "was" } else { "were" }
        )));
    }
    if d.malformed > 0 {
        d.notes.push(Note::warning(format!(
            "{} entit{} had a missing, non-finite or zero-sized value and {} skipped",
            d.malformed,
            if d.malformed == 1 { "y" } else { "ies" },
            if d.malformed == 1 { "was" } else { "were" }
        )));
    }
    if p.spline_over_cap > 0 {
        d.notes.push(Note::warning(format!(
            "{} SPLINE{} with more than {MAX_DXF_SPLINE_POINTS} control or fit points skipped",
            p.spline_over_cap,
            if p.spline_over_cap == 1 { "" } else { "s" }
        )));
    }
    if d.flattened > 0 {
        d.notes.push(Note::info(format!(
            "{} bulge{} inside non-uniformly scaled blocks flattened within {} mm (an arc under a non-uniform scale is an ellipse)",
            d.flattened,
            if d.flattened == 1 { "" } else { "s" },
            cadpole_core::context::fmt_num(tol)
        )));
    }
    if !emit.missing.is_empty() {
        let parts: Vec<String> = emit.missing.iter().map(|(b, n)| format!("{} ×{n}", label::quoted(b))).collect();
        d.notes.push(Note::warning(format!("INSERT of a block the file does not define, skipped: {}", parts.join(", "))));
    }
    if emit.depth_hits > 0 {
        d.notes.push(Note::warning(format!(
            "{} INSERT{} nested deeper than {MAX_INSERT_DEPTH} skipped",
            emit.depth_hits,
            if emit.depth_hits == 1 { "" } else { "s" }
        )));
    }
    if emit.cycles > 0 {
        d.notes.push(Note::warning(format!(
            "{} INSERT{} of a block inside itself (a cycle) skipped",
            emit.cycles,
            if emit.cycles == 1 { "" } else { "s" }
        )));
    }
    if p.text_cut > 0 {
        d.notes.push(Note::info(format!(
            "{} text{} longer than {MAX_DXF_TEXT_BYTES} bytes cut to that",
            p.text_cut,
            if p.text_cut == 1 { "" } else { "s" }
        )));
    }
    if d.unselected > 0 {
        d.notes.push(Note::info(format!("{} entities on layers layers= did not keep", d.unselected)));
    }
    d.notes.splice(0..0, notes);
    Ok(d)
}

/// Read a DXF under the pre-scan and the deadline.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Drawing, String> {
    prescan::dxf(bytes)?;
    let bytes = bytes.to_vec();
    let opts = opts.clone();
    match bounded::run(opts.budget_ms, move || import_unbounded(&bytes, &opts)) {
        Ok(r) => r,
        Err(stopped) => Err(format!("DXF import {stopped}")),
    }
}

// ---------------------------------------------------------------- derived views

/// What [`Drawing::sketch`] produces: loops and construction paths named by
/// layer and index.
#[derive(Clone, Debug, Default)]
pub struct SketchOut {
    pub loops: Vec<(String, Loop)>,
    pub paths: Vec<(String, Path)>,
    /// Chains that did not close and were not asked for.
    pub open_dropped: usize,
    /// Loops not asked for (`closed = Some(false)`).
    pub loops_dropped: usize,
}

/// One planar curve of the drawing.
#[derive(Clone, Debug)]
pub struct CurveOut {
    pub name: String,
    pub segments: Vec<Segment>,
    pub closed: bool,
    /// The one z every point of the entity shares, when it does.
    pub z: Option<f64>,
}

/// One label.
#[derive(Clone, Debug, PartialEq)]
pub struct TextOut {
    pub at: P3,
    pub text: String,
    pub height: f64,
    pub rotation: f64,
}

impl Drawing {
    /// The layers `patterns` select (empty: every layer `layers=` kept), as
    /// a mask over [`Drawing::layers`]; a pattern that matches no layer is
    /// an error listing what there is.
    pub fn select(&self, patterns: &[String]) -> Result<Vec<bool>, String> {
        if patterns.is_empty() {
            return Ok(self.layers.iter().map(|l| l.selected).collect());
        }
        let mut mask = vec![false; self.layers.len()];
        for pat in patterns {
            let mut any = false;
            for (i, l) in self.layers.iter().enumerate() {
                if l.selected && (glob(pat, &l.raw) || glob(pat, &l.name)) {
                    mask[i] = true;
                    any = true;
                }
            }
            if !any {
                let have: Vec<&str> = self.layers.iter().filter(|l| l.selected).map(|l| l.name.as_str()).collect();
                return Err(format!(
                    "layer {pat:?} matches no imported layer; the drawing has: {}",
                    if have.is_empty() { "none".to_owned() } else { have.join(", ") }
                ));
            }
        }
        Ok(mask)
    }

    /// The names of the layers `layers=` kept.
    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().filter(|l| l.selected).map(|l| l.name.clone()).collect()
    }

    /// The 2D segments of a geometry entity and whether it closes on its
    /// own; `None` for a point or a label.
    pub fn segments(&self, e: &Entity, tol: f64) -> Option<(Vec<Segment>, bool)> {
        let p2 = |p: &P3| P2::new(p[0], p[1]);
        Some(match &e.geometry {
            Geometry::Line { a, b } => (vec![Segment::Line { a: p2(a), b: p2(b) }], false),
            Geometry::Polyline { vertices, bulges, closed } => {
                let n = vertices.len();
                let edges = if *closed { n } else { n.saturating_sub(1) };
                let mut segs = Vec::with_capacity(edges);
                for i in 0..edges {
                    let a = p2(&vertices[i]);
                    let b = p2(&vertices[(i + 1) % n]);
                    if (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS {
                        continue;
                    }
                    segs.push(curve::bulge_segment(a, b, bulges.get(i).copied().unwrap_or(0.0)));
                }
                let closes = *closed && segs.len() >= 2;
                (segs, closes)
            }
            Geometry::Arc { center, radius, start, end } => (arc_segments(p2(center), *radius, *start, *end), false),
            Geometry::Circle { center, radius } => (Loop::circle(p2(center), *radius).segments, true),
            Geometry::Ellipse { center, u, v, start, end } => {
                let lu = (u[0] * u[0] + u[1] * u[1]).sqrt();
                let lv = (v[0] * v[0] + v[1] * v[1]).sqrt();
                let full = ((end - start).abs() - TAU).abs() < 1e-9;
                let circular = (lu - lv).abs() <= 1e-9 * lu.max(1.0) && (u[0] * v[0] + u[1] * v[1]).abs() <= 1e-9 * lu * lu;
                if circular && lu > EPS {
                    // A transformed circle: back to exact arcs. The
                    // parameter's zero is along `u`, and `v` says which way
                    // it turns.
                    let phase = fmath::atan2(u[1], u[0]);
                    let ccw = u[0] * v[1] - u[1] * v[0] > 0.0;
                    if full {
                        (Loop::circle(p2(center), lu).segments, true)
                    } else if ccw {
                        (arc_segments(p2(center), lu, phase + start, phase + end), false)
                    } else {
                        (arc_segments(p2(center), lu, phase - end, phase - start), false)
                    }
                } else {
                    (ellipse_beziers(&e.geometry, tol), full)
                }
            }
            Geometry::Spline { degree, control, knots, weights, fit, closed } => {
                let segs = if control.len() >= 2 {
                    let ctrl: Vec<P2> = control.iter().map(p2).collect();
                    curve::bspline_segments(*degree, &ctrl, knots, weights, tol)
                } else {
                    let pts: Vec<P2> = fit.iter().map(p2).collect();
                    sketch::catmull_rom(&pts, *closed, 0.5)
                };
                let closes = *closed && segs.len() >= 2;
                (segs, closes)
            }
            Geometry::Point { .. } | Geometry::Text { .. } => return None,
        })
    }

    /// Points of an entity whose bounding box is the entity's, for
    /// [`Drawing::bbox`] and the constant-z test: the ends of every piece
    /// and, for curves, the points where they turn back in x or y (an arc's
    /// axis crossings, an ellipse's tangents parallel to an axis, a Bézier's
    /// derivative roots), so the box is exact rather than sampled.
    fn extent_points(&self, e: &Entity) -> Vec<P3> {
        match &e.geometry {
            Geometry::Line { a, b } => vec![*a, *b],
            Geometry::Polyline { vertices, bulges, closed } => {
                let mut out = vertices.clone();
                let n = vertices.len();
                let edges = if *closed { n } else { n.saturating_sub(1) };
                for i in 0..edges {
                    let a = vertices[i];
                    let b = vertices[(i + 1) % n];
                    if let Some((c, r, t0, sweep)) = bulge_arc(P2::new(a[0], a[1]), P2::new(b[0], b[1]), bulges.get(i).copied().unwrap_or(0.0)) {
                        for p in arc_extremes(c, r, t0, sweep) {
                            out.push([p.x, p.y, a[2]]);
                        }
                    }
                }
                out
            }
            Geometry::Arc { center, radius, start, end } => {
                let sweep = sweep_ccw(*start, *end);
                arc_extremes(P2::new(center[0], center[1]), *radius, *start, sweep)
                    .into_iter()
                    .map(|p| [p.x, p.y, center[2]])
                    .collect()
            }
            Geometry::Circle { center, radius } => vec![
                [center[0] - radius, center[1] - radius, center[2]],
                [center[0] + radius, center[1] + radius, center[2]],
            ],
            Geometry::Ellipse { center, u, v, start, end } => {
                let at = |t: f64| {
                    let (sn, cs) = fmath::sin_cos(t);
                    [center[0] + u[0] * cs + v[0] * sn, center[1] + u[1] * cs + v[1] * sn, center[2]]
                };
                let mut out = vec![at(*start), at(*end)];
                // x' = -u.x sin t + v.x cos t is zero at atan2(v.x, u.x) and
                // half a turn on; likewise for y.
                for base in [fmath::atan2(v[0], u[0]), fmath::atan2(v[1], u[1])] {
                    for t in angles_within(base, PI, *start, *end - *start) {
                        out.push(at(t));
                    }
                }
                out
            }
            Geometry::Spline { degree, control, knots, weights, fit, closed } => {
                let pts = if control.len() >= 2 { control } else { fit };
                if pts.len() < 2 {
                    return pts.clone();
                }
                let p2 = |p: &P3| P2::new(p[0], p[1]);
                let (mut lo, mut hi) = (pts[0], pts[0]);
                for p in pts {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                // Only rational and high-degree splines are re-fitted at this
                // tolerance; the rest convert exactly.
                let tol = 1e-6 * fmath::hypot(hi[0] - lo[0], hi[1] - lo[1]).max(1e-9);
                let segs = if control.len() >= 2 {
                    let ctrl: Vec<P2> = control.iter().map(p2).collect();
                    curve::bspline_segments(*degree, &ctrl, knots, weights, tol)
                } else {
                    let f: Vec<P2> = fit.iter().map(p2).collect();
                    sketch::catmull_rom(&f, *closed, 0.5)
                };
                // The plan-view box is the curve's. A spline whose points do
                // not share one z keeps the control points' z range at both
                // corners, so the box covers it and constant_z says no.
                let mut out = Vec::new();
                for s in &segs {
                    for p in segment_extremes(s) {
                        out.push([p.x, p.y, lo[2]]);
                        if hi[2] != lo[2] {
                            out.push([p.x, p.y, hi[2]]);
                        }
                    }
                }
                if out.is_empty() { pts.clone() } else { out }
            }
            Geometry::Point { at } => vec![*at],
            Geometry::Text { at, .. } => vec![*at],
        }
    }

    /// The z every point of the entity shares, when there is one.
    pub fn constant_z(&self, e: &Entity) -> Option<f64> {
        let pts = self.extent_points(e);
        let z = pts.first()?[2];
        pts.iter().all(|p| (p[2] - z).abs() <= 1e-9).then_some(z)
    }

    /// The layer's closed loops and, unless `closed` says otherwise, its
    /// open chains: closed entities are loops on their own; the rest are
    /// joined end to end within `tol` (FreeCAD's "join geometry"), and a
    /// chain that closes is a loop, one that does not a construction path.
    /// `closed = Some(true)` keeps only loops, `Some(false)` only chains.
    /// Loops are `<layer>/loop[k]`, paths `<layer>/path[k]`, per layer.
    pub fn sketch(&self, patterns: &[String], closed: Option<bool>, tol: f64) -> Result<SketchOut, String> {
        let mask = self.select(patterns)?;
        let tol = tol.max(1e-9);
        let mut out = SketchOut::default();
        // Entities by layer, in file order, in one pass: filtering the whole
        // list once per layer was layers x entities.
        let mut by_layer: Vec<Vec<&Entity>> = vec![Vec::new(); self.layers.len()];
        for e in &self.entities {
            if mask[e.layer] {
                by_layer[e.layer].push(e);
            }
        }
        for (li, layer) in self.layers.iter().enumerate() {
            if !mask[li] {
                continue;
            }
            let mut loops = Vec::new();
            let mut loose = Vec::new();
            for e in std::mem::take(&mut by_layer[li]) {
                let Some((segs, closes)) = self.segments(e, tol) else { continue };
                if segs.is_empty() {
                    continue;
                }
                if closes {
                    loops.push(Loop { segments: segs });
                } else {
                    loose.extend(segs);
                }
            }
            let (joined, paths) = if loose.is_empty() { (Vec::new(), Vec::new()) } else { curve::join(loose, tol) };
            if closed == Some(false) {
                out.loops_dropped += loops.len() + joined.len();
            } else {
                for (k, l) in loops.into_iter().chain(joined).enumerate() {
                    out.loops.push((format!("{}/loop[{k}]", layer.name), l));
                }
            }
            if closed == Some(true) {
                out.open_dropped += paths.len();
            } else {
                for (k, p) in paths.into_iter().enumerate() {
                    out.paths.push((format!("{}/path[{k}]", layer.name), p));
                }
            }
        }
        Ok(out)
    }

    /// Every geometry entity as a planar curve, named `<layer>/<kind>[i]`
    /// with `i` its index among that kind on the layer, with the z it lies
    /// at when it lies at one.
    pub fn curves(&self, patterns: &[String], tol: f64) -> Result<Vec<CurveOut>, String> {
        let mask = self.select(patterns)?;
        let mut counts: HashMap<(usize, Kind), usize> = HashMap::new();
        let mut out = Vec::new();
        for e in &self.entities {
            if !mask[e.layer] {
                continue;
            }
            let Some((segments, closed)) = self.segments(e, tol) else { continue };
            if segments.is_empty() {
                continue;
            }
            let k = counts.entry((e.layer, e.kind)).or_insert(0);
            let name = format!("{}/{}[{}]", self.layers[e.layer].name, e.kind.name(), *k);
            *k += 1;
            out.push(CurveOut { name, segments, closed, z: self.constant_z(e) });
        }
        Ok(out)
    }

    /// `POINT` entities, and the vertices of lines and polylines that carry
    /// an elevation (any vertex off z = 0): what a survey's spot shots and
    /// 3D breaklines are.
    pub fn points(&self, patterns: &[String]) -> Result<Vec<P3>, String> {
        let mask = self.select(patterns)?;
        let mut out = Vec::new();
        for e in &self.entities {
            if !mask[e.layer] {
                continue;
            }
            match &e.geometry {
                Geometry::Point { at } => out.push(*at),
                Geometry::Line { a, b } if a[2] != 0.0 || b[2] != 0.0 => out.extend([*a, *b]),
                Geometry::Polyline { vertices, .. } if vertices.iter().any(|p| p[2] != 0.0) => out.extend(vertices.iter().copied()),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Every label: `TEXT`, `MTEXT`, and the `ATTRIB` values of blocks.
    pub fn texts(&self, patterns: &[String]) -> Result<Vec<TextOut>, String> {
        let mask = self.select(patterns)?;
        Ok(self
            .entities
            .iter()
            .filter(|e| mask[e.layer])
            .filter_map(|e| match &e.geometry {
                Geometry::Text { at, text, height, rotation } => {
                    Some(TextOut { at: *at, text: text.clone(), height: *height, rotation: *rotation })
                }
                _ => None,
            })
            .collect())
    }

    /// Spot elevations: every label whose text reads as one, at the label's
    /// placement point, the number scaled to millimetres as the coordinates
    /// were. See [`elevation_of`] for the convention and `pattern`.
    pub fn spot_elevations(&self, patterns: &[String], pattern: Option<&str>) -> Result<Vec<P3>, String> {
        if let Some(p) = pattern
            && !p.contains('#')
        {
            return Err(format!("pattern {p:?} has no `#`, the placeholder for the elevation"));
        }
        let mut out = Vec::new();
        for t in self.texts(patterns)? {
            if let Some(z) = elevation_of(&t.text, pattern) {
                out.push([t.at[0], t.at[1], z * self.scale]);
            }
        }
        Ok(out)
    }

    /// Contour lines: every polyline or spline on the layers, with its
    /// elevation from the entity's own z when every vertex shares one off
    /// zero, else from the last number in the layer's name (`CONTOUR-150`,
    /// `C-TOPO-152.5`), scaled to millimetres; an entity with neither is
    /// skipped. Each comes back flattened within `tol`, a closed one with
    /// its first point repeated at the end, in file order.
    pub fn contours(&self, patterns: &[String], tol: f64) -> Result<Vec<(f64, Vec<P2>)>, String> {
        let mask = self.select(patterns)?;
        let layer_elevation: Vec<Option<f64>> = self.layers.iter().map(|l| trailing_number(&l.raw)).collect();
        let mut out = Vec::new();
        for e in &self.entities {
            if !mask[e.layer] || !matches!(e.kind, Kind::LwPolyline | Kind::Polyline | Kind::Spline | Kind::Line) {
                continue;
            }
            let elevation = match self.constant_z(e) {
                Some(z) if z != 0.0 => z,
                _ => match layer_elevation[e.layer] {
                    Some(v) => v * self.scale,
                    None => continue,
                },
            };
            let Some((segs, closed)) = self.segments(e, tol) else { continue };
            if segs.is_empty() {
                continue;
            }
            let mut pts = flatten(&segs, tol);
            if closed && pts.len() > 1 {
                // A closed entity's chain already ends where it starts;
                // make the repeat explicit either way.
                let first = pts[0];
                let last = pts[pts.len() - 1];
                if (first.x - last.x).abs() > EPS || (first.y - last.y).abs() > EPS {
                    pts.push(first);
                }
            }
            out.push((elevation, pts));
        }
        Ok(out)
    }

    /// The bounding box of everything imported, exact for curves too, `None` when
    /// there is nothing.
    pub fn bbox(&self) -> Option<(P3, P3)> {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        let mut any = false;
        for e in &self.entities {
            for p in self.extent_points(e) {
                any = true;
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        any.then_some((lo, hi))
    }

    /// An estimate of what the drawing holds, for the memo's bound.
    pub fn bytes(&self) -> usize {
        let mut n = 256 + self.layers.len() * 128;
        for e in &self.entities {
            n += 96;
            n += match &e.geometry {
                Geometry::Polyline { vertices, .. } => vertices.len() * 32,
                Geometry::Spline { control, knots, weights, fit, .. } => (control.len() + fit.len()) * 24 + (knots.len() + weights.len()) * 8,
                Geometry::Text { text, .. } => text.len() + 32,
                _ => 48,
            };
        }
        for note in &self.notes {
            n += 48 + note.message.len();
        }
        n
    }
}

/// The last number in a name (`C-TOPO-152.5` → 152.5, `CONTOUR_150` →
/// 150), for layers named per contour.
pub fn trailing_number(name: &str) -> Option<f64> {
    let mut best: Option<f64> = None;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() || (b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric())) {
            let start = i;
            i += 1;
            let mut dot = false;
            while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'.' && !dot && i + 1 < b.len() && b[i + 1].is_ascii_digit())) {
                if b[i] == b'.' {
                    dot = true;
                }
                i += 1;
            }
            if let Ok(v) = name[start..i].parse::<f64>()
                && v.is_finite()
            {
                best = Some(v);
            }
        } else {
            i += 1;
        }
    }
    best
}

/// The elevation a label carries, if it is a spot elevation.
///
/// **The convention** (`pattern = None`): the label's whitespace-separated
/// tokens hold exactly one number, and that number has a fractional part
/// (`158.14`, not a lot number `12`); the other tokens, at most two, are
/// short alphabetic codes — the `BK`, `TC`, `FL`, `EG`, `TW`, `INV` a survey
/// writes beside a shot — so `158.14 BK`, `TC 157.99` and `158.14` all read
/// as 158.14 and `LOT 12`, `3 CARS` and `SEE NOTE 4.1 BELOW` do not. A
/// trailing foot mark (`158.14'`) is allowed.
///
/// **A pattern** is a template with `#` for a number (the first `#` is the
/// elevation), `*` for any run of characters, `?` for one, whitespace for a
/// run of whitespace, anything else itself, case-insensitively: `"EL #"`,
/// `"# BK"`, `"*=#"`.
pub fn elevation_of(text: &str, pattern: Option<&str>) -> Option<f64> {
    let text = text.trim();
    match pattern {
        Some(p) => {
            let p: Vec<char> = p.chars().collect();
            let s: Vec<char> = text.chars().collect();
            match_number(&p, &s)
        }
        None => {
            let mut number: Option<f64> = None;
            let mut codes = 0usize;
            for tok in text.split_whitespace() {
                let t = tok.strip_suffix('\'').unwrap_or(tok);
                let looks_numeric = t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.');
                if looks_numeric {
                    let v: f64 = t.parse().ok()?;
                    if !v.is_finite() || !t.contains('.') || number.is_some() {
                        return None;
                    }
                    number = Some(v);
                } else {
                    let ok = t.chars().count() <= 8 && t.chars().all(|c| c.is_alphabetic() || matches!(c, '.' | '-' | '_' | '/'));
                    if !ok {
                        return None;
                    }
                    codes += 1;
                    if codes > 2 {
                        return None;
                    }
                }
            }
            number
        }
    }
}

/// The `#`-template matcher behind [`elevation_of`]: the captured number
/// of the first `#` on the first successful match.
fn match_number(p: &[char], s: &[char]) -> Option<f64> {
    fn numeric_prefix(s: &[char]) -> usize {
        let mut i = 0;
        if i < s.len() && (s[i] == '-' || s[i] == '+') {
            i += 1;
        }
        let digits_start = i;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        if i < s.len() && s[i] == '.' && i + 1 < s.len() && s[i + 1].is_ascii_digit() {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == digits_start { 0 } else { i }
    }
    fn go(p: &[char], s: &[char], captured: Option<f64>) -> Option<Option<f64>> {
        match p.first() {
            None => s.is_empty().then_some(captured),
            Some('*') => (0..=s.len()).find_map(|i| go(&p[1..], &s[i..], captured)),
            Some('?') => (!s.is_empty()).then(|| go(&p[1..], &s[1..], captured)).flatten(),
            Some('#') => {
                let max = numeric_prefix(s);
                (1..=max).rev().find_map(|len| {
                    let t: String = s[..len].iter().collect();
                    let v: f64 = t.parse().ok()?;
                    go(&p[1..], &s[len..], captured.or(Some(v)))
                })
            }
            Some(c) if c.is_whitespace() => {
                if s.first().is_none_or(|x| !x.is_whitespace()) {
                    return None;
                }
                let mut pi = 0;
                while pi < p.len() && p[pi].is_whitespace() {
                    pi += 1;
                }
                let mut si = 0;
                while si < s.len() && s[si].is_whitespace() {
                    si += 1;
                }
                go(&p[pi..], &s[si..], captured)
            }
            Some(c) => {
                let same = s.first().is_some_and(|x| x.to_lowercase().eq(c.to_lowercase()));
                same.then(|| go(&p[1..], &s[1..], captured)).flatten()
            }
        }
    }
    go(p, s, None).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal ASCII DXF: header with `$INSUNITS`, then an ENTITIES section.
    fn file(insunits: i64, entities: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n{insunits}\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn with_blocks(insunits: i64, blocks: &str, entities: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{insunits}\n0\nENDSEC\n\
             0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn read(src: &str) -> Drawing {
        import_unbounded(src.as_bytes(), &Options::default()).unwrap()
    }

    fn kinds(d: &Drawing) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for e in &d.entities {
            *m.entry(e.kind.name()).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn every_entity_kind_is_read_and_the_rest_is_counted() {
        let e = "0\nLINE\n8\nA\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n\
                 0\nLWPOLYLINE\n8\nA\n70\n1\n90\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n0\n20\n10\n\
                 0\nPOLYLINE\n8\nA\n70\n0\n0\nVERTEX\n8\nA\n10\n0\n20\n0\n30\n0\n0\nVERTEX\n8\nA\n10\n5\n20\n5\n30\n0\n0\nSEQEND\n\
                 0\nARC\n8\nA\n10\n0\n20\n0\n30\n0\n40\n5\n50\n0\n51\n90\n\
                 0\nCIRCLE\n8\nB\n10\n0\n20\n0\n30\n0\n40\n3\n\
                 0\nELLIPSE\n8\nB\n10\n0\n20\n0\n30\n0\n11\n4\n21\n0\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n\
                 0\nSPLINE\n8\nB\n70\n8\n71\n3\n72\n8\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
                 10\n0\n20\n0\n30\n0\n10\n1\n20\n2\n30\n0\n10\n3\n20\n2\n30\n0\n10\n4\n20\n0\n30\n0\n\
                 0\nPOINT\n8\nB\n10\n7\n20\n8\n30\n9\n\
                 0\nTEXT\n8\nT\n10\n1\n20\n2\n30\n0\n40\n2.5\n1\n158.14 BK\n\
                 0\nMTEXT\n8\nT\n10\n3\n20\n4\n30\n0\n40\n2.5\n1\n\\A1;{\\fArial|b0;TC} 157.99\\P\n\
                 0\nSOLID\n8\nA\n10\n0\n20\n0\n11\n1\n21\n0\n12\n0\n22\n1\n13\n1\n23\n1\n\
                 0\nHATCH\n8\nH\n2\nSOLID\n70\n1\n71\n0\n91\n1\n92\n2\n72\n0\n73\n1\n93\n4\n10\n0\n20\n0\n10\n2\n20\n0\n10\n2\n20\n2\n10\n0\n20\n2\n97\n0\n\
                 0\nDIMENSION\n8\nD\n\
                 0\n3DSOLID\n8\nD\n\
                 0\nACAD_PROXY_ENTITY\n8\nD\n";
        let d = read(&file(4, e));
        let k = kinds(&d);
        for want in ["line", "lwpolyline", "polyline", "arc", "circle", "ellipse", "spline", "point", "text", "mtext", "solid", "hatch"] {
            assert_eq!(k.get(want), Some(&1), "{want}: {k:?}");
        }
        assert_eq!(d.skipped.get("DIMENSION"), Some(&1));
        assert_eq!(d.skipped.get("3DSOLID"), Some(&1));
        assert_eq!(d.skipped.get("ACAD_PROXY_ENTITY"), Some(&1));
        let note = d.notes.iter().find(|n| n.message.starts_with("not read")).unwrap();
        assert!(note.message.contains("ACIS") && note.message.contains("proxy") && note.message.contains("explode"), "{}", note.message);
        // Layer D holds only skipped entities and is not in the table, so it
        // is not a layer of the drawing.
        assert_eq!(d.layers.len(), 4, "{:?}", d.layers.iter().map(|l| &l.name).collect::<Vec<_>>());
        assert_eq!(d.version, "AC1015 (2000)");
        assert_eq!(d.unit, Unit::Mm);
        assert_eq!(d.unit_source, UnitSource::Header);
        // The texts, plain.
        let t = d.texts(&[]).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].text, "158.14 BK");
        assert_eq!(t[1].text, "TC 157.99\n");
        assert_eq!(t[0].at, [1.0, 2.0, 0.0]);
        assert_eq!(t[0].height, 2.5);
        // Every geometry entity is a curve (nine: the point and the two
        // labels are not), and the point is a point.
        assert_eq!(d.curves(&[], 0.01).unwrap().len(), 9);
        assert_eq!(d.points(&[]).unwrap(), vec![[7.0, 8.0, 9.0]]);
        // The closed ones are loops; the arc, the line and the open
        // polyline are chains that do not close.
        let s = d.sketch(&[], None, 0.01).unwrap();
        let loop_names: Vec<&str> = s.loops.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(loop_names, ["A/loop[0]", "A/loop[1]", "B/loop[0]", "B/loop[1]", "H/loop[0]"], "{loop_names:?}");
        assert_eq!(s.paths.len(), 3, "{:?}", s.paths.iter().map(|(n, _)| n).collect::<Vec<_>>());
        assert!(d.bbox().is_some());
    }

    #[test]
    fn a_bulge_is_an_exact_arc_and_a_polyline_keeps_its_elevation() {
        // A 20 × 20 square whose top-right corner is a quarter-circle fillet
        // of radius 5: the bulge for a 90° arc is tan(22.5°).
        let b = fmath::tan(22.5f64.to_radians());
        let e = format!(
            "0\nLWPOLYLINE\n8\nOUTLINE\n38\n12.5\n70\n1\n90\n5\n\
             10\n0\n20\n0\n10\n20\n20\n0\n10\n20\n20\n15\n42\n{b}\n10\n15\n20\n20\n10\n0\n20\n20\n"
        );
        let d = read(&file(4, &e));
        let s = d.sketch(&[], None, 0.01).unwrap();
        assert_eq!(s.loops.len(), 1);
        assert_eq!(s.loops[0].0, "OUTLINE/loop[0]");
        let arcs: Vec<&Segment> = s.loops[0].1.segments.iter().filter(|s| matches!(s, Segment::Arc { .. })).collect();
        assert_eq!(arcs.len(), 1);
        let Segment::Arc { a, b, via } = arcs[0] else { unreachable!() };
        let (c, r, _, _) = sketch::arc_params(*a, *b, *via).unwrap();
        assert!((r - 5.0).abs() < 1e-12, "fillet radius {r}");
        assert!((c.x - 15.0).abs() < 1e-12 && (c.y - 15.0).abs() < 1e-12, "centre {c:?}");
        assert_eq!(d.constant_z(&d.entities[0]), Some(12.5));
        let c = d.curves(&[], 0.01).unwrap();
        assert_eq!(c[0].name, "OUTLINE/lwpolyline[0]");
        assert_eq!(c[0].z, Some(12.5));
        assert!(c[0].closed);
    }

    #[test]
    fn an_insert_is_placed_with_rotation_scale_arrays_and_mirrors() {
        let blocks = "0\nBLOCK\n8\n0\n2\nSYM\n70\n0\n10\n0\n20\n0\n30\n0\n3\nSYM\n\
                      0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n\
                      0\nARC\n8\nDETAIL\n10\n0\n20\n0\n30\n0\n40\n5\n50\n0\n51\n90\n\
                      0\nCIRCLE\n8\n0\n10\n2\n20\n0\n30\n0\n40\n1\n\
                      0\nENDBLK\n8\n0\n";
        // Rotated 90° and scaled ×2 at (100, 50).
        let e = "0\nINSERT\n8\nSYMS\n2\nSYM\n10\n100\n20\n50\n30\n0\n41\n2\n42\n2\n43\n2\n50\n90\n";
        let d = read(&with_blocks(4, blocks, e));
        assert_eq!(d.blocks, 1);
        assert_eq!(d.inserts, 1);
        assert_eq!(d.entities.len(), 3);
        // Layer 0 inside the block takes the INSERT's layer; a named layer keeps its own.
        assert_eq!(d.layers[d.entities[0].layer].name, "SYMS");
        assert_eq!(d.layers[d.entities[1].layer].name, "DETAIL");
        assert_eq!(d.entities[0].block.as_deref(), Some("SYM"));
        let Geometry::Line { a, b } = &d.entities[0].geometry else { panic!() };
        assert!((a[0] - 100.0).abs() < 1e-12 && (a[1] - 50.0).abs() < 1e-12, "{a:?}");
        assert!((b[0] - 100.0).abs() < 1e-12 && (b[1] - 70.0).abs() < 1e-12, "{b:?}");
        let Geometry::Arc { center, radius, start, end } = &d.entities[1].geometry else { panic!("{:?}", d.entities[1].geometry) };
        assert!((center[0] - 100.0).abs() < 1e-12 && (center[1] - 50.0).abs() < 1e-12);
        assert!((radius - 10.0).abs() < 1e-12);
        assert!((start - PI / 2.0).abs() < 1e-12 && (end - PI).abs() < 1e-12, "{start} {end}");
        let Geometry::Circle { center, radius } = &d.entities[2].geometry else { panic!() };
        assert!((center[0] - 100.0).abs() < 1e-12 && (center[1] - 54.0).abs() < 1e-12, "{center:?}");
        assert!((radius - 2.0).abs() < 1e-12);

        // A 2 × 3 array.
        let e = "0\nINSERT\n8\nSYMS\n2\nSYM\n10\n0\n20\n0\n30\n0\n70\n2\n71\n3\n44\n50\n45\n40\n";
        let d = read(&with_blocks(4, blocks, e));
        assert_eq!(d.entities.len(), 18);
        let Geometry::Line { a, .. } = &d.entities[15].geometry else { panic!() };
        assert_eq!(*a, [50.0, 80.0, 0.0]);

        // Mirrored (x scale −1): the arc is still the same set of points.
        let e = "0\nINSERT\n8\nSYMS\n2\nSYM\n10\n0\n20\n0\n30\n0\n41\n-1\n42\n1\n43\n1\n";
        let d = read(&with_blocks(4, blocks, e));
        let Geometry::Arc { center, radius, start, end } = &d.entities[1].geometry else { panic!() };
        assert_eq!(*center, [0.0, 0.0, 0.0]);
        assert!((radius - 5.0).abs() < 1e-12);
        // The source arc runs 0..90°; mirrored in x it covers 90..180°.
        assert!((start - PI / 2.0).abs() < 1e-12 && (end - PI).abs() < 1e-12, "{start} {end}");

        // Non-uniform scale: the circle is an exact ellipse, the arc too.
        let e = "0\nINSERT\n8\nSYMS\n2\nSYM\n10\n0\n20\n0\n30\n0\n41\n2\n42\n1\n43\n1\n";
        let d = read(&with_blocks(4, blocks, e));
        assert_eq!(d.entities[1].kind, Kind::Arc);
        let Geometry::Ellipse { center, u, v, start, end } = &d.entities[2].geometry else { panic!("{:?}", d.entities[2].geometry) };
        assert_eq!(*center, [4.0, 0.0, 0.0]);
        assert_eq!(*u, [2.0, 0.0, 0.0]);
        assert_eq!(*v, [0.0, 1.0, 0.0]);
        assert_eq!((*start, *end), (0.0, TAU));
        let s = d.sketch(&["SYMS".into()], None, 0.001).unwrap();
        assert_eq!(s.loops.len(), 1, "the ellipse is a loop of Béziers");
        let (lo, hi) = {
            let pts = s.loops[0].1.sample_tolerance(0.0001);
            let lo = pts.iter().fold(P2::new(f64::MAX, f64::MAX), |l, p| P2::new(l.x.min(p.x), l.y.min(p.y)));
            let hi = pts.iter().fold(P2::new(f64::MIN, f64::MIN), |h, p| P2::new(h.x.max(p.x), h.y.max(p.y)));
            (lo, hi)
        };
        assert!((lo.x - 2.0).abs() < 1e-3 && (hi.x - 6.0).abs() < 1e-3 && (lo.y + 1.0).abs() < 1e-3 && (hi.y - 1.0).abs() < 1e-3, "{lo:?} {hi:?}");

        // A block inside itself is a cycle, said once; an unknown block is
        // said by name.
        let cyc = "0\nBLOCK\n8\n0\n2\nLOOP\n10\n0\n20\n0\n30\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nINSERT\n8\n0\n2\nLOOP\n10\n1\n20\n1\n30\n0\n0\nENDBLK\n";
        let d = read(&with_blocks(4, cyc, "0\nINSERT\n8\n0\n2\nLOOP\n10\n0\n20\n0\n30\n0\n0\nINSERT\n8\n0\n2\nNOPE\n10\n0\n20\n0\n30\n0\n"));
        assert_eq!(d.entities.len(), 1);
        assert!(d.notes.iter().any(|n| n.message.contains("cycle")), "{:?}", d.notes);
        assert!(d.notes.iter().any(|n| n.message.contains("\"NOPE\"")), "{:?}", d.notes);
    }

    #[test]
    fn mtext_formatting_codes_are_stripped() {
        assert_eq!(mtext_plain("\\A1;{\\fArial|b0|i0|c0|p34;\\H2.5x;TC} 157.99"), "TC 157.99");
        assert_eq!(mtext_plain("line one\\Pline two"), "line one\nline two");
        assert_eq!(mtext_plain("\\LUNDER\\l \\S1^2; \\U+00B0 100%%d \\~x"), "UNDER 1/2 ° 100° \u{a0}x".replace('\u{a0}', " "));
        assert_eq!(mtext_plain("\\C1;\\W0.8;a\\Q15;b\\T1.2;c\\pxqc;d"), "abcd");
        assert_eq!(mtext_plain("\\{braces\\} and \\\\ back"), "{braces} and \\ back");
        assert_eq!(text_plain("%%uTOTAL%%u 45%%d %%c12 %%p0.5 100%%%"), "TOTAL 45° Ø12 ±0.5 100%");
        assert_eq!(text_plain("esc\u{1b}[31m"), "esc[31m");
    }

    #[test]
    fn insunits_feet_scales_and_units_overrides_and_the_unknown_is_refused() {
        let e = "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n0\n40\n0.1\n1\n158.14\n";
        let d = read(&file(2, e));
        assert_eq!(d.unit, Unit::Ft);
        assert_eq!(d.scale, 304.8);
        let (lo, hi) = d.bbox().unwrap();
        assert!((hi[0] - lo[0] - 609.6).abs() < 1e-9, "{}", hi[0] - lo[0]);
        // The spot elevation is scaled like the coordinates.
        let z = d.spot_elevations(&[], None).unwrap();
        assert_eq!(z.len(), 1);
        assert!((z[0][2] - 158.14 * 304.8).abs() < 1e-9);
        // Unitless files warn and are read as mm.
        let d = read(&file(0, e));
        assert_eq!(d.unit, Unit::Mm);
        assert_eq!(d.unit_source, UnitSource::Assumed);
        assert!(d.notes.iter().any(|n| n.level == crate::Level::Warning && n.message.contains("$INSUNITS")), "{:?}", d.notes);
        // An explicit unit wins over the header, and says so.
        let d = import_unbounded(file(2, e).as_bytes(), &Options { units: Unit::In, ..Default::default() }).unwrap();
        assert_eq!(d.scale, 25.4);
        assert_eq!(d.unit_source, UnitSource::Option);
        assert!(d.notes.iter().any(|n| n.message.contains("overrides")), "{:?}", d.notes);
        // Survey feet are their own unit.
        let d = read(&file(21, e));
        assert_eq!(d.unit, Unit::SurveyFt);
        assert!((d.scale - 304.8006096).abs() < 1e-6);
        // Miles are not converted; the diagnostic says how to.
        let err = import_unbounded(file(3, e).as_bytes(), &Options::default()).unwrap_err();
        assert!(err.contains("miles") && err.contains("units="), "{err}");
        assert!(Unit::parse("furlong").unwrap_err().contains("valid: auto, mm"));
        for u in Unit::ALL {
            assert_eq!(Unit::parse(u.name()).unwrap(), u);
        }
    }

    /// A survey: lot lines on PL, contours on CONTOUR with their elevation
    /// as z, thirty spot shots as TEXT, a couple of trees as blocks.
    fn survey() -> String {
        let mut e = String::new();
        // Lot lines: four LINEs that close, in feet.
        e.push_str("0\nLINE\n8\nPL\n10\n0\n20\n0\n30\n0\n11\n100\n21\n0\n31\n0\n");
        e.push_str("0\nLINE\n8\nPL\n10\n100\n20\n0\n30\n0\n11\n100\n21\n60\n31\n0\n");
        e.push_str("0\nLINE\n8\nPL\n10\n100\n20\n60\n30\n0\n11\n0\n21\n60\n31\n0\n");
        e.push_str("0\nLINE\n8\nPL\n10\n0\n20\n60\n30\n0\n11\n0\n21\n0\n31\n0\n");
        // Contours every foot from 150 to 157: LWPOLYLINEs with an elevation.
        for k in 0..8 {
            let z = 150.0 + k as f64;
            e.push_str(&format!(
                "0\nLWPOLYLINE\n8\nCONTOUR\n38\n{z}\n70\n0\n90\n3\n10\n0\n20\n{y}\n10\n50\n20\n{y2}\n10\n100\n20\n{y}\n",
                y = 5.0 + 7.0 * k as f64,
                y2 = 8.0 + 7.0 * k as f64
            ));
        }
        // Thirty spot elevations: "158.14 BK", "TC 157.99", bare numbers.
        for i in 0..30 {
            let (x, y) = (3.0 + (i % 6) as f64 * 16.0, 4.0 + (i / 6) as f64 * 11.0);
            let z = 150.0 + i as f64 * 0.27;
            let label = match i % 3 {
                0 => format!("{z:.2} BK"),
                1 => format!("TC {z:.2}"),
                _ => format!("{z:.2}"),
            };
            e.push_str(&format!("0\nTEXT\n8\nSPOT\n10\n{x}\n20\n{y}\n30\n0\n40\n1.5\n1\n{label}\n"));
        }
        // Labels that are not elevations, on the same layer.
        e.push_str("0\nTEXT\n8\nSPOT\n10\n1\n20\n1\n30\n0\n40\n1.5\n1\nLOT 12\n");
        e.push_str("0\nTEXT\n8\nSPOT\n10\n2\n20\n2\n30\n0\n40\n1.5\n1\nSEE NOTE 4.1 BELOW\n");
        // Two trees.
        e.push_str("0\nINSERT\n8\nTREE\n2\nTREE\n10\n20\n20\n30\n30\n0\n0\nINSERT\n8\nTREE\n2\nTREE\n10\n70\n20\n40\n30\n0\n");
        let blocks = "0\nBLOCK\n8\n0\n2\nTREE\n10\n0\n20\n0\n30\n0\n0\nCIRCLE\n8\n0\n10\n0\n20\n0\n30\n0\n40\n3\n0\nENDBLK\n";
        with_blocks(2, blocks, &e)
    }

    #[test]
    fn a_survey_yields_its_spot_elevations_contours_lot_lines_and_trees() {
        let d = read(&survey());
        let spots = d.spot_elevations(&["SPOT".into()], None).unwrap();
        assert_eq!(spots.len(), 30, "the two other labels are not elevations");
        assert!((spots[0][2] - 150.0 * 304.8).abs() < 1e-6);
        assert!((spots[0][0] - 3.0 * 304.8).abs() < 1e-9);
        let contours = d.contours(&["CONTOUR".into()], 0.01).unwrap();
        assert_eq!(contours.len(), 8);
        assert!((contours[0].0 - 150.0 * 304.8).abs() < 1e-9);
        assert_eq!(contours[0].1.len(), 3);
        let lot = d.sketch(&["PL".into()], Some(true), 0.01).unwrap();
        assert_eq!(lot.loops.len(), 1, "four lines join into the lot");
        assert!((lot.loops[0].1.signed_area().abs() - 100.0 * 60.0 * 304.8 * 304.8).abs() < 1e-3);
        let trees = d.sketch(&["TREE".into()], None, 0.01).unwrap();
        assert_eq!(trees.loops.len(), 2);
        assert_eq!(trees.loops[1].0, "TREE/loop[1]");
        // Every contour vertex carries its z as a point.
        assert_eq!(d.points(&["CONTOUR".into()]).unwrap().len(), 24);
        // Layers, with counts and colours.
        let names: Vec<(&str, usize)> = d.layers.iter().map(|l| (l.name.as_str(), l.entities)).collect();
        assert_eq!(names, [("PL", 4), ("CONTOUR", 8), ("SPOT", 32), ("TREE", 2)]);
        assert_eq!(d.layers[0].color, [255, 255, 255]);
        // A selection at import narrows what is kept, and a miss lists the layers.
        let d = import_unbounded(survey().as_bytes(), &Options { layers: vec!["CONT*".into()], ..Default::default() }).unwrap();
        assert_eq!(d.entities.len(), 8);
        assert_eq!(d.unselected, 38);
        assert_eq!(d.layers.iter().filter(|l| l.selected).count(), 1);
        let err = import_unbounded(survey().as_bytes(), &Options { layers: vec!["ROADS".into()], ..Default::default() }).unwrap_err();
        assert!(err.contains("\"PL\"") && err.contains("\"CONTOUR\""), "{err}");
        let err = d.sketch(&["PL".into()], None, 0.01).unwrap_err();
        assert!(err.contains("no imported layer"), "{err}");
    }

    #[test]
    fn the_spot_elevation_convention_and_patterns() {
        assert_eq!(elevation_of("158.14 BK", None), Some(158.14));
        assert_eq!(elevation_of("TC 157.99", None), Some(157.99));
        assert_eq!(elevation_of("157.99", None), Some(157.99));
        assert_eq!(elevation_of("  -3.50'  ", None), Some(-3.5));
        assert_eq!(elevation_of("EG 101.5 TW", None), Some(101.5));
        assert_eq!(elevation_of("LOT 12", None), None, "no fractional part");
        assert_eq!(elevation_of("3 CARS", None), None);
        assert_eq!(elevation_of("SEE NOTE 4.1 BELOW", None), None, "too many words");
        assert_eq!(elevation_of("12.5 x 3.5", None), None, "two numbers");
        assert_eq!(elevation_of("", None), None);
        assert_eq!(elevation_of("EL 158", Some("EL #")), Some(158.0));
        assert_eq!(elevation_of("el 158.5", Some("EL #")), Some(158.5));
        assert_eq!(elevation_of("158.5 BK", Some("# BK")), Some(158.5));
        assert_eq!(elevation_of("158.5 TC", Some("# BK")), None);
        assert_eq!(elevation_of("Z=12.25", Some("*=#")), Some(12.25));
        assert_eq!(elevation_of("PT 7 EL 99.5", Some("PT ? EL #")), Some(99.5));
        assert_eq!(elevation_of("PT 7 EL 99.5", Some("PT # EL #")), Some(7.0), "the first # is the elevation");
        assert_eq!(elevation_of("12", Some("#x")), None);
        let d = read(&file(4, "0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n1\n5.5\n"));
        assert!(d.spot_elevations(&[], Some("EL")).unwrap_err().contains("`#`"));
    }

    #[test]
    fn contours_come_from_layer_names_too_and_arcs_are_flattened() {
        let b = fmath::tan(22.5f64.to_radians());
        let e = format!(
            "0\nLWPOLYLINE\n8\nC-TOPO-152.5\n70\n0\n90\n3\n10\n0\n20\n0\n10\n10\n20\n0\n42\n{b}\n10\n20\n20\n0\n\
             0\nLWPOLYLINE\n8\nCONTOUR_150\n70\n1\n90\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n0\n20\n10\n\
             0\nLWPOLYLINE\n8\nFLAT\n70\n0\n90\n2\n10\n0\n20\n0\n10\n10\n20\n0\n"
        );
        let d = read(&file(6, &e));
        let c = d.contours(&[], 0.01).unwrap();
        assert_eq!(c.len(), 2, "the FLAT one has no elevation anywhere");
        assert!((c[0].0 - 152.5 * 1000.0).abs() < 1e-9);
        assert!(c[0].1.len() > 5, "the bulge was flattened: {}", c[0].1.len());
        assert!((c[1].0 - 150_000.0).abs() < 1e-9);
        assert_eq!(c[1].1.first(), c[1].1.last(), "a closed contour repeats its first point");
        assert_eq!(trailing_number("C-TOPO-152.5"), Some(152.5));
        assert_eq!(trailing_number("CONTOUR_150"), Some(150.0));
        assert_eq!(trailing_number("A-WALL"), None);
        assert_eq!(trailing_number("MAJOR 5 CONTOUR 100"), Some(100.0));
    }

    #[test]
    fn open_chains_join_within_tolerance_and_closed_asks_for_only_loops() {
        // A square as four LINEs in a shuffled order, one reversed, one
        // corner 0.005 mm short; and a stray line.
        let e = "0\nLINE\n8\nA-WALL\n10\n0\n20\n0\n11\n10\n21\n0\n\
                 0\nLINE\n8\nA-WALL\n10\n10\n20\n10\n11\n0\n21\n10\n\
                 0\nLINE\n8\nA-WALL\n10\n10\n20\n0\n11\n10\n21\n9.995\n\
                 0\nLINE\n8\nA-WALL\n10\n0\n20\n10\n11\n0\n21\n0\n\
                 0\nLINE\n8\nA-WALL\n10\n50\n20\n50\n11\n60\n21\n50\n";
        let d = read(&file(4, e));
        let s = d.sketch(&[], None, 0.01).unwrap();
        assert_eq!(s.loops.len(), 1);
        assert_eq!(s.paths.len(), 1);
        assert_eq!(s.paths[0].0, "A-WALL/path[0]");
        assert!(s.loops[0].1.is_closed(1e-9));
        let s = d.sketch(&[], Some(true), 0.01).unwrap();
        assert_eq!((s.loops.len(), s.paths.len(), s.open_dropped), (1, 0, 1));
        let s = d.sketch(&[], Some(false), 0.01).unwrap();
        assert_eq!((s.loops.len(), s.paths.len(), s.loops_dropped), (0, 1, 1));
        // Too tight a tolerance, and the square is a chain.
        let s = d.sketch(&[], None, 0.001).unwrap();
        assert_eq!(s.loops.len(), 0);
    }

    #[test]
    fn colours_come_from_the_layer_the_entity_or_the_block() {
        let src = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
                   0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nRED\n62\n1\n70\n0\n0\nLAYER\n2\nOFF\n62\n-3\n70\n1\n0\nENDTAB\n0\nENDSEC\n\
                   0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nB\n10\n0\n20\n0\n30\n0\n0\nLINE\n8\n0\n62\n0\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n\
                   0\nSECTION\n2\nENTITIES\n\
                   0\nLINE\n8\nRED\n10\n0\n20\n0\n11\n1\n21\n0\n\
                   0\nLINE\n8\nRED\n62\n5\n10\n0\n20\n0\n11\n1\n21\n0\n\
                   0\nLINE\n8\nOFF\n420\n1122867\n10\n0\n20\n0\n11\n1\n21\n0\n\
                   0\nINSERT\n8\nRED\n62\n2\n2\nB\n10\n0\n20\n0\n30\n0\n\
                   0\nENDSEC\n0\nEOF\n";
        let d = read(src);
        assert_eq!(d.layers[0].color, [255, 0, 0]);
        assert!(d.layers[1].off && d.layers[1].frozen);
        assert_eq!(d.layers[1].color, [0, 255, 0]);
        assert_eq!(d.entities[0].color, [255, 0, 0], "by layer");
        assert_eq!(d.entities[1].color, [0, 0, 255], "its own index");
        assert_eq!(d.entities[2].color, [0x11, 0x22, 0x33], "true colour");
        assert_eq!(d.entities[3].color, [255, 255, 0], "by block: the INSERT's yellow");
        assert_eq!(aci_rgb(20), [255, 63, 0]);
        assert_eq!(aci_rgb(21), [255, 159, 127]);
        assert_eq!(aci_rgb(23), [204, 127, 102]);
        assert_eq!(aci_rgb(140), [0, 191, 255]);
        assert_eq!(aci_rgb(150), [0, 127, 255]);
        assert_eq!(aci_rgb(90), [0, 255, 0]);
        assert_eq!(aci_rgb(252), [132, 132, 132]);
    }

    #[test]
    fn ellipses_and_splines_are_curves_within_tolerance() {
        // A full ellipse 8 × 4 and a quarter of it; a cubic spline through
        // four control points.
        let e = "0\nELLIPSE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n4\n21\n0\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n\
                 0\nELLIPSE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n4\n21\n0\n31\n0\n40\n0.5\n41\n0\n42\n1.5707963267948966\n\
                 0\nSPLINE\n8\n0\n70\n8\n71\n3\n72\n8\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
                 10\n0\n20\n0\n30\n0\n10\n1\n20\n2\n30\n0\n10\n3\n20\n2\n30\n0\n10\n4\n20\n0\n30\n0\n";
        let d = read(&file(4, e));
        let c = d.curves(&[], 0.001).unwrap();
        assert_eq!(c.len(), 3);
        assert!(c[0].closed && !c[1].closed && !c[2].closed);
        // Every sample of the full ellipse satisfies its equation.
        let l = Loop { segments: c[0].segments.clone() };
        for p in l.sample_tolerance(0.0001) {
            let r = (p.x / 4.0) * (p.x / 4.0) + (p.y / 2.0) * (p.y / 2.0);
            assert!((r - 1.0).abs() < 2e-3, "{p:?} off the ellipse by {}", r - 1.0);
        }
        assert_eq!(c[1].segments.first().map(|s| s.start()), Some(P2::new(4.0, 0.0)));
        let end = c[1].segments.last().unwrap().end();
        assert!((end.x).abs() < 1e-9 && (end.y - 2.0).abs() < 1e-9, "{end:?}");
        // The spline is one exact cubic Bézier.
        assert_eq!(c[2].segments.len(), 1);
        assert!(matches!(c[2].segments[0], Segment::Bezier { .. }));
    }

    #[test]
    fn the_prescan_refuses_what_is_not_ascii_dxf_and_names_the_line() {
        let err = import(b"AutoCAD Binary DXF\r\n\x1a\x00", &Options::default()).unwrap_err();
        assert!(err.contains("binary DXF") && err.contains("ASCII"), "{err}");
        let err = import(b"AC1032xxxxxx", &Options::default()).unwrap_err();
        assert!(err.contains("DWG") && err.contains("proprietary"), "{err}");
        let err = import(b"0\nSECTION\n2\nENTITIES\nfoo\nLINE\n", &Options::default()).unwrap_err();
        assert!(err.contains("line 5") && err.contains("group code"), "{err}");
        // A code line with no value line after it (no trailing newline: a
        // `8\n` at the end is a code with an empty value, which is legal).
        let err = import(b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8", &Options::default()).unwrap_err();
        assert!(err.contains("without its value"), "{err}");
        let err = import(b"0\nLINE\n8\n0\n", &Options::default()).unwrap_err();
        assert!(err.contains("0 / SECTION"), "{err}");
        let err = import(b"8\n0\n0\nSECTION\n", &Options::default()).unwrap_err();
        assert!(err.contains("first group is 8"), "{err}");
        // An empty but well-formed file is a drawing with nothing in it.
        let d = import(b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n", &Options::default()).unwrap();
        assert!(d.entities.is_empty() && d.bbox().is_none());
        // CRLF, a BOM and comments are fine.
        let d = import("\u{feff}999\r\nmade by hand\r\n0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nPOINT\r\n8\r\n0\r\n10\r\n1\r\n20\r\n2\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n".as_bytes(), &Options::default()).unwrap();
        assert_eq!(d.points(&[]).unwrap(), vec![[1.0, 2.0, 0.0]]);
    }

    #[test]
    fn malformed_entities_are_counted_not_fatal() {
        let e = "0\nLINE\n8\n0\n10\nnan\n20\n0\n11\n1\n21\n0\n\
                 0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n-1\n\
                 0\nARC\n8\n0\n10\n0\n20\n0\n40\n1\n50\n0\n51\n90\n210\n0.5\n220\n0.5\n230\n0.7\n\
                 0\nLINE\n8\n0\n10\n1e308\n20\n0\n11\n1\n21\n0\n\
                 0\nPOINT\n8\n0\n10\n1\n20\n1\n";
        let d = import_unbounded(file(2, e).as_bytes(), &Options::default()).unwrap();
        assert_eq!(d.entities.len(), 1, "only the point survives");
        assert_eq!(d.malformed, 3, "nan, a negative radius, an overflow after scaling");
        assert_eq!(d.tilted, 1);
        assert!(d.notes.iter().any(|n| n.message.contains("outside the XY plane")), "{:?}", d.notes);
    }

    #[test]
    fn extents_and_the_ocs_flip() {
        let src = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n9\n$EXTMIN\n10\n-1\n20\n-2\n30\n0\n9\n$EXTMAX\n10\n3\n20\n4\n30\n0\n0\nENDSEC\n\
                   0\nSECTION\n2\nENTITIES\n0\nARC\n8\n0\n10\n1\n20\n0\n30\n0\n40\n1\n50\n0\n51\n90\n210\n0\n220\n0\n230\n-1\n0\nENDSEC\n0\nEOF\n";
        let d = read(src);
        let (lo, hi) = d.extents.unwrap();
        for (got, want) in lo.iter().chain(&hi).zip([-25.4, -50.8, 0.0, 76.2, 101.6, 0.0]) {
            assert!((got - want).abs() < 1e-9, "{lo:?} {hi:?}");
        }
        // An arc with extrusion −Z is mirrored in x: its centre at (−1, 0),
        // covering 90°..180°.
        let Geometry::Arc { center, start, end, .. } = &d.entities[0].geometry else { panic!() };
        assert!((center[0] + 25.4).abs() < 1e-9 && center[1].abs() < 1e-9, "{center:?}");
        assert!((start - PI / 2.0).abs() < 1e-12 && (end - PI).abs() < 1e-12);
    }

    fn assert_bbox(d: &Drawing, lo: [f64; 2], hi: [f64; 2]) {
        let (l, h) = d.bbox().unwrap();
        let ok = (l[0] - lo[0]).abs() < 1e-6 && (l[1] - lo[1]).abs() < 1e-6 && (h[0] - hi[0]).abs() < 1e-6 && (h[1] - hi[1]).abs() < 1e-6;
        assert!(ok, "bbox {l:?}..{h:?}, want {lo:?}..{hi:?}");
    }

    #[test]
    fn bbox_reaches_the_extremes_of_arcs_and_bulges() {
        // r = 100 from 10 to 170 degrees: the top is at y = 100, between
        // samples that used to step 36 degrees from the start.
        let d = read(&file(4, "0\nARC\n8\n0\n10\n0\n20\n0\n30\n0\n40\n100\n50\n10\n51\n170\n"));
        let (c, s) = (fmath::cos(10f64.to_radians()) * 100.0, fmath::sin(10f64.to_radians()) * 100.0);
        assert_bbox(&d, [-c, s], [c, 100.0]);
        // A bulged edge from (100,0) to (-100,0) bowing up: a half circle.
        let d = read(&file(4, "0\nLWPOLYLINE\n8\n0\n70\n0\n90\n2\n10\n100\n20\n0\n42\n1\n10\n-100\n20\n0\n"));
        assert_bbox(&d, [-100.0, 0.0], [100.0, 100.0]);
    }

    #[test]
    fn bbox_reaches_the_extremes_of_a_rotated_ellipse() {
        // Axis-aligned: semi-axes 100 x 50.
        let d = read(&file(4, "0\nELLIPSE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n100\n21\n0\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n"));
        assert_bbox(&d, [-100.0, -50.0], [100.0, 50.0]);
        // Rotated 30 degrees: the half-widths are sqrt(a² cos² + b² sin²)
        // and sqrt(a² sin² + b² cos²).
        let (sn, cs) = fmath::sin_cos(30f64.to_radians());
        let e = format!(
            "0\nELLIPSE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n{}\n21\n{}\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n",
            100.0 * cs,
            100.0 * sn
        );
        let d = read(&file(4, &e));
        let hx = (1e4 * cs * cs + 2500.0 * sn * sn).sqrt();
        let hy = (1e4 * sn * sn + 2500.0 * cs * cs).sqrt();
        assert_bbox(&d, [-hx, -hy], [hx, hy]);
    }

    #[test]
    fn bbox_of_a_spline_is_the_curve_not_its_control_polygon() {
        // Clamped cubic, one span: (0,0) (0,100) (100,100) (100,0). The
        // curve's top is at 3/4 of the control polygon's, y = 75.
        let d = read(&file(
            4,
            "0\nSPLINE\n8\n0\n70\n8\n71\n3\n72\n8\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
             10\n0\n20\n0\n30\n0\n10\n0\n20\n100\n30\n0\n10\n100\n20\n100\n30\n0\n10\n100\n20\n0\n30\n0\n",
        ));
        assert_bbox(&d, [0.0, 0.0], [100.0, 75.0]);
    }

    #[test]
    #[ignore = "wall-clock timing; flaky on a loaded host. Run with --ignored on a quiet machine"]
    fn sketch_of_many_layers_is_linear_in_entities_not_layers_times_entities() {
        // One LINE on each of n layers. sketch() used to scan every entity
        // once per layer, and reading scanned every earlier layer's name for
        // each new one: both quadratic. Timed at n and 4n in the same test,
        // so the machine's load cancels out: linear work grows about 4x,
        // quadratic about 16x.
        let run = |n: usize| {
            let mut e = String::with_capacity(n * 64);
            for i in 0..n {
                e.push_str(&format!("0\nLINE\n8\nL{i}\n10\n{i}\n20\n0\n30\n0\n11\n{i}\n21\n1\n31\n0\n"));
            }
            let text = file(4, &e);
            let t = std::time::Instant::now();
            let d = read(&text);
            let read_took = t.elapsed().as_secs_f64();
            assert_eq!(d.layers.len(), n);
            let t = std::time::Instant::now();
            let s = d.sketch(&[], None, 0.01).unwrap();
            let sketch_took = t.elapsed().as_secs_f64();
            assert_eq!(s.paths.len(), n);
            assert_eq!(s.paths[7].0, "L7/path[0]", "layer order and names are kept");
            (read_took, sketch_took)
        };
        let n = 10_000;
        let (r1, s1) = run(n);
        let (r4, s4) = run(4 * n);
        // A floor, so a run too quick to time does not divide by noise.
        let floor = 0.02;
        assert!(r4 < 8.0 * r1.max(floor), "reading: {r1:.3} s for {n} layers, {r4:.3} s for {}", 4 * n);
        assert!(s4 < 8.0 * s1.max(floor), "sketch(): {s1:.3} s for {n} layers, {s4:.3} s for {}", 4 * n);
    }
}
