//! A section render: every body cut by a plane, for the image only.
//!
//! `cadpole render --section z=40` shows a planter's wall thickness, a house
//! mass's floor plates, a bracket's bore — what the drawing pipeline's
//! `section:z=40` draws in 2D (`cadpole_drawing::section_loops`), as a shaded
//! picture. The cut is the mesh kernel's: each body's display mesh is
//! intersected with a half-space box through manifold, so the cut face is
//! capped and a solid reads as solid rather than as a hollow shell seen
//! from inside. Nothing in the document changes; the scene the renderer
//! sees is the only thing cut. A body manifold refuses (not watertight
//! after welding) is drawn whole, and the caller says so.

use crate::geom::{Plane, Vec3};
use crate::mesh_kernel::{from_manifold, to_manifold};
use crate::tessellate::Mesh;
use manifold_rust::types::OpType;

/// `--section x=22`: the axis, the coordinate, and which side is cut away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// 0, 1, 2 for x, y, z.
    pub axis: usize,
    pub at: f64,
    /// The side that is removed: `true` cuts away the positive side (the
    /// default, `x=22`), `false` the negative side (`-x=22`).
    pub remove_positive: bool,
}

impl Section {
    /// `x=22`, `y=-5`, `z=40`, or `-x=22` to keep the other side. Empty is
    /// no section.
    pub fn parse(s: &str) -> Result<Option<Section>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        let (remove_positive, rest) = match s.strip_prefix('-') {
            Some(r) => (false, r),
            None => (true, s),
        };
        let (axis, value) = rest
            .split_once('=')
            .ok_or_else(|| format!("section {s:?}: expected <axis>=<value>, as in z=40 or -x=22"))?;
        let axis = match axis.trim() {
            "x" | "X" => 0,
            "y" | "Y" => 1,
            "z" | "Z" => 2,
            other => return Err(format!("section {s:?}: the axis must be x, y or z, not {other:?}")),
        };
        let at: f64 = value.trim().parse().map_err(|_| format!("section {s:?}: {:?} is not a number", value.trim()))?;
        if !at.is_finite() {
            return Err(format!("section {s:?}: the coordinate must be finite"));
        }
        Ok(Some(Section { axis, at, remove_positive }))
    }

    /// The cutting plane, its normal pointing into the removed side.
    pub fn plane(&self) -> Plane {
        let mut n = Vec3::ZERO;
        match self.axis {
            0 => n.x = 1.0,
            1 => n.y = 1.0,
            _ => n.z = 1.0,
        }
        if !self.remove_positive {
            n = -n;
        }
        let mut origin = Vec3::ZERO;
        match self.axis {
            0 => origin.x = self.at,
            1 => origin.y = self.at,
            _ => origin.z = self.at,
        }
        let x_dir = if self.axis == 0 { Vec3::Y } else { Vec3::X };
        Plane { origin, normal: n, x_dir }
    }

    /// `x=22` / `-x=22`, for a label.
    pub fn label(&self) -> String {
        format!("{}{}={}", if self.remove_positive { "" } else { "-" }, ["x", "y", "z"][self.axis.min(2)], self.at)
    }
}

/// What the cut did to one mesh.
#[derive(Debug)]
pub enum Cut {
    /// The mesh cut and capped.
    Mesh(Mesh),
    /// Nothing of it lies on the kept side.
    Gone,
    /// The plane misses it: kept as it was.
    Untouched,
    /// The mesh kernel could not cut it (the reason); kept as it was.
    Refused(String),
}

/// A closed box mesh over `lo..hi`, outward normals, one face id per side.
fn box_mesh(lo: Vec3, hi: Vec3, face_base: u32) -> Mesh {
    let c = |x: f64, y: f64, z: f64| [x as f32, y as f32, z as f32];
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, -1.0], [c(lo.x, lo.y, lo.z), c(lo.x, hi.y, lo.z), c(hi.x, hi.y, lo.z), c(hi.x, lo.y, lo.z)]),
        ([0.0, 0.0, 1.0], [c(lo.x, lo.y, hi.z), c(hi.x, lo.y, hi.z), c(hi.x, hi.y, hi.z), c(lo.x, hi.y, hi.z)]),
        ([0.0, -1.0, 0.0], [c(lo.x, lo.y, lo.z), c(hi.x, lo.y, lo.z), c(hi.x, lo.y, hi.z), c(lo.x, lo.y, hi.z)]),
        ([0.0, 1.0, 0.0], [c(lo.x, hi.y, lo.z), c(lo.x, hi.y, hi.z), c(hi.x, hi.y, hi.z), c(hi.x, hi.y, lo.z)]),
        ([-1.0, 0.0, 0.0], [c(lo.x, lo.y, lo.z), c(lo.x, lo.y, hi.z), c(lo.x, hi.y, hi.z), c(lo.x, hi.y, lo.z)]),
        ([1.0, 0.0, 0.0], [c(hi.x, lo.y, lo.z), c(hi.x, hi.y, lo.z), c(hi.x, hi.y, hi.z), c(hi.x, lo.y, hi.z)]),
    ];
    let mut mesh = Mesh::default();
    for (fi, (n, quad)) in faces.iter().enumerate() {
        let base = (mesh.positions.len() / 3) as u32;
        for p in quad {
            mesh.positions.extend_from_slice(p);
            mesh.normals.extend_from_slice(n);
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        let id = face_base + fi as u32;
        mesh.face_ids.extend_from_slice(&[id, id]);
    }
    mesh
}

