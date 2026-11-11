//! Constrained sketches: named entities, named constraints, and a solve that
//! happens when the sketch is used.
//!
//! `constrained_sketch()` is the second way to build a profile. The first —
//! `sketch().rect(40, 20)` — writes coordinates down; this one writes
//! *dimensions and relationships* down and lets `cadpole-sketch-solve` work the
//! coordinates out. The difference shows up when a parameter changes: a hole
//! dimensioned 8 mm from the left edge stays 8 mm from the left edge when the
//! plate gets wider, and a literal `(-w/2 + 8, 0)` only does so because the
//! author did the algebra by hand.
//!
//! Three properties drive the design.
//!
//! * **Everything is named.** Entities carry the script's own names and
//!   constraints are labelled from them (`distance(a, b)` unless the script
//!   passes `name=`), because every diagnostic prints those labels and because
//!   the sketch tool rewrites one `constrained_sketch`
//!   chain by name, not by index — a drag must be a one-line diff.
//! * **Lazy, then cached.** Building is pure bookkeeping; the solve happens on
//!   first use (an extrude, a revolve, a query) and its result is kept until
//!   something changes the sketch. Extruding the same sketch twice solves once.
//! * **The output is ordinary geometry.** A solved sketch is turned into the
//!   same [`Sketch`] of [`Loop`]s that `rect()` produces, so nothing
//!   downstream — regions, nesting, the kernels, naming — knows the difference.
//!
//! Diagnostics are the reason to bother having a solver at all, so they are
//! carried through to the script author with the span of the call that caused
//! them: an under-constrained sketch is *info* (it is how modelling starts), a
//! redundant constraint is a *warning* naming what already implied it, and a
//! contradiction is an *error* naming both sides and the size of the
//! contradiction in millimetres, attributed to the offending `constrain()`
//! call rather than to the extrude that tripped over it.

use std::collections::BTreeMap;
use std::fmt;

use allocative::Allocative;
use cadpole_core::Context;
use cadpole_core::context::{Diagnostic, Span, fmt_num};
use cadpole_core::geom::Plane;
use cadpole_core::ids::FeatureId;
use cadpole_core::sketch::{Loop, P2, Path, Segment, Sketch};
use cadpole_sketch_solve::{
    Constraint, ConstraintId, CurveRef, Diagnosis, EntityId, LineId, PointId, PointRef,
    SolveResult, SolveStatus, System,
};
use starlark::environment::{Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::check::{finite, num};
use crate::store::{call_span, store};
use crate::values::{KPlane, KSketch};
use cadpole_core::fmath;

/// The constraint kinds `constrain()` understands, in the order
/// `docs/starlark-api.md` lists them. Named here so an unknown kind can say
/// what the alternatives are.
pub const KINDS: &[&str] = &[
    "coincident",
    "horizontal",
    "vertical",
    "distance",
    "angle",
    "parallel",
    "perpendicular",
    "tangent",
    "equal",
    "symmetric",
    "fix",
    "radius",
    "concentric",
    "midpoint",
    "on_curve",
];

// ------------------------------------------------------------------ state

/// Which point of an entity an operand names. A line and an arc both have a
/// `start` and an `end`; a circle and an arc have a `center`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Allocative)]
pub enum Part {
    Whole,
    Start,
    End,
    Center,
}

impl Part {
    fn suffix(self) -> &'static str {
        match self {
            Part::Whole => "",
            Part::Start => ".start",
            Part::End => ".end",
            Part::Center => ".center",
        }
    }
}

#[derive(Clone, Debug)]
struct EntRec {
    name: String,
    /// "point", "line", "arc" or "circle".
    kind: &'static str,
    id: EntityId,
    construction: bool,
}

#[derive(Clone, Debug)]
struct ConRec {
    label: String,
    span: Option<Span>,
}

/// One operand of a `constrain()` call, already resolved against the sketch.
#[derive(Clone, Debug)]
pub enum Operand {
    Entity { index: usize, part: Part },
    Number(f64),
}

/// A solved sketch: the geometry, and what the solver said about it.
#[derive(Clone, Debug)]
struct Solved {
    sketch: Sketch,
    dof: usize,
    /// Material curves the solve collapsed (a line from a point to itself),
    /// each named. The solve itself stands -- `dof`, `at` and `radius_of`
    /// still answer -- but no face can be built from the sketch.
    degenerate: Vec<String>,
}

/// A sketch defined by entities and constraints rather than by coordinates.
#[derive(Debug)]
pub struct CSketch {
    pub feature: FeatureId,
    pub plane: Plane,
    /// Span of the `constrained_sketch()` call, for diagnostics that belong to
    /// the sketch as a whole (its degrees of freedom, a failed solve).
    pub span: Option<Span>,
    sys: System,
    ents: Vec<EntRec>,
    by_name: BTreeMap<String, usize>,
    cons: Vec<ConRec>,
    /// Loops the script declared with `close([...])`: a name and the entities.
    declared: Vec<(String, Vec<usize>)>,
    /// `None` while dirty; `Some(Ok)` solved; `Some(Err)` solved and broken,
    /// kept so a second use reports the same thing instead of re-solving.
    solved: Option<Result<Solved, String>>,
}

impl CSketch {
    pub fn new(plane: Plane, feature: FeatureId, span: Option<Span>) -> CSketch {
        CSketch {
            feature,
            plane,
            span,
            sys: System::new(),
            ents: Vec::new(),
            by_name: BTreeMap::new(),
            cons: Vec::new(),
            declared: Vec::new(),
            solved: None,
        }
    }

    pub fn entity_count(&self) -> usize {
        self.ents.len()
    }

    pub fn constraint_count(&self) -> usize {
        self.cons.len()
    }

    pub fn entity_names(&self) -> Vec<String> {
        self.ents.iter().map(|e| e.name.clone()).collect()
    }

    pub fn constraint_labels(&self) -> Vec<String> {
        self.cons.iter().map(|c| c.label.clone()).collect()
    }

