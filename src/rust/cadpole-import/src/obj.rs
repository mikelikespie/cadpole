//! Wavefront OBJ into [`RawObject`]s.
//!
//! What is read: `v` (positions; a fourth `w` is ignored, and the common
//! vertex-colour extension `v x y z r g b` — what MeshLab, Open3D and a
//! phone scanner write — is read as a colour per vertex), `f`
//! (faces, in every index form — `a`, `a/b`, `a//c`, `a/b/c`, negative
//! indices counted back from the last vertex — with polygons
//! fan-triangulated), `o` (a new object) and `g` (a group within it, which
//! becomes a named face). What is ignored, deliberately: `vn` and `vt`
//! (normals are recomputed from the triangles, texture coordinates have no
//! meaning here), `usemtl`/`mtllib` (colour is `paint`'s business), `s`,
//! `l`, `p` and every other keyword. Vertices are global to the file; each
//! object takes the ones it uses, in the order it first uses them.
//!
//! **Vertex colours.** Six numbers on a `v` line are `x y z r g b`, seven
//! `x y z r g b a` (the alpha is not used). The extension has no
//! specification; the writers that use it put display (sRGB) values in
//! 0..1, and a file whose colours pass 1 is read as 0..255. A file where
//! only some vertices carry a colour gives the rest white, and says so.

use crate::Note;
use crate::mesh::{Appearance, RawObject};
use crate::prescan::{self, MAX_TRIANGLES, MAX_VERTICES};

struct Obj {
    name: String,
    /// Global vertex indices.
    tris: Vec<[usize; 3]>,
    tri_group: Vec<u32>,
    groups: Vec<String>,
}

impl Obj {
    fn new(name: String) -> Obj {
        Obj { name, tris: Vec::new(), tri_group: Vec::new(), groups: Vec::new() }
    }
}

