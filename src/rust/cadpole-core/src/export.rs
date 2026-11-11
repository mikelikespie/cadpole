//! Mesh exporters: STL and OBJ, written directly (STEP is
//! `cadpole-export-step`, 3MF `cadpole-export-3mf`).
//!
//! Neither writes a **collapsed** triangle: one with two corners closer
//! together than [`mesh_kernel::WELD_EPS`], which is
//! [`crate::solid::SolidReport::collapsed`] and what
//! `cadpole_export_3mf::append` drops too, because the 3MF specification
//! requires a triangle's three vertices to be distinct. Such a triangle is a
//! zero normal describing no surface, and every format leaves it out alike.
//!
//! Dropping them cannot open a mesh, and in practice it closes one. A
//! collapsed triangle's three directed edges are `a->B`, `B->B` and `B->a`
//! once its two near corners are one vertex: a matched pair on `{a, B}` plus a
//! self-loop, so removing it leaves every edge's parity alone and takes the
//! spurious uses with it. Measured on the two Gridfinity parts, non-manifold
//! edges went 24 -> 0 and 118 -> 0.
//!
//! A *sliver* — three distinct corners that happen to be collinear — is kept.
//! It has no area either, and `cadpole check` reports it as `[degenerate]` just
//! the same, but its three edges are real edges that other triangles share, so
//! dropping it would leave a hole in a mesh that did not have one. Neither
//! Gridfinity part has a single one; the defect to fix there is whatever made
//! the triangle, not the writer.
//!
//! What this does change is the triangle count in the STL header, which is why
//! that is written last.

pub mod gltf;

use crate::mesh_kernel::WELD_EPS;
use crate::tessellate::Mesh;

/// Which dialect a 3MF file is written in: the `flavor` vocabulary.
///
/// `Standard` is the 3MF core specification with the slicer paint layers
/// — one object per body, one build item per instance. `Bambu` is
/// a Bambu Studio *project*: the Production extension's object files under
/// `3D/Objects/`, plates in `Metadata/model_settings.config`, a filament
/// colour list and plate thumbnails. Several `export()` calls to the same
/// path accumulate into one project under `Bambu`; under `Standard` a path
/// is one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flavor {
    #[default]
    Standard,
    Bambu,
}

impl Flavor {
    /// Every member, in the order the `flavor` vocabulary lists them.
    pub const ALL: [Flavor; 2] = [Flavor::Standard, Flavor::Bambu];

    pub fn name(self) -> &'static str {
        match self {
            Flavor::Standard => "standard",
            Flavor::Bambu => "bambu",
        }
    }

    /// The vocabulary's spellings and nothing else (the namespace
    /// lists exactly what the parser takes). Empty means the default.
    pub fn parse(s: &str) -> Result<Flavor, String> {
        match s {
            "" | "standard" => Ok(Flavor::Standard),
            "bambu" => Ok(Flavor::Bambu),
            other => Err(format!("unknown flavor {other:?}; valid: standard, bambu")),
        }
    }
}

/// Two of this triangle's corners are the same point, to within the distance
/// the mesh kernel welds at — so it would collapse to a line or a point and
/// has no surface to describe.
fn collapsed(v: &[[f32; 3]; 3]) -> bool {
    let close = |p: [f32; 3], q: [f32; 3]| {
        let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= WELD_EPS * WELD_EPS
    };
    close(v[0], v[1]) || close(v[1], v[2]) || close(v[0], v[2])
}