    pub fn kind_of(&self, index: usize) -> &'static str {
        self.ents[index].kind
    }

    pub fn name_of(&self, index: usize) -> &str {
        &self.ents[index].name
    }

    pub fn lookup(&self, name: &str) -> anyhow::Result<usize> {
        self.by_name.get(name).copied().ok_or_else(|| {
            anyhow::anyhow!(
                "sketch {:?} has no entity named {name:?} (has: {})",
                self.feature.0,
                self.list_names()
            )
        })
    }

    fn list_names(&self) -> String {
        if self.ents.is_empty() {
            "none".to_owned()
        } else {
            self.entity_names().join(", ")
        }
    }

    /// Anything that changes the system invalidates the cached solve.
    fn dirty(&mut self) {
        self.solved = None;
    }

    fn claim_name(&mut self, name: &str) -> anyhow::Result<()> {
        if name.is_empty() {
            anyhow::bail!("sketch {:?}: an entity name must not be empty", self.feature.0);
        }
        if self.by_name.contains_key(name) {
            anyhow::bail!("sketch {:?}: entity name {name:?} used twice", self.feature.0);
        }
        Ok(())
    }

    fn push_entity(&mut self, name: &str, kind: &'static str, id: EntityId, construction: bool) -> usize {
        let index = self.ents.len();
        self.sys.name_entity(id, name);
        if construction {
            self.sys.set_construction(id, true);
        }
        self.ents.push(EntRec { name: name.to_owned(), kind, id, construction });
        self.by_name.insert(name.to_owned(), index);
        self.dirty();
        index
    }

    pub fn add_point(
        &mut self,
        name: &str,
        x: f64,
        y: f64,
        fix: bool,
        construction: bool,
        span: Option<Span>,
    ) -> anyhow::Result<usize> {
        self.claim_name(name)?;
        let p = self.sys.point(x, y);
        let index = self.push_entity(name, "point", p.0, construction);
        if fix {
            self.add_resolved(Constraint::fix(p), format!("fix({name})"), span)?;
        }
        Ok(index)
    }

    pub fn add_line(&mut self, name: &str, a: usize, b: usize, construction: bool) -> anyhow::Result<usize> {
        self.claim_name(name)?;
        let a = self.point_entity(a, "line: a")?;
        let b = self.point_entity(b, "line: b")?;
        let l = self.sys.line(a, b);
        Ok(self.push_entity(name, "line", l.0, construction))
    }

    pub fn add_circle(&mut self, name: &str, center: usize, r: f64, construction: bool) -> anyhow::Result<usize> {
        self.claim_name(name)?;
        if !(r > 0.0) {
            anyhow::bail!("circle {name:?}: r must be positive (got {})", fmt_num(r));
        }
        let c = self.point_entity(center, "circle: center")?;
        let e = self.sys.circle(c, r);
        Ok(self.push_entity(name, "circle", e.0, construction))
    }

    pub fn add_arc(
        &mut self,
        name: &str,
        center: usize,
        r: f64,
        start_deg: f64,
        end_deg: f64,
        construction: bool,
    ) -> anyhow::Result<usize> {
        self.claim_name(name)?;
        if !(r > 0.0) {
            anyhow::bail!("arc {name:?}: r must be positive (got {})", fmt_num(r));
        }
        let c = self.point_entity(center, "arc: center")?;
        let e = self.sys.arc(c, r, start_deg.to_radians(), end_deg.to_radians());
        Ok(self.push_entity(name, "arc", e.0, construction))
    }

    /// A point entity id, for the places that need a real point (a line's
    /// ends, a circle's centre) rather than any point-like reference.
    fn point_entity(&self, index: usize, what: &str) -> anyhow::Result<PointId> {
        let e = &self.ents[index];
        if e.kind != "point" {
            anyhow::bail!("{what} must be a point, but {:?} is a {}", e.name, e.kind);
        }
        Ok(PointId(e.id))
    }

    /// The name a diagnostic prints for an operand.
    fn operand_name(&self, op: &Operand) -> String {
        match op {
            Operand::Entity { index, part } => {
                format!("{}{}", self.ents[*index].name, part.suffix())
            }
            Operand::Number(n) => fmt_num(*n),
        }
    }

    fn point_ref(&self, op: &Operand, what: &str) -> anyhow::Result<PointRef> {
        let Operand::Entity { index, part } = op else {
            anyhow::bail!("{what} must be a sketch entity, not the number {}", self.operand_name(op));
        };
        let e = &self.ents[*index];
        Ok(match (part, e.kind) {
            (Part::Whole, "point") => PointRef::Point(PointId(e.id)),
            // A circle or an arc used where a point is wanted means its
            // centre: `constrain("distance", hole_a, hole_b, 30)` is the
            // dimension a draughtsman would write.
            (Part::Whole | Part::Center, "circle" | "arc") => PointRef::Center(e.id),
            (Part::Start, "line" | "arc") => PointRef::Start(e.id),
            (Part::End, "line" | "arc") => PointRef::End(e.id),
            _ => anyhow::bail!(
                "{what}: {:?} is a {} and has no {}",
                e.name,
                e.kind,
                match part {
                    Part::Whole => "point",
                    Part::Start => "start point",
                    Part::End => "end point",
                    Part::Center => "centre",
                }
            ),
        })
    }

    fn line_id(&self, op: &Operand, what: &str) -> anyhow::Result<LineId> {
        let index = self.whole(op, what)?;
        let e = &self.ents[index];
        if e.kind != "line" {
            anyhow::bail!("{what} must be a line, but {:?} is a {}", e.name, e.kind);
        }
        Ok(LineId(e.id))
    }

    fn curve_ref(&self, op: &Operand, what: &str) -> anyhow::Result<CurveRef> {
        let index = self.whole(op, what)?;
        let e = &self.ents[index];
        Ok(match e.kind {
            "line" => CurveRef::Line(LineId(e.id)),
            "circle" => CurveRef::Circle(cadpole_sketch_solve::CircleId(e.id)),
            "arc" => CurveRef::Arc(cadpole_sketch_solve::ArcId(e.id)),
            _ => anyhow::bail!("{what} must be a line, circle or arc, but {:?} is a point", e.name),
        })
    }

    fn whole(&self, op: &Operand, what: &str) -> anyhow::Result<usize> {
        match op {
            Operand::Entity { index, part: Part::Whole } => Ok(*index),
            Operand::Entity { index, part } => anyhow::bail!(
                "{what} must be a whole entity, not {}{}",
                self.ents[*index].name,
                part.suffix()
            ),
            Operand::Number(n) => {
                anyhow::bail!("{what} must be a sketch entity, not the number {}", fmt_num(*n))
            }
        }
    }

    fn number(&self, op: &Operand, what: &str) -> anyhow::Result<f64> {
        match op {
            Operand::Number(n) => Ok(*n),
            Operand::Entity { .. } => {
                anyhow::bail!("{what} must be a number, not the entity {}", self.operand_name(op))
            }
        }
    }

    fn radial(&self, op: &Operand, what: &str) -> anyhow::Result<EntityId> {
        let index = self.whole(op, what)?;
        let e = &self.ents[index];
        if e.kind != "circle" && e.kind != "arc" {
            anyhow::bail!("{what} must be a circle or an arc, but {:?} is a {}", e.name, e.kind);
        }
        Ok(e.id)
    }

    /// Add one constraint of `kind` over `ops`.
    ///
    /// The label is `name` if the script gave one, else the kind and the
    /// operand names — `distance(a, b)`, `horizontal(bottom)` — which is what
    /// every diagnostic about this constraint will print.
    pub fn constrain(
        &mut self,
        kind: &str,
        ops: &[Operand],
        name: Option<&str>,
        span: Option<Span>,
    ) -> anyhow::Result<()> {
        let arity = |n: usize, shape: &str| -> anyhow::Result<()> {
            if ops.len() == n {
                Ok(())
            } else {
                anyhow::bail!(
                    "constrain({kind:?}) takes {shape}, got {} argument{}",
                    ops.len(),
                    if ops.len() == 1 { "" } else { "s" }
                )
            }
        };
        let what = |i: usize| format!("constrain({kind:?}) argument {}", i + 1);
        let c = match kind {
            "coincident" => {
                arity(2, "(a, b)")?;
                Constraint::coincident(self.point_ref(&ops[0], &what(0))?, self.point_ref(&ops[1], &what(1))?)
            }
            "horizontal" => {
                arity(1, "(line)")?;
                Constraint::horizontal(self.line_id(&ops[0], &what(0))?)
            }
            "vertical" => {
                arity(1, "(line)")?;
                Constraint::vertical(self.line_id(&ops[0], &what(0))?)
            }
            "distance" => {
                arity(3, "(a, b, distance)")?;
                let d = self.number(&ops[2], &what(2))?;
                // `distance(point, line, d)` is the perpendicular distance to
                // the line — the dimension that puts a hole 8 mm from an edge.
                match self.line_id(&ops[1], &what(1)) {
                    Ok(line) => {
                        Constraint::point_line_distance(self.point_ref(&ops[0], &what(0))?, line, d)
                    }
                    Err(_) => Constraint::distance(
                        self.point_ref(&ops[0], &what(0))?,
                        self.point_ref(&ops[1], &what(1))?,
                        d,
                    ),
                }
            }
            "angle" => {
                arity(3, "(line_a, line_b, degrees)")?;
                Constraint::angle_degrees(
                    self.line_id(&ops[0], &what(0))?,
                    self.line_id(&ops[1], &what(1))?,
                    self.number(&ops[2], &what(2))?,
                )
            }
            "parallel" => {
                arity(2, "(line_a, line_b)")?;
                Constraint::parallel(self.line_id(&ops[0], &what(0))?, self.line_id(&ops[1], &what(1))?)
            }
            "perpendicular" => {
                arity(2, "(line_a, line_b)")?;
                Constraint::perpendicular(
                    self.line_id(&ops[0], &what(0))?,
                    self.line_id(&ops[1], &what(1))?,
                )
            }
            "tangent" => {
                if ops.len() != 2 && ops.len() != 3 {
                    anyhow::bail!(
                        "constrain(\"tangent\") takes (a, b) or (a, b, at_point), got {} arguments",
                        ops.len()
                    );
                }
                let (a, b) = (self.curve_ref(&ops[0], &what(0))?, self.curve_ref(&ops[1], &what(1))?);
                match ops.get(2) {
                    // The two curves already meet here, so the residual is
                    // "the tangents agree at this point" (see the solver's
                    // `Constraint::Tangent` docs).
                    Some(at) => Constraint::tangent_at(a, b, self.point_ref(at, &what(2))?),
                    None => Constraint::tangent(a, b),
                }
            }
            "equal" => {
                arity(2, "(a, b)")?;
                match (self.line_id(&ops[0], &what(0)), self.line_id(&ops[1], &what(1))) {
                    (Ok(a), Ok(b)) => Constraint::equal_length(a, b),
                    _ => Constraint::equal_radius(
                        self.radial(&ops[0], &format!("{} (two lines or two radial curves)", what(0)))?,
                        self.radial(&ops[1], &format!("{} (two lines or two radial curves)", what(1)))?,
                    ),
                }
            }
            "symmetric" => {
                arity(3, "(a, b, axis_line)")?;
                Constraint::symmetric(
                    self.point_ref(&ops[0], &what(0))?,
                    self.point_ref(&ops[1], &what(1))?,
                    self.line_id(&ops[2], &what(2))?,
                )
            }
            "fix" => {
                arity(1, "(entity)")?;
                Constraint::fix(self.ents[self.whole(&ops[0], &what(0))?].id)
            }
            "radius" => {
                arity(2, "(circle_or_arc, r)")?;
                Constraint::radius(self.radial(&ops[0], &what(0))?, self.number(&ops[1], &what(1))?)
            }
            "concentric" => {
                arity(2, "(a, b)")?;
                Constraint::concentric(self.radial(&ops[0], &what(0))?, self.radial(&ops[1], &what(1))?)
            }
            "midpoint" => {
                arity(2, "(point, line)")?;
                Constraint::midpoint(self.point_ref(&ops[0], &what(0))?, self.line_id(&ops[1], &what(1))?)
            }
            "on_curve" => {
                arity(2, "(point, curve)")?;
                Constraint::on_curve(self.point_ref(&ops[0], &what(0))?, self.curve_ref(&ops[1], &what(1))?)
            }
            other => anyhow::bail!(
                "unknown constraint {other:?} (expected one of: {})",
                KINDS.join(", ")
            ),
        };
        let label = match name {
            Some(n) => n.to_owned(),
            None => {
                let args: Vec<String> = ops
                    .iter()
                    .filter(|o| matches!(o, Operand::Entity { .. }))
                    .map(|o| self.operand_name(o))
                    .collect();
                format!("{kind}({})", args.join(", "))
            }
        };
        self.add_resolved(c, label, span)
    }

    fn add_resolved(&mut self, c: Constraint, label: String, span: Option<Span>) -> anyhow::Result<()> {
        // Two constraints of the same kind on the same entities would print
        // the same label, which is exactly the case a redundancy report has to
        // tell apart; number the later one.
        let label = if self.cons.iter().any(|r| r.label == label) {
            format!("{label}#{}", self.cons.len())
        } else {
            label
        };
        self.sys
            .constrain_named(label.clone(), c)
            .map_err(|e| anyhow::anyhow!("constrain: {e}"))?;
        self.cons.push(ConRec { label, span });
        self.dirty();
        Ok(())
    }

    /// Declare that these entities bound one loop, in any order.
    pub fn declare_loop(&mut self, name: Option<&str>, entities: Vec<usize>) -> anyhow::Result<()> {
        if entities.is_empty() {
            anyhow::bail!("close: needs at least one entity");
        }
        for &i in &entities {
            let e = &self.ents[i];
            if e.kind == "point" {
                anyhow::bail!("close: {:?} is a point; a loop is made of lines, arcs and circles", e.name);
            }
            if e.construction {
                anyhow::bail!("close: {:?} is construction geometry and cannot bound a face", e.name);
            }
        }
        let name = match name {
            Some(n) => n.to_owned(),
            None => self.ents[entities[0]].name.clone(),
        };
        self.declared.push((name, entities));
        self.dirty();
        Ok(())
    }

    // ---- solving ------------------------------------------------------

    /// Solve if needed and hand back the geometry.
    ///
    /// `what` names the operation for the error message ("extrude"), so a
    /// broken sketch says which use of it failed as well as why.
    pub fn solved_sketch(&mut self, ctx: &mut Context, what: &str) -> anyhow::Result<Sketch> {
        let solved = self.solve_once(ctx, what)?;
        if !solved.degenerate.is_empty() {
            anyhow::bail!("{what}: {}", solved.degenerate.join("; "));
        }
        Ok(solved.sketch.clone())
    }

    /// Solve if needed; fails only when the solve itself did. A query of
    /// the solver's state (`dof`, `at`, `radius_of`) needs nothing more.
    fn solve_once(&mut self, ctx: &mut Context, what: &str) -> anyhow::Result<&Solved> {
        if self.solved.is_none() {
            let outcome = self.run_solve(ctx);
            self.solved = Some(outcome);
        }
        match self.solved.as_ref().expect("just solved") {
            Ok(s) => Ok(s),
            Err(reason) => {
                anyhow::bail!("{what}: sketch {:?} cannot be solved: {reason}", self.feature.0)
            }
        }
    }

    /// Degrees of freedom left, solving first if necessary.
    pub fn dof(&mut self, ctx: &mut Context) -> anyhow::Result<usize> {
        Ok(self.solve_once(ctx, "dof")?.dof)
    }

    /// The solved position of anything point-like.
    pub fn position(&mut self, ctx: &mut Context, op: &Operand) -> anyhow::Result<(f64, f64)> {
        self.solve_once(ctx, "at")?;
        let r = self.point_ref(op, "at")?;
        self.sys
            .position(r)
            .ok_or_else(|| anyhow::anyhow!("at: {} has no position", self.operand_name(op)))
    }

    /// The solved radius of a circle or an arc.
    pub fn radius(&mut self, ctx: &mut Context, op: &Operand) -> anyhow::Result<f64> {
        self.solve_once(ctx, "radius_of")?;
        let e = self.radial(op, "radius_of")?;
        self.sys.radius(e).ok_or_else(|| anyhow::anyhow!("radius_of: not a circle or an arc"))
    }

    fn run_solve(&mut self, ctx: &mut Context) -> Result<Solved, String> {
        if self.ents.is_empty() {
            return Err("it has no entities".to_owned());
        }
        let result = self.sys.solve();
        let diag = self.sys.diagnose();
        let fatal = self.report(ctx, &result, &diag);
        // Record what the solver found on the feature, so the tree and the
        // sketch tool can show "3 DoF" without re-solving.
        ctx.set_feature_arg(&self.feature, "dof", diag.dof);
        ctx.set_feature_arg(&self.feature, "entities", self.ents.len());
        ctx.set_feature_arg(&self.feature, "constraints", self.cons.len());
        if let Some(reason) = fatal {
            return Err(reason);
        }
        let (sketch, warnings, errors) = self.assemble();
        for w in warnings {
            self.diagnose_into(ctx, Diagnostic::warning(w).with_span(self.span.clone()));
        }
        // A collapsed curve is reported by whatever builds from the sketch,
        // once, at that call.
        ctx.set_feature_arg(&self.feature, "loops", sketch.loops.len());
        Ok(Solved { sketch, dof: diag.dof, degenerate: errors })
    }

    /// Turn the solver's verdict into script diagnostics. Returns `Some` when
    /// the sketch cannot be used at all.
    fn report(&self, ctx: &mut Context, result: &SolveResult, d: &Diagnosis) -> Option<String> {
        let mut fatal: Option<String> = None;

        // A contradiction: name both sides and how far apart they are.
        for c in &d.conflicting {
            let with = quoted(&c.implied_by_labels);
            let msg = if with.is_empty() {
                format!(
                    "conflicting constraints: {:?} cannot be satisfied, off by {} mm",
                    c.label,
                    mm(c.gap)
                )
            } else {
                format!(
                    "conflicting constraints: {:?} contradicts {with} by {} mm",
                    c.label,
                    mm(c.gap)
                )
            };
            let span = self.span_of(c.constraint).or_else(|| self.span.clone());
            self.diagnose_into(ctx, Diagnostic::error(msg.clone()).with_span(span));
            if fatal.is_none() {
                fatal = Some(msg);
            }
        }

        // A constraint that adds nothing: annoying, deletable, not fatal.
        for r in &d.redundant {
            let msg = if r.implied_by_labels.is_empty() {
                format!("redundant constraint: {:?} has no effect on this sketch", r.label)
            } else {
                format!(
                    "redundant constraint: {:?} is already implied by {}",
                    r.label,
                    quoted(&r.implied_by_labels)
                )
            };
            let span = self.span_of(r.constraint).or_else(|| self.span.clone());
            self.diagnose_into(ctx, Diagnostic::warning(msg).with_span(span));
        }

        // Not converging is not the same as contradicting: say which.
        if fatal.is_none() && !result.status.is_converged() {
            let why = match result.status {
                SolveStatus::Inconsistent => "the constraints have no common solution",
                SolveStatus::MaxIterations => "it ran out of iterations",
                SolveStatus::Stalled => "the steps became too small to make progress",
                SolveStatus::Converged => unreachable!(),
            };
            let msg = format!(
                "the solver did not converge: {why} after {} iterations, worst constraint error {} mm",
                result.iterations,
                mm(result.max_residual)
            );
            self.diagnose_into(ctx, Diagnostic::error(msg.clone()).with_span(self.span.clone()));
            fatal = Some(msg);
        }

        // Under-constrained is how every sketch starts; report it, allow it.
        if fatal.is_none() && d.dof > 0 {
            let free: Vec<String> = d
                .free_entities
                .iter()
                .filter_map(|e| self.ents.iter().find(|r| r.id == *e))
                .map(|r| r.name.clone())
                .collect();
            let msg = format!(
                "sketch {:?} is under-constrained: {} degree{} of freedom remain{} (free: {})",
                self.feature.0,
                d.dof,
                if d.dof == 1 { "" } else { "s" },
                if d.dof == 1 { "s" } else { "" },
                if free.is_empty() { "none".to_owned() } else { free.join(", ") }
            );
            self.diagnose_into(ctx, Diagnostic::info(msg).with_span(self.span.clone()));
        }
        fatal
    }

    fn span_of(&self, id: ConstraintId) -> Option<Span> {
        self.cons.get(id.0 as usize).and_then(|c| c.span.clone())
    }

    /// Every diagnostic lands twice: on the document (what `cadpole eval` prints)
    /// and on the feature (what the tree and `ExplainRef` show).
    fn diagnose_into(&self, ctx: &mut Context, d: Diagnostic) {
        let d = d.with_feature(self.feature.clone());
        ctx.diagnostics.push(d.clone());
        if let Ok(f) = ctx.feature_mut(&self.feature) {
            f.diagnostics.push(d);
        }
    }

    // ---- turning solved entities into loops ----------------------------

    /// Chain the solved curves into closed loops.
    ///
    /// Lines share point *entities*, so a corner two lines were built from is
    /// exactly one point; arcs do not — their endpoints are derived from the
    /// centre, the radius and an angle, and a `coincident` constraint is what
    /// holds them together. After a solve both kinds agree to solver
    /// tolerance, so chaining is done on position, which is also what lets a
    /// line meet an arc.
    ///
    /// A curve the solve shrank to nothing — a line from a point to itself,
    /// two ends pulled together by `coincident`, an arc or circle of no
    /// radius — bounds no face. It would otherwise chain into a loop on its
    /// own (its start is its end) and reach the kernel as an edge from a
    /// vertex to itself, so it is an error naming the curve: the third list.
    fn assemble(&self) -> (Sketch, Vec<String>, Vec<String>) {
        let mut sketch = Sketch::new(self.plane);
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        let tol = 1e-6 * self.size().max(1.0);

        let mut used = vec![false; self.ents.len()];
        // Construction geometry shapes the solve and never becomes material.
        for (i, e) in self.ents.iter().enumerate() {
            if e.construction || e.kind == "point" {
                used[i] = true;
            }
        }

        for (i, e) in self.ents.iter().enumerate() {
            if used[i] {
                continue;
            }
            let length = match e.kind {
                "circle" => self.sys.radius(e.id).map(|r| std::f64::consts::TAU * r.abs()),
                _ => self.segment_of(i, false).map(|(segs, _, _)| segs.iter().map(Segment::length).sum()),
            };
            if let Some(length) = length
                && length <= tol
            {
                used[i] = true;
                let how = match e.kind {
                    "line" => "its two ends are the same point",
                    _ => "its radius is zero",
                };
                errors.push(format!(
                    "sketch {:?}: {} {:?} has zero length after solving ({how}), so it cannot bound a face; \
                     delete it, or the constraint that collapses it",
                    self.feature.0, e.kind, e.name
                ));
            }
        }

        // Circles are loops on their own.
        for (i, e) in self.ents.iter().enumerate() {
            if used[i] || e.kind != "circle" {
                continue;
            }
            used[i] = true;
            let (Some(c), Some(r)) = (self.sys.position(PointRef::Center(e.id)), self.sys.radius(e.id))
            else {
                warnings.push(format!("circle {:?} has no centre or radius", e.name));
                continue;
            };
            sketch.push_named_loop(
                Loop::circle(P2::new(c.0, c.1), r),
                format!("{}/loop[{}]", self.feature.0, e.name),
            );
        }

        // Loops the script declared, then whatever is left over.
        let groups: Vec<(Option<String>, Vec<usize>)> = self
            .declared
            .iter()
            .map(|(n, e)| (Some(n.clone()), e.clone()))
            .chain(std::iter::once((None, (0..self.ents.len()).collect())))
            .collect();
        for (name, members) in groups {
            let explicit = name.is_some();
            let candidates: Vec<usize> = members.into_iter().filter(|i| !used[*i]).collect();
            for chain in self.chains(&candidates, &mut used, tol) {
                let label = name.clone().unwrap_or_else(|| self.ents[chain.first].name.clone());
                if chain.closed {
                    sketch.push_named_loop(
                        Loop { segments: chain.segments },
                        format!("{}/loop[{label}]", self.feature.0),
                    );
                } else {
                    let gap = chain.gap;
                    warnings.push(format!(
                        "sketch {:?}: {} starting at {:?} does not close (ends {} mm apart), so it \
                         became a construction path and will not extrude{}",
                        self.feature.0,
                        if chain.segments.len() == 1 { "the curve".to_owned() } else { format!("the chain of {} curves", chain.segments.len()) },
                        self.ents[chain.first].name,
                        mm(gap),
                        if explicit { format!(" (declared by close({label:?}))") } else { String::new() }
                    ));
                    let start = chain.segments[0].start();
                    sketch.push_path(
                        Path { start, segments: chain.segments },
                        format!("{}/path[{label}]", self.feature.0),
                    );
                }
            }
        }
        (sketch, warnings, errors)
    }

    /// Greedy chaining of the given curves: start at the lowest unused index —
    /// so the answer does not depend on iteration order or hashing — and keep
    /// attaching whichever unused curve touches the open end.
    fn chains(&self, candidates: &[usize], used: &mut [bool], tol: f64) -> Vec<Chain> {
        let mut out = Vec::new();
        for &seed in candidates {
            if used[seed] {
                continue;
            }
            let Some((mut segs, start, mut end)) = self.segment_of(seed, false) else {
                continue;
            };
            used[seed] = true;
            let mut closed = near(start, end, tol);
            while !closed {
                let mut found = None;
                for &c in candidates {
                    if used[c] {
                        continue;
                    }
                    if let Some((s, a, b)) = self.segment_of(c, false) {
                        if near(a, end, tol) {
                            found = Some((c, s, b));
                            break;
                        }
                        if near(b, end, tol) {
                            // It touches at its far end, so walk it backwards.
                            let (s, _, b2) = self.segment_of(c, true).expect("reversible");
                            found = Some((c, s, b2));
                            break;
                        }
                    }
                }
                match found {
                    Some((c, s, b)) => {
                        used[c] = true;
                        segs.extend(s);
                        end = b;
                        closed = near(start, end, tol);
                    }
                    None => break,
                }
            }
            let gap = (fmath::powi(end.0 - start.0, 2) + fmath::powi(end.1 - start.1, 2)).sqrt();
            // Snap the last endpoint exactly onto the first: the solver's
            // answer is good to ~1e-12 of the sketch size, but `Loop::is_closed`
            // wants 1e-9 in absolute millimetres.
            if closed {
                snap(&mut segs);
            }
            out.push(Chain { first: seed, segments: segs, closed, gap });
        }
        out
    }

    /// The segment(s) one curve contributes, plus its start and end points.
    /// `reverse` walks it the other way, which is what a chain needs when it
    /// meets a curve at its far end.
    fn segment_of(&self, index: usize, reverse: bool) -> Option<(Vec<Segment>, (f64, f64), (f64, f64))> {
        let e = &self.ents[index];
        let (a, b) = (
            self.sys.position(PointRef::Start(e.id))?,
            self.sys.position(PointRef::End(e.id))?,
        );
        let (a, b) = if reverse { (b, a) } else { (a, b) };
        let segs = match e.kind {
            "line" => vec![Segment::Line { a: P2::new(a.0, a.1), b: P2::new(b.0, b.1) }],
            "arc" => {
                let c = self.sys.position(PointRef::Center(e.id))?;
                let r = self.sys.radius(e.id)?;
                let (t0, t1) = self.sys.arc_angles(cadpole_sketch_solve::ArcId(e.id))?;
                let mut sweep = (t1 - t0).rem_euclid(std::f64::consts::TAU);
                if sweep <= 1e-12 {
                    sweep = std::f64::consts::TAU;
                }
                // An arc of more than about half a turn has its endpoints close
                // together, where the three-point circle through them is badly
                // conditioned; split it so every emitted arc stays short.
                let parts = if sweep > std::f64::consts::PI { 2 } else { 1 };
                let mut segs = Vec::new();
                for k in 0..parts {
                    let (u0, u1) = (
                        t0 + sweep * k as f64 / parts as f64,
                        t0 + sweep * (k + 1) as f64 / parts as f64,
                    );
                    let at = |t: f64| P2::new(c.0 + r * fmath::cos(t), c.1 + r * fmath::sin(t));
                    segs.push(Segment::Arc { a: at(u0), b: at(u1), via: at((u0 + u1) / 2.0) });
                }
                if reverse {
                    segs = segs
                        .into_iter()
                        .rev()
                        .map(|s| match s {
                            Segment::Arc { a, b, via } => Segment::Arc { a: b, b: a, via },
                            other => other,
                        })
                        .collect();
                }
                segs
            }
            _ => return None,
        };
        Some((segs, a, b))
    }

    /// The sketch's overall size, for the chaining tolerance.
    fn size(&self) -> f64 {
        let mut lo = (f64::MAX, f64::MAX);
        let mut hi = (f64::MIN, f64::MIN);
        let mut any = false;
        for e in &self.ents {
            let refs = match e.kind {
                "point" => vec![PointRef::Point(PointId(e.id))],
                "circle" => vec![PointRef::Center(e.id)],
                _ => vec![PointRef::Start(e.id), PointRef::End(e.id), PointRef::Center(e.id)],
            };
            for r in refs {
                if let Some((x, y)) = self.sys.position(r) {
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                    any = true;
                }
            }
        }
        if any { (fmath::powi(hi.0 - lo.0, 2) + fmath::powi(hi.1 - lo.1, 2)).sqrt() } else { 1.0 }
    }
}

