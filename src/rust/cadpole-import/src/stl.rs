//! STL, binary and ASCII, into [`RawObject`]s.
//!
//! A binary STL is an 80-byte header, a `u32` triangle count and fifty bytes
//! per triangle (a normal nobody trusts, three corners, two attribute
//! bytes). It is recognised by its length — `84 + 50 n` for the `n` the
//! header declares — rather than by the header text, because files that
//! start with `solid` and are binary exist in numbers. An ASCII STL is
//! `solid NAME` … `facet normal` … `outer loop` … three `vertex` lines …
//! `endloop` `endfacet` … `endsolid`; a file may hold several solids, and
//! each becomes one object. Every triangle gets its own three vertices, as
//! the format has them: STL is a soup, and welding is the repair's decision
//! ([`crate::mesh::Repair`]), not the parser's.

use crate::Note;
use crate::mesh::RawObject;
use crate::prescan::{self, MAX_TRIANGLES};

/// Parse `bytes` as STL. `hint` names the file, for diagnostics.
pub fn parse(bytes: &[u8], hint: &str) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    let declared = (bytes.len() >= 84).then(|| u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize);
    let exact = declared.is_some_and(|n| n.checked_mul(50).and_then(|b| b.checked_add(84)) == Some(bytes.len()));
    // The text parser reads keywords in any case, so the sniff does too.
    let ascii = bytes.iter().skip_while(|b| b.is_ascii_whitespace()).take(5).copied().collect::<Vec<u8>>().eq_ignore_ascii_case(b"solid");
    // A binary record the header declares, with bytes after it.
    let trailing = declared.filter(|&n| n > 0 && n <= MAX_TRIANGLES && 84 + 50 * n < bytes.len());
    if exact {
        return binary(bytes, hint);
    }
    if ascii {
        match text(bytes) {
            Ok(r) => return Ok(r),
            // `solid` in a binary header is common (the module comment):
            // when the text does not parse and the header's count fits,
            // it is a binary file with trailing bytes, as it would be with
            // any other header.
            Err(e) if trailing.is_none() => return Err(e),
            Err(_) => {}
        }
    }
    if let Some(n) = trailing {
        let (objects, mut notes) = binary(bytes, hint)?;
        notes.push(Note::info(format!(
            "{} trailing byte{} after the {n} triangles the header declares were ignored",
            bytes.len() - (84 + 50 * n),
            if bytes.len() - (84 + 50 * n) == 1 { "" } else { "s" }
        )));
        return Ok((objects, notes));
    }
    // Neither: say why in the binary reader's terms, which name the count
    // and the length.
    prescan::binary_stl(bytes)?;
    Err("not an STL file: neither `solid` text nor a binary record of 84 + 50 n bytes".into())
}

fn binary(bytes: &[u8], _hint: &str) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    let n = prescan::binary_stl(bytes)?;
    if n == 0 {
        return Err("the binary STL declares 0 triangles".into());
    }
    let mut o = RawObject { positions: Vec::with_capacity(n * 3), triangles: Vec::with_capacity(n), ..Default::default() };
    let f = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as f64;
    for t in 0..n {
        let base = 84 + 50 * t + 12;
        let v = o.positions.len() as u32;
        for k in 0..3 {
            let at = base + 12 * k;
            o.positions.push([f(at), f(at + 4), f(at + 8)]);
        }
        o.triangles.push([v, v + 1, v + 2]);
    }
    Ok((vec![o], Vec::new()))
}

fn text(bytes: &[u8]) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    prescan::text_lines(bytes)?;
    let s = String::from_utf8_lossy(bytes);
    let mut objects: Vec<RawObject> = Vec::new();
    let mut cur: Option<RawObject> = None;
    let mut facet: Vec<[f64; 3]> = Vec::new();
    let mut in_facet = false;
    let mut total = 0usize;
    for (i, line) in s.lines().enumerate() {
        let ln = i + 1;
        let mut tok = line.split_whitespace();
        let Some(head) = tok.next() else { continue };
        match head.to_ascii_lowercase().as_str() {
            "solid" => {
                if let Some(o) = cur.take() {
                    // A missing `endsolid`: close what was open.
                    if !o.triangles.is_empty() {
                        objects.push(o);
                    }
                }
                let name = line.trim().get(5..).unwrap_or("").trim().to_owned();
                cur = Some(RawObject { name, ..Default::default() });
            }
            "facet" => {
                if cur.is_none() {
                    cur = Some(RawObject::default());
                }
                if in_facet {
                    return Err(format!("line {ln}: `facet` inside a facet (no `endfacet` before it)"));
                }
                in_facet = true;
                facet.clear();
            }
            "outer" | "endloop" => {}
            "vertex" => {
                if !in_facet {
                    return Err(format!("line {ln}: `vertex` outside a facet"));
                }
                let mut p = [0.0f64; 3];
                for (k, slot) in p.iter_mut().enumerate() {
                    let t = tok
                        .next()
                        .ok_or_else(|| format!("line {ln}: a vertex needs three coordinates, found {k}"))?;
                    *slot = t
                        .parse::<f64>()
                        .map_err(|_| format!("line {ln}: {t:?} is not a number"))?;
                    if !slot.is_finite() {
                        return Err(format!("line {ln}: coordinate {t:?} is not finite"));
                    }
                }
                facet.push(p);
                if facet.len() > 3 {
                    return Err(format!("line {ln}: a facet has more than three vertices (STL is triangles only)"));
                }
            }
            "endfacet" => {
                if !in_facet {
                    return Err(format!("line {ln}: `endfacet` without a facet"));
                }
                if facet.len() != 3 {
                    return Err(format!("line {ln}: the facet has {} vertices, not three", facet.len()));
                }
                in_facet = false;
                total += 1;
                if total > MAX_TRIANGLES {
                    return Err(format!("more than {MAX_TRIANGLES} triangles"));
                }
                let o = cur.as_mut().expect("a facet opens an object");
                let v = o.positions.len() as u32;
                o.positions.extend_from_slice(&facet);
                o.triangles.push([v, v + 1, v + 2]);
            }
            "endsolid" => {
                if in_facet {
                    return Err(format!("line {ln}: `endsolid` inside a facet"));
                }
                if let Some(o) = cur.take()
                    && !o.triangles.is_empty()
                {
                    objects.push(o);
                }
            }
            other => {
                return Err(format!(
                    "line {ln}: unexpected {other:?} (an ASCII STL has solid, facet, outer loop, vertex, \
                     endloop, endfacet and endsolid lines)"
                ));
            }
        }
    }
    if in_facet {
        return Err("the file ends inside a facet".into());
    }
    if let Some(o) = cur.take()
        && !o.triangles.is_empty()
    {
        objects.push(o);
    }
    if objects.is_empty() {
        return Err("the ASCII STL has no facets".into());
    }
    Ok((objects, Vec::new()))
}
