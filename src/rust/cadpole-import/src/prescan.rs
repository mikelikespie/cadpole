//! The byte-level pre-scan: what is refused before any parser runs
//! (docs/security.md, "Imports").
//!
//! A parser is the one place a document carries arbitrary bytes into
//! third-party code, and the failure modes that matter are not panics —
//! [`crate::bounded::run`] turns those into diagnostics — but the two it
//! cannot catch: a native stack overflow, which aborts the process, and a
//! parse that simply never ends. Both are decided by properties of the
//! *bytes* that one linear pass can measure, so they are measured here,
//! first, with the numbers below.
//!
//! | format | cap | why |
//! | --- | --- | --- |
//! | any | [`MAX_BYTES`] (1 GB) | the importer's own ceiling, above every cap a host sets on the asset table (32 MB where bytes travel, 512 MB for a mesh a native run reads from disk: `cadpole_assets::Limits`), so a reader handed bytes another way is still bounded |
//! | STEP | [`MAX_STEP_BYTES`] 64 MB text | above the asset cap on purpose |
//! | STEP | [`MAX_STEP_ENTITIES`] 2 000 000 entity instances | bounds parse time and the holder table's memory |
//! | STEP | [`MAX_PAREN_DEPTH`] 64 nested parentheses | `ruststep`'s parameter grammar recurses per level with no limit of its own; a B-spline surface's control-point grid, the deepest common case, is three levels inside a typed parameter |
//! | STEP | [`MAX_DIGIT_RUN`] 18 digits in an integer or an exponent | the lexer parses those with `expect` into an `i64`; 18 digits always fit. A real's integral and fractional parts go through an `f64` parse and may be any length — Rust prints `0.000123456789012345678` for a coordinate, and a cap on every digit run would refuse Cadpole's own files |
//! | STEP, OBJ, ASCII STL | [`MAX_LINE_BYTES`] 1 MB per line | a line is a token in every one of these grammars |
//! | STL, OBJ, 3MF, glTF | [`MAX_TRIANGLES`] 5 000 000 triangles | wasm memory, and what a display can carry; for glTF, counted over every node that places a mesh, before one is built |
//! | OBJ, glTF, 3MF | [`MAX_VERTICES`] 10 000 000 vertices | as above; for 3MF, counted over every placement |
//! | glTF | [`MAX_GLTF_JSON_BYTES`] 256 MB of JSON | a GLB's JSON chunk is a few kilobytes; a `.gltf` with its buffers inline as base64 is the one that is large |
//! | glTF | [`MAX_NODE_DEPTH`] 1024 nodes deep, never cyclic, a node under one parent | the scene graph is walked with a stack of its own; a node reached twice is a DAG the format forbids |
//! | glTF | JSON nesting 128 deep | `serde_json`'s own recursion limit |
//! | 3MF | [`MAX_ZIP_ENTRIES`] 256 entries, [`MAX_ZIP_INFLATED`] 256 MB inflated in all, [`MAX_ZIP_RATIO`] 100:1 per entry, no `..` or absolute entry names | a zip bomb is a directory of promises; a Bambu project is one entry per object plus two thumbnails per plate, so 64 would refuse a real one |
//! | 3MF | [`MAX_MODEL_BYTES`] 64 MB per model part, [`MAX_COMPONENT_DEPTH`] 8 | the XML is parsed whole; components may nest |
//! | 3MF | [`MAX_PLACEMENTS`] 1 000 000 object placements, [`MAX_VERTICES`] placed vertices, in the whole build | a component is a reference, so eight levels of eight references to one object are 16.8 M placements from a kilobyte, and a mesh with no triangles never moves the triangle count |
//! | DXF | [`MAX_DXF_BYTES`] 64 MB text | as STEP: above the asset cap on purpose |
//! | DXF | [`MAX_DXF_PAIRS`] 8 000 000 group-code/value pairs | a pair is at least five bytes, so this is the asset cap restated in the grammar's own unit; it bounds the walk |
//! | DXF | [`MAX_DXF_ENTITIES`] 1 000 000 entities before `INSERT` expansion, [`MAX_DXF_EXPANDED`] 2 000 000 after | a `MINSERT` array is a multiplier: 100 × 100 copies of a 1 000-entity block is ten million entities from a kilobyte |
//! | DXF | [`MAX_DXF_VERTICES`] 10 000 000 vertices in all | as OBJ |
//! | DXF | [`MAX_INSERT_DEPTH`] 8, never cyclic | as 3MF components: a block that inserts itself is a loop, not a drawing |
//! | DXF | [`MAX_DXF_TEXT_BYTES`] 4 KB per `TEXT`/`MTEXT` | a label is a label; `MTEXT` chunks are summed |
//! | DXF | [`MAX_DXF_SPLINE_POINTS`] 10 000 control or fit points per `SPLINE` | knot insertion is quadratic in the count |
//! | DXF, STEP, OBJ, ASCII STL | [`MAX_LINE_BYTES`] 1 MB per line | a line is a token |

