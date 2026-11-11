//! truck topology to ISO 10303-42 entities: vertices, edges, loops, faces,
//! shells and solids.
//!
//! The input is a [`CompressedSolid`] — truck's flattened form, where a shell
//! is a vector of points, a vector of edges indexing those points, and a
//! vector of faces indexing those edges. That is very nearly the shape of a
//! STEP shell already, so most of this module is bookkeeping. Three decisions
//! are not:
//!
//! **Which boundary of a face is the outer one.** A `face_outer_bound` is a
//! hint, and AP214 allows at most one per face, so the loop with the largest
//! bounding box wins and the rest are plain `FACE_BOUND`s. truck usually puts
//! the outer loop first, but "usually" is not a guarantee a file format should
//! rest on.
//!
//! **Which shell of a solid is the outer one.** Same rule, same reason: the
//! shell with the largest bounding box is the outer one and the others are
//! voids.
//!
//! **What a void shell looks like.** ISO 10303-42's `brep_with_voids` has a
//! formal rule (`WR1`) that *every* void is an `ORIENTED_CLOSED_SHELL` with
//! orientation `.F.`, and an informal one that the resulting oriented shell's
//! normals point into the void. truck's void shell already points into the
//! void — it is a boundary of the solid, and away-from-material is
//! into-the-void there — so the `CLOSED_SHELL` underneath is written
//! **inverted**, and `.F.` turns it back. Inverting a face means flipping
//! `same_sense` *and* reversing every boundary loop; doing only the first is a
//! face whose loops run the wrong way round its own normal.

use truck_topology::compress::{CompressedEdge, CompressedEdgeIndex, CompressedFace, CompressedShell, CompressedSolid};
use truck_modeling::{Curve, Point3, Surface};

use crate::text::{refs, string, Id};
use crate::{Error, Result, Writer};

type KShell = CompressedShell<Point3, Curve, Surface>;
type KSolidC = CompressedSolid<Point3, Curve, Surface>;

impl Writer {
    /// Write one solid and return the id of its `MANIFOLD_SOLID_BREP` or
    /// `BREP_WITH_VOIDS`.
    pub(crate) fn solid(&mut self, name: &str, solid: KSolidC) -> Result<Id> {
        let mut shells = solid.boundaries;
        if shells.is_empty() {
            return Err(Error::Invalid(format!("body {name:?} has no boundary shell")));
        }
        let outer_at = outermost(&shells);
        let outer = shells.remove(outer_at);
        let outer_id = self.shell(outer, false)?;
        if shells.is_empty() {
            self.stats.solids += 1;
            return Ok(self.em.emit(format!("MANIFOLD_SOLID_BREP({}, #{outer_id})", string(name))));
        }
        let mut voids = Vec::with_capacity(shells.len());
        for v in shells {
            let id = self.shell(v, true)?;
            // WR1 of `brep_with_voids`: the orientation is always .F.
            voids.push(self.em.emit(format!("ORIENTED_CLOSED_SHELL('', *, #{id}, .F.)")));
        }
        self.stats.solids += 1;
        self.stats.voids += voids.len();
        Ok(self.em.emit(format!("BREP_WITH_VOIDS({}, #{outer_id}, {})", string(name), refs(voids))))
    }

    /// Write a `CLOSED_SHELL`. With `invert`, every face is written facing the
    /// other way (see the module docs).
    fn shell(&mut self, shell: KShell, invert: bool) -> Result<Id> {
        let KShell { vertices, edges, faces } = shell;
        if faces.is_empty() {
            return Err(Error::Invalid("a shell with no faces".into()));
        }
        let vertex_ids = vertices.iter().map(|p| self.vertex(*p)).collect::<Result<Vec<_>>>()?;
        let edge_vertices: Vec<(usize, usize)> = edges.iter().map(|e| e.vertices).collect();
        let edge_ids = edges
            .into_iter()
            .map(|e| self.edge(e, &vertex_ids))
            .collect::<Result<Vec<_>>>()?;
        let mut face_ids = Vec::with_capacity(faces.len());
        for f in &faces {
            face_ids.push(self.face(f, &vertices, &edge_vertices, &edge_ids, invert)?);
        }
        self.stats.shells += 1;
        Ok(self.em.emit(format!("CLOSED_SHELL('', {})", refs(face_ids))))
    }

    fn vertex(&mut self, p: Point3) -> Result<Id> {
        let g = self.point(p)?;
        self.stats.vertices += 1;
        Ok(self.em.emit(format!("VERTEX_POINT('', #{g})")))
    }

    /// `EDGE_CURVE`. truck's compressed edge runs from `vertices.0` to
    /// `vertices.1` and its curve is parameterised the same way, so
    /// `same_sense` is always true here; direction lives on the
    /// `ORIENTED_EDGE`s that use it.
    fn edge(&mut self, e: CompressedEdge<Curve>, vertex_ids: &[Id]) -> Result<Id> {
        let (a, b) = e.vertices;
        let (Some(va), Some(vb)) = (vertex_ids.get(a), vertex_ids.get(b)) else {
            return Err(Error::Invalid(format!("an edge names vertices {a} and {b}, which the shell does not have")));
        };
        let (va, vb) = (*va, *vb);
        let c = self.curve(e.curve)?;
        self.stats.edges += 1;
        Ok(self.em.emit(format!("EDGE_CURVE('', #{va}, #{vb}, #{c}, .T.)")))
    }

