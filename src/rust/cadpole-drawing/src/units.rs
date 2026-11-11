//! Lengths in and out of the units people actually write on drawings.
//!
//! The engine unit is the millimetre, everywhere and always. This module is
//! the presentation layer: it turns 3810 mm into `12'-6"` for a residential
//! plan and back again, and it is the only place in the drawing pipeline that
//! knows what a foot is. Everything downstream — dimension text, room labels,
//! title blocks, schedules — calls [`format_length`].
//!
//! It deliberately names nothing from the rest of `cadpole-drawing`, so it
//! can become its own crate by moving the file.

/// How lengths are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LengthUnit {
    /// `3810` — the engine unit, what a mechanical drawing uses.
    #[default]
    Mm,
    /// `381`
    Cm,
    /// `3.81`
    M,
    /// `150"` — decimal or fractional inches, no feet.
    In,
    /// `12.5'` — decimal feet.
    Ft,
    /// `12'-6"` — feet and inches, the architectural default.
    FtIn,
}

impl LengthUnit {
    /// Parse a unit name as `unit=` accepts it.
    pub fn parse(name: &str) -> Option<LengthUnit> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "mm" | "millimeter" | "millimetre" => LengthUnit::Mm,
            "cm" | "centimeter" | "centimetre" => LengthUnit::Cm,
            "m" | "meter" | "metre" => LengthUnit::M,
            "in" | "inch" | "inches" => LengthUnit::In,
            "ft" | "foot" | "feet" => LengthUnit::Ft,
            "ft-in" | "ft_in" | "ftin" | "feet-inches" | "architectural" => LengthUnit::FtIn,
            _ => return None,
        })
    }

    /// The name `unit=` uses, for diagnostics and round-tripping.
    pub fn name(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
            LengthUnit::In => "in",
            LengthUnit::Ft => "ft",
            LengthUnit::FtIn => "ft-in",
        }
    }

    /// Millimetres in one of these.
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::In => 25.4,
            LengthUnit::Ft | LengthUnit::FtIn => 304.8,
        }
    }

    /// True when the unit is imperial, which is what decides whether a
    /// dimension rounds to a fraction or to a decimal.
    pub fn imperial(self) -> bool {
        matches!(self, LengthUnit::In | LengthUnit::Ft | LengthUnit::FtIn)
    }
}

/// The unit names `unit=` accepts, for the "expected one of" diagnostic.
pub const UNIT_NAMES: &[&str] = &["mm", "cm", "m", "in", "ft", "ft-in"];

/// The same six units as Starlark attribute names: the members of the
/// `units` vocabulary. `in` is a Starlark keyword and `ft-in`
/// is not an identifier, so those two carry the spellings [`LengthUnit::parse`]
/// also takes: `inch` and `ft_in`. Listed in [`UNIT_NAMES`] order, and
/// `every_member_parses_to_its_unit_name` holds the two lists together.
pub const UNIT_MEMBERS: &[&str] = &["mm", "cm", "m", "inch", "ft", "ft_in"];

/// The fraction denominators a document may round imperial lengths to:
/// `units(units.ft_in, fraction=8)` writes eighths. 0 is decimal inches.
pub const FRACTIONS: &[u32] = &[0, 1, 2, 4, 8, 16, 32, 64];

/// A document's display convention: the unit its readouts are
/// written in and, for imperial units, the fraction of an inch dimensions
/// round to. The kernel stays in millimetres; this decides how a number is
/// *written* — by `cadpole inspect`, `distance()`, the drawing pipeline and the
/// web UI — and nothing else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    pub unit: LengthUnit,
    /// `1/fraction` of an inch; 0 means decimal inches. Ignored by the metric
    /// units.
    pub fraction: u32,
}