/// Cut `mesh` by `section`, keeping the side the section keeps, capped.
/// `tolerance` is the tessellation tolerance the mesh was made at.
pub fn cut(mesh: &Mesh, section: &Section, tolerance: f64) -> Cut {
    let Some((lo, hi)) = mesh.bbox() else { return Cut::Untouched };
    let (min, max) = match section.axis {
        0 => (lo.x, hi.x),
        1 => (lo.y, hi.y),
        _ => (lo.z, hi.z),
    };
    // Entirely on one side: no kernel call needed, and no chance of the
    // kernel refusing a body the plane never touches.
    let all_kept = if section.remove_positive { max <= section.at } else { min >= section.at };
    let all_gone = if section.remove_positive { min >= section.at } else { max <= section.at };
    if all_kept {
        return Cut::Untouched;
    }
    if all_gone {
        return Cut::Gone;
    }
    // The kept half-space as a box well clear of the body on every other
    // side, and reaching from the plane away from the removed side.
    let pad = ((hi - lo).length() + 1.0) * 2.0;
    let (mut blo, mut bhi) = (lo - Vec3::new(pad, pad, pad), hi + Vec3::new(pad, pad, pad));
    let set = |v: &mut Vec3, axis: usize, x: f64| match axis {
        0 => v.x = x,
        1 => v.y = x,
        _ => v.z = x,
    };
    if section.remove_positive {
        set(&mut bhi, section.axis, section.at);
    } else {
        set(&mut blo, section.axis, section.at);
    }
    let n_faces = mesh.face_ids.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let keep = box_mesh(blo, bhi, n_faces);
    let a = match to_manifold(mesh, 0, tolerance) {
        Ok(m) => m,
        Err(e) => return Cut::Refused(e.to_string()),
    };
    let b = match to_manifold(&keep, 0, tolerance) {
        Ok(m) => m,
        Err(e) => return Cut::Refused(e.to_string()),
    };
    let out = a.boolean(&b, OpType::Intersect);
    match from_manifold(&out, &[], tolerance) {
        Ok((m, _)) if m.indices.is_empty() => Cut::Gone,
        Ok((mut m, topo)) => {
            // Manifold numbers the result's faces afresh; with no named
            // groups each face is named by the provenance id it came from
            // (`face:<id>`, a `#k` after it for one of several pieces),
            // which is the operand's own face id. Put those back, so what
            // the renderer keys by face (paint, texture frames, the
            // highlight) lands where it did before the cut.
            let ids: Vec<Option<u32>> = topo
                .faces
                .iter()
                .map(|f| crate::naming::Kind::Face.parse_index(f.name.split('#').next().unwrap_or("")).map(|i| i as u32))
                .collect();
            for f in &mut m.face_ids {
                if let Some(Some(id)) = ids.get(*f as usize) {
                    *f = *id;
                }
            }
            Cut::Mesh(m)
        }
        Err(e) => Cut::Refused(e.to_string()),
    }
}

