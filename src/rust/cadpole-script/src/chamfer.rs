//! The `chamfer` global (docs/starlark-api.md): the exact B-rep chamfer
//! by default, and the field path when asked for by
//! `kernel="field"`.
//!
//! The exact path never falls back on its own: what it cannot build is an
//! error naming the edge and the fix, and one of the fixes is always to say
//! `kernel="field"` and take a mesh body knowingly.

use cadpole_core::{Setback, BodyId};
use cadpole_field::fillet::EdgeSelection;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::Value;

use crate::check::finite;
use crate::field::{blend, resolve_selection};
use crate::store::{call_span, store};
use crate::values::KBody;

/// The setback the call asked for: `distance` alone, with `d2`, or with
/// `angle` (degrees). `d2` and `angle` together is a contradiction.
fn setback(distance: f64, d2: Option<f64>, angle: Option<f64>) -> anyhow::Result<Setback> {
    let d = finite(distance, "chamfer distance")?;
    if !(d > 0.0) {
        anyhow::bail!("chamfer distance must be a positive number of millimetres, got {d}");
    }
    match (d2, angle) {
        (Some(_), Some(_)) => anyhow::bail!("chamfer: pass d2= or angle=, not both: each fixes the second setback"),
        (Some(d2), None) => {
            let d2 = finite(d2, "chamfer d2")?;
            if !(d2 > 0.0) {
                anyhow::bail!("chamfer d2 must be a positive number of millimetres, got {d2}");
            }
            Ok(Setback::Two(d, d2))
        }
        (None, Some(a)) => {
            let a = finite(a, "chamfer angle")?;
            if !(a > 0.0 && a < 180.0) {
                anyhow::bail!("chamfer angle must be between 0 and 180 degrees, got {a}");
            }
            Ok(Setback::Angle { d, angle: a.to_radians() })
        }
        (None, None) => Ok(Setback::Equal(d)),
    }
}

#[starlark_module]
pub(crate) fn chamfer_globals(builder: &mut GlobalsBuilder) {
    /// Cut the selected edges of a body back by `distance` along each face,
    /// leaving a flat: `d2` sets a different setback along the second face
    /// (the faces in the order the edge's name spells them), `angle` the
    /// angle in degrees at which the flat leaves the first face.
    ///
    /// `edges` may be a body (every corner of it), a `Selection` from
    /// `body.edges(...)` or `body.faces(...)`, one `Edge`, a `Face` (its
    /// boundary edges), a list of any of those, or edge names; a Selection,
    /// a Face and an Edge remember which body they came from, so the body
    /// argument is optional with any of them.
    ///
    /// **Exact by default.** The result is a B-rep body: each chamfer is a
    /// planar face on a straight edge and a cone on a circular one, named
    /// `<feature>/face[<edge>]`, and every neighbour keeps its name. Cases
    /// in: closed loops (a rim, a blind hole's floor), straight edges ending
    /// at convex corners, and chains of straight edges turning convex
    /// corners, which mitre themselves. What the exact path cannot build is
    /// an error naming the edge and the fix — an edge that dies into a
    /// face, a chain that changes concavity, a setback wider than a
    /// neighbour, a smooth seam, a free-form or free-curve edge, the mouth
    /// of a hole or the root of a boss (an inner loop the exact kernel
    /// cannot enlarge), an arc mitring a straight edge on a curved wall.
    ///
    /// `kernel="field"` takes the field path instead — the blend of
    /// `fillet`, pointed at a chamfer — and returns a mesh body with the
    /// diagnostics `fillet` gives; `resolution` belongs to that path.
    fn chamfer<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        distance: Option<UnpackFloat>,
        #[starlark(require = named)] edges: Option<Value<'v>>,
        #[starlark(require = named)] d2: Option<UnpackFloat>,
        #[starlark(require = named)] angle: Option<UnpackFloat>,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] resolution: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let d = distance.ok_or_else(|| anyhow::anyhow!("chamfer: distance is required"))?.0;
        let field = match kernel {
            None | Some("brep") => false,
            Some("field") => true,
            Some(other) => anyhow::bail!("chamfer: kernel must be \"brep\" (exact, the default) or \"field\", got {other:?}"),
        };
        if field {
            if d2.is_some() || angle.is_some() {
                anyhow::bail!("chamfer: d2= and angle= are the exact path's; the field path cuts the same distance along both faces");
            }
            return blend("chamfer", target, d, edges, resolution.map(|v| v.0), name, eval);
        }
        if resolution.is_some() {
            anyhow::bail!("chamfer: resolution= is the field path's (pass kernel=\"field\"); the exact path has no grid");
        }
        let setback = setback(d, d2.map(|v| v.0), angle.map(|v| v.0))?;
        let heap = eval.heap();
        let (body, selection) = resolve_selection(target, edges, heap, "chamfer", eval)?;
        let names: Vec<String> = match selection {
            EdgeSelection::All => Vec::new(),
            EdgeSelection::Named(n) => n,
        };
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let bid: BodyId = body.body_id();
        let out = ctx.chamfer(bid, &names, setback, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[out.body], "chamfer")?;
        KBody::from_ctx(&ctx, out.body)
    }
}
