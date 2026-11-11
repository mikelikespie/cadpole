//! Constraints: their residuals and their analytic Jacobian rows.
//!
//! Every constraint is a function of the parameter vector that must equal
//! zero. A constraint contributes one row per scalar equation, and each row
//! carries its residual, the unit that residual is measured in (so the solver
//! can scale millimetres and radians onto the same footing) and the exact
//! partial derivative with respect to every parameter it touches. Nothing is
//! differentiated numerically.

use crate::entity::{
    CurveRef, Entities, EntityId, EntityKind, LineId, PointId, PointJac, PointRef, Unit,
};
use core::fmt;

/// Identifies a constraint in a [`System`](crate::System).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ConstraintId(pub u32);

impl fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Anything that can go wrong when a constraint is added.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The entity id does not exist in this system.
    UnknownEntity(EntityId),
    /// `PointRef::Center` of a line, `Start` of a circle, and similar.
    BadPointRef(PointRef),
    /// A radius, concentricity or equal-radius constraint on something that is
    /// not a circle or an arc.
    NotRadial(EntityId),
    /// Two straight lines are parallel or crossing, never tangent.
    TangentNeedsACurve,
    /// A non-positive dimension where only a positive one makes sense.
    BadDimension(f64),
    /// A radius that is not a positive length: zero would shrink the circle
    /// or arc to a point, which bounds nothing.
    BadRadius(f64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownEntity(e) => write!(f, "no entity {e} in this system"),
            Error::BadPointRef(r) => write!(f, "{r:?} does not name a point on that entity"),
            Error::NotRadial(e) => write!(f, "entity {e} is not a circle or an arc"),
            Error::TangentNeedsACurve => {
                write!(f, "tangency needs at least one circle or arc; two lines cannot be tangent")
            }
            Error::BadDimension(d) => write!(f, "dimension {d} must be finite and non-negative"),
            Error::BadRadius(r) => {
                write!(f, "radius {r} must be a positive length; a radius of 0 shrinks the curve to a point")
            }
        }
    }
}

impl std::error::Error for Error {}

/// One geometric constraint.
///
/// Build these with the constructors below rather than the variants; the
/// constructors take `impl Into<…>`, so a `PointId` works wherever a
/// [`PointRef`] is wanted and a `CircleId` wherever a [`CurveRef`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum Constraint {
    /// Two points are the same point. 2 rows.
    Coincident { a: PointRef, b: PointRef },
    /// A line's endpoints share a `y`. 1 row.
    Horizontal(LineId),
    /// A line's endpoints share an `x`. 1 row.
    Vertical(LineId),
    /// The distance between two points is `d`. 1 row.
    Distance { a: PointRef, b: PointRef, d: f64 },
    /// The perpendicular distance from a point to an (infinite) line is `d`.
    /// 1 row.
    PointLineDistance { p: PointRef, line: LineId, d: f64 },
    /// The counter-clockwise angle from line `a` to line `b`. 1 row.
    Angle { a: LineId, b: LineId, radians: f64 },
    /// Two lines have the same direction (up to sign). 1 row.
    Parallel { a: LineId, b: LineId },
    /// Two lines meet at a right angle. 1 row.
    Perpendicular { a: LineId, b: LineId },
    /// Two curves touch. 1 row.
    ///
    /// With `at: None` the residual is a distance between the curves, which is
    /// the right formulation when they are apart. When the two curves already
    /// *share* that point — a line ending on the arc it runs into, as in a
    /// slot — the distance residual is stationary at the solution and its
    /// gradient vanishes, so pass `at: Some(point)` and the residual becomes
    /// "the two tangent directions at this point are parallel", which is well
    /// conditioned there. [`Constraint::tangent_at`] is the constructor.
    Tangent { a: CurveRef, b: CurveRef, at: Option<PointRef> },
    /// Two line segments have the same length. 1 row.
    EqualLength { a: LineId, b: LineId },
    /// Two circles or arcs have the same radius. 1 row.
    EqualRadius { a: EntityId, b: EntityId },
    /// Two points are mirror images across a line. 2 rows.
    Symmetric { a: PointRef, b: PointRef, axis: LineId },
    /// Hold every parameter of an entity at the value it had when the
    /// constraint was added. One row per parameter (a point 2, a line 4, a
    /// circle 3, an arc 5).
    Fix(EntityId),
    /// Hold a point at a given position. 2 rows. This is what dragging adds.
    PointAt { p: PointRef, x: f64, y: f64 },
    /// A circle or arc has radius `r`. 1 row.
    Radius { curve: EntityId, r: f64 },
    /// Two circles or arcs share a centre. 2 rows.
    Concentric { a: EntityId, b: EntityId },
    /// A point is the midpoint of a line's two endpoints. 2 rows.
    Midpoint { p: PointRef, line: LineId },
    /// A point lies on a curve (the infinite line, or the full circle an arc
    /// lies on). 1 row.
    OnCurve { p: PointRef, curve: CurveRef },
}