use cadpole_assets::AssetKind;

/// Largest input any reader takes, in bytes: the importer's own ceiling,
/// above every cap a host puts on its asset table (`cadpole_assets::Limits`),
/// which is what binds in practice.
pub const MAX_BYTES: u64 = 1 << 30;
/// Largest STEP text, in bytes.
pub const MAX_STEP_BYTES: usize = 64 * 1024 * 1024;
/// Most entity instances in a STEP file.
pub const MAX_STEP_ENTITIES: usize = 2_000_000;
/// Deepest parenthesis nesting in a STEP file, outside strings.
pub const MAX_PAREN_DEPTH: usize = 64;
/// Longest integer literal or exponent in a STEP file, outside strings.
pub const MAX_DIGIT_RUN: usize = 18;
/// Longest line in a text format, in bytes.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Most triangles in one mesh asset.
pub const MAX_TRIANGLES: usize = 5_000_000;
/// Most vertices in one mesh asset.
pub const MAX_VERTICES: usize = 10_000_000;
/// Largest glTF JSON — a GLB's JSON chunk, or a whole `.gltf` — in bytes.
pub const MAX_GLTF_JSON_BYTES: usize = 256 * 1024 * 1024;
/// Deepest glTF node hierarchy.
pub const MAX_NODE_DEPTH: usize = 1024;
/// Most entries in a 3MF package.
pub const MAX_ZIP_ENTRIES: usize = 256;
/// Most bytes a 3MF package may inflate to, in all.
pub const MAX_ZIP_INFLATED: u64 = 256 * 1024 * 1024;
/// Largest inflation ratio of one 3MF entry.
pub const MAX_ZIP_RATIO: u64 = 100;
/// Largest model part in a 3MF package, in bytes.
pub const MAX_MODEL_BYTES: usize = 64 * 1024 * 1024;
/// Deepest `<components>` nesting in a 3MF package.
pub const MAX_COMPONENT_DEPTH: usize = 8;
/// Most objects a 3MF build may place, counting every visit of a mesh or a
/// component list: a shared object is placed once per reference.
pub const MAX_PLACEMENTS: usize = 1_000_000;
/// Largest DXF text, in bytes.
pub const MAX_DXF_BYTES: usize = 64 * 1024 * 1024;
/// Most group-code/value pairs in a DXF.
pub const MAX_DXF_PAIRS: usize = 8_000_000;
/// Most entities in a DXF before `INSERT` expansion (`BLOCKS` and
/// `ENTITIES` together; `VERTEX` and `ATTRIB` records count).
pub const MAX_DXF_ENTITIES: usize = 1_000_000;
/// Most entities a DXF may expand to once every `INSERT` is placed.
pub const MAX_DXF_EXPANDED: usize = 2_000_000;
/// Most vertices in a DXF, in all.
pub const MAX_DXF_VERTICES: usize = 10_000_000;
/// Deepest `INSERT` nesting.
pub const MAX_INSERT_DEPTH: usize = 8;
/// Longest `TEXT`/`MTEXT` value kept, in bytes; the rest is cut with a note.
pub const MAX_DXF_TEXT_BYTES: usize = 4096;
/// Most control or fit points in one `SPLINE`.
pub const MAX_DXF_SPLINE_POINTS: usize = 10_000;

/// The formats this crate reads, as the bytes say (never the file name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Step,
    Stl,
    Obj,
    ThreeMf,
    Dxf,
    /// glTF 2.0, binary or JSON.
    Gltf,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Step => "STEP",
            Format::Stl => "STL",
            Format::Obj => "OBJ",
            Format::ThreeMf => "3MF",
            Format::Dxf => "DXF",
            Format::Gltf => "glTF",
        }
    }
}