/// Clip a polyline to the kept side of the section: the runs on the kept
/// side, each ending where it crosses the plane.
pub fn clip_polyline(poly: &[Vec3], section: &Section) -> Vec<Vec<Vec3>> {
    let coord = |p: Vec3| match section.axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    };
    let kept = |p: Vec3| if section.remove_positive { coord(p) <= section.at } else { coord(p) >= section.at };
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut run: Vec<Vec3> = Vec::new();
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ka, kb) = (kept(a), kept(b));
        if ka {
            if run.is_empty() {
                run.push(a);
            }
        }
        if ka != kb {
            let da = coord(a) - section.at;
            let db = coord(b) - section.at;
            let t = if (da - db).abs() > 1e-12 { da / (da - db) } else { 0.5 };
            let x = a + (b - a) * t;
            if ka {
                run.push(x);
                out.push(std::mem::take(&mut run));
            } else {
                run.push(x);
            }
        }
        if kb {
            run.push(b);
        }
    }
    if run.len() >= 2 {
        out.push(run);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::testing::box_mesh as test_box;

    #[test]
    fn the_spec_parses_and_names_its_plane() {
        assert_eq!(Section::parse("").unwrap(), None);
        let s = Section::parse("z=40").unwrap().unwrap();
        assert_eq!(s, Section { axis: 2, at: 40.0, remove_positive: true });
        assert_eq!(s.plane().normal, Vec3::Z);
        assert_eq!(s.plane().origin, Vec3::new(0.0, 0.0, 40.0));
        assert_eq!(s.label(), "z=40");
        let s = Section::parse("-x=22.5").unwrap().unwrap();
        assert_eq!(s, Section { axis: 0, at: 22.5, remove_positive: false });
        assert_eq!(s.plane().normal, -Vec3::X);
        assert_eq!(s.label(), "-x=22.5");
        for bad in ["w=1", "x", "x=abc", "x=inf"] {
            assert!(Section::parse(bad).is_err(), "{bad}");
        }
    }

    /// A 10 mm cube cut at z = 4 keeps the bottom 4 mm, capped: the volume
    /// is 400 mm³, the mesh is closed, and every vertex is at or below the
    /// plane. Cut the other way it keeps the top 6 mm. A plane that misses
    /// leaves the mesh untouched; one past it removes it.
    #[test]
    fn a_cube_is_cut_and_capped() {
        let cube = test_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let below = cut(&cube, &Section::parse("z=4").unwrap().unwrap(), 0.05);
        let Cut::Mesh(m) = below else { panic!("{below:?}") };
        assert!((m.volume() - 400.0).abs() < 1e-3, "{}", m.volume());
        assert!(m.welded(1e-4).is_watertight(), "the cut is capped");
        assert!(m.positions.chunks_exact(3).all(|p| p[2] <= 4.0 + 1e-4));
        let above = cut(&cube, &Section::parse("-z=4").unwrap().unwrap(), 0.05);
        let Cut::Mesh(m) = above else { panic!("{above:?}") };
        assert!((m.volume() - 600.0).abs() < 1e-3, "{}", m.volume());
        assert!(m.positions.chunks_exact(3).all(|p| p[2] >= 4.0 - 1e-4));
        assert!(matches!(cut(&cube, &Section::parse("z=11").unwrap().unwrap(), 0.05), Cut::Untouched));
        assert!(matches!(cut(&cube, &Section::parse("z=-1").unwrap().unwrap(), 0.05), Cut::Gone));
        // A single open quad is not a solid: refused, not cut.
        let sheet = crate::render::testing::quad([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 10.0], [0.0, 10.0, 10.0]);
        assert!(matches!(cut(&sheet, &Section::parse("z=4").unwrap().unwrap(), 0.05), Cut::Refused(_)));
    }

    /// A cut keeps the face ids the triangles came from, so whatever the
    /// renderer keys by face (paint, machined-texture frames, the selection
    /// highlight) still lands on the same faces; the cap gets ids of its own
    /// past the mesh's. Manifold numbers its output faces afresh, and the
    /// cut used to hand that numbering on.
    #[test]
    fn a_cut_keeps_the_face_ids_it_came_from() {
        let cube = test_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let n_faces = cube.face_ids.iter().copied().max().unwrap() + 1;
        let id_with_normal = |m: &Mesh, n: [f32; 3]| -> Vec<u32> {
            let mut ids: Vec<u32> = (0..m.triangle_count())
                .filter(|&t| {
                    let i = m.indices[t * 3] as usize * 3;
                    (0..3).all(|k| (m.normals[i + k] - n[k]).abs() < 1e-3)
                })
                .map(|t| m.face_ids[t])
                .collect();
            ids.sort();
            ids.dedup();
            ids
        };
        let Cut::Mesh(m) = cut(&cube, &Section::parse("z=4").unwrap().unwrap(), 0.05) else { panic!() };
        for n in [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]] {
            let want = id_with_normal(&cube, n);
            assert_eq!(want.len(), 1, "the test box has one face per side");
            assert_eq!(id_with_normal(&m, n), want, "the face with normal {n:?}");
        }
        let cap = id_with_normal(&m, [0.0, 0.0, 1.0]);
        assert!(!cap.is_empty() && cap.iter().all(|&f| f >= n_faces), "the cap is a face of its own: {cap:?}");
    }

    #[test]
    fn polylines_are_clipped_at_the_plane() {
        let s = Section::parse("x=5").unwrap().unwrap();
        let line = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)];
        let runs = clip_polyline(&line, &s);
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[0], vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0)]);
        assert_eq!(runs[1], vec![Vec3::new(5.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)]);
        assert!(clip_polyline(&line[1..3], &s).is_empty());
    }
}