struct Chain {
    first: usize,
    segments: Vec<Segment>,
    closed: bool,
    gap: f64,
}

fn near(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
}

/// Make the last segment end exactly where the first one starts.
fn snap(segs: &mut [Segment]) {
    let Some(first) = segs.first().map(|s| s.start()) else { return };
    if let Some(last) = segs.last_mut() {
        match last {
            Segment::Line { b, .. } | Segment::Arc { b, .. } | Segment::Bezier { b, .. } => *b = first,
        }
    }
}

/// A length for a message: millimetres, rounded to something a person reads.
fn mm(v: f64) -> String {
    let r = (v.abs() * 1e4).round() / 1e4;
    fmt_num(r)
}

fn quoted(labels: &[String]) -> String {
    labels.iter().map(|l| format!("{l:?}")).collect::<Vec<_>>().join(", ")
}

// ------------------------------------------------------------- Starlark

/// A handle to a constrained sketch. The sketch itself lives in the
/// evaluation [`Store`](crate::store::Store), so two handles to the same
/// sketch see the same entities — `s.point(...)` adds to `s`, it does not
/// return a new sketch the way `sketch().rect()` does.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KConstrained {
    /// Index into the store's sketch table.
    pub index: usize,
    pub feature: String,
}
starlark_simple_value!(KConstrained);

