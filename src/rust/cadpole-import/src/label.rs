//! One sanitiser for every name a file hands us.
//!
//! A STEP `name` attribute, an OBJ `o`/`g` line, a 3MF `object/@name` and a
//! Bambu part name all become local names in Cadpole's topology grammar
//! or text in a diagnostic, and a file is untrusted input:
//! the label is where a hostile file puts control characters, terminal
//! escapes and the reserved characters that would make `a/b|c#1` parse as
//! something else. So every label goes through [`sanitise`]: STEP's own
//! escapes (`\X2\…\X0\` for UTF-16, `\X\hh` for ISO 8859-1, `''` for a
//! quote) are decoded first, then control characters and the reserved set
//! `[ ] ( ) : | @ # /` become `_`, runs of whitespace and underscores
//! collapse to one `_`, and the result is capped at [`MAX_LABEL`]
//! characters. What the file actually said is kept beside it by the
//! caller, unsanitised, only ever shown quoted.

/// Longest local name a file may contribute, in characters.
pub const MAX_LABEL: usize = 64;

/// Characters the topology grammar reserves in a local name.
pub const RESERVED: &[char] = &['[', ']', '(', ')', ':', '|', '@', '#', '/'];

/// Decode ISO 10303-21's string escapes. Unknown or malformed escapes are
/// kept as they are, minus the backslashes, which is what a reader that
/// does not understand them would show.
pub fn decode_step(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' && b.get(i + 1) == Some(&b'\'') {
            out.push('\'');
            i += 2;
            continue;
        }
        if b[i] == b'\\' {
            // \X2\<hex4>*\X0\  -- UTF-16BE code units
            if s[i..].starts_with("\\X2\\") || s[i..].starts_with("\\X4\\") {
                let width = if s[i..].starts_with("\\X2\\") { 4 } else { 8 };
                let body_start = i + 4;
                if let Some(end) = s[body_start..].find("\\X0\\") {
                    let hex = &s[body_start..body_start + end];
                    let mut units = Vec::new();
                    let mut ok = true;
                    for chunk in hex.as_bytes().chunks(width) {
                        match std::str::from_utf8(chunk).ok().and_then(|h| u32::from_str_radix(h, 16).ok()) {
                            Some(u) => units.push(u),
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        if width == 4 {
                            let u16s: Vec<u16> = units.iter().map(|&u| u as u16).collect();
                            out.push_str(&String::from_utf16_lossy(&u16s));
                        } else {
                            for u in units {
                                out.push(char::from_u32(u).unwrap_or('\u{fffd}'));
                            }
                        }
                        i = body_start + end + 4;
                        continue;
                    }
                }
            }
            // \X\hh  -- one ISO 8859-1 byte
            if s[i..].starts_with("\\X\\") && i + 5 <= s.len() {
                if let Ok(v) = u8::from_str_radix(&s[i + 3..i + 5], 16) {
                    out.push(v as char);
                    i += 5;
                    continue;
                }
            }
            // \S\c  -- upper-half ISO 8859-1 (c + 128), \N\ newline, \P?\ page
            if s[i..].starts_with("\\S\\") && i + 4 <= s.len() {
                let c = b[i + 3];
                out.push((c.wrapping_add(128)) as char);
                i += 4;
                continue;
            }
            if s[i..].starts_with("\\N\\") {
                out.push(' ');
                i += 3;
                continue;
            }
            if s[i..].starts_with("\\\\") {
                out.push('\\');
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// A file's label as a local name: decoded, reserved and control
/// characters replaced, whitespace collapsed, capped. Empty when nothing
/// usable is left, which callers treat as "unnamed".
pub fn sanitise(raw: &str) -> String {
    let decoded = decode_step(raw);
    let mut out = String::with_capacity(decoded.len().min(MAX_LABEL));
    let mut last_sep = true;
    for ch in decoded.chars() {
        let sep = ch.is_control() || ch.is_whitespace() || RESERVED.contains(&ch) || ch == '_' || ch == '\\';
        if sep {
            if !last_sep {
                out.push('_');
            }
            last_sep = true;
        } else {
            out.push(ch);
            last_sep = false;
        }
        if out.chars().count() >= MAX_LABEL {
            break;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// Text for a diagnostic: control characters out, quoted, capped, so a
/// label can never write an escape sequence into a terminal or a log.
pub fn quoted(raw: &str) -> String {
    let mut s: String = decode_step(raw).chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
    if decode_step(raw).chars().filter(|c| !c.is_control()).count() > MAX_LABEL {
        s.push('…');
    }
    format!("{s:?}")
}

/// `name`, then `name#1`, `name#2`, … for the second and later uses of the
/// same sanitised label in one scope (the split rule of generated names,
/// applied to a file that named two faces alike).
pub fn dedupe(names: &mut [String]) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for n in names.iter_mut() {
        if n.is_empty() {
            continue;
        }
        let k = seen.entry(n.clone()).or_insert(0);
        if *k > 0 {
            *n = format!("{n}#{k}");
        }
        *k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_escapes_decode() {
        assert_eq!(decode_step("Motor \\X2\\00E9\\X0\\ 2"), "Motor é 2");
        assert_eq!(decode_step("it''s"), "it's");
        assert_eq!(decode_step("\\X\\E9"), "é");
        assert_eq!(decode_step("a\\S\\ib"), "aéb");
        assert_eq!(decode_step("\\X2\\zz\\X0\\"), "X2zzX0", "a malformed escape keeps its text");
    }

    #[test]
    fn reserved_and_control_characters_become_underscores() {
        assert_eq!(sanitise("Shaft (front) #2"), "Shaft_front_2");
        assert_eq!(sanitise("a/b|c:d@e[f]"), "a_b_c_d_e_f");
        assert_eq!(sanitise("\u{1b}[31mred\u{1b}[0m"), "31mred_0m");
        assert_eq!(sanitise("   "), "");
        assert_eq!(sanitise("__x__"), "x");
        assert_eq!(sanitise(&"y".repeat(200)).chars().count(), MAX_LABEL);
    }

    #[test]
    fn quoted_text_cannot_carry_an_escape() {
        let q = quoted("bad\u{1b}]0;title\u{7}name");
        assert!(!q.contains('\u{1b}') && !q.contains('\u{7}'), "{q}");
        assert!(q.starts_with('"') && q.ends_with('"'));
    }

    #[test]
    fn duplicates_take_a_split_suffix() {
        let mut v = vec!["a".to_owned(), "b".to_owned(), "a".to_owned(), "".to_owned(), "a".to_owned()];
        dedupe(&mut v);
        assert_eq!(v, ["a", "b", "a#1", "", "a#2"]);
    }
}
