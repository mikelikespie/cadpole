//! `Selection`: what every geometric selector returns (docs/design.md).
//!
//! A selection is an ordered, deduplicated set of faces or edges of **one**
//! body. It behaves like a list — `len()`, `[i]`, `for f in`, comprehensions,
//! `sorted()` — so everything written against the old list-returning
//! `faces()` keeps working, and it adds the two things a list cannot do: set
//! algebra (`|`, `&`, `-`) and the reducers that turn a set into a pick
//! (`.one()`, `.largest()`, `.nearest()`, `.max()`).
//!
//! The order is [`cadpole_core::select::sort_key`]'s and nothing else's: each
//! member remembers its **rank** in the body's full canonical order, so a
//! union of two selections comes back in the same order as one selector that
//! had asked for both.

use std::fmt;

use allocative::Allocative;
use cadpole_core::geom::Vec3;
use cadpole_core::naming::{self, Kind, Resolution};
use cadpole_core::select::{self, Candidate, SetOp};
use starlark::environment::{Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueError,
    ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::check::{VecArg, vec3};
use crate::convert::alloc_str_list;
use crate::store::store;
use crate::values::{KBody, KEdge, KFace};

/// A body's entities of one kind, the geometry a filter reads, and each
/// entity's rank in the canonical order.
pub struct View<T> {
    pub items: Vec<T>,
    pub cands: Vec<Candidate>,
    /// `rank[i]` is where topology entity `i` sits in the canonical order.
    pub rank: Vec<u32>,
}

impl<T> View<T> {
    fn build(items: Vec<T>, cands: Vec<Candidate>) -> View<T> {
        let rank = select::ranks(&cands);
        View { items, cands, rank }
    }
}

/// The faces of `body`, with the geometry a selector filters on.
pub fn face_view(body: &KBody, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<View<KFace>> {
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let (mesh, topo) = ctx.body_mesh(body.body_id())?;
    let cands = select::face_candidates(topo, mesh);
    let items = topo
        .faces
        .iter()
        .map(|f| KFace {
            body: body.id,
            name: f.name.clone(),
            aliases: f.aliases.clone(),
            kind: f.kind().to_owned(),
            surface_kind: f.surface_kind.to_owned(),
            normal: f.normal,
            centroid: f.centroid,
            area: f.area,
            edges: f.edges.clone(),
        })
        .collect();
    Ok(View::build(items, cands))
}

/// The edges of `body`, with the geometry a selector filters on.
pub fn edge_view(body: &KBody, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<View<KEdge>> {
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let (_, topo) = ctx.body_mesh(body.body_id())?;
    let cands = select::edge_candidates(topo);
    let items = topo.edges.iter().map(|e| KEdge::from_topology(body.id, e)).collect();
    Ok(View::build(items, cands))
}

/// Resolve a face name (full, local, alias or `face:N`) to its topology index,
/// so `edges(of_face=...)` can ask the element map rather than a string.
pub fn face_index(cands: &[Candidate], feature: &str, query: &str) -> anyhow::Result<u32> {
    struct ByName<'a>(&'a Candidate);
    impl naming::Named for ByName<'_> {
        fn name(&self) -> &str {
            &self.0.name
        }
        fn aliases(&self) -> &[String] {
            &self.0.aliases
        }
        fn fingerprint(&self) -> naming::Fingerprint {
            naming::Fingerprint::default()
        }
    }
    let items: Vec<ByName<'_>> = cands.iter().map(ByName).collect();
    match naming::resolve(&items, Kind::Face, feature, query, None, 0.0) {
        Resolution::Ok(i) | Resolution::Moved(i) => Ok(i as u32),
        Resolution::Ambiguous(all) => anyhow::bail!(
            "of_face={query:?} names {} faces ({}); it is a group, so ask for one of them by name",
            all.len(),
            all.iter().map(|i| cands[*i].name.as_str()).collect::<Vec<_>>().join(", ")
        ),
        Resolution::Stale => anyhow::bail!("of_face={query:?}: this body has no such face"),
    }
}

// ---------------------------------------------------------------- the value

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSelection {
    pub body: u32,
    pub feature: String,
    /// `"face"` or `"edge"`.
    pub what: String,
    pub faces: Vec<KFace>,
    pub edges: Vec<KEdge>,
    /// Canonical rank of each member, in the same order as the members.
    pub ranks: Vec<u32>,
    /// How the selection was asked for, for `str()` and for diagnostics.
    pub query: String,
}
starlark_simple_value!(KSelection);

impl KSelection {
    pub fn len(&self) -> usize {
        if self.what == "face" { self.faces.len() } else { self.edges.len() }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn names(&self) -> Vec<String> {
        if self.what == "face" {
            self.faces.iter().map(|f| f.name.clone()).collect()
        } else {
            self.edges.iter().map(|e| e.name.clone()).collect()
        }
    }

    fn member<'v>(&self, i: usize, heap: Heap<'v>) -> Value<'v> {
        if self.what == "face" { heap.alloc(self.faces[i].clone()) } else { heap.alloc(self.edges[i].clone()) }
    }

    fn centroid(&self, i: usize) -> Vec3 {
        if self.what == "face" {
            self.faces[i].centroid
        } else {
            (self.edges[i].start + self.edges[i].end) * 0.5
        }
    }

    fn size(&self, i: usize) -> f64 {
        if self.what == "face" { self.faces[i].area } else { self.edges[i].length }
    }

    /// Combine two selections of the same kind and body. The algebra itself is
    /// `cadpole_core::select::combine`, over the members' canonical ranks, so the
    /// result is in the same order a single selector would have produced.
    fn combine(&self, other: &KSelection, how: &str) -> anyhow::Result<KSelection> {
        if self.body != other.body {
            anyhow::bail!(
                "cannot {how} selections from two different bodies ({} and {}); \
                 take both selections from the same body",
                self.feature,
                other.feature
            );
        }
        if self.what != other.what {
            anyhow::bail!("cannot {how} a {} selection with an {} selection", self.what, other.what);
        }
        let op = match how {
            "union" => SetOp::Union,
            "intersection" => SetOp::Intersection,
            _ => SetOp::Difference,
        };
        let members = select::combine(&self.ranks, &other.ranks, op);
        let mut out = KSelection {
            body: self.body,
            feature: self.feature.clone(),
            what: self.what.clone(),
            faces: Vec::new(),
            edges: Vec::new(),
            ranks: Vec::new(),
            query: format!("{} {} {}", self.query, symbol(how), other.query),
        };
        for m in &members {
            let src = if m.from_b { other } else { self };
            out.ranks.push(src.ranks[m.at]);
            if self.what == "face" {
                out.faces.push(src.faces[m.at].clone());
            } else {
                out.edges.push(src.edges[m.at].clone());
            }
        }
        Ok(out)
    }

    /// The single member, or an error naming what it found instead. This is
    /// the other half of the empty-set policy: a *set* may be empty, but
    /// asking a set for "the one" may not be.
    fn the_one(&self, what_for: &str) -> anyhow::Result<usize> {
        match self.len() {
            1 => Ok(0),
            0 => anyhow::bail!("{what_for}: {} matched no {}s", self.query, self.what),
            n => anyhow::bail!(
                "{what_for}: {} matched {n} {}s ({}); narrow it, or use .largest() / .nearest() / .nth()",
                self.query,
                self.what,
                listing(&self.names())
            ),
        }
    }
}

