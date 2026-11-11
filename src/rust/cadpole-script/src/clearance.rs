//! `distance(a, b)` and `clearance_map(a, b, below=)`: the clearance query
//! as the script sees it, and the plumbing that hands a set of bodies to the
//! multi-body print checks.
//!
//! The geometry is `cadpole_core::clearance`; what this module adds is names —
//! the closest points land on faces, and Cadpole names faces, so the answer
//! says `sun/side[3]` rather than a triangle index — and the `Clearance`
//! value a script asserts on. Both operands are the bodies' tessellations —
//! each at its own bounds (`body.tessellation(tolerance=)`, else the
//! document's), which is what `export` and `print_check` see too — and the
//! value carries the coarser of the two tolerances as its uncertainty:
//! each surface may be off its chords by its own tolerance, so the
//! true gap lies within the sum of the two of the measured one, and twice
//! the coarser bounds that.

use std::fmt;

use allocative::Allocative;
use cadpole_core::context::{Context, fmt_num};
use cadpole_core::geom::Vec3;
use cadpole_core::ids::BodyId;
use cadpole_core::tessellate::{Mesh, Topology};
use cadpole_drawing::units::Units;
use starlark::starlark_simple_value;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value};
use starlark_derive::starlark_value;

use crate::convert::alloc_vec3;

/// What `distance(a, b)` hands back.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KClearance {
    pub a: String,
    pub b: String,
    /// mm; 0 when the bodies touch or overlap.
    pub distance: f64,
    /// The bodies share material.
    pub overlap: bool,
    #[allocative(skip)]
    pub a_point: Vec3,
    #[allocative(skip)]
    pub b_point: Vec3,
    /// The named faces the closest points lie on.
    pub face_a: String,
    pub face_b: String,
    /// The tessellation tolerance the answer was measured at: each surface
    /// may be off its chords by this much.
    pub tolerance: f64,
    /// The document's display convention at the time of the
    /// measurement, for `str()`: a gap reads `0.35"` in a house script and
    /// `8.89 mm` in a bracket's. The numbers above are millimetres either way.
    #[allocative(skip)]
    pub units: Option<Units>,
}
starlark_simple_value!(KClearance);

impl KClearance {
    /// A gap in the document's units: a decimal, never a fraction.
    fn gap(&self, mm: f64) -> String {
        match &self.units {
            Some(u) => u.clearance(mm),
            None => format!("{} mm", fmt_num(mm)),
        }
    }
}

impl fmt::Display for KClearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.overlap {
            write!(f, "clearance({} and {} overlap, between {} and {})", self.a, self.b, self.face_a, self.face_b)
        } else if self.distance == 0.0 {
            write!(f, "clearance({} and {} touch, between {} and {})", self.a, self.b, self.face_a, self.face_b)
        } else {
            write!(
                f,
                "clearance({} between {} and {}, ±{})",
                self.gap(self.distance),
                self.face_a,
                self.face_b,
                self.gap(self.tolerance)
            )
        }
    }
}

/// The document's display convention, as a formatter, if it declared one.
pub(crate) fn document_units(ctx: &Context) -> Option<Units> {
    ctx.display_units.as_ref().and_then(|u| Units::parse(&u.unit, u.fraction).ok())
}

#[starlark_value(type = "Clearance")]
impl<'v> StarlarkValue<'v> for KClearance {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "distance" => Some(heap.alloc(self.distance)),
            "overlap" => Some(heap.alloc(self.overlap)),
            // Zero distance without shared material.
            "touching" => Some(heap.alloc(self.distance == 0.0 && !self.overlap)),
            "at" => Some(heap.alloc((alloc_vec3(heap, self.a_point), alloc_vec3(heap, self.b_point)))),
            "a_point" => Some(alloc_vec3(heap, self.a_point)),
            "b_point" => Some(alloc_vec3(heap, self.b_point)),
            "face_a" => Some(heap.alloc(self.face_a.as_str())),
            "face_b" => Some(heap.alloc(self.face_b.as_str())),
            "tolerance" => Some(heap.alloc(self.tolerance)),
            "bodies" => Some(heap.alloc((self.a.as_str(), self.b.as_str()))),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["distance", "overlap", "touching", "at", "a_point", "b_point", "face_a", "face_b", "tolerance", "bodies"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// A body's mesh and topology, copied out of the context so several can be
/// held at once (`Context::body_mesh` borrows the context mutably, because
/// it tessellates on first use).
pub(crate) struct Meshed {
    pub name: String,
    pub mesh: Mesh,
    pub topo: Topology,
    /// The chordal tolerance this mesh was actually made at: the body's own
    /// (`tessellation(tolerance=)`, or its source's for a rigid copy), else
    /// the document's (the tessellator never moves off it). A mesh
    /// body's triangles are its geometry and carry the document's figure.
    pub tolerance: f64,
}

pub(crate) fn meshed(ctx: &mut Context, ids: &[BodyId]) -> anyhow::Result<Vec<Meshed>> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        let name = ctx.body(id)?.name.clone();
        let (mesh, topo) = ctx.body_mesh(id)?.clone();
        let tolerance = ctx.mesh_quality(id)?.map(|q| q.tessellated_at).unwrap_or(ctx.tolerance);
        out.push(Meshed { name, mesh, topo, tolerance });
    }
    Ok(out)
}

