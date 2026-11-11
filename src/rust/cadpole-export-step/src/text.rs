//! The part 21 lexical layer: entity instances, and the four literal forms a
//! STEP exchange file is allowed to contain.
//!
//! ISO 10303-21 clause 6 ("exchange structure") is small enough to implement
//! exactly, and doing so is the difference between a file a reader accepts and
//! one it rejects on the first REAL. Three rules earn their own functions:
//!
//! * a REAL **must** contain a decimal point (`1` is an INTEGER and
//!   `LENGTH_MEASURE(1)` is a type error), and its exponent, if any, is
//!   written `E`, never `e`;
//! * a STRING is single-quoted, an embedded quote is doubled, a backslash is
//!   doubled, and anything outside printable ASCII is escaped as
//!   `\X2\<utf-16 units>\X0\`;
//! * every entity instance is `#<id> = <name>(<params>);` on its own line,
//!   and `<id>` is a positive integer unique in the data section.

use std::collections::HashMap;
use std::fmt::Write as _;

/// An entity instance name (`#42`), as an index into [`Emitter`].
pub type Id = usize;

/// Collects entity instances and hands out their ids.
///
/// Ids are handed out in emission order, but an id may be **reserved** before
/// its text is known ([`Emitter::reserve`]): the product-structure preamble
/// refers forward to the solids, and the solids refer back to the shared
/// representation context, so one of the two has to be written late.
#[derive(Default)]
pub struct Emitter {
    /// `bodies[i]` is the text of `#(i + 1)`, without the `#n = ` prefix.
    bodies: Vec<Option<String>>,
    /// Value entities (points, directions, placements) deduplicated by their
    /// rendered text. Sharing a `CARTESIAN_POINT` between two entities is
    /// ordinary STEP and halves the size of a typical file.
    cache: HashMap<String, Id>,
}

impl Emitter {
    pub fn new() -> Emitter {
        Emitter::default()
    }

    /// Reserve an id whose text is supplied later with [`Emitter::fill`].
    pub fn reserve(&mut self) -> Id {
        self.bodies.push(None);
        self.bodies.len()
    }

    /// Supply the text of a reserved id. Panics if the id was already written:
    /// that is a bug in this crate, not bad input.
    pub fn fill(&mut self, id: Id, body: impl Into<String>) {
        let slot = &mut self.bodies[id - 1];
        assert!(slot.is_none(), "entity #{id} written twice");
        *slot = Some(body.into());
    }

    /// Write an entity instance and return its id.
    pub fn emit(&mut self, body: impl Into<String>) -> Id {
        let id = self.reserve();
        self.fill(id, body);
        id
    }

    /// Write a *value* entity, reusing an identical one if it already exists.
    /// Only use this for entities with no identity of their own: a
    /// `VERTEX_POINT` is shared through the topology, never through this.
    pub fn emit_shared(&mut self, body: impl Into<String>) -> Id {
        let body = body.into();
        if let Some(id) = self.cache.get(&body) {
            return *id;
        }
        let id = self.emit(body.clone());
        self.cache.insert(body, id);
        id
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// The data section body, one instance per line.
    pub fn render(&self) -> String {
        let mut s = String::with_capacity(self.bodies.len() * 48);
        for (i, b) in self.bodies.iter().enumerate() {
            let body = b.as_deref().unwrap_or_else(|| panic!("entity #{} was reserved but never written", i + 1));
            let _ = writeln!(s, "#{} = {};", i + 1, body);
        }
        s
    }
}

/// A part 21 REAL literal.
///
/// Rust's `{:?}` gives the shortest decimal that round-trips, which is exactly
/// what an exchange file wants; all this adds is the decimal point part 21
/// insists on and the upper-case exponent marker.
pub fn real(x: f64) -> String {
    debug_assert!(x.is_finite(), "non-finite reals are rejected before they reach here");
    let s = format!("{x:?}");
    match s.find(['e', 'E']) {
        Some(at) => {
            let (mantissa, exp) = s.split_at(at);
            let mut m = mantissa.to_owned();
            if !m.contains('.') {
                m.push('.');
            }
            format!("{m}E{}", &exp[1..])
        }
        None if s.contains('.') => s,
        None => format!("{s}."),
    }
}

/// A list of REALs: `(1., 2.5, 3.)`.
pub fn reals(xs: impl IntoIterator<Item = f64>) -> String {
    let mut s = String::from("(");
    for (i, x) in xs.into_iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&real(x));
    }
    s.push(')');
    s
}