fn symbol(how: &str) -> &'static str {
    match how {
        "union" => "|",
        "intersection" => "&",
        _ => "-",
    }
}

/// At most twelve names, so an error message stays readable on a long body.
pub fn listing(names: &[String]) -> String {
    if names.len() > 12 {
        format!("{}, ...", names[..12].join(", "))
    } else {
        names.join(", ")
    }
}

impl fmt::Display for KSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "selection({} {}", self.len(), self.what)?;
        if self.len() != 1 {
            write!(f, "s")?;
        }
        let names = self.names();
        if names.is_empty() { write!(f, ")") } else { write!(f, ": {})", listing(&names)) }
    }
}

#[starlark_value(type = "Selection")]
impl<'v> StarlarkValue<'v> for KSelection {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("selection_methods", selection_methods);
        Some(RES.methods())
    }

    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            // "face" or "edge": what this selection is a set of.
            "kind" => Some(heap.alloc(self.what.as_str())),
            "names" => Some(alloc_str_list(heap, &self.names())),
            // The filters that produced it, as a string, for diagnostics.
            "query" => Some(heap.alloc(self.query.as_str())),
            _ => None,
        }
    }

    fn dir_attr(&self) -> Vec<String> {
        ["kind", "names", "query"].iter().map(|s| s.to_string()).collect()
    }

    fn length(&self) -> starlark::Result<i32> {
        Ok(self.len() as i32)
    }

    fn at(&self, index: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        let i = index.unpack_i32().ok_or(ValueError::IncorrectParameterType)?;
        let n = self.len() as i32;
        let k = if i < 0 { i + n } else { i };
        if k < 0 || k >= n {
            return Err(ValueError::IndexOutOfBound(i).into());
        }
        Ok(self.member(k as usize, heap))
    }

    fn iterate_collect(&self, heap: Heap<'v>) -> starlark::Result<Vec<Value<'v>>> {
        Ok((0..self.len()).map(|i| self.member(i, heap)).collect())
    }

    fn is_in(&self, other: Value<'v>) -> starlark::Result<bool> {
        let want = if let Some(f) = other.downcast_ref::<KFace>() {
            f.name.clone()
        } else if let Some(e) = other.downcast_ref::<KEdge>() {
            e.name.clone()
        } else if let Some(s) = other.unpack_str() {
            s.to_owned()
        } else {
            return Ok(false);
        };
        Ok(self.names().contains(&want))
    }

    /// `+` and `|` are both union: `+` because a list habit expects it, `|`
    /// because this is a set.
    fn add(&self, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        Some(self.set_op(rhs, "union", heap))
    }

    fn bit_or(&self, other: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        self.set_op(other, "union", heap)
    }

    fn bit_and(&self, other: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        self.set_op(other, "intersection", heap)
    }

    fn sub(&self, other: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        self.set_op(other, "difference", heap)
    }

    fn equals(&self, other: Value<'v>) -> starlark::Result<bool> {
        match other.downcast_ref::<KSelection>() {
            Some(o) => Ok(self.body == o.body && self.what == o.what && self.ranks == o.ranks),
            None => Ok(false),
        }
    }
}