/// What the bytes are, by their magic (`cadpole_assets::sniff` with the file
/// name as the tie-breaker it already is), or why they are not something
/// this crate reads.
pub fn format_of(name: &str, bytes: &[u8]) -> Result<Format, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("{} bytes is over the importer's {} MB limit", bytes.len(), MAX_BYTES / (1024 * 1024)));
    }
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    match cadpole_assets::sniff(name, bytes) {
        AssetKind::Step => Ok(Format::Step),
        AssetKind::Stl => Ok(Format::Stl),
        AssetKind::Obj => Ok(Format::Obj),
        AssetKind::ThreeMf => Ok(Format::ThreeMf),
        AssetKind::Dxf => Ok(Format::Dxf),
        AssetKind::Gltf => Ok(Format::Gltf),
        other => Err(format!(
            "not a STEP, STL, OBJ, 3MF, glTF or DXF file (the bytes sniff as {}); a STEP file starts with \
             `ISO-10303-21;`, a binary STL is 84 + 50 n bytes, an ASCII STL starts with `solid`, \
             an OBJ has `v`/`f` lines, a 3MF is a zip, a GLB starts with `glTF`, a .gltf is JSON with \
             an `asset` and a DXF starts with a `0` / `SECTION` pair",
            other.name()
        )),
    }
}

/// The DXF pre-scan: size, the two things that are not ASCII DXF (a binary
/// DXF's sentinel, a DWG's version stamp), line length, the pair grammar —
/// every non-blank line at an even position is an integer group code — the
/// pair, entity and vertex counts, and the framing (`0` / `SECTION` first,
/// comments aside). Returns the number of pairs.
///
/// Counting what the grammar makes countable here, before any value is
/// interpreted, is what lets [`crate::dxf`] walk the pairs without a cap
/// check at every step: a code-`0` line is an entity, a code-`10` line is a
/// vertex, and both are bounded on the way in.
pub fn dxf(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() > MAX_DXF_BYTES {
        return Err(format!("{} bytes of DXF text is over the {} MB limit", bytes.len(), MAX_DXF_BYTES / (1024 * 1024)));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = &bytes[..bytes.len().min(64)];
    if head.starts_with(b"AutoCAD Binary DXF") {
        return Err("a binary DXF (the `AutoCAD Binary DXF` sentinel); Cadpole reads ASCII DXF — save as \
                    `AutoCAD DXF (R12 or later)` rather than `Binary DXF`"
            .into());
    }
    if head.len() >= 6 && head.starts_with(b"AC1") && head[3..6].iter().all(u8::is_ascii_digit) {
        return Err("a DWG (the `AC10xx` version stamp): DWG is Autodesk's proprietary format and is not read; \
                    export a DXF from the CAD program (or ODA File Converter) and import that"
            .into());
    }
    let mut line = 0usize;
    let mut pairs = 0usize;
    let mut entities = 0usize;
    let mut vertices = 0usize;
    let mut framed = false;
    let mut code: Option<i64> = None;
    for raw in bytes.split(|&b| b == b'\n') {
        line += 1;
        if raw.len() > MAX_LINE_BYTES {
            return Err(format!("line {line} is longer than {} bytes", MAX_LINE_BYTES));
        }
        match code {
            None => {
                let t = raw.trim_ascii();
                if t.is_empty() {
                    continue;
                }
                let c = std::str::from_utf8(t).ok().and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| {
                    format!(
                        "line {line}: expected a group code (an integer) but found {}; an ASCII DXF alternates \
                         group codes and values, one per line",
                        crate::label::quoted(&String::from_utf8_lossy(&t[..t.len().min(32)]))
                    )
                })?;
                code = Some(c);
            }
            Some(c) => {
                pairs += 1;
                if pairs > MAX_DXF_PAIRS {
                    return Err(format!("more than {MAX_DXF_PAIRS} group-code/value pairs"));
                }
                match c {
                    999 => {}
                    0 => {
                        if !framed {
                            let v = raw.trim_ascii();
                            if v != b"SECTION" {
                                return Err(format!(
                                    "not a DXF: the first group is 0 / {} rather than 0 / SECTION",
                                    crate::label::quoted(&String::from_utf8_lossy(&v[..v.len().min(32)]))
                                ));
                            }
                            framed = true;
                        }
                        entities += 1;
                        if entities > MAX_DXF_ENTITIES {
                            return Err(format!("more than {MAX_DXF_ENTITIES} entities"));
                        }
                    }
                    10 => {
                        vertices += 1;
                        if vertices > MAX_DXF_VERTICES {
                            return Err(format!("more than {MAX_DXF_VERTICES} vertices"));
                        }
                    }
                    _ => {
                        if !framed {
                            return Err(format!(
                                "not a DXF: the first group is {c} / {} rather than 0 / SECTION",
                                crate::label::quoted(&String::from_utf8_lossy(&raw[..raw.len().min(32)]))
                            ));
                        }
                    }
                }
                code = None;
            }
        }
    }
    if let Some(c) = code {
        return Err(format!("the file ends after group code {c} on line {line} without its value"));
    }
    if !framed {
        return Err("not a DXF: no `0` / `SECTION` pair".into());
    }
    Ok(pairs)
}