impl Constraint {
    pub fn coincident(a: impl Into<PointRef>, b: impl Into<PointRef>) -> Constraint {
        Constraint::Coincident { a: a.into(), b: b.into() }
    }
    pub fn horizontal(line: LineId) -> Constraint {
        Constraint::Horizontal(line)
    }
    pub fn vertical(line: LineId) -> Constraint {
        Constraint::Vertical(line)
    }
    pub fn distance(a: impl Into<PointRef>, b: impl Into<PointRef>, d: f64) -> Constraint {
        Constraint::Distance { a: a.into(), b: b.into(), d }
    }
    pub fn point_line_distance(p: impl Into<PointRef>, line: LineId, d: f64) -> Constraint {
        Constraint::PointLineDistance { p: p.into(), line, d }
    }
    /// The counter-clockwise angle from `a` to `b`, in radians.
    pub fn angle(a: LineId, b: LineId, radians: f64) -> Constraint {
        Constraint::Angle { a, b, radians }
    }
    /// The counter-clockwise angle from `a` to `b`, in degrees — the unit the
    /// Starlark API uses.
    pub fn angle_degrees(a: LineId, b: LineId, degrees: f64) -> Constraint {
        Constraint::Angle { a, b, radians: degrees.to_radians() }
    }
    pub fn parallel(a: LineId, b: LineId) -> Constraint {
        Constraint::Parallel { a, b }
    }
    pub fn perpendicular(a: LineId, b: LineId) -> Constraint {
        Constraint::Perpendicular { a, b }
    }
    /// Tangency between two curves that do not already touch at a known point.
    pub fn tangent(a: impl Into<CurveRef>, b: impl Into<CurveRef>) -> Constraint {
        Constraint::Tangent { a: a.into(), b: b.into(), at: None }
    }
    /// Tangency at a point the two curves already share (an arc endpoint that
    /// a line starts from, for instance). See [`Constraint::Tangent`].
    pub fn tangent_at(
        a: impl Into<CurveRef>,
        b: impl Into<CurveRef>,
        at: impl Into<PointRef>,
    ) -> Constraint {
        Constraint::Tangent { a: a.into(), b: b.into(), at: Some(at.into()) }
    }
    pub fn equal_length(a: LineId, b: LineId) -> Constraint {
        Constraint::EqualLength { a, b }
    }
    pub fn equal_radius(a: impl Into<EntityId>, b: impl Into<EntityId>) -> Constraint {
        Constraint::EqualRadius { a: a.into(), b: b.into() }
    }
    pub fn symmetric(a: impl Into<PointRef>, b: impl Into<PointRef>, axis: LineId) -> Constraint {
        Constraint::Symmetric { a: a.into(), b: b.into(), axis }
    }
    pub fn fix(e: impl Into<EntityId>) -> Constraint {
        Constraint::Fix(e.into())
    }
    pub fn point_at(p: impl Into<PointRef>, x: f64, y: f64) -> Constraint {
        Constraint::PointAt { p: p.into(), x, y }
    }
    pub fn radius(curve: impl Into<EntityId>, r: f64) -> Constraint {
        Constraint::Radius { curve: curve.into(), r }
    }
    pub fn concentric(a: impl Into<EntityId>, b: impl Into<EntityId>) -> Constraint {
        Constraint::Concentric { a: a.into(), b: b.into() }
    }
    pub fn midpoint(p: impl Into<PointRef>, line: LineId) -> Constraint {
        Constraint::Midpoint { p: p.into(), line }
    }
    pub fn on_curve(p: impl Into<PointRef>, curve: impl Into<CurveRef>) -> Constraint {
        Constraint::OnCurve { p: p.into(), curve: curve.into() }
    }

    /// The short name used in diagnostics: `distance#7`, `tangent#12`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Constraint::Coincident { .. } => "coincident",
            Constraint::Horizontal(_) => "horizontal",
            Constraint::Vertical(_) => "vertical",
            Constraint::Distance { .. } => "distance",
            Constraint::PointLineDistance { .. } => "point_line_distance",
            Constraint::Angle { .. } => "angle",
            Constraint::Parallel { .. } => "parallel",
            Constraint::Perpendicular { .. } => "perpendicular",
            Constraint::Tangent { .. } => "tangent",
            Constraint::EqualLength { .. } => "equal",
            Constraint::EqualRadius { .. } => "equal_radius",
            Constraint::Symmetric { .. } => "symmetric",
            Constraint::Fix(_) => "fix",
            Constraint::PointAt { .. } => "point_at",
            Constraint::Radius { .. } => "radius",
            Constraint::Concentric { .. } => "concentric",
            Constraint::Midpoint { .. } => "midpoint",
            Constraint::OnCurve { .. } => "on_curve",
        }
    }
}