impl Units {
    /// Parse a unit name and a fraction as `units(...)` takes them, refusing
    /// with a message that lists what is allowed.
    pub fn parse(unit: &str, fraction: u32) -> Result<Units, String> {
        let u = LengthUnit::parse(unit)
            .ok_or_else(|| format!("units {unit:?} must be one of {}", UNIT_NAMES.join(", ")))?;
        if !FRACTIONS.contains(&fraction) {
            return Err(format!(
                "fraction {fraction} must be one of {} (1/{fraction} of an inch is not a drawing fraction)",
                FRACTIONS.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(Units { unit: u, fraction })
    }

    /// The default for a unit: sixteenths for the imperial units, which is
    /// what an architectural dimension string rounds to.
    pub fn of(unit: LengthUnit) -> Units {
        Units { unit, fraction: if unit.imperial() { 16 } else { 0 } }
    }

    /// The format every readout uses: the unit, and the fraction for the
    /// imperial ones.
    pub fn format(&self) -> UnitFormat {
        if self.unit.imperial() {
            UnitFormat { unit: self.unit, ..UnitFormat::ft_in() }.with_denominator(self.fraction)
        } else {
            UnitFormat { unit: self.unit, ..UnitFormat::mm() }
        }
    }

    /// A length, in this convention: `45'-6"`, `3810`, `3.81 m`.
    pub fn length(&self, mm: f64) -> String {
        format_length(mm, &self.format())
    }

    /// A clearance or a gap, in this convention: `0.35"` rather than `3/8"`,
    /// because a fit is a decimal quantity and a fraction would round it to
    /// the nearest sixteenth and hide the difference between 0.35 and 0.375.
    pub fn clearance(&self, mm: f64) -> String {
        format_clearance(mm, &self.format())
    }

    /// An area, in this convention: `1,240 sq ft` or `115.2 m²`.
    pub fn area(&self, mm2: f64) -> String {
        format_area(mm2, &self.format())
    }

    /// A point `(x, y, z)`, every coordinate in this convention.
    pub fn point(&self, p: [f64; 3]) -> String {
        format!("({}, {}, {})", self.length(p[0]), self.length(p[1]), self.length(p[2]))
    }
}

/// Format a clearance: a decimal in the unit's small unit, never a fraction.
/// Imperial conventions write inches to a hundredth (`0.35"`), the metric
/// ones the unit to three places, trailing zeros trimmed (`0.35 mm` reads
/// `0.35`, `3.81 m` reads `3.81`).
pub fn format_clearance(mm: f64, f: &UnitFormat) -> String {
    let sign = if mm < -1e-12 { "-" } else { "" };
    let v = mm.abs();
    match f.unit {
        LengthUnit::In | LengthUnit::Ft | LengthUnit::FtIn => {
            format!("{sign}{}\"", decimal(v / 25.4, 2))
        }
        LengthUnit::Mm => format!("{sign}{}{}", decimal(v, 3), suffix(f, "mm")),
        LengthUnit::Cm => format!("{sign}{}{}", decimal(v / 10.0, 3), suffix(f, "cm")),
        LengthUnit::M => format!("{sign}{}{}", decimal(v / 1000.0, 3), suffix(f, "m")),
    }
}

/// Everything a dimension needs to write a number down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitFormat {
    pub unit: LengthUnit,
    /// Round imperial lengths to `1/denominator` of an inch. 16 is the
    /// millwork default, 2 or 4 is framing practice; 1 means whole inches and
    /// 0 means "decimal inches, no fractions".
    pub denominator: u32,
    /// Decimal places for metric and for decimal-inch output.
    pub decimals: usize,
    /// Write the unit suffix (`mm`, `m`). Imperial always carries `'` and `"`,
    /// which are not optional.
    pub suffix: bool,
    /// `0'-6"` is written `6"`. Standard practice; off for tabulated output
    /// where the columns should line up.
    pub suppress_zero_feet: bool,
}

impl Default for UnitFormat {
    fn default() -> Self {
        UnitFormat {
            unit: LengthUnit::Mm,
            denominator: 16,
            decimals: 2,
            suffix: false,
            suppress_zero_feet: true,
        }
    }
}

impl UnitFormat {
    /// Millimetres, no suffix: the mechanical default.
    pub const fn mm() -> UnitFormat {
        UnitFormat {
            unit: LengthUnit::Mm,
            denominator: 0,
            decimals: 2,
            suffix: false,
            suppress_zero_feet: true,
        }
    }

    /// Feet and inches to 1/16", the architectural default.
    pub const fn ft_in() -> UnitFormat {
        UnitFormat {
            unit: LengthUnit::FtIn,
            denominator: 16,
            decimals: 2,
            suffix: false,
            suppress_zero_feet: true,
        }
    }

    /// The same format rounding to a coarser fraction (framing wants 1/2").
    pub fn with_denominator(mut self, d: u32) -> UnitFormat {
        self.denominator = d;
        self
    }
}

/// Format `mm` the way this drawing writes lengths.
///
/// ```
/// use cadpole_drawing::units::{UnitFormat, format_length};
/// assert_eq!(format_length(3810.0, &UnitFormat::ft_in()), "12'-6\"");
/// assert_eq!(format_length(3810.0, &UnitFormat::mm()), "3810");
/// ```
pub fn format_length(mm: f64, f: &UnitFormat) -> String {
    // Under a nanometre is zero: float noise (a mesh's f32 rounding at a
    // metre-scale coordinate) is not a length, and `parse_length` snaps to
    // the nanometre too.
    let (sign, v) = if mm.abs() < 1e-6 { ("", 0.0) } else { (if mm < 0.0 { "-" } else { "" }, mm.abs()) };
    match f.unit {
        // The most places each unit is printed to: its nanometre.
        LengthUnit::Mm => format!("{sign}{}{}", significant(v, f.decimals, 6), suffix(f, "mm")),
        LengthUnit::Cm => format!("{sign}{}{}", significant(v / 10.0, f.decimals, 7), suffix(f, "cm")),
        LengthUnit::M => format!(
            "{sign}{}{}",
            significant(v / 1000.0, f.decimals.max(2), 9),
            suffix(f, "m")
        ),
        LengthUnit::Ft => format!("{sign}{}'", significant(v / 304.8, f.decimals, 8)),
        LengthUnit::In => format!("{sign}{}\"", inches(v / 25.4, f)),
        LengthUnit::FtIn => {
            let total = round_inches(v / 25.4, f.denominator);
            let feet = (total / 12.0).floor();
            let rest = total - feet * 12.0;
            // Rounding can push 11 63/64" up to a full 12".
            let (feet, rest) = if (rest - 12.0).abs() < 1e-9 {
                (feet + 1.0, 0.0)
            } else {
                (feet, rest)
            };
            if feet == 0.0 && f.suppress_zero_feet {
                format!("{sign}{}\"", inches(rest, f))
            } else {
                format!("{sign}{}'-{}\"", feet as i64, inches(rest, f))
            }
        }
    }
}

/// The unit's own name, for a label that is not a dimension ("scale 1:48,
/// dimensions in millimetres").
pub fn unit_label(f: &UnitFormat) -> &'static str {
    match f.unit {
        LengthUnit::Mm => "millimetres",
        LengthUnit::Cm => "centimetres",
        LengthUnit::M => "metres",
        LengthUnit::In => "inches",
        LengthUnit::Ft => "feet",
        LengthUnit::FtIn => "feet and inches",
    }
}

/// Format an area: square millimetres in, `142 sq ft` or `13.2 m²` out.
pub fn format_area(mm2: f64, f: &UnitFormat) -> String {
    if f.unit.imperial() {
        format!("{} sq ft", grouped(decimal(mm2 / (304.8 * 304.8), 0)))
    } else {
        format!("{} m²", decimal(mm2 / 1.0e6, 2))
    }
}

/// `1240` → `1,240`: a zoning figure is read in thousands of square feet.
fn grouped(s: String) -> String {
    let (sign, digits) = match s.strip_prefix('-') {
        Some(d) => ("-", d),
        None => ("", s.as_str()),
    };
    let (whole, frac) = digits.split_once('.').map(|(w, f)| (w, Some(f))).unwrap_or((digits, None));
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

fn suffix(f: &UnitFormat, s: &'static str) -> String {
    if f.suffix {
        format!(" {s}")
    } else {
        String::new()
    }
}

/// Inches with a fraction, `6`, `6 1/2`, `0`.
fn inches(v: f64, f: &UnitFormat) -> String {
    if f.denominator == 0 {
        return decimal(v, f.decimals);
    }
    let d = f.denominator.max(1);
    let total = round_inches(v, d);
    let whole = total.floor();
    let num = ((total - whole) * d as f64).round() as u32;
    if num == 0 {
        return format!("{}", whole as i64);
    }
    let (mut n, mut den) = (num, d);
    while n % 2 == 0 && den % 2 == 0 {
        n /= 2;
        den /= 2;
    }
    if whole == 0.0 {
        format!("{n}/{den}")
    } else {
        format!("{} {n}/{den}", whole as i64)
    }
}

fn round_inches(v: f64, denominator: u32) -> f64 {
    if denominator == 0 {
        return v;
    }
    let d = denominator.max(1) as f64;
    (v * d).round() / d
}

/// Significant digits a non-zero decimal length always keeps.
const MIN_SIGNIFICANT: i32 = 3;

/// [`decimal`] with `places` places, or more when that many would leave a
/// non-zero length with fewer than [`MIN_SIGNIFICANT`] digits: a fixed
/// number of places is a resolution in the unit, and in metres or feet two
/// places is 10 mm or 3 mm, so a small part would be dimensioned `0`.
fn significant(v: f64, places: usize, most: usize) -> String {
    if !(v.is_finite() && v > 0.0) {
        return decimal(v, places);
    }
    let needed = MIN_SIGNIFICANT - 1 - v.log10().floor() as i32;
    decimal(v, places.max((needed.max(0) as usize).min(most)))
}

/// A decimal with trailing zeros (and a trailing point) trimmed, so 3810.00
/// prints as `3810` and 3.65 prints as `3.65`.
fn decimal(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    if !s.contains('.') {
        return s;
    }
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// Parse a length written the way a person writes it, returning millimetres.
///
/// Accepts `12'-6"`, `12' 6 1/2"`, `12ft 6in`, `3.65 m`, `90mm`, `150`
/// (bare numbers are millimetres, the engine unit).
///
/// The grammar is strict, because a length that is misread becomes geometry
/// without a word: one term (a number or fraction with an optional unit), or
/// a feet term followed by an inch term (`12'-6"`, `12' 6`, the inch mark
/// optional after feet). A whole number and a fraction (`6 1/2`, or
/// `6-1/2` as feet and inches are often written, `12'-6-1/2"`) need a unit.
/// Anything else (a comma, a second bare number, a repeated or out-of-order
/// unit, trailing text) is `None`.
///
/// ```
/// use cadpole_drawing::units::parse_length;
/// assert_eq!(parse_length("12'-6\""), Some(3810.0));
/// assert_eq!(parse_length("1,000"), None);
/// ```
pub fn parse_length(s: &str) -> Option<f64> {
    let t = s.trim();
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest.trim_start()),
        None => (1.0, t.strip_prefix('+').unwrap_or(t).trim_start()),
    };
    let first = take_term(t)?;
    let mm = match first.unit {
        Some(LengthUnit::Ft) => {
            // Feet, then optionally inches: `12'-6"`, `12' 6"`, `12ft 6in`,
            // `12' 6`. The separator is spaces and at most one hyphen.
            let feet = first.value * LengthUnit::Ft.mm();
            let rest = first.rest.trim_start();
            if rest.is_empty() {
                feet
            } else {
                let rest = rest.strip_prefix('-').unwrap_or(rest).trim_start();
                let inches = take_term(rest)?;
                if !inches.rest.trim().is_empty() {
                    return None;
                }
                match inches.unit {
                    Some(LengthUnit::In) | None => feet + inches.value * 25.4,
                    Some(_) => return None,
                }
            }
        }
        unit => {
            if !first.rest.trim().is_empty() {
                return None;
            }
            match unit {
                Some(u) => first.value * u.mm(),
                // `6 1/2` with no unit is neither millimetres nor inches.
                None if first.mixed => return None,
                None => first.value,
            }
        }
    };
    // 25.4 is not exact in binary, so 12' + 6" lands a few ulps off 3810. A
    // length is snapped to the nanometre on the way in: far below any
    // tolerance in the kernel, and it makes `12'-6\"` exactly 3810 mm, which
    // is what anyone comparing two parsed lengths expects.
    Some(snap(sign * mm))
}

/// One term of a written length: a number, a fraction (`1/2`) or a whole
/// number and a fraction (`6 1/2`), then an optional unit.
struct Term<'a> {
    value: f64,
    unit: Option<LengthUnit>,
    /// A whole number and a fraction, which only means something with a
    /// unit.
    mixed: bool,
    rest: &'a str,
}

