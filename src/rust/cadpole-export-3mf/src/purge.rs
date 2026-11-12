//! Purge (flush) volumes: how much filament a multi-colour printer pushes
//! through the nozzle when it changes from one filament to the next, so the
//! old colour is gone before the new one reaches the part.
//!
//! A Bambu project carries them as `flush_volumes_matrix`: for each nozzle,
//! an `n x n` table in mm³, row = the filament being unloaded, column = the
//! filament being loaded, flattened row by row and stored as strings, with
//! `"0"` on the diagonal (no change, no purge). `flush_multiplier` (one
//! string per nozzle) scales the table; Cadpole writes `"1"`. A project with
//! no table purges nothing between colours, and the slicer warns that the
//! print may mix colours.
//!
//! # Where the numbers come from
//!
//! Cadpole's own rule of thumb, from the two colours alone (no filament
//! brand or material data is used):
//!
//! ```text
//! volume = 100 + 250 · d + 450 · max(0, L*to − L*from) / 100,   clamped to 100..800 mm³
//! ```
//!
//! * `d` is the distance between the two colours in sRGB, 0 for the same
//!   colour and 1 for black and white: any change of colour needs some
//!   purge, more the further apart the colours are.
//! * `L*` is the CIE lightness (0 black, 100 white) of each colour. A light
//!   filament following a dark one shows every trace of the dark one, so
//!   going lighter adds up to 450 mm³; going darker adds nothing, because a
//!   dark colour hides a trace of a light one.
//! * 100 mm³ is the least for any change (about 40 mm of 1.75 mm
//!   filament): enough to clear the melt zone of a nozzle. 800 mm³ is the
//!   most: black to white, the worst case.
//!
//! So black to white is 800, white to black 350, two near-identical colours
//! 100. Values are whole mm³. A user who has measured their own filaments
//! replaces the table in the slicer, or passes a `template=` project, whose
//! own table is kept.

/// The least purge for any change of filament, mm³.
pub const PURGE_MIN: f64 = 100.0;
/// The most, mm³: black to white.
pub const PURGE_MAX: f64 = 800.0;

/// `#RRGGBB` (or `RRGGBB`, or `#RRGGBBAA`) → sRGB in 0..1; grey if unreadable.
fn srgb(hex: &str) -> [f64; 3] {
    let h = hex.trim().trim_start_matches('#');
    let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0],
        _ => [0.5; 3],
    }
}

/// CIE L* (0..100) of an sRGB colour.
fn lightness(c: [f64; 3]) -> f64 {
    let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let y = 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
    let f = if y > 216.0 / 24389.0 { y.cbrt() } else { (24389.0 / 27.0 * y + 16.0) / 116.0 };
    116.0 * f - 16.0
}

/// The purge, in whole mm³, changing from the filament coloured `from` to
/// the one coloured `to` (both `#RRGGBB`): 0 for the same slot is the
/// caller's business; two slots of one colour still get [`PURGE_MIN`].
pub fn purge_volume(from: &str, to: &str) -> u32 {
    let (a, b) = (srgb(from), srgb(to));
    let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() / 3f64.sqrt();
    let lighter = ((lightness(b) - lightness(a)) / 100.0).max(0.0);
    (PURGE_MIN + 250.0 * d + 450.0 * lighter).clamp(PURGE_MIN, PURGE_MAX).round() as u32
}

/// `flush_volumes_matrix` for `colours` (slot 1 first) on a machine with
/// `nozzles` nozzles: per nozzle, the `n x n` table row by row, as strings.
pub fn flush_volumes_matrix(colours: &[String], nozzles: usize) -> Vec<String> {
    let n = colours.len();
    let mut out = Vec::with_capacity(nozzles.max(1) * n * n);
    for _ in 0..nozzles.max(1) {
        for (i, from) in colours.iter().enumerate() {
            for (j, to) in colours.iter().enumerate() {
                out.push(if i == j { "0".to_owned() } else { purge_volume(from, to).to_string() });
            }
        }
    }
    out
}

/// `flush_multiplier`: `"1"` per nozzle.
pub fn flush_multiplier(nozzles: usize) -> Vec<String> {
    vec!["1".to_owned(); nozzles.max(1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_lighter_purges_more_than_going_darker() {
        assert_eq!(purge_volume("#000000", "#FFFFFF"), 800);
        assert_eq!(purge_volume("#FFFFFF", "#000000"), 350);
        assert_eq!(purge_volume("#5B7FA3", "#5B7FA3"), 100, "the same colour in two slots");
        // A slate blue, an off-white and a near-black: the values a script
        // computing the same rule must agree with.
        assert_eq!(purge_volume("#5B7FA3", "#F4F3EE"), 414);
        assert_eq!(purge_volume("#F4F3EE", "#5B7FA3"), 217);
        assert_eq!(purge_volume("#161616", "#F4F3EE"), 714);
        assert_eq!(purge_volume("#5B7FA3", "#161616"), 207);
        assert!(purge_volume("#161616", "#FFCE1F") > purge_volume("#FFCE1F", "#161616"));
        assert!(purge_volume("#161616", "#F4F3EE") > purge_volume("#161616", "#5B7FA3"));
        for a in ["#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFCE1F", "nonsense"] {
            for b in ["#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFCE1F"] {
                let v = purge_volume(a, b);
                assert!((100..=800).contains(&v), "{a} -> {b}: {v}");
            }
        }
    }

    #[test]
    fn the_matrix_is_per_nozzle_row_by_row_with_a_zero_diagonal() {
        let colours: Vec<String> = ["#000000", "#FFFFFF", "#FF0000"].map(String::from).to_vec();
        let m = flush_volumes_matrix(&colours, 2);
        assert_eq!(m.len(), 2 * 9);
        assert_eq!(m[1], "800", "row = from black, column = to white");
        assert_eq!(m[3], "350", "from white to black");
        for b in 0..2 {
            for i in 0..3 {
                assert_eq!(m[b * 9 + i * 3 + i], "0");
            }
        }
        assert_eq!(m[..9], m[9..], "both nozzles the same table");
        assert_eq!(flush_multiplier(2), ["1", "1"]);
        assert_eq!(flush_volumes_matrix(&colours, 0).len(), 9, "at least one nozzle");
    }
}