impl KSelection {
    fn set_op<'v>(&self, other: Value<'v>, how: &str, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        let o = other
            .downcast_ref::<KSelection>()
            .ok_or_else(|| ValueError::OperationNotSupportedBinary {
                op: symbol(how).to_owned(),
                left: "Selection".to_owned(),
                right: other.get_type().to_owned(),
            })?;
        match self.combine(o, how) {
            Ok(s) => Ok(heap.alloc(s)),
            Err(e) => Err(starlark::Error::new_other(e)),
        }
    }
}

#[starlark_module]
fn selection_methods(builder: &mut MethodsBuilder) {
    /// How many entities this selection holds. `len(sel)` says the same thing;
    /// this reads better inside an assertion.
    fn count(this: &KSelection) -> anyhow::Result<i32> {
        Ok(this.len() as i32)
    }

    /// The generated names of the selected entities, in the selection's order.
    fn names<'v>(this: &KSelection, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(alloc_str_list(heap, &this.names()))
    }

    /// The one entity in this selection. An error when there are none or
    /// several, naming what it found — a selector that silently picked the
    /// first would be the bug this whole layer exists to remove.
    fn one<'v>(this: &'v KSelection, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let i = this.the_one("one()")?;
        Ok(this.member(i, heap))
    }

    /// The `k`-th entity in the selection's canonical order (negative counts
    /// from the end).
    fn nth<'v>(
        this: &'v KSelection,
        #[starlark(require = pos)] k: i32,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let n = this.len() as i32;
        let i = if k < 0 { k + n } else { k };
        if i < 0 || i >= n {
            anyhow::bail!("nth({k}): {} holds {n} {}s", this.query, this.what);
        }
        Ok(this.member(i as usize, heap))
    }

    /// The largest entity by area (faces) or length (edges).
    fn largest<'v>(this: &'v KSelection, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(this.member(pick_by_size(this, 1.0)?, heap))
    }

    /// The smallest entity by area (faces) or length (edges).
    fn smallest<'v>(this: &'v KSelection, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(this.member(pick_by_size(this, -1.0)?, heap))
    }

    /// The entity whose centroid is closest to `point`.
    fn nearest<'v>(
        this: &'v KSelection,
        #[starlark(require = pos)] point: VecArg,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let p = vec3(&point, "nearest")?;
        if this.is_empty() {
            anyhow::bail!("nearest(): {} matched no {}s", this.query, this.what);
        }
        let best = (0..this.len())
            .min_by_key(|&i| (naming::quantize((this.centroid(i) - p).length()), this.ranks[i]))
            .unwrap();
        Ok(this.member(best, heap))
    }

    /// The entity furthest along `axis` — `"x"`, `"y"`, `"z"` or a vector.
    fn max<'v>(
        this: &'v KSelection,
        #[starlark(require = pos)] axis: Value<'v>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        Ok(this.member(pick_extreme(this, axis, 1.0)?, heap))
    }

    /// The entity least far along `axis` — `"x"`, `"y"`, `"z"` or a vector.
    fn min<'v>(
        this: &'v KSelection,
        #[starlark(require = pos)] axis: Value<'v>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        Ok(this.member(pick_extreme(this, axis, -1.0)?, heap))
    }

    /// Union with another selection of the same kind, on the same body.
    /// `a | b` and `a + b` say the same thing.
    fn union(
        this: &KSelection,
        #[starlark(require = pos)] other: &KSelection,
    ) -> anyhow::Result<KSelection> {
        this.combine(other, "union")
    }

    /// The entities in both selections. `a & b` says the same thing.
    fn intersection(
        this: &KSelection,
        #[starlark(require = pos)] other: &KSelection,
    ) -> anyhow::Result<KSelection> {
        this.combine(other, "intersection")
    }

    /// The entities in this selection and not the other. `a - b` says the
    /// same thing.
    fn difference(
        this: &KSelection,
        #[starlark(require = pos)] other: &KSelection,
    ) -> anyhow::Result<KSelection> {
        this.combine(other, "difference")
    }

    /// The edges that bound the selected faces, as a selection.
    fn edges<'v>(this: &KSelection, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KSelection> {
        if this.what != "face" {
            anyhow::bail!("edges(): this is already a selection of edges");
        }
        let body = KBody { id: this.body, feature: this.feature.clone(), name: String::new(), kind: String::new() };
        let view = edge_view(&body, eval)?;
        let want: Vec<String> = this.faces.iter().flat_map(|f| f.edges.iter().cloned()).collect();
        let picked: Vec<usize> =
            (0..view.items.len()).filter(|&i| want.contains(&view.items[i].name)).collect();
        Ok(from_picked(&body, "edge", &view.items, &view.rank, &picked, &format!("{}.edges()", this.query)))
    }

    /// The faces the selected edges lie between, as a selection.
    fn faces<'v>(this: &KSelection, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KSelection> {
        if this.what != "edge" {
            anyhow::bail!("faces(): this is already a selection of faces");
        }
        let body = KBody { id: this.body, feature: this.feature.clone(), name: String::new(), kind: String::new() };
        let view = face_view(&body, eval)?;
        let want: Vec<String> = this.edges.iter().flat_map(|e| e.faces.iter().cloned()).collect();
        let picked: Vec<usize> =
            (0..view.items.len()).filter(|&i| want.contains(&view.items[i].name)).collect();
        Ok(from_picked(&body, "face", &view.items, &view.rank, &picked, &format!("{}.faces()", this.query)))
    }
}