/// The uncertainty a measurement between these meshes carries: the coarsest
/// tolerance any of them was made at.
pub(crate) fn coarsest(m: &[Meshed]) -> f64 {
    m.iter().map(|x| x.tolerance).fold(0.0, f64::max)
}

fn face_name(topo: &Topology, id: u32) -> String {
    topo.faces.get(id as usize).map(|f| f.name.clone()).unwrap_or_else(|| format!("face:{id}"))
}

/// `distance(a, b)`.
pub(crate) fn measure(ctx: &mut Context, a: BodyId, b: BodyId) -> anyhow::Result<KClearance> {
    let m = meshed(ctx, &[a, b])?;
    let tolerance = coarsest(&m);
    let (ma, mb) = (&m[0], &m[1]);
    let c = cadpole_core::clearance::distance(&ma.mesh, &mb.mesh, tolerance).ok_or_else(|| {
        anyhow::anyhow!(
            "distance: {} has no surface to measure",
            if ma.mesh.triangle_count() == 0 { &ma.name } else { &mb.name }
        )
    })?;
    Ok(KClearance {
        a: ma.name.clone(),
        b: mb.name.clone(),
        distance: c.distance,
        overlap: c.overlap,
        a_point: c.a_point,
        b_point: c.b_point,
        face_a: face_name(&ma.topo, c.face_a),
        face_b: face_name(&mb.topo, c.face_b),
        tolerance: c.tolerance,
        units: document_units(ctx),
    })
}

/// One entry of `clearance_map`.
pub(crate) struct Near {
    pub distance: f64,
    pub a_point: Vec3,
    pub b_point: Vec3,
    pub face_a: String,
    pub face_b: String,
}

/// `clearance_map(a, b, below=, spacing=, limit=)`: the entries and how
/// many regions there were before the cap.
pub(crate) fn map(
    ctx: &mut Context,
    a: BodyId,
    b: BodyId,
    below: f64,
    spacing: f64,
    limit: usize,
) -> anyhow::Result<(Vec<Near>, usize)> {
    let m = meshed(ctx, &[a, b])?;
    let (ma, mb) = (&m[0], &m[1]);
    let nm = cadpole_core::clearance::near(&ma.mesh, &mb.mesh, below, spacing, limit);
    let entries = nm
        .pairs
        .iter()
        .map(|p| Near {
            distance: p.distance,
            a_point: p.a_point,
            b_point: p.b_point,
            face_a: face_name(&ma.topo, p.face_a),
            face_b: face_name(&mb.topo, p.face_b),
        })
        .collect();
    Ok((entries, nm.regions))
}

/// The multi-body print checks over a set of bodies, by index into `ids`.
pub(crate) fn assembly_findings(
    ctx: &mut Context,
    ids: &[BodyId],
    profile: &cadpole_print::Profile,
) -> anyhow::Result<Vec<cadpole_print::assembly::Finding>> {
    if ids.len() < 2 {
        return Ok(Vec::new());
    }
    let m = meshed(ctx, ids)?;
    let tolerance = coarsest(&m);
    let refs: Vec<cadpole_print::assembly::BodyRef<'_>> = m
        .iter()
        .map(|x| cadpole_print::assembly::BodyRef { name: &x.name, mesh: &x.mesh, topo: Some(&x.topo) })
        .collect();
    Ok(cadpole_print::assembly::check(&refs, profile, tolerance))
}