/// A constraint as stored, with everything that had to be resolved against the
/// sketch at the moment it was added.
#[derive(Clone, Debug)]
pub(crate) struct ConstraintRecord {
    pub id: ConstraintId,
    pub kind: Constraint,
    pub name: Option<String>,
    /// `Fix` only: the `(parameter, value, unit)` triples it holds.
    pub fixed: Vec<(usize, f64, Unit)>,
    /// Circle-to-circle tangency only: internal (one inside the other) or
    /// external. Chosen from the sketch as it stood when the constraint was
    /// added, so the solve stays deterministic.
    pub tangent_internal: bool,
    pub rows: usize,
    /// Relative importance in the least-squares objective. Always 1 for a
    /// constraint the user added; dragging uses a lighter temporary pin so
    /// that the sketch's real constraints win wherever the two disagree.
    pub weight: f64,
}

impl ConstraintRecord {
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("{}#{}", self.kind.type_name(), self.id.0),
        }
    }
}

/// One scalar equation.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub residual: f64,
    pub unit: Unit,
    pub terms: Vec<(usize, f64)>,
}

#[derive(Default)]
struct Terms(Vec<(usize, f64)>);

impl Terms {
    fn add(&mut self, idx: usize, coeff: f64) {
        if coeff == 0.0 {
            return;
        }
        for t in self.0.iter_mut() {
            if t.0 == idx {
                t.1 += coeff;
                return;
            }
        }
        self.0.push((idx, coeff));
    }

    /// Chain `∂f/∂p = (cx, cy)` through a point's own Jacobian.
    fn point(&mut self, j: &PointJac, cx: f64, cy: f64) {
        for &(idx, dx, dy) in j.terms() {
            self.add(idx, cx * dx + cy * dy);
        }
    }

    fn take(self) -> Vec<(usize, f64)> {
        self.0
    }
}

/// How many rows a constraint contributes.
pub(crate) fn row_count(ents: &Entities, c: &Constraint) -> usize {
    match c {
        Constraint::Coincident { .. }
        | Constraint::Symmetric { .. }
        | Constraint::PointAt { .. }
        | Constraint::Concentric { .. }
        | Constraint::Midpoint { .. } => 2,
        Constraint::Fix(e) => fix_params(ents, *e).len(),
        _ => 1,
    }
}

/// Which parameters `fix` holds, and in what unit.
pub(crate) fn fix_params(ents: &Entities, e: EntityId) -> Vec<(usize, Unit)> {
    let rec = ents.get(e);
    let mut out = Vec::new();
    let push_point = |p: PointId, out: &mut Vec<(usize, Unit)>| {
        let b = ents.get(p.0).base;
        out.push((b, Unit::Length));
        out.push((b + 1, Unit::Length));
    };
    match rec.kind {
        EntityKind::Point => push_point(PointId(e), &mut out),
        EntityKind::Line { a, b } => {
            push_point(a, &mut out);
            push_point(b, &mut out);
        }
        EntityKind::Circle { center } => {
            push_point(center, &mut out);
            out.push((rec.base, Unit::Length));
        }
        EntityKind::Arc { center } => {
            push_point(center, &mut out);
            out.push((rec.base, Unit::Length));
            out.push((rec.base + 1, Unit::Angle));
            out.push((rec.base + 2, Unit::Angle));
        }
    }
    out
}