impl fmt::Display for KConstrained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "constrained_sketch({})", self.feature)
    }
}

/// One entity of a constrained sketch: a point, a line, an arc or a circle,
/// under the name the script gave it.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KEntity {
    pub sketch: usize,
    pub index: usize,
    pub part: Part,
    pub kind: String,
    pub name: String,
}
starlark_simple_value!(KEntity);

impl fmt::Display for KEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:?}{}", self.kind, self.name, self.part.suffix())
    }
}

impl KEntity {
    fn with_part(&self, part: Part) -> KEntity {
        KEntity { part, ..self.clone() }
    }
    fn operand(&self) -> Operand {
        Operand::Entity { index: self.index, part: self.part }
    }
}

#[starlark_value(type = "SketchEntity")]
impl<'v> StarlarkValue<'v> for KEntity {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.name.as_str())),
            // "point", "line", "arc" or "circle".
            "kind" => Some(heap.alloc(self.kind.as_str())),
            // The point-like parts of this entity, for constraints:
            // `s.constrain("coincident", left.end, top.start)`.
            "start" => Some(heap.alloc(self.with_part(Part::Start))),
            "end" => Some(heap.alloc(self.with_part(Part::End))),
            "center" => Some(heap.alloc(self.with_part(Part::Center))),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["name", "kind", "start", "end", "center"].iter().map(|s| s.to_string()).collect()
    }
}