/// The STEP pre-scan: size, entity count, parenthesis depth, digit runs and
/// line length, measured outside string literals, plus the framing every
/// Part 21 file has. Returns the entity count.
pub fn step(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() > MAX_STEP_BYTES {
        return Err(format!(
            "{} bytes of STEP text is over the {} MB limit",
            bytes.len(),
            MAX_STEP_BYTES / (1024 * 1024)
        ));
    }
    let head = &bytes[..bytes.len().min(64)];
    let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    if !head.starts_with(b"ISO-10303-21;") {
        return Err("not a STEP file: it does not start with `ISO-10303-21;`".into());
    }
    let mut depth = 0usize;
    let mut digits = 0usize;
    // The byte before the current digit run: `.` means the run is a real's
    // fraction, which the lexer parses as an `f64`.
    let mut run_prev = 0u8;
    // A run that ends at `b`: an integer the lexer parses as an `i64`
    // unless it is the integral part of a real (`b == '.'`) or its
    // fraction (`run_prev == '.'`).
    let too_long = |digits: usize, run_prev: u8, b: u8, line: usize| -> Result<(), String> {
        if digits > MAX_DIGIT_RUN && run_prev != b'.' && b != b'.' {
            return Err(format!(
                "line {line} has an integer of more than {MAX_DIGIT_RUN} digits; no STEP entity needs that and \
                 the lexer's integer parse would fail on it"
            ));
        }
        Ok(())
    };
    let mut line_len = 0usize;
    let mut line = 1usize;
    let mut in_string = false;
    let mut in_comment = false;
    let mut entities = 0usize;
    let mut at_line_start = true;
    let mut prev = 0u8;
    for &b in bytes {
        line_len += 1;
        if line_len > MAX_LINE_BYTES {
            return Err(format!("line {line} is longer than {} bytes", MAX_LINE_BYTES));
        }
        if b == b'\n' {
            too_long(digits, run_prev, b, line)?;
            line += 1;
            line_len = 0;
            at_line_start = true;
            digits = 0;
            prev = b;
            continue;
        }
        if in_comment {
            if prev == b'*' && b == b'/' {
                in_comment = false;
            }
            prev = b;
            continue;
        }
        if in_string {
            if b == b'\'' {
                in_string = false;
            }
            prev = b;
            continue;
        }
        match b {
            b'\'' => in_string = true,
            b'*' if prev == b'/' => in_comment = true,
            b'(' => {
                depth += 1;
                if depth > MAX_PAREN_DEPTH {
                    return Err(format!(
                        "line {line} nests parentheses more than {MAX_PAREN_DEPTH} deep; a STEP entity never \
                         does, and the parser recurses per level"
                    ));
                }
            }
            b')' => depth = depth.saturating_sub(1),
            b'#' if at_line_start || prev.is_ascii_whitespace() => {
                // `#123 =` at the start of a line is an entity instance.
                if at_line_start {
                    entities += 1;
                    if entities > MAX_STEP_ENTITIES {
                        return Err(format!("more than {MAX_STEP_ENTITIES} entity instances"));
                    }
                }
            }
            _ => {}
        }
        if b.is_ascii_digit() {
            if digits == 0 {
                run_prev = prev;
            }
            digits += 1;
        } else {
            too_long(digits, run_prev, b, line)?;
            digits = 0;
        }
        if !b.is_ascii_whitespace() {
            at_line_start = false;
        }
        prev = b;
    }
    too_long(digits, run_prev, 0, line)?;
    if in_string {
        return Err("a string literal is not closed before the end of the file".into());
    }
    Ok(entities)
}

/// The text-mesh pre-scan (ASCII STL, OBJ): size and line length.
pub fn text_lines(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("{} bytes is over the {} MB limit", bytes.len(), MAX_BYTES / (1024 * 1024)));
    }
    let mut line = 1usize;
    let mut len = 0usize;
    for &b in bytes {
        if b == b'\n' {
            line += 1;
            len = 0;
        } else {
            len += 1;
            if len > MAX_LINE_BYTES {
                return Err(format!("line {line} is longer than {} bytes", MAX_LINE_BYTES));
            }
        }
    }
    Ok(())
}

