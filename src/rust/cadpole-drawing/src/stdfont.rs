//! Advance widths of the standard PDF Type 1 faces every reader carries, so
//! a sheet can measure a string without embedding a font.
//!
//! One entry per WinAnsiEncoding code 32..=255, in thousandths of the em, from
//! Adobe's Core 14 AFM metrics (Helvetica 003.001, Helvetica-Bold 003.001;
//! redistributable per the AFM licence), read out of groff's `devps/HR` and
//! `devps/HB` font descriptions, which carry the same numbers. The oblique
//! faces share their upright's widths, and every Courier glyph is 600.
//! Cap heights are the AFM `CapHeight` values: Helvetica 718, Courier 562.

/// Helvetica, WinAnsi codes 32..=255.
pub const HELVETICA: [u16; 224] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, 350,
    556, 350, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 578, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,
];

/// Helvetica-Bold, WinAnsi codes 32..=255.
pub const HELVETICA_BOLD: [u16; 224] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584, 350,
    556, 350, 278, 556, 500, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 278, 278, 500, 500, 350, 556, 1000, 333, 1000, 556, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 611, 556, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    722, 722, 722, 722, 722, 722, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556, 278, 278, 278, 278,
    611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611, 556,
];

/// One of the standard 14 faces a PDF reader is required to have. Text set
/// in one is written as *text* — selectable, searchable — with no font
/// program embedded; an SVG names the family and a DXF the style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StdFont {
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

impl StdFont {
    /// Every face, in the order the docs list them.
    pub const ALL: [StdFont; 8] = [
        StdFont::Helvetica,
        StdFont::HelveticaBold,
        StdFont::HelveticaOblique,
        StdFont::HelveticaBoldOblique,
        StdFont::Courier,
        StdFont::CourierBold,
        StdFont::CourierOblique,
        StdFont::CourierBoldOblique,
    ];

    /// The family names `font=` accepts.
    pub const FAMILIES: &'static [&'static str] = &["helvetica", "courier"];

    /// A face by family (`"helvetica"`, `"courier"`; `"sans"` and `"mono"`
    /// are aliases), weight and slant.
    pub fn parse(family: &str, bold: bool, italic: bool) -> Option<StdFont> {
        let mono = match family.trim().to_ascii_lowercase().as_str() {
            "helvetica" | "sans" | "arial" => false,
            "courier" | "mono" | "monospace" => true,
            _ => return None,
        };
        Some(match (mono, bold, italic) {
            (false, false, false) => StdFont::Helvetica,
            (false, true, false) => StdFont::HelveticaBold,
            (false, false, true) => StdFont::HelveticaOblique,
            (false, true, true) => StdFont::HelveticaBoldOblique,
            (true, false, false) => StdFont::Courier,
            (true, true, false) => StdFont::CourierBold,
            (true, false, true) => StdFont::CourierOblique,
            (true, true, true) => StdFont::CourierBoldOblique,
        })
    }

    /// The same face in the other weight.
    pub fn with_bold(self, bold: bool) -> StdFont {
        StdFont::parse(self.family(), bold, self.italic()).unwrap_or(self)
    }

    pub fn family(self) -> &'static str {
        if self.mono() {
            "courier"
        } else {
            "helvetica"
        }
    }

    pub fn mono(self) -> bool {
        matches!(
            self,
            StdFont::Courier | StdFont::CourierBold | StdFont::CourierOblique | StdFont::CourierBoldOblique
        )
    }

    pub fn bold(self) -> bool {
        matches!(
            self,
            StdFont::HelveticaBold
                | StdFont::HelveticaBoldOblique
                | StdFont::CourierBold
                | StdFont::CourierBoldOblique
        )
    }

    pub fn italic(self) -> bool {
        matches!(
            self,
            StdFont::HelveticaOblique
                | StdFont::HelveticaBoldOblique
                | StdFont::CourierOblique
                | StdFont::CourierBoldOblique
        )
    }

    /// The PDF `BaseFont` name.
    pub fn base_font(self) -> &'static str {
        match self {
            StdFont::Helvetica => "Helvetica",
            StdFont::HelveticaBold => "Helvetica-Bold",
            StdFont::HelveticaOblique => "Helvetica-Oblique",
            StdFont::HelveticaBoldOblique => "Helvetica-BoldOblique",
            StdFont::Courier => "Courier",
            StdFont::CourierBold => "Courier-Bold",
            StdFont::CourierOblique => "Courier-Oblique",
            StdFont::CourierBoldOblique => "Courier-BoldOblique",
        }
    }

    /// What an SVG `font-family` says, with the usual fallbacks.
    pub fn svg_family(self) -> &'static str {
        if self.mono() {
            "Courier, 'Courier New', monospace"
        } else {
            "Helvetica, Arial, sans-serif"
        }
    }

    /// Cap height as a fraction of the em (the AFM `CapHeight`).
    pub fn cap_ratio(self) -> f64 {
        if self.mono() {
            0.562
        } else {
            0.718
        }
    }

    /// Advance width of one character in thousandths of the em, or `None`
    /// for a character WinAnsiEncoding cannot write.
    pub fn width_units(self, c: char) -> Option<u16> {
        let code = winansi(c)?;
        if self.mono() {
            return Some(600);
        }
        let i = (code - 32) as usize;
        Some(if self.bold() {
            HELVETICA_BOLD[i]
        } else {
            HELVETICA[i]
        })
    }

    /// Advance width of `text` in units of the em (so `× em size` is
    /// millimetres). A character the encoding lacks is measured as a bullet,
    /// which is what the PDF would show for it.
    pub fn advance_em(self, text: &str) -> f64 {
        text.chars()
            .map(|c| self.width_units(c).unwrap_or(HELVETICA[(149 - 32) as usize]) as f64 / 1000.0)
            .sum()
    }
}