/// Run `f` against the sketch this handle points at.
fn with<R>(
    this: &KConstrained,
    eval: &Evaluator<'_, '_, '_>,
    f: impl FnOnce(&mut CSketch) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let st = store(eval)?;
    let mut sketches = st.sketches.borrow_mut();
    let cs = sketches
        .get_mut(this.index)
        .ok_or_else(|| anyhow::anyhow!("constrained sketch {} is gone", this.feature))?;
    f(cs)
}

/// Run `f` against the sketch *and* the document, for the operations that
/// solve and therefore produce diagnostics.
fn with_ctx<R>(
    this: &KConstrained,
    eval: &Evaluator<'_, '_, '_>,
    f: impl FnOnce(&mut CSketch, &mut Context) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let st = store(eval)?;
    let mut sketches = st.sketches.borrow_mut();
    let cs = sketches
        .get_mut(this.index)
        .ok_or_else(|| anyhow::anyhow!("constrained sketch {} is gone", this.feature))?;
    let mut ctx = st.ctx.borrow_mut();
    f(cs, &mut ctx)
}

/// Keep the feature's argument summary in step with the sketch.
fn note_counts(this: &KConstrained, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<()> {
    let st = store(eval)?;
    let (ents, cons) = {
        let sketches = st.sketches.borrow();
        let cs = &sketches[this.index];
        (cs.entity_count(), cs.constraint_count())
    };
    let mut ctx = st.ctx.borrow_mut();
    let fid = FeatureId(this.feature.clone());
    ctx.set_feature_arg(&fid, "entities", ents);
    ctx.set_feature_arg(&fid, "constraints", cons);
    Ok(())
}

/// Turn a Starlark argument into a constraint operand: a number, an entity, or
/// the name of an entity of this sketch.
fn operand(this: &KConstrained, v: Value<'_>, eval: &Evaluator<'_, '_, '_>, what: &str) -> anyhow::Result<Operand> {
    if let Some(e) = v.downcast_ref::<KEntity>() {
        if e.sketch != this.index {
            anyhow::bail!("{what}: entity {:?} belongs to another sketch", e.name);
        }
        return Ok(e.operand());
    }
    if let Some(s) = v.unpack_str() {
        let index = with(this, eval, |cs| cs.lookup(s))?;
        return Ok(Operand::Entity { index, part: Part::Whole });
    }
    if let Some(n) = starlark::values::UnpackValue::unpack_value(v).map_err(|e| e.into_anyhow())? {
        let n: UnpackFloat = n;
        return Ok(Operand::Number(finite(n.0, what)?));
    }
    anyhow::bail!("{what}: expected a sketch entity, an entity name or a number, got {}", v.get_type())
}

fn entity_index(this: &KConstrained, v: Value<'_>, eval: &Evaluator<'_, '_, '_>, what: &str) -> anyhow::Result<usize> {
    match operand(this, v, eval, what)? {
        Operand::Entity { index, part: Part::Whole } => Ok(index),
        Operand::Entity { index, part } => {
            let name = with(this, eval, |cs| Ok(cs.name_of(index).to_owned()))?;
            anyhow::bail!("{what}: expected a whole entity, not {name}{}", part.suffix())
        }
        Operand::Number(n) => anyhow::bail!("{what}: expected a sketch entity, got {}", fmt_num(n)),
    }
}

fn handle(this: &KConstrained, index: usize, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<KEntity> {
    let (kind, name) = with(this, eval, |cs| Ok((cs.kind_of(index), cs.name_of(index).to_owned())))?;
    Ok(KEntity { sketch: this.index, index, part: Part::Whole, kind: kind.to_owned(), name })
}

#[starlark_value(type = "ConstrainedSketch")]
impl<'v> StarlarkValue<'v> for KConstrained {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("constrained_methods", constrained_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        vec!["id".to_string()]
    }
}

#[starlark_module]
fn constrained_methods(builder: &mut MethodsBuilder) {
    /// Add a named point at `(x, y)`. That position is only a starting guess
    /// for the solver unless `fix=True`, which pins it exactly (the usual way
    /// to anchor a sketch to its origin). `construction=True` makes it shape
    /// the solve without becoming geometry.
    fn point<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = pos)] y: UnpackFloat,
        #[starlark(require = named)] name: &str,
        #[starlark(require = named, default = false)] fix: bool,
        #[starlark(require = named, default = false)] construction: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEntity> {
        let span = call_span(eval);
        let (x, y) = (num(&x, "point: x")?, num(&y, "point: y")?);
        let index = with(this, eval, |cs| cs.add_point(name, x, y, fix, construction, span))?;
        note_counts(this, eval)?;
        handle(this, index, eval)
    }

    /// Add a named line between two points of this sketch. The line owns no
    /// coordinates: two lines given the same end point share that corner
    /// exactly and need no `coincident` constraint to stay together.
    fn line<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] name: &str,
        #[starlark(require = named, default = false)] construction: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEntity> {
        let a = entity_index(this, a, eval, "line: a")?;
        let b = entity_index(this, b, eval, "line: b")?;
        let index = with(this, eval, |cs| cs.add_line(name, a, b, construction))?;
        note_counts(this, eval)?;
        handle(this, index, eval)
    }

    /// Add a named circle of radius `r` about a point of this sketch.
    fn circle<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] center: Value<'v>,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] name: &str,
        #[starlark(require = named, default = false)] construction: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEntity> {
        let c = entity_index(this, center, eval, "circle: center")?;
        let r = num(&r, "circle: r")?;
        let index = with(this, eval, |cs| cs.add_circle(name, c, r, construction))?;
        note_counts(this, eval)?;
        handle(this, index, eval)
    }

    /// Add a named arc about a point of this sketch, counter-clockwise from
    /// `start` to `end` degrees. Its `.start` and `.end` are points like any
    /// other, so a line can be made coincident and tangent to them and the
    /// arc's angles will move to suit.
    fn arc<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] center: Value<'v>,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] start: UnpackFloat,
        #[starlark(require = named)] end: UnpackFloat,
        #[starlark(require = named)] name: &str,
        #[starlark(require = named, default = false)] construction: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEntity> {
        let c = entity_index(this, center, eval, "arc: center")?;
        let (r, start, end) = (num(&r, "arc: r")?, num(&start, "arc: start")?, num(&end, "arc: end")?);
        let index = with(this, eval, |cs| cs.add_arc(name, c, r, start, end, construction))?;
        note_counts(this, eval)?;
        handle(this, index, eval)
    }

    /// Constrain entities of this sketch. `kind` is one of `coincident`,
    /// `horizontal`, `vertical`, `distance`, `angle`, `parallel`,
    /// `perpendicular`, `tangent`, `equal`, `symmetric`, `fix`, `radius`,
    /// `concentric`, `midpoint`, `on_curve`; `a`, `b` and `c` are the
    /// operands that kind takes (entities, `entity.start` / `.end` /
    /// `.center`, entity names, or a dimension). `name=` labels the
    /// constraint in diagnostics; without it the label is the kind and the
    /// operands, as in `distance(a, b)`. Returns the sketch, so calls chain.
    fn constrain<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] kind: &str,
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Option<Value<'v>>,
        #[starlark(require = pos)] c: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KConstrained> {
        let span = call_span(eval);
        let mut ops = vec![operand(this, a, eval, "constrain")?];
        for (i, v) in [b, c].into_iter().flatten().enumerate() {
            ops.push(operand(this, v, eval, &format!("constrain argument {}", i + 3))?);
        }
        with(this, eval, |cs| cs.constrain(kind, &ops, name, span))?;
        note_counts(this, eval)?;
        Ok(this.clone())
    }

    /// Declare that these entities bound one closed loop. Only needed when
    /// the curves could be chained more than one way; otherwise every
    /// non-construction curve is chained into loops automatically.
    fn close<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] entities: UnpackList<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KConstrained> {
        let mut idx = Vec::new();
        for (i, v) in entities.items.iter().enumerate() {
            idx.push(entity_index(this, *v, eval, &format!("close: entity {}", i + 1))?);
        }
        with(this, eval, |cs| cs.declare_loop(name, idx))?;
        Ok(this.clone())
    }

    /// Solve now instead of at first use, and return the sketch. Handy when a
    /// script wants the diagnostics (or `at()`) before it extrudes anything.
    fn solve<'v>(
        this: &KConstrained,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KConstrained> {
        with_ctx(this, eval, |cs, ctx| cs.solved_sketch(ctx, "solve"))?;
        Ok(this.clone())
    }

    /// Degrees of freedom left after solving: 0 is fully constrained.
    fn dof<'v>(this: &KConstrained, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<i32> {
        Ok(with_ctx(this, eval, |cs, ctx| cs.dof(ctx))? as i32)
    }

    /// The solved position of a point, or of a line's or arc's `.start` /
    /// `.end`, or of a circle's centre, as `(x, y)`.
    fn at<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] entity: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let op = operand(this, entity, eval, "at")?;
        let (x, y) = with_ctx(this, eval, |cs, ctx| cs.position(ctx, &op))?;
        Ok(heap.alloc((x, y)))
    }

    /// The solved radius of a circle or an arc.
    fn radius_of<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] entity: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let op = operand(this, entity, eval, "radius_of")?;
        with_ctx(this, eval, |cs, ctx| cs.radius(ctx, &op))
    }

    /// The entity of this sketch with that name.
    fn entity<'v>(
        this: &KConstrained,
        #[starlark(require = pos)] name: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEntity> {
        let index = with(this, eval, |cs| cs.lookup(name))?;
        handle(this, index, eval)
    }

    /// The names of every entity, in creation order.
    fn entities<'v>(this: &KConstrained, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Vec<String>> {
        with(this, eval, |cs| Ok(cs.entity_names()))
    }

    /// The labels of every constraint, in the order they were added — the
    /// names diagnostics print.
    fn constraints<'v>(this: &KConstrained, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Vec<String>> {
        with(this, eval, |cs| Ok(cs.constraint_labels()))
    }

    /// Solve and hand back an ordinary `Sketch` of loops — the same value
    /// `sketch().rect(...)` produces. `extrude` and `revolve` accept the
    /// constrained sketch directly, so this is only needed to inspect the
    /// result or to hold on to the geometry.
    fn sketch<'v>(this: &KConstrained, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KSketch> {
        let sketch = with_ctx(this, eval, |cs, ctx| cs.solved_sketch(ctx, "sketch"))?;
        Ok(KSketch::new(this.feature.clone(), sketch))
    }

    /// The sketch plane.
    fn plane<'v>(this: &KConstrained, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KPlane> {
        with(this, eval, |cs| Ok(KPlane(cs.plane)))
    }
}

