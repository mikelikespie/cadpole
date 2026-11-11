//! The painted-triangle string both PrusaSlicer (`slic3rpe:mmu_segmentation`)
//! and Bambu Studio / OrcaSlicer (`paint_color`) carry on a `<triangle>`.
//!
//! Implemented from the format description, not from any slicer's code (they
//! are AGPL-3.0; only the format is targeted).
//!
//! **The encoding.** A triangle's paint is a tree: the root is the whole
//! triangle, and a node may be split into sub-triangles. It is serialized as a
//! stream of 4-bit nibbles, written as hex digits and **read from the end of
//! the string** — the last character is the root.
//!
//! Each nibble is `(x << 2) | split_sides`. On a leaf (`split_sides == 0`),
//! `x` is the state: 0 = unpainted (use the object's own extruder), *k* =
//! filament *k*. Because `x` only holds two bits, states from 3 up use an
//! escape: the leaf nibble is `0b1100` (`C`) and the state is `3 +` the value
//! read from the nibbles before it, where each `F` contributes 15 and the last
//! nibble contributes its own value.
//!
//! Cadpole only ever emits **whole-triangle** paint — a face is painted, not a
//! patch of a face — so every string here is one leaf:
//!
//! | filament | string |
//! | --- | --- |
//! | 1 | `4` |
//! | 2 | `8` |
//! | 3 | `0C` |
//! | 4 | `1C` |
//! | 17 | `EC` |
//! | 18 | `0FC` |

/// The paint string for a whole triangle painted with filament `slot`
/// (1-based). Slot 0 means "not painted": the caller omits the attribute
/// entirely, which is what the slicers write for the base colour.
pub fn whole_triangle(slot: u32) -> Option<String> {
    if slot == 0 {
        return None;
    }
    if slot < 3 {
        // (state << 2) | 0 split sides.
        return Some(format!("{:X}", slot << 2));
    }
    // The escape: nibbles for `slot - 3`, then the 0xC sentinel. The extra
    // nibbles are read leftwards from the sentinel, so they are written in the
    // reverse of the order they are read.
    let mut value = slot - 3;
    let mut read_order = Vec::new();
    while value >= 15 {
        read_order.push(0xFu32);
        value -= 15;
    }
    read_order.push(value);
    let mut s: String = read_order.iter().rev().map(|n| std::char::from_digit(*n, 16).unwrap_or('0')).collect();
    s.make_ascii_uppercase();
    s.push('C');
    Some(s)
}

/// Decode a whole-triangle paint string back to its filament slot, for the
/// round-trip tests and, later, the 3MF reader. `None` for anything with a
/// split (which we never write) or for a malformed string.
pub fn decode_whole_triangle(s: &str) -> Option<u32> {
    let nibbles: Vec<u32> = s.chars().rev().map(|c| c.to_digit(16)).collect::<Option<Vec<_>>>()?;
    let root = *nibbles.first()?;
    if root & 0b11 != 0 {
        return None; // a split triangle
    }
    let state = root >> 2;
    if state < 3 {
        return Some(state);
    }
    let mut total = 0u32;
    for &n in &nibbles[1..] {
        total += n;
        if n != 0xF {
            return Some(3 + total);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_whole_triangle_strings() {
        assert_eq!(whole_triangle(0), None);
        assert_eq!(whole_triangle(1).unwrap(), "4");
        assert_eq!(whole_triangle(2).unwrap(), "8");
        assert_eq!(whole_triangle(3).unwrap(), "0C");
        assert_eq!(whole_triangle(4).unwrap(), "1C");
        assert_eq!(whole_triangle(5).unwrap(), "2C");
        assert_eq!(whole_triangle(17).unwrap(), "EC");
        assert_eq!(whole_triangle(18).unwrap(), "0FC");
    }

    #[test]
    fn every_slot_round_trips() {
        for slot in 1..64u32 {
            let s = whole_triangle(slot).unwrap();
            assert_eq!(decode_whole_triangle(&s), Some(slot), "slot {slot} encoded as {s:?}");
        }
    }

    #[test]
    fn a_split_triangle_is_refused_rather_than_guessed() {
        // Low two bits set means the root was split into sub-triangles.
        assert_eq!(decode_whole_triangle("5"), None);
        assert_eq!(decode_whole_triangle("zz"), None);
    }
}
