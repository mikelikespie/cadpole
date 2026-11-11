//! Entities and the flat parameter vector.
//!
//! The parameterisation is deliberately minimal, because every extra
//! parameter is an extra degree of freedom the user has to constrain away:
//!
//! | entity   | parameters                          |
//! | -------- | ----------------------------------- |
//! | point    | `x`, `y`                            |
//! | line     | *none* — it references two points   |
//! | circle   | `r` (its centre is a point)         |
//! | arc      | `r`, `start`, `end` (radians)       |
//!
//! Anything that *looks* like a point — a line's endpoint, an arc's endpoint,
//! a circle's centre — is a [`PointRef`], and a reference knows both its
//! position and its derivative with respect to the parameters that produce
//! it. Constraints are therefore written once against `PointRef` and work
//! unchanged on an arc endpoint, where the position is
//! `centre + r·(cos θ, sin θ)` and four parameters are involved.

use core::fmt;

/// Identifies any entity in a [`System`](crate::System).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EntityId(pub u32);

macro_rules! typed_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("A handle to ", $what, ". Converts into [`EntityId`].")]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub EntityId);

        impl From<$name> for EntityId {
            fn from(v: $name) -> EntityId {
                v.0
            }
        }
    };
}

typed_id!(PointId, "a point");
typed_id!(LineId, "a line segment");
typed_id!(CircleId, "a circle");
typed_id!(ArcId, "a circular arc");

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What an entity is, and which other entities it is built from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntityKind {
    /// A free point: parameters `x`, `y`.
    Point,
    /// A segment between two points. Contributes no parameters of its own.
    Line { a: PointId, b: PointId },
    /// A full circle: a centre point plus one radius parameter.
    Circle { center: PointId },
    /// An arc: a centre point plus `r`, `start`, `end` (radians, CCW).
    Arc { center: PointId },
}

impl EntityKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Point => "point",
            EntityKind::Line { .. } => "line",
            EntityKind::Circle { .. } => "circle",
            EntityKind::Arc { .. } => "arc",
        }
    }
}

/// The unit a parameter or residual is measured in. Used for scaling, so a
/// millimetre and a radian converge together (see [`crate::solver`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// A length, in sketch units (millimetres by convention).
    Length,
    /// An angle in radians.
    Angle,
    /// Dimensionless, on the order of a sine or cosine.
    Ratio,
}

#[derive(Clone, Debug)]
pub(crate) struct EntityRecord {
    pub kind: EntityKind,
    /// Index of this entity's first parameter in the flat vector.
    pub base: usize,
    /// How many parameters it owns.
    pub nparams: usize,
    pub construction: bool,
    pub name: Option<String>,
}

/// A reference to something that behaves like a point.
///
/// `PointId` converts into `PointRef`, so `Constraint::coincident(p, q)`
/// takes raw points, arc endpoints or circle centres interchangeably.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointRef {
    /// A free point entity.
    Point(PointId),
    /// The centre of a circle or an arc.
    Center(EntityId),
    /// A line's first point, or an arc's `start` endpoint.
    Start(EntityId),
    /// A line's second point, or an arc's `end` endpoint.
    End(EntityId),
}

impl PointRef {
    pub fn start(e: impl Into<EntityId>) -> PointRef {
        PointRef::Start(e.into())
    }
    pub fn end(e: impl Into<EntityId>) -> PointRef {
        PointRef::End(e.into())
    }
    pub fn center(e: impl Into<EntityId>) -> PointRef {
        PointRef::Center(e.into())
    }
}

impl From<PointId> for PointRef {
    fn from(p: PointId) -> PointRef {
        PointRef::Point(p)
    }
}

/// A reference to something a point can lie on or a line can be tangent to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurveRef {
    Line(LineId),
    Circle(CircleId),
    Arc(ArcId),
}

impl From<LineId> for CurveRef {
    fn from(v: LineId) -> CurveRef {
        CurveRef::Line(v)
    }
}
impl From<CircleId> for CurveRef {
    fn from(v: CircleId) -> CurveRef {
        CurveRef::Circle(v)
    }
}
impl From<ArcId> for CurveRef {
    fn from(v: ArcId) -> CurveRef {
        CurveRef::Arc(v)
    }
}

impl CurveRef {
    pub fn entity(self) -> EntityId {
        match self {
            CurveRef::Line(v) => v.0,
            CurveRef::Circle(v) => v.0,
            CurveRef::Arc(v) => v.0,
        }
    }
}