/// Check a constraint against the entity table before it is stored.
pub(crate) fn validate(ents: &Entities, c: &Constraint) -> Result<(), Error> {
    let n = ents.recs.len() as u32;
    let check = |e: EntityId| if e.0 < n { Ok(()) } else { Err(Error::UnknownEntity(e)) };
    let point = |r: PointRef| -> Result<(), Error> {
        match r {
            PointRef::Point(p) => check(p.0)?,
            PointRef::Center(e) | PointRef::Start(e) | PointRef::End(e) => check(e)?,
        }
        if ents.valid_point_ref(r) { Ok(()) } else { Err(Error::BadPointRef(r)) }
    };
    let radial = |e: EntityId| -> Result<(), Error> {
        check(e)?;
        match ents.get(e).kind {
            EntityKind::Circle { .. } | EntityKind::Arc { .. } => Ok(()),
            _ => Err(Error::NotRadial(e)),
        }
    };
    let dim = |d: f64| -> Result<(), Error> {
        if d.is_finite() && d >= 0.0 { Ok(()) } else { Err(Error::BadDimension(d)) }
    };
    match c {
        Constraint::Coincident { a, b } | Constraint::Symmetric { a, b, .. } => {
            point(*a)?;
            point(*b)?;
            if let Constraint::Symmetric { axis, .. } = c {
                check(axis.0)?;
            }
        }
        Constraint::Horizontal(l) | Constraint::Vertical(l) => check(l.0)?,
        Constraint::Distance { a, b, d } => {
            point(*a)?;
            point(*b)?;
            dim(*d)?;
        }
        Constraint::PointLineDistance { p, line, d } => {
            point(*p)?;
            check(line.0)?;
            dim(*d)?;
        }
        Constraint::Angle { a, b, .. }
        | Constraint::Parallel { a, b }
        | Constraint::Perpendicular { a, b }
        | Constraint::EqualLength { a, b } => {
            check(a.0)?;
            check(b.0)?;
        }
        Constraint::Tangent { a, b, at } => {
            check(a.entity())?;
            check(b.entity())?;
            if matches!(a, CurveRef::Line(_)) && matches!(b, CurveRef::Line(_)) {
                return Err(Error::TangentNeedsACurve);
            }
            if let Some(p) = at {
                point(*p)?;
            }
        }
        Constraint::EqualRadius { a, b } | Constraint::Concentric { a, b } => {
            radial(*a)?;
            radial(*b)?;
        }
        Constraint::Fix(e) => check(*e)?,
        Constraint::PointAt { p, .. } => point(*p)?,
        Constraint::Radius { curve, r } => {
            radial(*curve)?;
            dim(*r)?;
            if !(*r > 0.0) {
                return Err(Error::BadRadius(*r));
            }
        }
        Constraint::Midpoint { p, line } => {
            point(*p)?;
            check(line.0)?;
        }
        Constraint::OnCurve { p, curve } => {
            point(*p)?;
            check(curve.entity())?;
        }
    }
    Ok(())
}

/// The counter-clockwise angle from `u` to `v`, with `∂θ/∂u` and `∂θ/∂v`.
///
/// `θ = atan2(uₓvᵧ − uᵧvₓ, u·v) = atan2(vᵧ, vₓ) − atan2(uᵧ, uₓ)`, so the
/// derivatives are the familiar `∂atan2/∂` pair and do not need the quotient
/// rule on the cross and dot products.
fn angle_between(u: (f64, f64), v: (f64, f64)) -> Option<(f64, (f64, f64), (f64, f64))> {
    let lu = u.0 * u.0 + u.1 * u.1;
    let lv = v.0 * v.0 + v.1 * v.1;
    if lu <= 0.0 || lv <= 0.0 {
        return None;
    }
    let cross = u.0 * v.1 - u.1 * v.0;
    let d = u.0 * v.0 + u.1 * v.1;
    Some((libm::atan2(cross, d), (u.1 / lu, -u.0 / lu), (-v.1 / lv, v.0 / lv)))
}

/// Wrap into `(−π, π]` so an angle residual is continuous near its target.
fn wrap_pi(a: f64) -> f64 {
    let tau = core::f64::consts::TAU;
    let mut a = a % tau;
    if a > core::f64::consts::PI {
        a -= tau;
    } else if a <= -core::f64::consts::PI {
        a += tau;
    }
    a
}

/// Signed perpendicular distance from `p` to the line through `a`, `b`
/// (positive to the left of `a → b`), with its derivatives.
struct LineDist {
    sd: f64,
    dp: (f64, f64),
    da: (f64, f64),
    db: (f64, f64),
}

fn line_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> Option<LineDist> {
    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
    let l2 = ux * ux + uy * uy;
    if l2 <= 0.0 {
        return None;
    }
    let l = l2.sqrt();
    let (wx, wy) = (p.0 - a.0, p.1 - a.1);
    let cross = ux * wy - uy * wx;
    let sd = cross / l;
    Some(LineDist {
        sd,
        dp: (-uy / l, ux / l),
        da: ((-wy + uy) / l + sd * ux / l2, (-ux + wx) / l + sd * uy / l2),
        db: (wy / l - sd * ux / l2, -wx / l - sd * uy / l2),
    })
}

/// The tangent direction of a curve, and how to chain a derivative through it.
enum Dir {
    /// `t = p₁ − p₀` (a line).
    Chord(PointJac, PointJac, (f64, f64)),
    /// `t = perp(p − c)` (a circle or arc at the point `p`).
    Radial(PointJac, PointJac, (f64, f64)),
}