/// A list of INTEGERs: `(3, 1, 1, 3)`.
pub fn ints(xs: impl IntoIterator<Item = usize>) -> String {
    let mut s = String::from("(");
    for (i, x) in xs.into_iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let _ = write!(s, "{x}");
    }
    s.push(')');
    s
}

/// A list of entity references: `(#1, #2, #3)`.
pub fn refs(ids: impl IntoIterator<Item = Id>) -> String {
    let mut s = String::from("(");
    for (i, id) in ids.into_iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let _ = write!(s, "#{id}");
    }
    s.push(')');
    s
}

/// A part 21 STRING literal, quotes and all.
///
/// Part 21 strings are ISO 8859-1 with escape sequences; Cadpole's names are
/// UTF-8 and may hold anything, so everything outside printable ASCII goes
/// through `\X2\`, which carries UTF-16 code units and is what every modern
/// reader implements.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    let mut pending_x2 = false;
    for c in s.chars() {
        let plain = matches!(c, ' '..='~');
        if !plain {
            if !pending_x2 {
                out.push_str("\\X2\\");
                pending_x2 = true;
            }
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                let _ = write!(out, "{unit:04X}");
            }
            continue;
        }
        if pending_x2 {
            out.push_str("\\X0\\");
            pending_x2 = false;
        }
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    if pending_x2 {
        out.push_str("\\X0\\");
    }
    out.push('\'');
    out
}

/// `.T.` / `.F.`, the BOOLEAN literal.
pub fn boolean(b: bool) -> &'static str {
    if b { ".T." } else { ".F." }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_real_carries_a_decimal_point() {
        for x in [0.0, 1.0, -1.0, 42.0, 0.5, -0.125, 1e20, 1e-20, 1.0 / 3.0, f64::MIN_POSITIVE] {
            let s = real(x);
            assert!(s.contains('.'), "{x} rendered as {s} with no decimal point");
            assert!(!s.contains('e'), "{x} rendered as {s} with a lower-case exponent");
        }
    }

    #[test]
    fn reals_round_trip_through_their_text() {
        for x in [0.1, 1.0 / 3.0, 1e-7, -2.5e17, 83.5, 1.0000000000000002] {
            let s = real(x);
            let back: f64 = s.replace('E', "e").trim_end_matches('.').parse().unwrap_or_else(|e| {
                panic!("{s} does not parse back: {e}");
            });
            assert_eq!(back, x, "{x} rendered as {s}");
        }
    }

    #[test]
    fn negative_zero_is_not_written_as_a_bare_integer() {
        assert_eq!(real(-0.0), "-0.0");
        assert_eq!(real(0.0), "0.0");
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_non_ascii() {
        assert_eq!(string("plain"), "'plain'");
        assert_eq!(string("it's"), "'it''s'");
        assert_eq!(string("a\\b"), "'a\\\\b'");
        // U+00E9 is one UTF-16 unit; the escape closes again for the ASCII tail.
        assert_eq!(string("caf\u{e9}s"), "'caf\\X2\\00E9\\X0\\s'");
        // Outside the BMP: two surrogate units inside one escape.
        assert_eq!(string("\u{1F600}"), "'\\X2\\D83DDE00\\X0\\'");
    }

    #[test]
    fn lists_render_with_the_expected_separators() {
        assert_eq!(reals([1.0, 2.5]), "(1.0, 2.5)");
        assert_eq!(ints([3usize, 1]), "(3, 1)");
        assert_eq!(refs([7usize, 9]), "(#7, #9)");
        assert_eq!(reals(Vec::<f64>::new()), "()");
    }

    #[test]
    fn reserved_ids_can_be_filled_out_of_order() {
        let mut e = Emitter::new();
        let first = e.reserve();
        let second = e.emit("DIRECTION('', (0.0, 0.0, 1.0))");
        e.fill(first, format!("PLANE('', #{second})"));
        assert_eq!(e.render(), "#1 = PLANE('', #2);\n#2 = DIRECTION('', (0.0, 0.0, 1.0));\n");
    }

    #[test]
    fn identical_value_entities_are_shared() {
        let mut e = Emitter::new();
        let a = e.emit_shared("CARTESIAN_POINT('', (0.0, 0.0, 0.0))");
        let b = e.emit_shared("CARTESIAN_POINT('', (0.0, 0.0, 0.0))");
        let c = e.emit_shared("CARTESIAN_POINT('', (1.0, 0.0, 0.0))");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(e.len(), 2);
    }
}