/// The WinAnsiEncoding byte for `c`, or `None` when the encoding has no
/// slot for it. Latin-1 is the identity; the 0x80–0x9F row carries the
/// typographic quotes, dashes, the bullet and the Euro.
pub fn winansi(c: char) -> Option<u8> {
    let u = c as u32;
    if (32..127).contains(&u) || (160..256).contains(&u) {
        return Some(u as u8);
    }
    Some(match c {
        '\u{20AC}' => 128,
        '\u{201A}' => 130,
        '\u{0192}' => 131,
        '\u{201E}' => 132,
        '\u{2026}' => 133,
        '\u{2020}' => 134,
        '\u{2021}' => 135,
        '\u{02C6}' => 136,
        '\u{2030}' => 137,
        '\u{0160}' => 138,
        '\u{2039}' => 139,
        '\u{0152}' => 140,
        '\u{017D}' => 142,
        '\u{2018}' => 145,
        '\u{2019}' => 146,
        '\u{201C}' => 147,
        '\u{201D}' => 148,
        '\u{2022}' => 149,
        '\u{2013}' => 150,
        '\u{2014}' => 151,
        '\u{02DC}' => 152,
        '\u{2122}' => 153,
        '\u{0161}' => 154,
        '\u{203A}' => 155,
        '\u{0153}' => 156,
        '\u{017E}' => 158,
        '\u{0178}' => 159,
        // The primes a dimension might carry read as the ASCII quotes.
        '\u{2032}' => b'\'',
        '\u{2033}' => b'"',
        _ => return None,
    })
}

/// `text` as WinAnsi bytes, or `None` when any character has no slot — the
/// caller then falls back to outlines rather than writing the wrong glyph.
pub fn encode_winansi(text: &str) -> Option<Vec<u8>> {
    text.chars().map(winansi).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_hold_adobes_numbers() {
        // Helvetica: space 278, A 667, a 556, degree 400, Oslash 778.
        assert_eq!(StdFont::Helvetica.width_units(' '), Some(278));
        assert_eq!(StdFont::Helvetica.width_units('A'), Some(667));
        assert_eq!(StdFont::Helvetica.width_units('a'), Some(556));
        assert_eq!(StdFont::Helvetica.width_units('\u{b0}'), Some(400));
        assert_eq!(StdFont::Helvetica.width_units('\u{d8}'), Some(778));
        assert_eq!(StdFont::HelveticaBold.width_units('A'), Some(722));
        assert_eq!(StdFont::Courier.width_units('W'), Some(600));
        // Oblique shares its upright's widths.
        assert_eq!(
            StdFont::HelveticaOblique.advance_em("12'-6\""),
            StdFont::Helvetica.advance_em("12'-6\"")
        );
        // "Hello" in Helvetica: 722 + 556 + 222 + 222 + 556 = 2278.
        assert!((StdFont::Helvetica.advance_em("Hello") - 2.278).abs() < 1e-9);
    }

    #[test]
    fn winansi_covers_latin_1_and_the_typographic_row() {
        assert_eq!(winansi('A'), Some(65));
        assert_eq!(winansi('\u{e9}'), Some(233));
        assert_eq!(winansi('\u{2019}'), Some(146));
        assert_eq!(winansi('\u{2013}'), Some(150));
        assert_eq!(winansi('\u{20AC}'), Some(128));
        assert_eq!(winansi('\u{4e2d}'), None);
        assert_eq!(encode_winansi("12'-6\""), Some(b"12'-6\"".to_vec()));
        assert_eq!(encode_winansi("\u{4e2d}"), None);
    }

    #[test]
    fn faces_parse_by_family_weight_and_slant() {
        assert_eq!(StdFont::parse("helvetica", false, false), Some(StdFont::Helvetica));
        assert_eq!(StdFont::parse("Helvetica", true, false), Some(StdFont::HelveticaBold));
        assert_eq!(StdFont::parse("courier", true, true), Some(StdFont::CourierBoldOblique));
        assert_eq!(StdFont::parse("mono", false, false), Some(StdFont::Courier));
        assert_eq!(StdFont::parse("times", false, false), None);
        assert_eq!(StdFont::Helvetica.with_bold(true).base_font(), "Helvetica-Bold");
        for f in StdFont::ALL {
            assert_eq!(StdFont::parse(f.family(), f.bold(), f.italic()), Some(f));
        }
    }
}