/// The three corner positions of triangle `t`.
fn corners(m: &Mesh, tri: &[u32]) -> [[f32; 3]; 3] {
    let p = |i: u32| {
        let i = i as usize * 3;
        [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
    };
    [p(tri[0]), p(tri[1]), p(tri[2])]
}

/// Binary STL of one or more meshes (units: mm, as everything in Cadpole).
pub fn stl(meshes: &[&Mesh]) -> Vec<u8> {
    let upper: usize = meshes.iter().map(|m| m.triangle_count()).sum();
    let mut out = Vec::with_capacity(84 + upper * 50);
    let mut header = [0u8; 80];
    let tag = b"cadpole binary stl";
    header[..tag.len()].copy_from_slice(tag);
    out.extend_from_slice(&header);
    // Patched once the degenerate triangles have been counted out.
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut written = 0u32;
    for m in meshes {
        for tri in m.indices.chunks_exact(3) {
            let v = corners(m, tri);
            if collapsed(&v) {
                continue;
            }
            let [a, b, c] = v;
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                n = [n[0] / l, n[1] / l, n[2] / l];
            }
            for f in n.iter().chain(a.iter()).chain(b.iter()).chain(c.iter()) {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&0u16.to_le_bytes());
            written += 1;
        }
    }
    out[80..84].copy_from_slice(&written.to_le_bytes());
    out
}

/// Wavefront OBJ with one object per mesh.
pub fn obj(meshes: &[(&str, &Mesh)]) -> String {
    let bodies: Vec<ObjBody<'_>> = meshes.iter().map(|(name, mesh)| ObjBody { name, layer: "", hidden: false, mesh }).collect();
    obj_layered(&bodies)
}

/// One body of an OBJ: its name, its display layer (empty for none), and
/// whether that layer is hidden.
pub struct ObjBody<'a> {
    pub name: &'a str,
    pub layer: &'a str,
    pub hidden: bool,
    pub mesh: &'a Mesh,
}