fn take_term(s: &str) -> Option<Term<'_>> {
    let (mut value, mut rest) = take_number(s)?;
    let mut mixed = false;
    if let Some(f) = rest.trim_start().strip_prefix('/') {
        let (den, after) = take_number(f.trim_start())?;
        if den == 0.0 {
            return None;
        }
        value /= den;
        rest = after;
    } else if let Some(r) = rest
        .strip_prefix('-')
        .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        .or_else(|| rest.starts_with(' ').then(|| rest.trim_start()))
    {
        // `6 1/2` or `6-1/2`: a second number counts only as the fraction
        // of the first, so `1-2` and `6-` are not read this way.
        if let Some((num, after)) = take_number(r)
            && let Some(f) = after.trim_start().strip_prefix('/')
        {
            let (den, after) = take_number(f.trim_start())?;
            if den == 0.0 {
                return None;
            }
            value += num / den;
            rest = after;
            mixed = true;
        }
    }
    let (unit, after_unit) = take_unit(rest.trim_start());
    let rest = if unit.is_some() { after_unit } else { rest };
    Some(Term { value, unit, mixed, rest })
}

/// Round to the nanometre, killing the float noise of imperial conversion.
fn snap(mm: f64) -> f64 {
    if !mm.is_finite() {
        return mm;
    }
    let r = (mm * 1.0e9).round() / 1.0e9;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// Parse a roof pitch, `6:12` or `"30"` (degrees), as rise over run.
pub fn parse_pitch(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some((rise, run)) = t.split_once(':') {
        let rise: f64 = rise.trim().parse().ok()?;
        let run: f64 = run.trim().parse().ok()?;
        if run == 0.0 {
            None
        } else {
            Some(rise / run)
        }
    } else {
        let deg: f64 = t.parse().ok()?;
        Some(deg.to_radians().tan())
    }
}

fn take_number(s: &str) -> Option<(f64, &str)> {
    let mut end = 0;
    let bytes = s.as_bytes();
    let mut seen_digit = false;
    while end < bytes.len() {
        let c = bytes[end] as char;
        if c.is_ascii_digit() {
            seen_digit = true;
            end += 1;
        } else if c == '.' && !s[..end].contains('.') {
            end += 1;
        } else {
            break;
        }
    }
    if !seen_digit {
        return None;
    }
    s[..end].parse().ok().map(|v| (v, &s[end..]))
}

fn take_unit(s: &str) -> (Option<LengthUnit>, &str) {
    // Longest match first so `in` does not eat the `i` of nothing.
    const NAMES: &[(&str, LengthUnit)] = &[
        ("millimetres", LengthUnit::Mm),
        ("millimeters", LengthUnit::Mm),
        ("inches", LengthUnit::In),
        ("metres", LengthUnit::M),
        ("meters", LengthUnit::M),
        ("feet", LengthUnit::Ft),
        ("foot", LengthUnit::Ft),
        ("inch", LengthUnit::In),
        ("mm", LengthUnit::Mm),
        ("cm", LengthUnit::Cm),
        ("ft", LengthUnit::Ft),
        ("in", LengthUnit::In),
        ("m", LengthUnit::M),
        ("'", LengthUnit::Ft),
        ("\u{2032}", LengthUnit::Ft),
        ("\"", LengthUnit::In),
        ("\u{2033}", LengthUnit::In),
    ];
    let lower = s.to_ascii_lowercase();
    for (name, unit) in NAMES {
        if lower.starts_with(name) {
            return (Some(*unit), &s[name.len()..]);
        }
    }
    (None, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_twelve_foot_six_wall_reads_both_ways() {
        // 12'-6" is 150 inches is 3810 mm, exactly.
        assert_eq!(format_length(3810.0, &UnitFormat::ft_in()), "12'-6\"");
        assert_eq!(format_length(3810.0, &UnitFormat::mm()), "3810");
        assert_eq!(parse_length("12'-6\""), Some(3810.0));
        assert_eq!(parse_length("12' 6\""), Some(3810.0));
        assert_eq!(parse_length("12ft 6in"), Some(3810.0));
    }

    #[test]
    fn fractions_round_to_the_documents_precision() {
        let sixteenth = UnitFormat::ft_in();
        // 6 1/2" over 12 feet.
        assert_eq!(format_length(3810.0 + 12.7, &sixteenth), "12'-6 1/2\"");
        assert_eq!(
            format_length(304.8 * 3.0 + 25.4 * 2.5, &sixteenth),
            "3'-2 1/2\""
        );
        // 1/16" rounding, then the same length at coarser precisions: 3/16
        // is 1/4 to the nearest quarter and vanishes at the nearest half.
        let odd = 25.4 * (7.0 + 3.0 / 16.0);
        assert_eq!(format_length(odd, &sixteenth), "7 3/16\"");
        assert_eq!(
            format_length(odd, &sixteenth.with_denominator(4)),
            "7 1/4\""
        );
        assert_eq!(format_length(odd, &sixteenth.with_denominator(2)), "7\"");
        assert_eq!(format_length(odd, &sixteenth.with_denominator(1)), "7\"");
    }

    /// Float noise is not a length: under a nanometre prints as 0, and no
    /// unit prints past its nanometre.
    #[test]
    fn noise_under_a_nanometre_prints_as_zero() {
        assert_eq!(format_length(1e-10, &UnitFormat::mm()), "0");
        assert_eq!(format_length(1e-7, &UnitFormat::mm()), "0");
        assert_eq!(format_length(-1e-10, &UnitFormat::mm()), "0");
        let m = UnitFormat { unit: LengthUnit::M, ..UnitFormat::mm() };
        assert_eq!(format_length(1e-10, &m), "0");
        assert_eq!(format_length(2.4e-4, &UnitFormat::mm()), "0.00024");
    }

    #[test]
    fn zero_feet_is_suppressed_and_negatives_keep_their_sign() {
        assert_eq!(format_length(152.4, &UnitFormat::ft_in()), "6\"");
        let keep = UnitFormat {
            suppress_zero_feet: false,
            ..UnitFormat::ft_in()
        };
        assert_eq!(format_length(152.4, &keep), "0'-6\"");
        assert_eq!(format_length(-3810.0, &UnitFormat::ft_in()), "-12'-6\"");
    }

    #[test]
    fn rounding_up_carries_into_the_next_foot() {
        // 11 63/64" rounds to 12", which must read 1'-0" and not 0'-12".
        let mm = 25.4 * (11.0 + 63.0 / 64.0);
        assert_eq!(format_length(mm, &UnitFormat::ft_in()), "1'-0\"");
    }

    #[test]
    fn metric_forms() {
        assert_eq!(format_length(3650.0, &UnitFormat::mm()), "3650");
        let m = UnitFormat {
            unit: LengthUnit::M,
            suffix: true,
            ..UnitFormat::mm()
        };
        assert_eq!(format_length(3650.0, &m), "3.65 m");
        let cm = UnitFormat {
            unit: LengthUnit::Cm,
            ..UnitFormat::mm()
        };
        assert_eq!(format_length(3650.0, &cm), "365");
    }

    #[test]
    fn parsing_the_forms_people_type() {
        assert_eq!(parse_length("3.65 m"), Some(3650.0));
        assert_eq!(parse_length("90mm"), Some(90.0));
        assert_eq!(parse_length("150"), Some(150.0));
        assert_eq!(parse_length("6\""), Some(152.4));
        let half = parse_length("12' 6 1/2\"").unwrap();
        assert!((half - (3810.0 + 12.7)).abs() < 1e-9, "{half}");
        assert_eq!(parse_length("-12'-6\""), Some(-3810.0));
        assert_eq!(parse_length("nonsense"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn a_length_that_is_not_one_of_the_forms_is_refused() {
        // Each of these used to come back as some other length: a comma or a
        // second number was summed as inches, a unit could repeat or come
        // out of order.
        for bad in [
            "1,000", "1,5", "3,000 m", "1 2", "1 2 3", "1m5", "1'2'", "12'-6\"-3", "6\" 2'",
            "1.5.2", "1 1/2", "5 mm mm", "1m 5", "6in 2ft", "12' 6\" 1", "--5", "1/0\"", "'", "\u{2032}", "12'-", "6-", "1-2",
        ] {
            assert_eq!(parse_length(bad), None, "{bad:?}");
        }
        // The documented forms, and the edges of them, still read.
        let ok: &[(&str, f64)] = &[
            ("1000", 1000.0),
            ("12'-6\"", 3810.0),
            ("12' - 6\"", 3810.0),
            ("12' 6", 3810.0),
            ("12'6\"", 3810.0),
            ("12 ft 6 in", 3810.0),
            ("12ft", 3657.6),
            ("1/2\"", 12.7),
            ("6 1/2\"", 165.1),
            ("6-1/2\"", 165.1),
            ("12'-6-1/2\"", 3822.7),
            ("1'-0\"", 304.8),
            ("150 inches", 3810.0),
            ("2.5cm", 25.0),
            (".5 m", 500.0),
            ("+3", 3.0),
            ("- 12'-6\"", -3810.0),
        ];
        for (good, mm) in ok {
            let v = parse_length(good).unwrap_or_else(|| panic!("{good:?}"));
            assert!((v - mm).abs() < 1e-9, "{good:?}: {v}");
        }
        let v = parse_length("12' 6 1/2\"").unwrap();
        assert!((v - 3822.7).abs() < 1e-9, "{v}");
    }

    #[test]
    fn a_small_length_never_reads_zero_in_a_large_unit() {
        // Metres were fixed at two places, so a 4 mm side read "0" and a
        // 25.37 mm one "0.03"; feet could not tell 3 mm from 4 mm.
        let m = UnitFormat { unit: LengthUnit::M, ..UnitFormat::mm() };
        assert_eq!(format_length(4.0, &m), "0.004");
        assert_eq!(format_length(0.4, &m), "0.0004");
        assert_eq!(format_length(25.37, &m), "0.0254");
        let ft = UnitFormat { unit: LengthUnit::Ft, ..UnitFormat::mm() };
        assert_ne!(format_length(3.0, &ft), format_length(4.0, &ft));
        assert_eq!(format_length(4.0, &ft), "0.0131'");
        // Lengths that already had the digits keep the format they had.
        assert_eq!(format_length(13868.4, &m), "13.87");
        assert_eq!(format_length(3650.0, &m), "3.65");
        assert_eq!(format_length(2438.4, &UnitFormat::mm()), "2438.4");
        assert_eq!(format_length(0.0, &m), "0");
    }

    #[test]
    fn pitch_reads_rise_over_run() {
        assert_eq!(parse_pitch("6:12"), Some(0.5));
        assert!((parse_pitch("45").unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unit_names_round_trip() {
        for n in UNIT_NAMES {
            let u = LengthUnit::parse(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(LengthUnit::parse(u.name()), Some(u));
        }
        assert_eq!(LengthUnit::parse("furlong"), None);
    }

    #[test]
    fn areas_follow_the_flavor() {
        assert_eq!(
            format_area(304.8 * 304.8 * 142.0, &UnitFormat::ft_in()),
            "142 sq ft"
        );
        assert_eq!(format_area(13.2e6, &UnitFormat::mm()), "13.2 m²");
        // A zoning figure reads in thousands.
        assert_eq!(
            format_area(304.8 * 304.8 * 2480.0, &UnitFormat::ft_in()),
            "2,480 sq ft"
        );
        assert_eq!(
            format_area(304.8 * 304.8 * 1234567.0, &UnitFormat::ft_in()),
            "1,234,567 sq ft"
        );
    }

    /// The document convention, one row per unit: the same length,
    /// the same clearance and the same area written in each.
    #[test]
    fn the_units_table() {
        // 45'-6" is 13868.4 mm; a 0.35" gap is 8.89 mm; 1240 sq ft.
        let len = 45.0 * 304.8 + 6.0 * 25.4;
        let gap = 0.35 * 25.4;
        let area = 1240.0 * 304.8 * 304.8;
        let rows: &[(&str, u32, &str, &str, &str)] = &[
            ("mm", 0, "13868.4", "8.89", "115.2 m²"),
            ("cm", 0, "1386.84", "0.889", "115.2 m²"),
            ("m", 0, "13.87", "0.009", "115.2 m²"),
            ("in", 16, "546\"", "0.35\"", "1,240 sq ft"),
            ("ft", 16, "45.5'", "0.35\"", "1,240 sq ft"),
            ("ft-in", 16, "45'-6\"", "0.35\"", "1,240 sq ft"),
            ("ft_in", 16, "45'-6\"", "0.35\"", "1,240 sq ft"),
            ("inch", 16, "546\"", "0.35\"", "1,240 sq ft"),
        ];
        for (unit, fraction, l, g, a) in rows {
            let u = Units::parse(unit, *fraction).unwrap_or_else(|e| panic!("{unit}: {e}"));
            assert_eq!(u.length(len), *l, "{unit}: length");
            assert_eq!(u.clearance(gap), *g, "{unit}: clearance");
            assert_eq!(u.area(area), *a, "{unit}: area");
        }
        // The point form, and a negative elevation keeps its sign.
        let u = Units::parse("ft-in", 16).unwrap();
        assert_eq!(u.point([0.0, 3810.0, -3810.0]), "(0\", 12'-6\", -12'-6\")");
    }

    /// The fraction is the document's: 1/16" by default, 1/8" when asked,
    /// and a fraction that is not a drawing fraction is refused with the
    /// list.
    #[test]
    fn the_fraction_is_a_parameter() {
        let x = 25.4 * (7.0 + 3.0 / 16.0);
        assert_eq!(Units::parse("ft-in", 16).unwrap().length(x), "7 3/16\"");
        assert_eq!(Units::parse("ft-in", 8).unwrap().length(x), "7 1/4\"");
        assert_eq!(Units::parse("ft-in", 0).unwrap().length(x), "7.19\"");
        assert_eq!(Units::of(LengthUnit::FtIn).fraction, 16);
        assert_eq!(Units::of(LengthUnit::Mm).fraction, 0);
        // The metric units ignore it.
        assert_eq!(Units::parse("mm", 8).unwrap().length(x), "182.56");
        let e = Units::parse("ft-in", 3).unwrap_err();
        assert!(e.contains("fraction 3 must be one of 0, 1, 2, 4, 8, 16, 32, 64"), "{e}");
        let e = Units::parse("furlong", 16).unwrap_err();
        assert!(e.contains("ft-in"), "{e}");
    }

    /// Every attribute spelling of the vocabulary is a unit the parser
    /// takes, and names the unit at the same position of `UNIT_NAMES`.
    #[test]
    fn every_member_parses_to_its_unit_name() {
        assert_eq!(UNIT_MEMBERS.len(), UNIT_NAMES.len());
        for (member, name) in UNIT_MEMBERS.iter().zip(UNIT_NAMES) {
            let u = LengthUnit::parse(member).unwrap_or_else(|| panic!("{member}"));
            assert_eq!(u.name(), *name, "{member}");
        }
    }
}