impl Dir {
    fn value(&self) -> (f64, f64) {
        match self {
            Dir::Chord(_, _, v) | Dir::Radial(_, _, v) => *v,
        }
    }
    /// Add `∂f/∂t = (cx, cy)` chained through the parameters behind `t`.
    fn chain(&self, terms: &mut Terms, cx: f64, cy: f64) {
        match self {
            Dir::Chord(j0, j1, _) => {
                terms.point(j1, cx, cy);
                terms.point(j0, -cx, -cy);
            }
            // t = (c_y − p_y, p_x − c_x)
            Dir::Radial(jp, jc, _) => {
                terms.point(jp, cy, -cx);
                terms.point(jc, -cy, cx);
            }
        }
    }
}

/// A line's two endpoints with their Jacobians.
fn line_ends_of(
    ents: &Entities,
    l: LineId,
    x: &[f64],
) -> (((f64, f64), PointJac), ((f64, f64), PointJac)) {
    let (a, b) = match ents.get(l.0).kind {
        EntityKind::Line { a, b } => (a, b),
        _ => unreachable!("validated at add time"),
    };
    let pt = |p: PointId| ents.point(PointRef::Point(p), x).expect("validated at add time");
    (pt(a), pt(b))
}

/// Which function of the angle between two lines a constraint asks for.
enum AngleForm {
    /// `θ − target`, in radians.
    Exact(f64),
    /// `sin θ`: zero when the lines point the same way or opposite ways.
    Parallel,
    /// `cos θ`: zero at a right angle.
    Perpendicular,
}

/// The single row shared by `angle`, `parallel` and `perpendicular`.
fn emit_angle(ents: &Entities, x: &[f64], a: LineId, b: LineId, form: AngleForm, out: &mut Vec<Row>) {
    let ((pa0, ja0), (pa1, ja1)) = line_ends_of(ents, a, x);
    let ((pb0, jb0), (pb1, jb1)) = line_ends_of(ents, b, x);
    let u = (pa1.0 - pa0.0, pa1.1 - pa0.1);
    let v = (pb1.0 - pb0.0, pb1.1 - pb0.1);
    let unit = match form {
        AngleForm::Exact(_) => Unit::Angle,
        _ => Unit::Ratio,
    };
    let Some((theta, du, dv)) = angle_between(u, v) else {
        out.push(Row { residual: 0.0, unit, terms: Vec::new() });
        return;
    };
    // r = g(θ), so every partial is g'(θ) times the partials of θ itself.
    let (residual, dr) = match form {
        AngleForm::Exact(target) => (wrap_pi(theta - target), 1.0),
        AngleForm::Parallel => (libm::sin(theta), libm::cos(theta)),
        AngleForm::Perpendicular => (libm::cos(theta), -libm::sin(theta)),
    };
    let mut t = Terms::default();
    t.point(&ja1, dr * du.0, dr * du.1);
    t.point(&ja0, -dr * du.0, -dr * du.1);
    t.point(&jb1, dr * dv.0, dr * dv.1);
    t.point(&jb0, -dr * dv.0, -dr * dv.1);
    out.push(Row { residual, unit, terms: t.take() });
}