/// Wavefront OBJ with one object per body, `o <body>`, and the body's
/// display layer as its group, `g <layer>`: what Blender, three.js
/// and most DCC importers read as object and group. A layer name's spaces
/// become underscores, because `g a b` is two groups; a control character
/// (a line break above all) in a body or layer name becomes one too, so a
/// name can never write a statement. A body on a hidden
/// layer, written because the caller asked for hidden layers too, is marked
/// by a `# hidden layer` comment under its `o` line.
pub fn obj_layered(bodies: &[ObjBody<'_>]) -> String {
    use std::fmt::Write;
    let mut s = String::from("# cadpole\n");
    let mut base = 1usize;
    for b in bodies {
        let m = b.mesh;
        // A statement is a line: a line break (or any control character)
        // in a name would end the `o` line and start statements of its
        // own, so each becomes `_`. Spaces stay; `o` takes the whole line.
        let name: String = b.name.chars().map(|c| if c.is_control() { '_' } else { c }).collect();
        let _ = writeln!(s, "o {name}");
        if b.hidden {
            let _ = writeln!(s, "# hidden layer");
        }
        if !b.layer.is_empty() {
            let group: String = b.layer.chars().map(|c| if c.is_whitespace() || c.is_control() { '_' } else { c }).collect();
            let _ = writeln!(s, "g {group}");
        }
        for p in m.positions.chunks_exact(3) {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
        }
        for n in m.normals.chunks_exact(3) {
            let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
        }
        for t in m.indices.chunks_exact(3) {
            if collapsed(&corners(m, t)) {
                continue;
            }
            let (a, b, c) = (t[0] as usize + base, t[1] as usize + base, t[2] as usize + base);
            let _ = writeln!(s, "f {a}//{a} {b}//{b} {c}//{c}");
        }
        base += m.vertex_count();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> Mesh {
        Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            face_ids: vec![0],
            colors: Vec::new(),
        }
    }

    #[test]
    fn stl_has_header_count_and_one_record() {
        let bytes = stl(&[&tri()]);
        assert_eq!(bytes.len(), 84 + 50);
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 1);
        // normal is +Z
        let nz = f32::from_le_bytes(bytes[92..96].try_into().unwrap());
        assert!((nz - 1.0).abs() < 1e-6);
    }

    /// A box with one extra triangle whose second and third corners are the
    /// same point: the collapsed case, which is what the mesh kernel leaves
    /// behind on the Gridfinity parts.
    fn box_with_collapsed() -> Mesh {
        use crate::geom::Vec3;
        let mut m = crate::solid::fixtures::box_mesh(10.0, 10.0, 10.0);
        let p = Vec3::new(0.0, 0.0, 0.0);
        crate::solid::fixtures::push_tri(&mut m, Vec3::new(10.0, 0.0, 0.0), p, p, 99);
        m
    }

    #[test]
    fn neither_writer_puts_a_collapsed_triangle_in_the_file() {
        // The 3MF writer has always dropped these -- the specification
        // requires three distinct vertices -- so the same model exported a
        // cleaner 3MF than STL.
        let m = box_with_collapsed();
        assert_eq!(crate::solid::validate(&m).collapsed, 1, "the fixture has exactly one");

        let bytes = stl(&[&m]);
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(count, m.triangle_count() - 1, "the collapsed one is left out");
        // The count in the header is the count in the file: a reader that
        // trusts one and not the other must see the same number.
        assert_eq!(bytes.len(), 84 + 50 * count);
        // And no record has a zero normal.
        for i in 0..count {
            let at = 84 + 50 * i;
            let n: Vec<f32> = (0..3)
                .map(|k| f32::from_le_bytes(bytes[at + k * 4..at + k * 4 + 4].try_into().unwrap()))
                .collect();
            assert!(n.iter().any(|v| v.abs() > 0.5), "record {i} has normal {n:?}");
        }

        let s = obj(&[("b", &m)]);
        assert_eq!(s.matches("\nf ").count(), m.triangle_count() - 1);
        // The vertices stay: dropping a face must not renumber the rest.
        assert_eq!(s.matches("\nv ").count(), m.vertex_count());
    }

    #[test]
    fn a_sliver_is_kept_because_dropping_it_would_open_the_mesh() {
        // Three distinct corners in a line. It has no area and `cadpole check`
        // calls it degenerate too, but its three edges are shared with the
        // triangles around it, so leaving it out would make a hole where the
        // mesh had none. Only the collapsed case is the writer's business.
        let m = crate::solid::fixtures::box_with_degenerate(10.0, 10.0, 10.0);
        let r = crate::solid::validate(&m);
        assert_eq!(r.collapsed, 0, "distinct corners: nothing collapses");
        assert_eq!(r.degenerate.len(), 1, "but it still has no area");
        let bytes = stl(&[&m]);
        assert_eq!(
            u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize,
            m.triangle_count()
        );
    }

    #[test]
    fn obj_groups_each_body_by_its_layer() {
        let t = tri();
        let s = obj_layered(&[
            ObjBody { name: "deck", layer: "site work", hidden: false, mesh: &t },
            ObjBody { name: "fence", layer: "neighbours", hidden: true, mesh: &t },
            ObjBody { name: "loose", layer: "", hidden: false, mesh: &t },
        ]);
        assert!(s.contains("o deck\ng site_work\nv "), "{s}");
        assert!(s.contains("o fence\n# hidden layer\ng neighbours\nv "), "{s}");
        assert!(s.contains("o loose\nv "), "a body on no layer has no group: {s}");
        // The second body's faces index past the first body's vertices.
        assert!(s.contains("f 4//4 5//5 6//6"), "{s}");
    }

    #[test]
    fn obj_lists_vertices_and_faces() {
        let s = obj(&[("t", &tri())]);
        assert!(s.contains("o t\n"));
        assert_eq!(s.matches("\nv ").count(), 3);
        assert!(s.contains("f 1//1 2//2 3//3"));
    }

    /// A body name or layer is data (a script string, an imported file's
    /// label); a line break in one wrote `v` lines of its own into the file
    /// and shifted every later face index.
    #[test]
    fn obj_names_cannot_write_statements() {
        let s = obj_layered(&[
            ObjBody { name: "p\nv 100 100 100\r\nv 200 200 200", layer: "a\u{b}b\u{1b}[31m", hidden: false, mesh: &tri() },
            ObjBody { name: "q", layer: "", hidden: false, mesh: &tri() },
        ]);
        assert_eq!(s.lines().filter(|l| l.starts_with("v ")).count(), 6, "{s}");
        assert!(s.contains("o p_v 100 100 100__v 200 200 200\n"), "{s}");
        assert!(s.contains("g a_b_[31m\n"), "{s}");
        assert!(!s.chars().any(|c| c.is_control() && c != '\n'), "{s:?}");
        assert!(s.contains("f 4//4 5//5 6//6"), "{s}");
    }
}
