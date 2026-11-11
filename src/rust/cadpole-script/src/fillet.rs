//! The `fillet` global (docs/starlark-api.md): the exact B-rep fillet
//! by default, and the field path when asked for by
//! `kernel="field"`.
//!
//! The exact path never falls back on its own: what it cannot build is an
//! error naming the edge and the fix, and one of the fixes is always to say
//! `kernel="field"` and take a mesh body knowingly.

use cadpole_core::BodyId;
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

#[starlark_module]
pub(crate) fn fillet_globals(builder: &mut GlobalsBuilder) {
    /// Round the selected edges of a body with a constant-radius blend.
    ///
    /// `edges` may be a body (every corner of it), a `Selection` from
    /// `body.edges(...)` or `body.faces(...)`, one `Edge`, a `Face` (its
    /// boundary edges), a list of any of those, or edge names; a Selection,
    /// a Face and an Edge remember which body they came from, so the body
    /// argument is optional with any of them. An `edges=` that selected
    /// nothing is an error: rounding nothing is never what was meant.
    ///
    /// **Exact by default.** The result is a B-rep body: each blend is the
    /// natural quadric tangent to both faces — a cylinder along a straight
    /// edge, a torus round a circular one, a sphere octant where three
    /// blends meet at a box corner — named `<feature>/blend[<edge>]` (the
    /// octant `<feature>/vertex[<vertex>]`), and every neighbour keeps its
    /// name. Cases in: a closed loop on a planar face (a rim, a boss root, a
    /// hole's mouth or floor, a rounded outline's rim) is re-wired with no
    /// boolean at all; straight edges ending at convex corners, chains that
    /// turn corners (which meet in elliptical mitres) and all twelve edges
    /// of a box (sphere octants at the corners) take their topology from
    /// the chamfer's cutter. What the exact path cannot build is an error
    /// naming the edge and the fix — a corner that is not a right angle
    /// with its third edge sharp, a radius wider than a face or deeper than
    /// a wall, a feature inside the band the blend takes, a blend whose end
    /// face is oblique, a free-form edge, a mesh body.
    ///
    /// `kernel="field"` takes the field path instead and returns a mesh
    /// body: the body is imported as a signed distance field, a blend that
    /// is live only near the selected edges is added to it, and the result
    /// is contoured back to triangles; an info diagnostic says so with the
    /// resolution it used and the error it measured. `resolution` belongs
    /// to that path and defaults to `radius / 6`. A radius that does not
    /// fit is a warning there, naming the edge and the largest radius that
    /// does.
    fn fillet<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        radius: Option<UnpackFloat>,
        #[starlark(require = named)] edges: Option<Value<'v>>,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] resolution: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let r = radius.ok_or_else(|| anyhow::anyhow!("fillet: radius is required"))?.0;
        let field = match kernel {
            None | Some("brep") => false,
            Some("field") => true,
            Some(other) => anyhow::bail!("fillet: kernel must be \"brep\" (exact, the default) or \"field\", got {other:?}"),
        };
        if field {
            return blend("fillet", target, r, edges, resolution.map(|v| v.0), name, eval);
        }
        if resolution.is_some() {
            anyhow::bail!("fillet: resolution= is the field path's (pass kernel=\"field\"); the exact path has no grid");
        }
        let r = finite(r, "fillet radius")?;
        if !(r > 0.0) {
            anyhow::bail!("fillet radius must be a positive number of millimetres, got {r}");
        }
        let heap = eval.heap();
        let (body, selection) = resolve_selection(target, edges, heap, "fillet", eval)?;
        let names: Vec<String> = match selection {
            EdgeSelection::All => Vec::new(),
            EdgeSelection::Named(n) => n,
        };
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let bid: BodyId = body.body_id();
        let out = ctx.fillet(bid, &names, r, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[out.body], "fillet")?;
        KBody::from_ctx(&ctx, out.body)
    }
}