/// Every row of one constraint, evaluated at `x`.
pub(crate) fn rows_of(ents: &Entities, rec: &ConstraintRecord, x: &[f64]) -> Vec<Row> {
    let mut out = Vec::with_capacity(rec.rows);
    let pt = |r: PointRef| ents.point(r, x).expect("validated at add time");
    let line_ends = |l: LineId| {
        let (a, b) = match ents.get(l.0).kind {
            EntityKind::Line { a, b } => (a, b),
            _ => unreachable!("validated at add time"),
        };
        (pt(PointRef::Point(a)), pt(PointRef::Point(b)))
    };
    let zero_row = |unit: Unit| Row { residual: 0.0, unit, terms: Vec::new() };

    match &rec.kind {
        Constraint::Coincident { a, b } => {
            let (pa, ja) = pt(*a);
            let (pb, jb) = pt(*b);
            for (k, (va, vb)) in [(pa.0, pb.0), (pa.1, pb.1)].into_iter().enumerate() {
                let (cx, cy) = if k == 0 { (1.0, 0.0) } else { (0.0, 1.0) };
                let mut t = Terms::default();
                t.point(&ja, cx, cy);
                t.point(&jb, -cx, -cy);
                out.push(Row { residual: va - vb, unit: Unit::Length, terms: t.take() });
            }
        }
        Constraint::Horizontal(l) | Constraint::Vertical(l) => {
            let ((pa, ja), (pb, jb)) = line_ends(*l);
            let horizontal = matches!(rec.kind, Constraint::Horizontal(_));
            let (cx, cy) = if horizontal { (0.0, 1.0) } else { (1.0, 0.0) };
            let mut t = Terms::default();
            t.point(&ja, cx, cy);
            t.point(&jb, -cx, -cy);
            let residual = if horizontal { pa.1 - pb.1 } else { pa.0 - pb.0 };
            out.push(Row { residual, unit: Unit::Length, terms: t.take() });
        }
        Constraint::Distance { a, b, d } => {
            let (pa, ja) = pt(*a);
            let (pb, jb) = pt(*b);
            let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
            let len = (dx * dx + dy * dy).sqrt();
            let (ex, ey) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
            let mut t = Terms::default();
            t.point(&ja, ex, ey);
            t.point(&jb, -ex, -ey);
            out.push(Row { residual: len - d, unit: Unit::Length, terms: t.take() });
        }
        Constraint::PointLineDistance { p, line, d } => {
            let (pp, jp) = pt(*p);
            let ((pa, ja), (pb, jb)) = line_ends(*line);
            match line_distance(pp, pa, pb) {
                None => out.push(zero_row(Unit::Length)),
                Some(ld) => {
                    // |signed distance| − d, differentiated with the sign the
                    // point currently has, so the point keeps the side it is
                    // on rather than being flipped across the line.
                    let s = if ld.sd < 0.0 { -1.0 } else { 1.0 };
                    let mut t = Terms::default();
                    t.point(&jp, s * ld.dp.0, s * ld.dp.1);
                    t.point(&ja, s * ld.da.0, s * ld.da.1);
                    t.point(&jb, s * ld.db.0, s * ld.db.1);
                    out.push(Row {
                        residual: s * ld.sd - d,
                        unit: Unit::Length,
                        terms: t.take(),
                    });
                }
            }
        }
        Constraint::Angle { a, b, radians } => {
            emit_angle(ents, x, *a, *b, AngleForm::Exact(*radians), &mut out)
        }
        Constraint::Parallel { a, b } => {
            emit_angle(ents, x, *a, *b, AngleForm::Parallel, &mut out)
        }
        Constraint::Perpendicular { a, b } => {
            emit_angle(ents, x, *a, *b, AngleForm::Perpendicular, &mut out)
        }
        Constraint::Tangent { a, b, at } => match at {
            Some(p) => {
                let (pp, jp) = pt(*p);
                let dir = |c: &CurveRef| -> Option<Dir> {
                    match c {
                        CurveRef::Line(l) => {
                            let ((p0, j0), (p1, j1)) = line_ends(*l);
                            Some(Dir::Chord(j0, j1, (p1.0 - p0.0, p1.1 - p0.1)))
                        }
                        CurveRef::Circle(_) | CurveRef::Arc(_) => {
                            let (pc, jc) = pt(PointRef::Center(c.entity()));
                            Some(Dir::Radial(jp, jc, (pc.1 - pp.1, pp.0 - pc.0)))
                        }
                    }
                };
                let (da, db) = (dir(a), dir(b));
                match (da, db) {
                    (Some(da), Some(db)) => {
                        match angle_between(da.value(), db.value()) {
                            None => out.push(zero_row(Unit::Ratio)),
                            Some((phi, du, dv)) => {
                                // sin φ = 0: the tangent directions line up.
                                let dr = libm::cos(phi);
                                let mut t = Terms::default();
                                da.chain(&mut t, dr * du.0, dr * du.1);
                                db.chain(&mut t, dr * dv.0, dr * dv.1);
                                out.push(Row {
                                    residual: libm::sin(phi),
                                    unit: Unit::Ratio,
                                    terms: t.take(),
                                });
                            }
                        }
                    }
                    _ => out.push(zero_row(Unit::Ratio)),
                }
            }
            None => {
                let (line, curve) = match (a, b) {
                    (CurveRef::Line(l), c) => (Some(*l), *c),
                    (c, CurveRef::Line(l)) => (Some(*l), *c),
                    _ => (None, *a),
                };
                match line {
                    Some(l) => {
                        // Distance from the centre to the line equals r.
                        let (pc, jc) = pt(PointRef::Center(curve.entity()));
                        let ((pa, ja), (pb, jb)) = line_ends(l);
                        let ri = ents.radius_param(curve.entity()).expect("validated");
                        match line_distance(pc, pa, pb) {
                            None => out.push(zero_row(Unit::Length)),
                            Some(ld) => {
                                let s = if ld.sd < 0.0 { -1.0 } else { 1.0 };
                                let mut t = Terms::default();
                                t.point(&jc, s * ld.dp.0, s * ld.dp.1);
                                t.point(&ja, s * ld.da.0, s * ld.da.1);
                                t.point(&jb, s * ld.db.0, s * ld.db.1);
                                t.add(ri, -1.0);
                                out.push(Row {
                                    residual: s * ld.sd - x[ri],
                                    unit: Unit::Length,
                                    terms: t.take(),
                                });
                            }
                        }
                    }
                    None => {
                        // Circle to circle: centre distance equals the sum of
                        // the radii (external) or their difference (internal).
                        let (pa, ja) = pt(PointRef::Center(a.entity()));
                        let (pb, jb) = pt(PointRef::Center(b.entity()));
                        let ra = ents.radius_param(a.entity()).expect("validated");
                        let rb = ents.radius_param(b.entity()).expect("validated");
                        let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
                        let len = (dx * dx + dy * dy).sqrt();
                        let (ex, ey) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
                        let mut t = Terms::default();
                        t.point(&ja, ex, ey);
                        t.point(&jb, -ex, -ey);
                        let residual = if rec.tangent_internal {
                            let diff = x[ra] - x[rb];
                            let s = if diff < 0.0 { -1.0 } else { 1.0 };
                            t.add(ra, -s);
                            t.add(rb, s);
                            len - diff.abs()
                        } else {
                            t.add(ra, -1.0);
                            t.add(rb, -1.0);
                            len - (x[ra] + x[rb])
                        };
                        out.push(Row { residual, unit: Unit::Length, terms: t.take() });
                    }
                }
            }
        },
        Constraint::EqualLength { a, b } => {
            let ((pa0, ja0), (pa1, ja1)) = line_ends(*a);
            let ((pb0, jb0), (pb1, jb1)) = line_ends(*b);
            let mut t = Terms::default();
            let mut len_of = |p0: (f64, f64), p1: (f64, f64), j0, j1, sign: f64| -> f64 {
                let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
                let len = (dx * dx + dy * dy).sqrt();
                let (ex, ey) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
                t.point(j1, sign * ex, sign * ey);
                t.point(j0, -sign * ex, -sign * ey);
                len
            };
            let la = len_of(pa0, pa1, &ja0, &ja1, 1.0);
            let lb = len_of(pb0, pb1, &jb0, &jb1, -1.0);
            out.push(Row { residual: la - lb, unit: Unit::Length, terms: t.take() });
        }
        Constraint::EqualRadius { a, b } => {
            let ra = ents.radius_param(*a).expect("validated");
            let rb = ents.radius_param(*b).expect("validated");
            let mut t = Terms::default();
            t.add(ra, 1.0);
            t.add(rb, -1.0);
            out.push(Row { residual: x[ra] - x[rb], unit: Unit::Length, terms: t.take() });
        }
        Constraint::Symmetric { a, b, axis } => {
            let (pa, ja) = pt(*a);
            let (pb, jb) = pt(*b);
            let ((q0, jq0), (q1, jq1)) = line_ends(*axis);
            let m = ((pa.0 + pb.0) * 0.5, (pa.1 + pb.1) * 0.5);
            match line_distance(m, q0, q1) {
                None => {
                    out.push(zero_row(Unit::Length));
                    out.push(zero_row(Unit::Length));
                }
                Some(ld) => {
                    // 1. the midpoint lies on the axis
                    let mut t = Terms::default();
                    t.point(&ja, 0.5 * ld.dp.0, 0.5 * ld.dp.1);
                    t.point(&jb, 0.5 * ld.dp.0, 0.5 * ld.dp.1);
                    t.point(&jq0, ld.da.0, ld.da.1);
                    t.point(&jq1, ld.db.0, ld.db.1);
                    out.push(Row { residual: ld.sd, unit: Unit::Length, terms: t.take() });

                    // 2. a → b is perpendicular to the axis
                    let (ux, uy) = (q1.0 - q0.0, q1.1 - q0.1);
                    let l2 = ux * ux + uy * uy;
                    let l = l2.sqrt();
                    let (qx, qy) = (pb.0 - pa.0, pb.1 - pa.1);
                    let f = (ux * qx + uy * qy) / l;
                    let (dux, duy) = (qx / l - f * ux / l2, qy / l - f * uy / l2);
                    let mut t = Terms::default();
                    t.point(&jb, ux / l, uy / l);
                    t.point(&ja, -ux / l, -uy / l);
                    t.point(&jq1, dux, duy);
                    t.point(&jq0, -dux, -duy);
                    out.push(Row { residual: f, unit: Unit::Length, terms: t.take() });
                }
            }
        }
        Constraint::Fix(_) => {
            for &(idx, value, unit) in &rec.fixed {
                let mut t = Terms::default();
                t.add(idx, 1.0);
                out.push(Row { residual: x[idx] - value, unit, terms: t.take() });
            }
        }
        Constraint::PointAt { p, x: tx, y: ty } => {
            let (pp, jp) = pt(*p);
            for (k, (v, target)) in [(pp.0, *tx), (pp.1, *ty)].into_iter().enumerate() {
                let (cx, cy) = if k == 0 { (1.0, 0.0) } else { (0.0, 1.0) };
                let mut t = Terms::default();
                t.point(&jp, cx, cy);
                out.push(Row { residual: v - target, unit: Unit::Length, terms: t.take() });
            }
        }
        Constraint::Radius { curve, r } => {
            let ri = ents.radius_param(*curve).expect("validated");
            let mut t = Terms::default();
            t.add(ri, 1.0);
            out.push(Row { residual: x[ri] - r, unit: Unit::Length, terms: t.take() });
        }
        Constraint::Concentric { a, b } => {
            let (pa, ja) = pt(PointRef::Center(*a));
            let (pb, jb) = pt(PointRef::Center(*b));
            for (k, (va, vb)) in [(pa.0, pb.0), (pa.1, pb.1)].into_iter().enumerate() {
                let (cx, cy) = if k == 0 { (1.0, 0.0) } else { (0.0, 1.0) };
                let mut t = Terms::default();
                t.point(&ja, cx, cy);
                t.point(&jb, -cx, -cy);
                out.push(Row { residual: va - vb, unit: Unit::Length, terms: t.take() });
            }
        }
        Constraint::Midpoint { p, line } => {
            let (pp, jp) = pt(*p);
            let ((pa, ja), (pb, jb)) = line_ends(*line);
            for k in 0..2 {
                let (cx, cy) = if k == 0 { (1.0, 0.0) } else { (0.0, 1.0) };
                let (vp, va, vb) = if k == 0 {
                    (pp.0, pa.0, pb.0)
                } else {
                    (pp.1, pa.1, pb.1)
                };
                let mut t = Terms::default();
                t.point(&jp, cx, cy);
                t.point(&ja, -0.5 * cx, -0.5 * cy);
                t.point(&jb, -0.5 * cx, -0.5 * cy);
                out.push(Row {
                    residual: vp - 0.5 * (va + vb),
                    unit: Unit::Length,
                    terms: t.take(),
                });
            }
        }
        Constraint::OnCurve { p, curve } => {
            let (pp, jp) = pt(*p);
            match curve {
                CurveRef::Line(l) => {
                    let ((pa, ja), (pb, jb)) = line_ends(*l);
                    match line_distance(pp, pa, pb) {
                        None => out.push(zero_row(Unit::Length)),
                        Some(ld) => {
                            let mut t = Terms::default();
                            t.point(&jp, ld.dp.0, ld.dp.1);
                            t.point(&ja, ld.da.0, ld.da.1);
                            t.point(&jb, ld.db.0, ld.db.1);
                            out.push(Row { residual: ld.sd, unit: Unit::Length, terms: t.take() });
                        }
                    }
                }
                _ => {
                    let e = curve.entity();
                    let (pc, jc) = pt(PointRef::Center(e));
                    let ri = ents.radius_param(e).expect("validated");
                    let (dx, dy) = (pp.0 - pc.0, pp.1 - pc.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    let (ex, ey) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
                    let mut t = Terms::default();
                    t.point(&jp, ex, ey);
                    t.point(&jc, -ex, -ey);
                    t.add(ri, -1.0);
                    out.push(Row { residual: len - x[ri], unit: Unit::Length, terms: t.take() });
                }
            }
        }
    }
    debug_assert_eq!(out.len(), rec.rows, "{} row count", rec.kind.type_name());
    out
}

/// Which tangency mode (internal or external) the sketch is nearest to now.
/// Called once, when the constraint is added.
pub(crate) fn pick_tangent_mode(ents: &Entities, c: &Constraint, x: &[f64]) -> bool {
    let (a, b) = match c {
        Constraint::Tangent { a, b, at: None } => (a, b),
        _ => return false,
    };
    if matches!(a, CurveRef::Line(_)) || matches!(b, CurveRef::Line(_)) {
        return false;
    }
    let (Some(((ax, ay), _)), Some(((bx, by), _))) = (
        ents.point(PointRef::Center(a.entity()), x),
        ents.point(PointRef::Center(b.entity()), x),
    ) else {
        return false;
    };
    let (Some(ra), Some(rb)) =
        (ents.radius_param(a.entity()), ents.radius_param(b.entity()))
    else {
        return false;
    };
    let d = (((ax - bx) * (ax - bx)) + ((ay - by) * (ay - by))).sqrt();
    let external = (d - (x[ra] + x[rb])).abs();
    let internal = (d - (x[ra] - x[rb]).abs()).abs();
    internal < external
}