/// The binary STL pre-scan: the header's triangle count against the length.
/// Returns the count.
pub fn binary_stl(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 84 {
        return Err(format!("{} bytes is too short for a binary STL (84-byte header)", bytes.len()));
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    if n > MAX_TRIANGLES {
        return Err(format!("{n} triangles is over the {MAX_TRIANGLES} limit"));
    }
    let need = 84 + 50 * n;
    if bytes.len() < need {
        return Err(format!(
            "the header promises {n} triangles ({need} bytes) but the file is {} bytes: truncated",
            bytes.len()
        ));
    }
    Ok(n)
}

/// The caps a 3MF package is read under.
pub fn zip_caps() -> cadpole_export_3mf::zip::Caps {
    cadpole_export_3mf::zip::Caps { max_entries: MAX_ZIP_ENTRIES, max_inflated: MAX_ZIP_INFLATED, max_ratio: MAX_ZIP_RATIO }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_file_passes_and_its_entities_are_counted() {
        let s = b"ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n#1 = CARTESIAN_POINT('', (0., 0., 0.));\n#2 = DIRECTION('a ( deep ( string', (0., 0., 1.));\nENDSEC;\nEND-ISO-10303-21;\n";
        assert_eq!(step(s).unwrap(), 2);
    }

    #[test]
    fn the_nesting_bound_holds() {
        let mut s = b"ISO-10303-21;\nDATA;\n#1 = X(".to_vec();
        s.extend(std::iter::repeat_n(b'(', 70));
        assert!(step(&s).unwrap_err().contains("nests parentheses"));
        let mut ok = b"ISO-10303-21;\nDATA;\n#1 = X(".to_vec();
        ok.extend(std::iter::repeat_n(b'(', 60));
        ok.extend(std::iter::repeat_n(b')', 61));
        assert!(step(&ok).is_ok());
    }

    #[test]
    fn a_long_number_and_a_long_line_are_refused() {
        let s = b"ISO-10303-21;\nDATA;\n#1 = X(1234567890123456789);\n";
        assert!(step(s).unwrap_err().contains("digits"));
        // The lexer parses a real through an f64, however long its parts:
        // Rust's own float printing writes fractions this long.
        assert!(step(b"ISO-10303-21;\nDATA;\n#1 = X(0.000123456789012345678901);\n").is_ok());
        assert!(step(b"ISO-10303-21;\nDATA;\n#1 = X(1234567890123456789012345.);\n").is_ok());
        assert!(step(b"ISO-10303-21;\nDATA;\n#1 = X(-1234567890123456789012345.5E-3);\n").is_ok());
        // An exponent is an integer to the lexer.
        assert!(step(b"ISO-10303-21;\nDATA;\n#1 = X(1.5E1234567890123456789);\n").unwrap_err().contains("digits"));
        // As is an entity id, and a run that ends the file.
        assert!(step(b"ISO-10303-21;\nDATA;\n#1234567890123456789012 = X();\n").unwrap_err().contains("digits"));
        assert!(step(b"ISO-10303-21;\nDATA;\n#1 = X(1234567890123456789012").unwrap_err().contains("digits"));
        let mut long = b"ISO-10303-21;\nDATA;\n#1 = X('".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_LINE_BYTES + 10));
        assert!(step(&long).unwrap_err().contains("longer than"));
        // A long number *inside a string* is text, not a number.
        let s2 = b"ISO-10303-21;\nDATA;\n#1 = X('1234567890123456789012345');\n";
        assert!(step(s2).is_ok());
    }

    #[test]
    fn the_wrong_magic_is_named() {
        assert!(step(b"solid cube\n").unwrap_err().contains("ISO-10303-21"));
        assert!(format_of("x.bin", b"\x89PNG\r\n\x1a\n").unwrap_err().contains("image"));
        assert_eq!(format_of("x.step", b"ISO-10303-21;\n").unwrap(), Format::Step);
        assert_eq!(format_of("x.stl", b"solid cube\nfacet normal 0 0 1\n").unwrap(), Format::Stl);
        assert!(format_of("x.stl", b"").is_err());
    }

    #[test]
    fn a_binary_stl_must_be_as_long_as_its_header_says() {
        let mut b = vec![0u8; 84];
        b[80..84].copy_from_slice(&3u32.to_le_bytes());
        assert!(binary_stl(&b).unwrap_err().contains("truncated"));
        b.extend(vec![0u8; 150]);
        assert_eq!(binary_stl(&b).unwrap(), 3);
        b[80..84].copy_from_slice(&(MAX_TRIANGLES as u32 + 1).to_le_bytes());
        assert!(binary_stl(&b).unwrap_err().contains("limit"));
    }
}