/// Resolve a sketch argument that may be a constrained sketch.
///
/// This is where "solve on use" happens: `extrude` and `revolve` call it, so
/// the solve — and the diagnostics that come with it — belongs to the
/// operation that needed the geometry, and building the sketch costs nothing
/// until then. Returns `None` when the value is not a constrained sketch, so
/// the caller can fall back to the literal [`KSketch`].
pub fn as_solved_sketch(
    v: Value<'_>,
    eval: &Evaluator<'_, '_, '_>,
    what: &str,
) -> anyhow::Result<Option<(String, Sketch)>> {
    let Some(cs) = v.downcast_ref::<KConstrained>() else { return Ok(None) };
    let sketch = with_ctx(cs, eval, |c, ctx| c.solved_sketch(ctx, what))?;
    Ok(Some((cs.feature.clone(), sketch)))
}

/// Create a constrained sketch and the feature that stands for it.
pub fn new_constrained(
    plane: Plane,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KConstrained> {
    let span = call_span(eval);
    let st = store(eval)?;
    let feature = {
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("plane".into(), format!("{}", KPlane(plane)));
        // The tree shows it as a sketch — it is one — but the UI and the
        // sketch tool need to know it is driven by constraints.
        args.insert("constrained".into(), "true".into());
        args.insert("entities".into(), "0".into());
        args.insert("constraints".into(), "0".into());
        ctx.new_feature("sketch", name.map(str::to_owned), vec![], args, span.clone())?
    };
    let mut sketches = st.sketches.borrow_mut();
    let index = sketches.len();
    sketches.push(CSketch::new(plane, feature.clone(), span));
    Ok(KConstrained { index, feature: feature.0 })
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    /// A line whose two ends are one point — drawn that way, or collapsed by
    /// a `coincident` — is not a loop. It used to reach the kernel as a
    /// one-edge loop from a vertex to itself and panic the evaluator; it must
    /// be a diagnostic naming the line instead.
    #[test]
    fn a_line_drawn_from_a_point_to_itself_is_named_not_a_panic() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="s")
a = s.point(0, 0, name="a")
b = s.point(10, 0, name="b")
c = s.point(10, 10, name="c")
s.line(a, a, name="z")
s.line(a, b, name="l1")
s.line(b, c, name="l2")
s.line(c, a, name="l3")
e = extrude(s, 2, name="e")
"#);
        let errs = errors(&ctx);
        assert!(
            errs.iter().any(|m| m.contains("\"z\"") && m.contains("zero length")),
            "{errs:?}"
        );
        assert!(!ctx.bodies.iter().any(|b| b.name == "e"));
    }

    #[test]
    fn a_line_collapsed_by_a_constraint_is_named_not_a_panic() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="s")
