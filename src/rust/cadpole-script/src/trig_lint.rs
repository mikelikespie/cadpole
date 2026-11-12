//! A warning for radians handed to the trigonometry in `math`, which works
//! in degrees (`mathns`, "Angles are degrees").
//!
//! `math.sin(math.pi / 4)` is the line a reader of C or Python writes
//! without thinking, and in Cadpole it is the sine of 0.785 degrees: a
//! number, not an error, and a wrong model with nothing pointing back here.
//! The semantics stay as they are -- every other angle a script writes is in
//! degrees -- so this catches the spellings that can only mean radians, on
//! the text, before the script runs:
//!
//! * `math.pi` or `math.tau` inside the argument of `math.sin`, `math.cos`
//!   or `math.tan`;
//! * `math.radians(...)` inside one of them, which converts degrees into
//!   the unit the function does not take;
//! * `math.degrees(...)` straight around `math.asin`, `math.acos`,
//!   `math.atan` or `math.atan2`, which already answer in degrees.
//!
//! An argument that converts with `math.degrees(...)` (radians into
//! degrees, which is right) is left alone. A radian value reached through a
//! variable is not seen; the docs say the rule, and this catches the
//! common shapes of breaking it.
//!
//! The scan only has to know where strings and comments are, so it blanks
//! them out and then reads the code as plain text.

use cadpole_core::context::{Diagnostic, Span};

const FORWARD: [&str; 3] = ["sin", "cos", "tan"];
const INVERSE: [&str; 4] = ["asin", "acos", "atan", "atan2"];

/// Warnings for `source`, which is the file `path`.
pub fn check(path: &str, source: &str) -> Vec<Diagnostic> {
    let code = blank_strings_and_comments(source);
    let mut out = Vec::new();
    for (at, name) in calls(&code) {
        let open = at + "math.".len() + name.len();
        let Some(close) = matching_paren(&code, open) else { continue };
        let arg = &code[open + 1..close];
        let message = if FORWARD.contains(&name) {
            if arg.contains("math.degrees") {
                None
            } else if let Some(what) = ["math.pi", "math.tau"].into_iter().find(|w| contains_word(arg, w)) {
                Some(format!(
                    "math.{name} takes degrees, and its argument uses {what}, which only a radian angle \
                     would: math.{name}(math.pi / 4) is the {name} of 0.785 degrees. Write the angle in \
                     degrees (math.{name}(45)), or convert a radian value with math.degrees(...)"
                ))
            } else if contains_word(arg, "math.radians") {
                Some(format!(
                    "math.{name} takes degrees, and math.radians(...) inside it turns the angle into \
                     radians first; pass the degrees straight in"
                ))
            } else {
                None
            }
        } else if name == "degrees" {
            let inner = arg.trim_start().strip_prefix("math.").unwrap_or("");
            INVERSE.iter().find(|f| starts_with_call(inner, f)).map(|f| {
                format!(
                    "math.{f} already answers in degrees, so math.degrees(...) around it converts \
                     degrees as if they were radians; drop the math.degrees"
                )
            })
        } else {
            None
        };
        if let Some(m) = message {
            out.push(Diagnostic::warning(m).with_span(Some(span(path, source, at, close + 1))));
        }
    }
    out
}

/// Every `math.NAME(` in `code` with NAME a forward function or `degrees`,
/// as (byte offset of `math`, NAME).
fn calls(code: &str) -> Vec<(usize, &'static str)> {
    let names: Vec<&'static str> = FORWARD.iter().copied().chain(["degrees"]).collect();
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = code[from..].find("math.") {
        let at = from + k;
        from = at + 1;
        if at > 0 && is_ident(b[at - 1]) || at > 0 && b[at - 1] == b'.' {
            continue;
        }
        let rest = &code[at + "math.".len()..];
        if let Some(name) = names.iter().find(|n| starts_with_call(rest, n)) {
            out.push((at, *name));
        }
    }
    out
}

/// `s` starts with `name(`, allowing spaces before the parenthesis.
fn starts_with_call(s: &str, name: &str) -> bool {
    let Some(rest) = s.strip_prefix(name) else { return false };
    let rest = rest.trim_start_matches([' ', '\t']);
    rest.starts_with('(')
}