/// The derivative of a point's position with respect to the parameters that
/// produce it. At most four parameters are ever involved (an arc endpoint:
/// centre x, centre y, radius, angle).
#[derive(Clone, Copy, Debug, Default)]
pub struct PointJac {
    len: usize,
    terms: [(usize, f64, f64); 4],
}

impl PointJac {
    fn push(&mut self, idx: usize, dx: f64, dy: f64) {
        self.terms[self.len] = (idx, dx, dy);
        self.len += 1;
    }
    /// `(parameter index, ∂x/∂p, ∂y/∂p)` for each parameter involved.
    pub fn terms(&self) -> &[(usize, f64, f64)] {
        &self.terms[..self.len]
    }
}

/// The entity table and the layout of the parameter vector.
#[derive(Clone, Debug, Default)]
pub(crate) struct Entities {
    pub recs: Vec<EntityRecord>,
    pub nparams: usize,
}

impl Entities {
    pub fn add(&mut self, kind: EntityKind, nparams: usize) -> EntityId {
        let id = EntityId(self.recs.len() as u32);
        self.recs.push(EntityRecord {
            kind,
            base: self.nparams,
            nparams,
            construction: false,
            name: None,
        });
        self.nparams += nparams;
        id
    }

    pub fn get(&self, id: EntityId) -> &EntityRecord {
        &self.recs[id.0 as usize]
    }

    /// Which entity owns parameter `p`.
    pub fn owner_of_param(&self, p: usize) -> EntityId {
        for (i, r) in self.recs.iter().enumerate() {
            if r.nparams > 0 && p >= r.base && p < r.base + r.nparams {
                return EntityId(i as u32);
            }
        }
        EntityId(0)
    }

    /// Resolve a point reference to `(x, y)` and its Jacobian.
    ///
    /// Returns `None` when the reference does not name a point on that kind of
    /// entity (`Center` of a line, say); the system validates references when
    /// a constraint is added, so this only fires on internal misuse.
    pub fn point(&self, r: PointRef, x: &[f64]) -> Option<((f64, f64), PointJac)> {
        match r {
            PointRef::Point(p) => Some(self.raw_point(p.0, x)),
            PointRef::Center(e) => match self.get(e).kind {
                EntityKind::Circle { center } | EntityKind::Arc { center } => {
                    Some(self.raw_point(center.0, x))
                }
                _ => None,
            },
            PointRef::Start(e) | PointRef::End(e) => {
                let is_start = matches!(r, PointRef::Start(_));
                match self.get(e).kind {
                    EntityKind::Line { a, b } => {
                        Some(self.raw_point(if is_start { a.0 } else { b.0 }, x))
                    }
                    EntityKind::Arc { center } => {
                        let rec = self.get(e);
                        let (ir, ia) = (rec.base, rec.base + if is_start { 1 } else { 2 });
                        let (radius, theta) = (x[ir], x[ia]);
                        let ((cx, cy), cj) = self.raw_point(center.0, x);
                        let (co, si) = (libm::cos(theta), libm::sin(theta));
                        let mut j = PointJac::default();
                        for &(idx, dx, dy) in cj.terms() {
                            j.push(idx, dx, dy);
                        }
                        j.push(ir, co, si);
                        j.push(ia, -radius * si, radius * co);
                        Some(((cx + radius * co, cy + radius * si), j))
                    }
                    _ => None,
                }
            }
        }
    }

    fn raw_point(&self, id: EntityId, x: &[f64]) -> ((f64, f64), PointJac) {
        let rec = self.get(id);
        let b = rec.base;
        let mut j = PointJac::default();
        j.push(b, 1.0, 0.0);
        j.push(b + 1, 0.0, 1.0);
        ((x[b], x[b + 1]), j)
    }

    /// Index of the radius parameter of a circle or arc.
    pub fn radius_param(&self, e: EntityId) -> Option<usize> {
        match self.get(e).kind {
            EntityKind::Circle { .. } | EntityKind::Arc { .. } => Some(self.get(e).base),
            _ => None,
        }
    }

    /// `PointRef` validity: does this reference name a point on this entity?
    pub fn valid_point_ref(&self, r: PointRef) -> bool {
        match r {
            PointRef::Point(p) => matches!(self.get(p.0).kind, EntityKind::Point),
            PointRef::Center(e) => {
                matches!(self.get(e).kind, EntityKind::Circle { .. } | EntityKind::Arc { .. })
            }
            PointRef::Start(e) | PointRef::End(e) => {
                matches!(self.get(e).kind, EntityKind::Line { .. } | EntityKind::Arc { .. })
            }
        }
    }
}