fn pick_by_size(sel: &KSelection, sign: f64) -> anyhow::Result<usize> {
    if sel.is_empty() {
        anyhow::bail!(
            "{}(): {} matched no {}s",
            if sign > 0.0 { "largest" } else { "smallest" },
            sel.query,
            sel.what
        );
    }
    Ok((0..sel.len())
        .max_by_key(|&i| (naming::quantize(sel.size(i) * sign), std::cmp::Reverse(sel.ranks[i])))
        .unwrap())
}

fn pick_extreme(sel: &KSelection, axis: Value<'_>, sign: f64) -> anyhow::Result<usize> {
    let a = axis_vector(axis)?;
    if sel.is_empty() {
        anyhow::bail!(
            "{}(): {} matched no {}s",
            if sign > 0.0 { "max" } else { "min" },
            sel.query,
            sel.what
        );
    }
    Ok((0..sel.len())
        .max_by_key(|&i| {
            (naming::quantize(sel.centroid(i).dot(a) * sign), std::cmp::Reverse(sel.ranks[i]))
        })
        .unwrap())
}

/// `"x"` / `"y"` / `"z"` (or `"-z"`), or any vector.
pub fn axis_vector(v: Value<'_>) -> anyhow::Result<Vec3> {
    if let Some(s) = v.unpack_str() {
        let (sign, letter) = match s.strip_prefix('-') {
            Some(rest) => (-1.0, rest),
            None => (1.0, s.strip_prefix('+').unwrap_or(s)),
        };
        return match letter.to_ascii_lowercase().as_str() {
            "x" => Ok(Vec3::X * sign),
            "y" => Ok(Vec3::Y * sign),
            "z" => Ok(Vec3::Z * sign),
            _ => anyhow::bail!("axis {s:?}: expected \"x\", \"y\", \"z\" (optionally signed) or a vector"),
        };
    }
    let arg: VecArg = starlark::values::UnpackValue::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("axis: expected \"x\", \"y\", \"z\" or a vector, got {}", v.get_type()))?;
    let a = vec3(&arg, "axis")?;
    if a.length() == 0.0 {
        anyhow::bail!("axis: must be non-zero");
    }
    Ok(a)
}

/// Build a selection from topology indices already in canonical order.
pub fn from_picked<T: Clone + Selectable>(
    body: &KBody,
    what: &str,
    items: &[T],
    rank: &[u32],
    picked: &[usize],
    query: &str,
) -> KSelection {
    let mut order: Vec<usize> = picked.to_vec();
    order.sort_by_key(|&i| rank[i]);
    let mut sel = KSelection {
        body: body.id,
        feature: body.feature.clone(),
        what: what.to_owned(),
        faces: Vec::new(),
        edges: Vec::new(),
        ranks: order.iter().map(|&i| rank[i]).collect(),
        query: query.to_owned(),
    };
    for &i in &order {
        items[i].push_into(&mut sel);
    }
    sel
}

/// Lets `from_picked` work for both kinds without a second copy of it.
pub trait Selectable {
    fn push_into(&self, sel: &mut KSelection);
}

impl Selectable for KFace {
    fn push_into(&self, sel: &mut KSelection) {
        sel.faces.push(self.clone());
    }
}

impl Selectable for KEdge {
    fn push_into(&self, sel: &mut KSelection) {
        sel.edges.push(self.clone());
    }
}