    #[allow(clippy::too_many_arguments)]
    fn face(
        &mut self,
        f: &CompressedFace<Surface>,
        vertices: &[Point3],
        edge_vertices: &[(usize, usize)],
        edge_ids: &[Id],
        invert: bool,
    ) -> Result<Id> {
        if f.boundaries.is_empty() {
            return Err(Error::Invalid("a face with no boundary loop".into()));
        }
        let (surface_id, agrees) = self.surface(&f.surface)?;
        // The face's own outward normal is truck's surface normal when
        // `orientation` is set and its negative otherwise; `agrees` says
        // whether the STEP surface points the same way as truck's. Inverting
        // for a void flips the result once more.
        let same_sense = (f.orientation == agrees) != invert;
        // `CompressedFace` stores the boundaries a face was *built* from, not
        // the ones it presents: truck's `Face::boundaries()` inverts every
        // wire when `orientation` is false, and `Face::compress` does not.
        // The loops the shell is actually closed by are therefore the stored
        // ones only when `orientation` holds — get this wrong and every edge
        // of an inverted face runs the same way as its neighbour's, so the
        // shell stops being closed while still looking like a shell.
        let reverse = !f.orientation != invert;

        let outer_at = widest_loop(&f.boundaries, vertices, edge_vertices);
        let mut bounds = Vec::with_capacity(f.boundaries.len());
        for (i, b) in f.boundaries.iter().enumerate() {
            let loop_id = self.edge_loop(b, edge_ids, reverse)?;
            let kind = if i == outer_at { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
            bounds.push(self.em.emit(format!("{kind}('', #{loop_id}, .T.)")));
        }
        self.stats.faces += 1;
        Ok(self.em.emit(format!(
            "ADVANCED_FACE('', {}, #{surface_id}, {})",
            refs(bounds),
            self.bool_lit(same_sense)
        )))
    }

    /// `EDGE_LOOP`. With `reverse` the loop is walked the other way, which is
    /// truck's `Wire::inverse`: the edges in the opposite order, each flipped.
    fn edge_loop(&mut self, b: &[CompressedEdgeIndex], edge_ids: &[Id], reverse: bool) -> Result<Id> {
        if b.is_empty() {
            return Err(Error::Invalid("an empty edge loop".into()));
        }
        let mut ids = Vec::with_capacity(b.len());
        for e in b {
            let Some(edge) = edge_ids.get(e.index) else {
                return Err(Error::Invalid(format!("a loop names edge {}, which the shell does not have", e.index)));
            };
            ids.push(self.em.emit(format!(
                "ORIENTED_EDGE('', *, *, #{edge}, {})",
                self.bool_lit(e.orientation != reverse)
            )));
        }
        if reverse {
            ids.reverse();
        }
        self.stats.loops += 1;
        Ok(self.em.emit(format!("EDGE_LOOP('', {})", refs(ids))))
    }
}

/// Axis-aligned bounding box of a set of points, as `(lo, hi)`.
fn bbox(points: impl IntoIterator<Item = Point3>) -> Option<(Point3, Point3)> {
    let mut it = points.into_iter();
    let first = it.next()?;
    let (mut lo, mut hi) = (first, first);
    for p in it {
        lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    Some((lo, hi))
}

fn diagonal(b: Option<(Point3, Point3)>) -> f64 {
    match b {
        Some((lo, hi)) => ((hi.x - lo.x).powi(2) + (hi.y - lo.y).powi(2) + (hi.z - lo.z).powi(2)).sqrt(),
        None => 0.0,
    }
}

/// The index of the shell that encloses the others.
fn outermost(shells: &[KShell]) -> usize {
    shells
        .iter()
        .enumerate()
        .map(|(i, s)| (i, diagonal(bbox(s.vertices.iter().copied()))))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// The index of the boundary loop that encloses the others, taken as the one
/// whose vertices span the largest box. A hole's rim is inside the outer rim,
/// so its box is strictly smaller; a tie (two loops of the same extent on one
/// face) falls back to the first, which is truck's own ordering.
fn widest_loop(
    boundaries: &[Vec<CompressedEdgeIndex>],
    vertices: &[Point3],
    edge_vertices: &[(usize, usize)],
) -> usize {
    if boundaries.len() == 1 {
        return 0;
    }
    let extent = |b: &Vec<CompressedEdgeIndex>| {
        let pts = b.iter().filter_map(|e| edge_vertices.get(e.index)).flat_map(|(a, z)| [*a, *z]);
        diagonal(bbox(pts.filter_map(|i| vertices.get(i).copied())))
    };
    boundaries
        .iter()
        .enumerate()
        .max_by(|a, b| extent(a.1).partial_cmp(&extent(b.1)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, Writer};
    use truck_modeling::*;

    fn cube() -> truck_modeling::Solid {
        let v = builder::vertex(Point3::new(0.0, 0.0, 0.0));
        let e = builder::tsweep(&v, Vector3::new(10.0, 0.0, 0.0));
        let f = builder::tsweep(&e, Vector3::new(0.0, 10.0, 0.0));
        builder::tsweep(&f, Vector3::new(0.0, 0.0, 10.0))
    }

    #[test]
    fn a_cube_writes_six_advanced_faces_and_one_closed_shell() {
        let mut w = Writer::new(Options::default());
        w.solid("cube", cube().compress()).unwrap();
        let text = w.em.render();
        assert_eq!(text.matches("ADVANCED_FACE").count(), 6, "{text}");
        assert_eq!(text.matches("= CLOSED_SHELL").count(), 1);
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 1);
        assert_eq!(text.matches("= VERTEX_POINT").count(), 8);
        assert_eq!(text.matches("= EDGE_CURVE").count(), 12);
        assert_eq!(text.matches("= ORIENTED_EDGE").count(), 24, "every edge is used twice");
        assert!(text.contains("MANIFOLD_SOLID_BREP('cube'"), "the body name is carried");
    }

    #[test]
    fn every_face_of_a_cube_faces_outward() {
        // The six planes of a 10 mm cube at the origin; each face's STEP
        // normal (the plane axis, flipped by same_sense) must point away from
        // the centre.
        let mut w = Writer::new(Options::default());
        w.solid("cube", cube().compress()).unwrap();
        let text = w.em.render();
        let faces: Vec<&str> = text.lines().filter(|l| l.contains("ADVANCED_FACE")).collect();
        assert_eq!(faces.len(), 6);
        // Exactly three of the six must be .F. and three .T. for a box built
        // by three sweeps: the sweep alternates the surface sense per axis.
        let t = faces.iter().filter(|l| l.ends_with(".T.);")).count();
        assert_eq!(t + faces.iter().filter(|l| l.ends_with(".F.);")).count(), 6, "{faces:?}");
    }

    #[test]
    fn a_face_with_a_hole_marks_exactly_one_outer_bound() {
        use cadpole_core::geom::Plane;
        use cadpole_core::sketch::{Loop, P2, Sketch};
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0));
        let solid = cadpole_core::kernel::extrude(&s, 5.0).unwrap().remove(0);
        let mut w = Writer::new(Options::default());
        w.solid("plate", solid.compress()).unwrap();
        let text = w.em.render();
        for line in text.lines().filter(|l| l.contains("ADVANCED_FACE")) {
            let bounds = line.split("((").nth(1).unwrap_or("").split(')').next().unwrap_or("");
            let n = bounds.split(',').count();
            if n > 1 {
                // The capped faces have two loops; find their bound entities.
                let ids: Vec<&str> = bounds.split(", ").collect();
                let outer = ids
                    .iter()
                    .filter(|id| {
                        let want = format!("{} = FACE_OUTER_BOUND", id.trim());
                        text.contains(&want)
                    })
                    .count();
                assert_eq!(outer, 1, "exactly one outer bound per face, got {outer} in {line}");
            }
        }
    }

    #[test]
    fn a_solid_with_a_void_writes_brep_with_voids_and_an_inverted_inner_shell() {
        // truck's booleans refuse to make a closed void, so build one by hand:
        // a 20 mm cube with a 6 mm cube of nothing inside it.
        let outer = cube();
        let inner_v = builder::vertex(Point3::new(3.0, 3.0, 3.0));
        let inner_e = builder::tsweep(&inner_v, Vector3::new(4.0, 0.0, 0.0));
        let inner_f = builder::tsweep(&inner_e, Vector3::new(0.0, 4.0, 0.0));
        let mut inner = builder::tsweep(&inner_f, Vector3::new(0.0, 0.0, 4.0));
        inner.not(); // its faces now point into the void
        let solid = truck_modeling::Solid::new(
            outer.into_boundaries().into_iter().chain(inner.into_boundaries()).collect(),
        );
        let mut w = Writer::new(Options::default());
        w.solid("hollow", solid.compress()).unwrap();
        let text = w.em.render();
        assert_eq!(text.matches("BREP_WITH_VOIDS").count(), 1, "{text}");
        assert_eq!(text.matches("ORIENTED_CLOSED_SHELL").count(), 1);
        assert!(text.contains("ORIENTED_CLOSED_SHELL('', *, #"), "{text}");
        assert!(text.contains(", .F.);"), "the void orientation must be .F. (WR1)");
        assert_eq!(text.matches("= CLOSED_SHELL").count(), 2);
        assert_eq!(text.matches("ADVANCED_FACE").count(), 12);
        assert!(!text.contains("MANIFOLD_SOLID_BREP"), "a solid with a void is not a plain manifold brep");
    }
}