a = s.point(0, 0, name="a", fix=True)
b = s.point(10, 0, name="b")
c = s.point(10, 10, name="c", fix=True)
s.line(a, b, name="l1")
s.line(b, c, name="l2")
s.line(c, a, name="l3")
s.constrain("coincident", a, b)
e = extrude(s, 2, name="e")
"#);
        let errs = errors(&ctx);
        assert!(
            errs.iter().any(|m| m.contains("\"l1\"") && m.contains("zero length")),
            "{errs:?}"
        );
        assert!(!ctx.bodies.iter().any(|b| b.name == "e"));
    }

    /// The solve stands when a curve collapses: `dof()` and `at()` still
    /// answer, which is how the script finds out why the line collapsed,
    /// while a build from the sketch fails naming the line, once.
    #[test]
    fn a_collapsed_line_still_lets_the_solve_be_queried() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="s")
a = s.point(0, 0, name="a", fix=True)
b = s.point(10, 0, name="b")
c = s.point(10, 10, name="c", fix=True)
s.line(a, b, name="l1")
s.line(b, c, name="l2")
s.line(c, a, name="l3")
s.constrain("coincident", a, b)
print("dof", s.dof())
p = s.at(b)
print("b", abs(p[0]) < 1e-6 and abs(p[1]) < 1e-6)
e = extrude(s, 2, name="e")
"#);
        let infos: Vec<&str> = ctx.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(infos.iter().any(|m| m.contains("print: dof 0")), "{infos:?}");
        assert!(infos.iter().any(|m| m.contains("print: b True")), "{infos:?}");
        let errs = errors(&ctx);
        let named: Vec<&String> = errs.iter().filter(|m| m.contains("\"l1\"") && m.contains("zero length")).collect();
        assert_eq!(named.len(), 1, "named once: {errs:?}");
        assert!(named[0].starts_with("extrude: sketch \"s\": line \"l1\""), "{errs:?}");
        assert!(!errs.iter().any(|m| m.contains("cannot be solved")), "{errs:?}");
        assert!(!ctx.bodies.iter().any(|b| b.name == "e"));
    }

    /// The control: an ordinary constrained triangle still extrudes.
    #[test]
    fn a_proper_constrained_triangle_still_extrudes() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="s")
a = s.point(0, 0, name="a")
b = s.point(10, 0, name="b")
c = s.point(10, 10, name="c")
s.line(a, b, name="l1")
s.line(b, c, name="l2")
s.line(c, a, name="l3")
e = extrude(s, 2, name="e")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.bodies.iter().any(|b| b.name == "e"));
    }
}