/// Parse `bytes` as OBJ. `hint` names the file, for diagnostics.
pub fn parse(bytes: &[u8], _hint: &str) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    prescan::text_lines(bytes)?;
    // A UTF-8 byte order mark is not whitespace to `trim`: left in, it
    // makes the first token `\u{feff}v` and the first line is read past.
    let s = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes));
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    // `v x y z r g b`: the colour of every vertex, raw, and how many had one.
    let mut colors: Vec<[f32; 3]> = Vec::new();
    let mut colored = 0usize;
    let mut not_colors = 0usize;
    let mut color_max = 0f32;
    let mut objects: Vec<Obj> = Vec::new();
    let mut cur = Obj::new(String::new());
    let mut group: Option<u32> = None;
    let mut polygons = 0usize;
    let mut total = 0usize;
    for (i, raw) in s.lines().enumerate() {
        let ln = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tok = line.split_whitespace();
        let Some(head) = tok.next() else { continue };
        match head {
            "v" => {
                let mut p = [0.0f64; 3];
                for (k, slot) in p.iter_mut().enumerate() {
                    let t = tok.next().ok_or_else(|| format!("line {ln}: a vertex needs three coordinates, found {k}"))?;
                    *slot = t.parse::<f64>().map_err(|_| format!("line {ln}: {t:?} is not a number"))?;
                    if !slot.is_finite() {
                        return Err(format!("line {ln}: coordinate {t:?} is not finite"));
                    }
                }
                // What follows: nothing, `w`, or a colour (`r g b`, `r g b a`).
                let rest: Vec<&str> = tok.collect();
                let mut rgb = [f32::NAN; 3];
                if rest.len() == 3 || rest.len() == 4 {
                    let parsed: Vec<f32> = rest[..3].iter().filter_map(|t| t.parse::<f32>().ok()).filter(|c| c.is_finite() && *c >= 0.0).collect();
                    if parsed.len() == 3 {
                        rgb = [parsed[0], parsed[1], parsed[2]];
                        color_max = color_max.max(rgb[0]).max(rgb[1]).max(rgb[2]);
                        colored += 1;
                    } else {
                        // Not a colour (a writer's own extra numbers): the
                        // position stands, the rest is read past.
                        not_colors += 1;
                    }
                }
                colors.push(rgb);
                vertices.push(p);
                if vertices.len() > MAX_VERTICES {
                    return Err(format!("more than {MAX_VERTICES} vertices"));
                }
            }
            "f" => {
                let mut idx: Vec<usize> = Vec::with_capacity(4);
                for t in tok {
                    let first = t.split('/').next().unwrap_or("");
                    let n: i64 = first.parse().map_err(|_| format!("line {ln}: {t:?} is not a vertex index"))?;
                    let v = if n > 0 {
                        (n - 1) as usize
                    } else if n < 0 {
                        let back = (-n) as usize;
                        if back > vertices.len() {
                            return Err(format!(
                                "line {ln}: index {n} counts back past the first vertex ({} so far)",
                                vertices.len()
                            ));
                        }
                        vertices.len() - back
                    } else {
                        return Err(format!("line {ln}: vertex index 0 (OBJ indices start at 1)"));
                    };
                    if v >= vertices.len() {
                        return Err(format!(
                            "line {ln}: vertex index {n} is past the end ({} vertices so far)",
                            vertices.len()
                        ));
                    }
                    idx.push(v);
                }
                if idx.len() < 3 {
                    return Err(format!("line {ln}: a face needs at least three vertices, found {}", idx.len()));
                }
                if idx.len() > 3 {
                    polygons += 1;
                }
                let g = match group {
                    Some(g) => g,
                    None => {
                        if cur.groups.is_empty() {
                            0
                        } else {
                            // Faces before any `g` in an object that has
                            // groups: OBJ's own default group.
                            let g = group_index(&mut cur, "default");
                            group = Some(g);
                            g
                        }
                    }
                };
                for k in 1..idx.len() - 1 {
                    cur.tris.push([idx[0], idx[k], idx[k + 1]]);
                    cur.tri_group.push(g);
                    total += 1;
                    if total > MAX_TRIANGLES {
                        return Err(format!("more than {MAX_TRIANGLES} triangles"));
                    }
                }
            }
            "o" => {
                if !cur.tris.is_empty() {
                    objects.push(cur);
                }
                cur = Obj::new(line[1..].trim().to_owned());
                group = None;
            }
            "g" => {
                let name = line[1..].trim();
                let name = if name.is_empty() { "default" } else { name };
                // Triangles already in the object without a group go into
                // `default`, so the group list stays consistent.
                if cur.groups.is_empty() && !cur.tris.is_empty() {
                    let d = group_index(&mut cur, "default");
                    for g in &mut cur.tri_group {
                        *g = d;
                    }
                }
                group = Some(group_index(&mut cur, name));
            }
            // Normals, texture coordinates, materials, smoothing groups,
            // lines, points, curves: read past.
            _ => {}
        }
    }
    if !cur.tris.is_empty() {
        objects.push(cur);
    }
    if objects.is_empty() {
        return Err(if vertices.is_empty() {
            "the OBJ has no `v` and no `f` lines".to_owned()
        } else {
            format!("the OBJ has {} vertices but no faces", vertices.len())
        });
    }
    let mut notes = Vec::new();
    // 0..1 unless a colour passes 1; then 0..255.
    let scale = if color_max > 1.0 { 1.0 / 255.0 } else { 1.0 };
    if colored > 0 {
        notes.push(Note::info(format!(
            "vertex colours (`v x y z r g b`) on {colored} of {} vertices, read as sRGB {}{}",
            vertices.len(),
            if scale == 1.0 { "0..1" } else { "0..255" },
            if colored < vertices.len() { "; the vertices without one are white" } else { "" }
        )));
    }
    if not_colors > 0 {
        notes.push(Note::info(format!(
            "{not_colors} `v` line{} carried three or four more numbers that are not a colour (0..1 or 0..255); read past",
            if not_colors == 1 { "" } else { "s" }
        )));
    }
    if polygons > 0 {
        notes.push(Note::info(format!(
            "{polygons} polygon{} with more than three sides were fan-triangulated",
            if polygons == 1 { "" } else { "s" }
        )));
    }
    let mut out = Vec::with_capacity(objects.len());
    let byte = |c: f32| -> u8 { if c.is_nan() { 255 } else { (c * scale * 255.0).round().clamp(0.0, 255.0) as u8 } };
    // Global vertex -> this object's index, `u32::MAX` for unused. One
    // table for every object, reset after each from the object's own
    // triangles: a table per object costs objects x vertices, which a file
    // of many small objects over a large vertex list makes minutes.
    let mut local: Vec<u32> = vec![u32::MAX; vertices.len()];
    for o in objects {
        // Local vertices in first-use order.
        let mut positions: Vec<[f64; 3]> = Vec::new();
        let mut kept: Vec<[u8; 3]> = Vec::new();
        let mut triangles = Vec::with_capacity(o.tris.len());
        for t in &o.tris {
            let mut tri = [0u32; 3];
            for (k, &v) in t.iter().enumerate() {
                if local[v] == u32::MAX {
                    local[v] = positions.len() as u32;
                    positions.push(vertices[v]);
                    if colored > 0 {
                        kept.push(colors[v].map(byte));
                    }
                }
                tri[k] = local[v];
            }
            triangles.push(tri);
        }
        for t in &o.tris {
            for &v in t {
                local[v] = u32::MAX;
            }
        }
        // A body that carries its colour per vertex is white underneath,
        // so a viewer's base colour does not tint it.
        let appearance = (colored > 0).then(Appearance::default);
        out.push(RawObject {
            name: o.name,
            positions,
            triangles,
            tri_group: o.tri_group,
            groups: o.groups,
            colors: kept,
            appearance,
            ..Default::default()
        });
    }
    Ok((out, notes))
}

fn group_index(o: &mut Obj, name: &str) -> u32 {
    match o.groups.iter().position(|g| g == name) {
        Some(i) => i as u32,
        None => {
            o.groups.push(name.to_owned());
            (o.groups.len() - 1) as u32
        }
    }
}