/// `word` occurs in `s` as a whole name (`math.pi` but not `math.pipe`).
fn contains_word(s: &str, word: &str) -> bool {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(k) = s[from..].find(word) {
        let at = from + k;
        let end = at + word.len();
        let before_ok = at == 0 || !(is_ident(b[at - 1]) || b[at - 1] == b'.');
        let after_ok = end >= b.len() || !is_ident(b[end]);
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
    }
    false
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The `)` that closes the `(` at or after `from`.
fn matching_paren(code: &str, from: usize) -> Option<usize> {
    let b = code.as_bytes();
    let open = from + code[from..].find('(')?;
    let mut depth = 0usize;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return (c == b')').then_some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `source` with every string literal's contents and every comment replaced
/// by spaces, byte for byte, so offsets (and lines) are unchanged. The
/// expressions inside an f-string's braces are blanked too: a warning there
/// is not worth a second tokenizer.
fn blank_strings_and_comments(source: &str) -> String {
    let b = source.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            q @ (b'"' | b'\'') => {
                let triple = b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q);
                let width = if triple { 3 } else { 1 };
                i += width;
                while i < b.len() {
                    if b[i] == b'\\' {
                        out[i] = b' ';
                        if i + 1 < b.len() && b[i + 1] != b'\n' {
                            out[i + 1] = b' ';
                        }
                        i += 2;
                        continue;
                    }
                    if b[i] == q && (!triple || (b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q))) {
                        i += width;
                        break;
                    }
                    if !triple && b[i] == b'\n' {
                        break;
                    }
                    if b[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    // Only ASCII bytes were replaced, and only whole characters inside
    // strings and comments (a multi-byte character's every byte is
    // replaced), so this is still UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

fn span(path: &str, source: &str, start: usize, end: usize) -> Span {
    let pos = |at: usize| {
        let before = &source[..at.min(source.len())];
        let line = before.matches('\n').count() as u32 + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) as u32 + 1;
        (line, col)
    };
    let (start_line, start_col) = pos(start);
    let (end_line, end_col) = pos(end);
    Span { path: path.to_owned(), start_line, start_col, end_line, end_col }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warned(src: &str) -> Vec<String> {
        check("main.star", src).into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn radians_in_a_forward_function_are_warned_about() {
        for src in [
            "x = math.sin(math.pi / 4)\n",
            "x = math.cos(2 * math.pi * i / n)\n",
            "x = math.tan( math.tau/8 )\n",
            "x = math.sin(a * math.pi / 180)\n",
            "x = math.cos(math.radians(30))\n",
        ] {
            let w = warned(src);
            assert_eq!(w.len(), 1, "{src}: {w:?}");
            assert!(w[0].contains("takes degrees"), "{w:?}");
        }
    }

    #[test]
    fn degrees_around_an_inverse_function_is_warned_about() {
        for src in ["a = math.degrees(math.acos(c))\n", "a = math.degrees( math.atan2(y, x))\n"] {
            let w = warned(src);
            assert_eq!(w.len(), 1, "{src}: {w:?}");
            assert!(w[0].contains("already answers in degrees"), "{w:?}");
        }
    }

    #[test]
    fn degree_arithmetic_and_conversions_are_left_alone() {
        for src in [
            "x = math.sin(45)\n",
            "x = math.cos(360.0 * i / n) * r\n",
            "x = math.sin(math.degrees(math.pi / 4))\n",
            "a = math.radians(math.atan2(y, x))\n",
            "c = 2 * math.pi * r\nx = math.sin(theta)\n",
            "x = math.sin(a) * math.pi\n",
            "s = \"math.sin(math.pi)\"\n# math.cos(math.pi)\n",
            "s = '''\nmath.sin(math.pi)\n'''\n",
            "x = mymath.sin(math.pi)\n",
            "x = math.sin(math.pipe)\n",
        ] {
            assert!(warned(src).is_empty(), "{src}: {:?}", warned(src));
        }
    }

    #[test]
    fn the_warning_points_at_the_call() {
        let d = check("part.star", "r = 5\n\nx = r * math.sin(math.pi / 6)\n");
        assert_eq!(d.len(), 1);
        let s = d[0].span.clone().unwrap();
        assert_eq!((s.path.as_str(), s.start_line, s.start_col, s.end_line), ("part.star", 3, 9, 3));
        assert_eq!(s.end_col, 9 + "math.sin(math.pi / 6)".len() as u32);
    }

    /// Every example and the standard library pass without a warning: the
    /// rule is one they already follow.
    #[test]
    fn the_examples_and_the_standard_library_are_clean() {
        let mut seen = 0;
        for var in ["CADPOLE_EXAMPLES", "CADPOLE_STDLIB"] {
            let Ok(dir) = std::env::var(var) else { continue };
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("star") {
                    continue;
                }
                let src = std::fs::read_to_string(&p).unwrap();
                let w = warned(&src);
                assert!(w.is_empty(), "{}: {w:?}", p.display());
                seen += 1;
            }
        }
        assert!(seen > 0 || std::env::var("CADPOLE_EXAMPLES").is_err(), "no scripts found");
    }
}
