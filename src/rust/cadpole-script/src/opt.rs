//! The `opt` namespace: linear and mixed-integer programs, written in a
//! script and solved by the host's solver.
//!
//! A script declares variables, writes linear rules and an objective, and
//! asks for the best values:
//!
//! ```python
//! p = opt.problem(name="sizes")
//! w = p.var("w", min=8, max=20)
//! d = p.var("d", min=8, max=20)
//! p.constrain(w + d, max=30, name="perimeter")
//! p.maximize(2 * w + d)
//! sol = p.solve()
//! print(sol.value(w), sol.binding())    # 20.0 ["perimeter"]
//! ```
//!
//! **Where the solve happens.** Everything up to `solve()` is bookkeeping
//! here, in every engine. The solve itself is `cadpole_core::opt::installed()`:
//! HiGHS in the CLI, `cadpole serve` and `cadpole mcp` (`cadpole-opt`, native only),
//! nothing in the wasm engine or the Apple apps, where `solve()` fails with
//! [`cadpole_core::opt::NOT_INSTALLED`]. Published sites are baked, so nothing
//! in a browser ever needs it.
//!
//! **The language's limits, designed around.** starlark-rust has `radd` and
//! `rmul` but no reflected subtraction, so `2 - x` cannot work: write `-x +
//! 2`, or move the constant into a bound. Its comparisons return an ordering
//! or a bool, so `x <= 3` cannot build a rule either; comparing an
//! expression is an error that says to write `p.constrain(x, max=3)`, and a
//! bound may itself be an expression (`p.constrain(a, max=b)` is `a <= b`).
//! Starlark has no `sum()`, hence `opt.sum` and `opt.dot`.
//!
//! **Numbers are plain numbers.** Nothing here knows about units; a script
//! that models in feet divides by `ft(1)` going in and multiplies coming out.
//! It should: HiGHS's tolerances are absolute (1e-7 on a constraint, 1e-6 on
//! integrality), so a model whose areas are in square millimetres -- a
//! thousand square feet is 9.3e7 mm² -- asks for more digits than a double
//! has, and big-M rules in such units are where solvers go wrong.
//!
//! **Memoised**: a solve is keyed by the SHA-256 of everything in the
//! problem and its options, in declaration order, plus the solver's identity,
//! so re-evaluating an unchanged script in `cadpole serve` does not solve
//! again. A solve a time limit stopped is never kept.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use allocative::Allocative;
use cadpole_core::context::{Diagnostic, Span, fmt_num};
use cadpole_core::ids::FeatureId;
use cadpole_core::memo::KeyBuilder;
use cadpole_core::opt::{
    Column, DEFAULT_MIP_REL_GAP, Deleted, DeletionLimits, Model, NOT_INSTALLED, Options, Row, Sense, Side, Solution,
    Status, Stop, conflict_by_deletion,
};
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::dict::UnpackDictEntries;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::none::NoneType;
use starlark::collections::StarlarkHasher;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value, ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::limits::Limits;
use crate::store::{call_span, store};

// ------------------------------------------------------------------ state

/// The objective, once set.
struct Objective {
    sense: Sense,
    terms: Vec<(u32, f64)>,
    constant: f64,
    span: Option<Span>,
}

/// One problem being built, in the evaluation's store. The model's column
/// costs stay zero while building; the objective is kept apart and applied
/// to the copy each solve takes.
pub struct ProblemState {
    feature: FeatureId,
    model: Model,
    var_index: HashMap<String, u32>,
    var_spans: Vec<Option<Span>>,
    row_index: HashMap<String, usize>,
    row_spans: Vec<Option<Span>>,
    /// Each row's tags (`family`, `floor`, `room`, or any other key), sorted
    /// by key, the strings shared: tens of thousands of rows have a handful
    /// of distinct values.
    row_tags: Vec<Tags>,
    tag_text: HashMap<String, Arc<str>>,
    objective: Option<Objective>,
}

/// A row's tags, sorted by key.
type Tags = Vec<(Arc<str>, Arc<str>)>;

/// One clause of a tag selector: every key must have one of its values.
type Clause = Vec<(String, Vec<String>)>;

fn matches(tags: &Tags, clause: &Clause) -> bool {
    clause.iter().all(|(k, vals)| tags.iter().any(|(tk, tv)| **tk == **k && vals.iter().any(|v| **tv == **v)))
}

/// The tags a name of the form `family:floor:room:what` carries when the
/// rule gives none: the convention `@cadpole/plan` writes its rows in. A name of
/// any other shape has none; an empty part is no tag.
fn tags_from_name(name: &str) -> Vec<(String, String)> {
    let parts: Vec<&str> = name.splitn(4, ':').collect();
    if parts.len() < 4 {
        return Vec::new();
    }
    ["family", "floor", "room"]
        .iter()
        .zip(&parts[..3])
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl ProblemState {
    fn new(feature: FeatureId) -> ProblemState {
        ProblemState {
            feature,
            model: Model::new(Sense::Minimize),
            var_index: HashMap::new(),
            var_spans: Vec::new(),
            row_index: HashMap::new(),
            row_spans: Vec::new(),
            row_tags: Vec::new(),
            tag_text: HashMap::new(),
            objective: None,
        }
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.tag_text.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.tag_text.insert(s.to_owned(), a.clone());
        a
    }

    /// The rows a tag selector picks, in order.
    fn select(&self, clauses: &[Clause]) -> Vec<usize> {
        (0..self.row_tags.len()).filter(|&r| clauses.iter().any(|c| matches(&self.row_tags[r], c))).collect()
    }

    /// The model as it stands, objective applied, for one solve, with that
    /// solve's own changes: variables fixed, rules relaxed, the objective left
    /// out. None of them touches the problem, so the next solve is the
    /// problem as written again.
    fn snapshot(&self, changes: &Changes) -> Snapshot {
        let mut model = self.model.clone();
        if let Some(o) = self.objective.as_ref().filter(|_| changes.objective) {
            model.sense = o.sense;
            model.offset = o.constant;
            for &(j, a) in &o.terms {
                model.columns[j as usize].cost = a;
            }
        }
        for &(j, v) in &changes.fix {
            let c = &mut model.columns[j as usize];
            c.lower = v;
            c.upper = v;
        }
        let mut relaxed = Vec::with_capacity(changes.relax.len());
        for &r in &changes.relax {
            relaxed.push((r, model.rows[r].lower, model.rows[r].upper));
            model.rows[r].lower = f64::NEG_INFINITY;
            model.rows[r].upper = f64::INFINITY;
        }
        Snapshot {
            model,
            row_index: self.row_index.clone(),
            row_spans: self.row_spans.clone(),
            row_tags: self.row_tags.clone(),
            relaxed,
        }
    }
}

/// What one `solve()` changes for itself alone.
struct Changes {
    fix: Vec<(u32, f64)>,
    relax: Vec<usize>,
    objective: bool,
}

/// What one solve saw: the model it solved and what is needed to name its
/// rows. A solution keeps its own, so adding rules after a solve and
/// solving again leaves the first answer as it was.
pub struct Snapshot {
    model: Model,
    row_index: HashMap<String, usize>,
    row_spans: Vec<Option<Span>>,
    row_tags: Vec<Tags>,
    /// The rules this solve left out (`relax=`, `drop=`), with the bounds
    /// they have in the problem, by row: a solution measures them against
    /// those.
    relaxed: Vec<(usize, f64, f64)>,
}

impl Snapshot {
    /// The rows as families for a deletion filter: one group per `family`
    /// tag, in the order the families first appear. A row with no family is
    /// in none, which the filter takes as a group of its own.
    fn families(&self) -> Vec<Vec<usize>> {
        let mut order: Vec<Arc<str>> = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (r, tags) in self.row_tags.iter().enumerate() {
            let Some((_, fam)) = tags.iter().find(|(k, _)| &**k == "family") else { continue };
            match order.iter().position(|f| f == fam) {
                Some(i) => groups[i].push(r),
                None => {
                    order.push(fam.clone());
                    groups.push(vec![r]);
                }
            }
        }
        groups
    }
}

impl Snapshot {
    /// The bounds row `r` has in the problem, whether or not this solve
    /// kept them.
    fn bounds(&self, r: usize) -> (f64, f64, bool) {
        match self.relaxed.binary_search_by_key(&r, |&(k, _, _)| k) {
            Ok(i) => (self.relaxed[i].1, self.relaxed[i].2, true),
            Err(_) => (self.model.rows[r].lower, self.model.rows[r].upper, false),
        }
    }
}

fn where_(span: &Option<Span>) -> String {
    match span {
        Some(s) => format!(" ({}:{})", s.path, s.start_line),
        None => String::new(),
    }
}

// ------------------------------------------------------------ expressions

/// One term of an expression: a column and its coefficient, with the
/// column's name for printing.
#[derive(Debug, Clone, Allocative)]
struct Term {
    index: u32,
    coef: f64,
    #[allocative(skip)]
    name: Arc<str>,
}

/// A linear expression in the making: terms in the order they were written
/// (repeats allowed; [`Lin::canonical`] merges them) and a constant. `problem`
/// is `None` for a plain number.
#[derive(Debug, Clone, Allocative)]
struct Lin {
    problem: Option<usize>,
    terms: Vec<Term>,
    constant: f64,
}

impl Lin {
    fn constant(c: f64) -> Lin {
        Lin { problem: None, terms: Vec::new(), constant: c }
    }

    fn join(problem: Option<usize>, other: Option<usize>) -> anyhow::Result<Option<usize>> {
        match (problem, other) {
            (Some(a), Some(b)) if a != b => {
                anyhow::bail!("these variables belong to two different opt.problem()s; an expression is in one problem")
            }
            (Some(a), _) | (None, Some(a)) => Ok(Some(a)),
            (None, None) => Ok(None),
        }
    }

    fn plus(mut self, other: Lin, sign: f64) -> anyhow::Result<Lin> {
        self.problem = Lin::join(self.problem, other.problem)?;
        self.terms.reserve(other.terms.len());
        for t in other.terms {
            self.terms.push(Term { coef: sign * t.coef, ..t });
        }
        self.constant += sign * other.constant;
        Ok(self)
    }

    fn scaled(mut self, k: f64) -> Lin {
        for t in &mut self.terms {
            t.coef *= k;
        }
        self.constant *= k;
        self
    }

    /// Distinct columns in increasing order, repeats summed in the order
    /// they were written (a stable sort keeps it), zeros dropped: one
    /// spelling per rule, so one memo key.
    fn canonical(&self) -> Vec<(u32, f64)> {
        let mut t: Vec<(u32, f64)> = self.terms.iter().map(|t| (t.index, t.coef)).collect();
        t.sort_by_key(|&(j, _)| j);
        let mut out: Vec<(u32, f64)> = Vec::with_capacity(t.len());
        for (j, a) in t {
            match out.last_mut() {
                Some((k, b)) if *k == j => *b += a,
                _ => out.push((j, a)),
            }
        }
        out.retain(|&(_, a)| a != 0.0);
        out
    }

    fn is_constant(&self) -> bool {
        self.terms.is_empty()
    }
}

fn finite(v: f64, what: &str) -> anyhow::Result<f64> {
    crate::check::finite(v, what)
}

/// The expression a value stands for: a variable, an expression or a
/// number. `Ok(None)` for anything else, so an operator can decline it.
fn lin_of(v: Value<'_>) -> anyhow::Result<Option<Lin>> {
    if let Some(x) = v.downcast_ref::<KVar>() {
        return Ok(Some(x.lin()));
    }
    if let Some(e) = v.downcast_ref::<KExpr>() {
        return Ok(Some(e.lin.clone()));
    }
    // A bool is not a number in Starlark, and `x == 3` is the bool a
    // comparison makes; say what to write instead (`expr_arg`).
    if v.unpack_bool().is_some() {
        return Ok(None);
    }
    if let Some(n) = UnpackFloat::unpack_value(v).map_err(|e| e.into_anyhow())? {
        return Ok(Some(Lin::constant(finite(n.0, "opt: a number in an expression")?)));
    }
    Ok(None)
}

/// An argument that must be an expression or a number.
fn expr_arg(v: Value<'_>, what: &str) -> anyhow::Result<Lin> {
    if v.unpack_bool().is_some() {
        anyhow::bail!(
            "{what}: got a bool. Comparisons cannot build rules in Starlark (`x == 3` is just True or False): \
             write p.constrain(x, equals=3), max= or min= instead"
        );
    }
    lin_of(v)?.ok_or_else(|| anyhow::anyhow!("{what}: expected a variable, an expression or a number, got {}", v.get_type()))
}

fn number(v: Value<'_>) -> anyhow::Result<Option<f64>> {
    if v.unpack_bool().is_some() || v.downcast_ref::<KVar>().is_some() || v.downcast_ref::<KExpr>().is_some() {
        return Ok(None);
    }
    Ok(UnpackFloat::unpack_value(v).map_err(|e| e.into_anyhow())?.map(|n| n.0))
}

fn not_linear(op: &str) -> starlark::Error {
    starlark::Error::new_other(anyhow::anyhow!(
        "opt: {op} of two variables is not linear; a linear program multiplies variables by numbers only"
    ))
}

fn compare_error() -> starlark::Error {
    starlark::Error::new_other(anyhow::anyhow!(
        "opt: comparisons cannot build rules in Starlark, since <, <= and == only ever give a bool: \
         write p.constrain(expr, max=...), min= or equals= instead (a bound may be an expression)"
    ))
}

fn alloc_lin<'v>(heap: Heap<'v>, lin: Lin) -> Value<'v> {
    heap.alloc(KExpr { lin })
}

/// `self op rhs` for both value types. `None` declines a right-hand side
/// that is not an expression, so Starlark can try the other operand.
fn binop<'v>(lhs: Lin, rhs: Value<'v>, heap: Heap<'v>, sign: f64) -> Option<starlark::Result<Value<'v>>> {
    let rhs = match lin_of(rhs) {
        Ok(Some(r)) => r,
        Ok(None) => return None,
        Err(e) => return Some(Err(starlark::Error::new_other(e))),
    };
    Some(lhs.plus(rhs, sign).map(|l| alloc_lin(heap, l)).map_err(starlark::Error::new_other))
}

fn mul<'v>(lhs: Lin, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
    match number(rhs) {
        Ok(Some(k)) => Some(finite(k, "opt: a coefficient").map(|k| alloc_lin(heap, lhs.scaled(k))).map_err(starlark::Error::new_other)),
        Ok(None) => {
            if rhs.downcast_ref::<KVar>().is_some() || rhs.downcast_ref::<KExpr>().is_some() {
                Some(Err(not_linear("a product")))
            } else {
                None
            }
        }
        Err(e) => Some(Err(starlark::Error::new_other(e))),
    }
}

fn div<'v>(lhs: Lin, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
    match number(rhs).map_err(starlark::Error::new_other)? {
        Some(k) if k == 0.0 => Err(starlark::Error::new_other(anyhow::anyhow!("opt: division of an expression by zero"))),
        Some(k) => Ok(alloc_lin(heap, lhs.scaled(1.0 / finite(k, "opt: a divisor").map_err(starlark::Error::new_other)?))),
        None if rhs.downcast_ref::<KVar>().is_some() || rhs.downcast_ref::<KExpr>().is_some() => Err(not_linear("a quotient")),
        None => Err(starlark::Error::new_other(anyhow::anyhow!("opt: cannot divide an expression by {}", rhs.get_type()))),
    }
}

fn sub<'v>(lhs: Lin, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
    match binop(lhs, rhs, heap, -1.0) {
        Some(r) => r,
        None => Err(starlark::Error::new_other(anyhow::anyhow!("opt: cannot subtract {} from an expression", rhs.get_type()))),
    }
}

fn show(lin: &Lin, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut first = true;
    for t in &lin.terms {
        let (sign, mag) = if t.coef < 0.0 { ("-", -t.coef) } else { ("+", t.coef) };
        if first {
            if sign == "-" {
                write!(f, "-")?;
            }
        } else {
            write!(f, " {sign} ")?;
        }
        if mag == 1.0 {
            write!(f, "{}", t.name)?;
        } else {
            write!(f, "{}*{}", fmt_num(mag), t.name)?;
        }
        first = false;
    }
    if first {
        write!(f, "{}", fmt_num(lin.constant))
    } else if lin.constant > 0.0 {
        write!(f, " + {}", fmt_num(lin.constant))
    } else if lin.constant < 0.0 {
        write!(f, " - {}", fmt_num(-lin.constant))
    } else {
        Ok(())
    }
}

// ----------------------------------------------------------------- values

/// A handle to a problem being built. The problem lives in the evaluation's
/// store, because building it is stateful -- `p.var(...)` adds to `p` --
/// while Starlark values are immutable.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KProblem {
    pub index: usize,
    pub feature: String,
}
starlark_simple_value!(KProblem);

impl fmt::Display for KProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opt.problem({})", self.feature)
    }
}

#[starlark_value(type = "OptProblem")]
impl<'v> StarlarkValue<'v> for KProblem {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("opt_problem_methods", problem_methods);
        Some(RES.methods())
    }
}

/// A variable of one problem. It is also an expression: arithmetic on it
/// makes an [`KExpr`].
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KVar {
    pub problem: usize,
    pub index: u32,
    #[allocative(skip)]
    name: Arc<str>,
    lower: f64,
    upper: f64,
    integer: bool,
}
starlark_simple_value!(KVar);

impl KVar {
    fn lin(&self) -> Lin {
        Lin { problem: Some(self.problem), terms: vec![Term { index: self.index, coef: 1.0, name: self.name.clone() }], constant: 0.0 }
    }
}

impl fmt::Display for KVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn bound<'v>(heap: Heap<'v>, v: f64) -> Value<'v> {
    if v.is_finite() { heap.alloc(v) } else { Value::new_none() }
}

#[starlark_value(type = "OptVar")]
impl<'v> StarlarkValue<'v> for KVar {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("opt_var_methods", var_methods);
        Some(RES.methods())
    }
    fn equals(&self, other: Value<'v>) -> starlark::Result<bool> {
        Ok(other.downcast_ref::<KVar>().is_some_and(|o| o.problem == self.problem && o.index == self.index))
    }
    fn write_hash(&self, hasher: &mut StarlarkHasher) -> starlark::Result<()> {
        use std::hash::Hash;
        (self.problem, self.index).hash(hasher);
        Ok(())
    }
    fn compare(&self, _other: Value<'v>) -> starlark::Result<std::cmp::Ordering> {
        Err(compare_error())
    }
    fn plus(&self, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        Ok(alloc_lin(heap, self.lin()))
    }
    fn minus(&self, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        Ok(alloc_lin(heap, self.lin().scaled(-1.0)))
    }
    fn add(&self, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        binop(self.lin(), rhs, heap, 1.0)
    }
    fn radd(&self, lhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        binop(self.lin(), lhs, heap, 1.0)
    }
    fn sub(&self, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        sub(self.lin(), rhs, heap)
    }
    fn mul(&self, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        mul(self.lin(), rhs, heap)
    }
    fn rmul(&self, lhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        mul(self.lin(), lhs, heap)
    }
    fn div(&self, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        div(self.lin(), rhs, heap)
    }
}

/// A linear expression: numbers times variables of one problem, plus a
/// constant.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KExpr {
    lin: Lin,
}
starlark_simple_value!(KExpr);

impl fmt::Display for KExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        show(&self.lin, f)
    }
}

#[starlark_value(type = "OptExpr")]
impl<'v> StarlarkValue<'v> for KExpr {
    fn compare(&self, _other: Value<'v>) -> starlark::Result<std::cmp::Ordering> {
        Err(compare_error())
    }
    fn plus(&self, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        Ok(alloc_lin(heap, self.lin.clone()))
    }
    fn minus(&self, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        Ok(alloc_lin(heap, self.lin.clone().scaled(-1.0)))
    }
    fn add(&self, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        binop(self.lin.clone(), rhs, heap, 1.0)
    }
    fn radd(&self, lhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        binop(self.lin.clone(), lhs, heap, 1.0)
    }
    fn sub(&self, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        sub(self.lin.clone(), rhs, heap)
    }
    fn mul(&self, rhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        mul(self.lin.clone(), rhs, heap)
    }
    fn rmul(&self, lhs: Value<'v>, heap: Heap<'v>) -> Option<starlark::Result<Value<'v>>> {
        mul(self.lin.clone(), lhs, heap)
    }
    fn div(&self, rhs: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        div(self.lin.clone(), rhs, heap)
    }
}

/// One answer to one solve.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSolution {
    problem: usize,
    feature: String,
    #[allocative(skip)]
    snapshot: Arc<Snapshot>,
    #[allocative(skip)]
    outcome: Arc<Vec<Solution>>,
    which: usize,
    /// The options the script asked for, and the digest of the question, for
    /// a later `conflict()` to probe with and be memoised under.
    #[allocative(skip)]
    options: Arc<Options>,
    #[allocative(skip)]
    digest: [u8; 32],
}
starlark_simple_value!(KSolution);

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snapshot({} variables, {} constraints)", self.model.columns.len(), self.model.rows.len())
    }
}

impl KSolution {
    fn sol(&self) -> &Solution {
        &self.outcome[self.which]
    }

    /// The point, or an error that says why there is none.
    fn point(&self) -> anyhow::Result<&[f64]> {
        let s = self.sol();
        if let Some(v) = &s.values {
            return Ok(v);
        }
        let why = match s.status {
            Status::Infeasible => {
                let names = conflict_names(self);
                if names.is_empty() {
                    "it is infeasible, and its LP relaxation is not: integrality is part of the conflict; call \
                     sol.conflict() for the rules that clash"
                        .to_owned()
                } else {
                    format!("it is infeasible; these cannot all hold: {}", self.described(&names))
                }
            }
            Status::Unbounded => "the objective can improve without limit: bound the variables it grows along".to_owned(),
            Status::InfeasibleOrUnbounded => "it is infeasible or unbounded".to_owned(),
            Status::TimeLimit => "the time limit stopped the solver before it found a feasible point".to_owned(),
            Status::NodeLimit => "the node limit stopped the search before it found a feasible point".to_owned(),
            // The host's sentence names the job and what lands it.
            Status::Pending if !s.message.is_empty() => format!("it is still being solved: {}", s.message),
            Status::Pending => "it is still being solved as a job; evaluate again to read the answer".to_owned(),
            Status::Error => format!("the solver failed: {}", s.message),
            other => format!("the solver stopped ({})", other.as_str()),
        };
        anyhow::bail!("{}: no solution to read: {why}", self.feature)
    }

    /// Names with their source lines, for a message.
    fn described(&self, names: &[String]) -> String {
        names
            .iter()
            .map(|n| {
                let row = self.snapshot.row_index.get(n.as_str()).copied();
                let at = row.map(|r| where_(&self.snapshot.row_spans[r])).unwrap_or_default();
                format!("{n:?}{at}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn row(&self, name: &str) -> anyhow::Result<usize> {
        self.snapshot.row_index.get(name).copied().ok_or_else(|| {
            let mut have: Vec<&str> = self.snapshot.model.rows.iter().map(|r| r.name.as_str()).take(12).collect();
            if self.snapshot.model.rows.len() > 12 {
                have.push("...");
            }
            anyhow::anyhow!("{}: no constraint named {name:?} (have: {})", self.feature, have.join(", "))
        })
    }

    /// How far row `r` is from its nearest bound at the solution, exactly 0
    /// within the solver's tolerance, and whether this solve left the row
    /// out. A row it kept is never short of its bounds beyond the
    /// tolerance; a row it left out may be, and is negative by as much.
    fn slack_of(&self, r: usize, x: &[f64]) -> (f64, bool) {
        let (lower, upper, relaxed) = self.snapshot.bounds(r);
        let a = match &self.sol().activities {
            Some(act) => act[r],
            None => self.snapshot.model.activity(r, x),
        };
        let mut s = f64::INFINITY;
        let mut scale: f64 = 1.0;
        if lower.is_finite() {
            s = s.min(a - lower);
            scale = scale.max(lower.abs());
        }
        if upper.is_finite() {
            s = s.min(upper - a);
            scale = scale.max(upper.abs());
        }
        (if s.abs() <= BINDING_TOLERANCE * scale { 0.0 } else { s }, relaxed)
    }
}

/// A row binds when its slack is within this, relative to the bound (or
/// absolute below 1): HiGHS's own primal tolerance is 1e-7, and a point with
/// rounded integers can sit a little past that.
const BINDING_TOLERANCE: f64 = 1e-6;

fn conflict_names(s: &KSolution) -> Vec<String> {
    let Some(c) = &s.sol().conflict else { return Vec::new() };
    let model = &s.snapshot.model;
    let mut out: Vec<String> = c.rows.iter().map(|&(r, _)| model.rows[r].name.clone()).collect();
    for &(j, side) in &c.columns {
        let name = &model.columns[j].name;
        match side {
            Side::Lower => out.push(format!("{name}.min")),
            Side::Upper => out.push(format!("{name}.max")),
            Side::Both => {
                out.push(format!("{name}.min"));
                out.push(format!("{name}.max"));
            }
        }
    }
    out
}

impl fmt::Display for KSolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.sol();
        match s.objective {
            Some(o) => write!(f, "opt.solution({}, {}, objective={})", self.feature, s.status.as_str(), fmt_num(o)),
            None => write!(f, "opt.solution({}, {})", self.feature, s.status.as_str()),
        }
    }
}

#[starlark_value(type = "OptSolution")]
impl<'v> StarlarkValue<'v> for KSolution {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("opt_solution_methods", solution_methods);
        Some(RES.methods())
    }
}

// ------------------------------------------------------------------ calls

fn with_problem<R>(
    p: &KProblem,
    eval: &Evaluator<'_, '_, '_>,
    f: impl FnOnce(&mut ProblemState) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let st = store(eval)?;
    let mut problems = st.problems.borrow_mut();
    let state = problems.get_mut(p.index).ok_or_else(|| anyhow::anyhow!("opt problem {} is gone", p.feature))?;
    f(state)
}

/// Keep the feature's counts in step with the problem, for the tree and
/// `cadpole inspect`.
fn note_counts(p: &KProblem, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<()> {
    let st = store(eval)?;
    let (vars, rows, nz) = {
        let problems = st.problems.borrow();
        let m = &problems[p.index].model;
        (m.columns.len(), m.rows.len(), m.index.len())
    };
    let mut ctx = st.ctx.borrow_mut();
    let fid = FeatureId(p.feature.clone());
    ctx.set_feature_arg(&fid, "variables", vars);
    ctx.set_feature_arg(&fid, "constraints", rows);
    ctx.set_feature_arg(&fid, "nonzeros", nz);
    Ok(())
}

fn optional_bound(v: Option<Value<'_>>, what: &str) -> anyhow::Result<Option<f64>> {
    match v {
        None => Ok(None),
        Some(v) if v.is_none() => Ok(None),
        Some(v) => match number(v)? {
            Some(n) => Ok(Some(finite(n, what)?)),
            None => anyhow::bail!("{what}: expected a number or None, got {}", v.get_type()),
        },
    }
}

/// A bound of `constrain`: a number, or an expression the rule moves to the
/// left-hand side.
enum BoundArg {
    Number(f64),
    Expr(Lin),
}

fn bound_arg(v: Option<Value<'_>>, what: &str) -> anyhow::Result<Option<BoundArg>> {
    let Some(v) = v else { return Ok(None) };
    if v.is_none() {
        return Ok(None);
    }
    let lin = expr_arg(v, what)?;
    Ok(Some(if lin.is_constant() { BoundArg::Number(lin.constant) } else { BoundArg::Expr(lin) }))
}

fn new_var(
    p: &KProblem,
    name: &str,
    lower: Option<f64>,
    upper: Option<f64>,
    integer: bool,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KVar> {
    if name.is_empty() {
        anyhow::bail!("{}: a variable needs a name", p.feature);
    }
    let span = call_span(eval);
    let lower = lower.unwrap_or(f64::NEG_INFINITY);
    let upper = upper.unwrap_or(f64::INFINITY);
    let var = with_problem(p, eval, |s| {
        if let Some(&j) = s.var_index.get(name) {
            anyhow::bail!("{}: variable {name:?} is already declared{}", p.feature, where_(&s.var_spans[j as usize]));
        }
        if s.model.columns.len() >= Limits::MAX_OPT_VARIABLES {
            anyhow::bail!("{}: more than {} variables in one problem", p.feature, Limits::MAX_OPT_VARIABLES);
        }
        let index = s.model.columns.len() as u32;
        let name: Arc<str> = Arc::from(name);
        s.model.columns.push(Column { name: name.to_string(), lower, upper, integer, cost: 0.0 });
        s.var_index.insert(name.to_string(), index);
        s.var_spans.push(span);
        Ok(KVar { problem: p.index, index, name, lower, upper, integer })
    })?;
    note_counts(p, eval)?;
    Ok(var)
}

fn check_vars(p: &KProblem, lin: &Lin, what: &str) -> anyhow::Result<()> {
    if let Some(q) = lin.problem
        && q != p.index
    {
        anyhow::bail!("{what}: the expression's variables belong to another opt.problem() than {}", p.feature);
    }
    Ok(())
}

fn constrain(
    p: &KProblem,
    expr: Value<'_>,
    min: Option<Value<'_>>,
    max: Option<Value<'_>>,
    equals: Option<Value<'_>>,
    name: Option<&str>,
    tags: Option<Value<'_>>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<String> {
    let what = format!("{}: constrain", p.feature);
    let explicit = match tags.filter(|t| !t.is_none()) {
        Some(t) => Some(tag_arg(t, &what)?),
        None => None,
    };
    let lhs = expr_arg(expr, &what)?;
    check_vars(p, &lhs, &what)?;
    let (min, max, equals) =
        (bound_arg(min, &format!("{what}: min="))?, bound_arg(max, &format!("{what}: max="))?, bound_arg(equals, &format!("{what}: equals="))?);
    // Move an expression bound to the left: `a <= b` is `a - b <= 0`.
    let (lin, lower, upper) = match (min, max, equals) {
        (None, None, None) => anyhow::bail!("{what}: give max=, min= (or both, for a range) or equals="),
        (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => anyhow::bail!("{what}: equals= stands alone; it cannot go with min= or max="),
        (None, None, Some(BoundArg::Number(e))) => (lhs, e, e),
        (Some(BoundArg::Number(a)), None, None) => (lhs, a, f64::INFINITY),
        (None, Some(BoundArg::Number(b)), None) => (lhs, f64::NEG_INFINITY, b),
        (Some(BoundArg::Number(a)), Some(BoundArg::Number(b)), None) => (lhs, a, b),
        (None, None, Some(BoundArg::Expr(e))) => (lhs.plus(e, -1.0)?, 0.0, 0.0),
        (Some(BoundArg::Expr(e)), None, None) => (lhs.plus(e, -1.0)?, 0.0, f64::INFINITY),
        (None, Some(BoundArg::Expr(e)), None) => (lhs.plus(e, -1.0)?, f64::NEG_INFINITY, 0.0),
        (Some(_), Some(_), None) => anyhow::bail!(
            "{what}: a range takes numbers for both min= and max=; with an expression for either, write two rules"
        ),
    };
    check_vars(p, &lin, &what)?;
    let terms = lin.canonical();
    // The constant moves to the bounds.
    let (lower, upper) = (lower - lin.constant, upper - lin.constant);
    let span = call_span(eval);
    let name = with_problem(p, eval, |s| {
        if s.model.rows.len() >= Limits::MAX_OPT_CONSTRAINTS {
            anyhow::bail!("{}: more than {} constraints in one problem", p.feature, Limits::MAX_OPT_CONSTRAINTS);
        }
        if s.model.index.len() + terms.len() > Limits::MAX_OPT_NONZEROS {
            anyhow::bail!("{}: more than {} nonzero coefficients in one problem", p.feature, Limits::MAX_OPT_NONZEROS);
        }
        let name = match name {
            Some(n) => {
                if n.is_empty() {
                    anyhow::bail!("{what}: name= may not be empty");
                }
                if let Some(&r) = s.row_index.get(n) {
                    anyhow::bail!("{what}: a constraint is already named {n:?}{}", where_(&s.row_spans[r]));
                }
                n.to_owned()
            }
            None => {
                let mut k = s.model.rows.len() + 1;
                loop {
                    let n = format!("constraint_{k}");
                    if !s.row_index.contains_key(&n) {
                        break n;
                    }
                    k += 1;
                }
            }
        };
        let mut tags: Vec<(Arc<str>, Arc<str>)> = explicit
            .unwrap_or_else(|| tags_from_name(&name))
            .iter()
            .map(|(k, v)| (s.intern(k), s.intern(v)))
            .collect();
        tags.sort_by(|a, b| a.0.cmp(&b.0));
        tags.dedup_by(|a, b| a.0 == b.0);
        s.row_index.insert(name.clone(), s.model.rows.len());
        s.row_spans.push(span);
        s.row_tags.push(tags);
        s.model.push_row(Row { name: name.clone(), lower, upper }, &terms);
        Ok(name)
    })?;
    note_counts(p, eval)?;
    Ok(name)
}

fn set_objective(p: &KProblem, sense: Sense, expr: Value<'_>, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<()> {
    let what = format!("{}: {}", p.feature, if sense == Sense::Maximize { "maximize" } else { "minimize" });
    let lin = expr_arg(expr, &what)?;
    check_vars(p, &lin, &what)?;
    let span = call_span(eval);
    with_problem(p, eval, |s| {
        if let Some(o) = &s.objective {
            let first = if o.sense == Sense::Maximize { "maximize" } else { "minimize" };
            anyhow::bail!("{what}: the objective is already set by {first}(){}; a problem has one", where_(&o.span));
        }
        s.objective = Some(Objective { sense, terms: lin.canonical(), constant: lin.constant, span });
        Ok(())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What remains of the evaluation's deadline, in seconds, or `None` when it
/// has none. A solve is a native op the deadline cannot interrupt, so the
/// solver is given the rest of the budget as its own limit.
fn budget_left(st: &crate::store::Store) -> Option<f64> {
    let budget = st.deadline.budget_ms();
    if budget == 0 {
        return None;
    }
    Some(((budget as f64 - st.deadline.elapsed_ms()) / 1000.0).max(1e-3))
}

/// What `solve()` was asked, before it is checked.
struct SolveArgs<'v> {
    time_limit: Option<f64>,
    gap: Option<f64>,
    count: i32,
    distinct: Option<Value<'v>>,
    fix: Option<Value<'v>>,
    relax: Option<Value<'v>>,
    drop: Option<Value<'v>>,
    objective: bool,
    max_nodes: Option<i32>,
    start: Option<Value<'v>>,
    job: bool,
}

/// `fix=`: a dict from this problem's variables to the numbers they take for
/// this solve. A whole-number variable takes a whole number.
fn fixes(p: &KProblem, v: Option<Value<'_>>, what: &str, arg: &str) -> anyhow::Result<Vec<(u32, f64)>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(Vec::new()) };
    let d: UnpackDictEntries<Value<'_>, Value<'_>> = UnpackDictEntries::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: {arg} is a dict from variables to numbers, got {}", v.get_type()))?;
    let mut out = Vec::with_capacity(d.entries.len());
    for (k, val) in d.entries {
        let x = k
            .downcast_ref::<KVar>()
            .ok_or_else(|| anyhow::anyhow!("{what}: {arg} keys are variables, got {}", k.get_type()))?;
        if x.problem != p.index {
            anyhow::bail!("{what}: {arg} names {} of another problem", x.name);
        }
        let n = number(val)?
            .ok_or_else(|| anyhow::anyhow!("{what}: {arg} gives {} {}, not a number", x.name, val.get_type()))?;
        let n = finite(n, &format!("{what}: {arg} {}", x.name))?;
        if x.integer && n.fract() != 0.0 {
            anyhow::bail!("{what}: {arg} gives {} {}, but it takes whole numbers", x.name, fmt_num(n));
        }
        out.push((x.index, n));
    }
    out.sort_by_key(|&(j, _)| j);
    Ok(out)
}

/// A tag selector: a dict from tag to a value or a list of values (every key
/// must match), or a list of such dicts (any one may).
fn selector(v: Value<'_>, what: &str) -> anyhow::Result<Vec<Clause>> {
    let one = |d: Value<'_>| -> anyhow::Result<Clause> {
        let e: UnpackDictEntries<&str, Value<'_>> = UnpackDictEntries::unpack_value(d)
            .map_err(|e| e.into_anyhow())?
            .ok_or_else(|| anyhow::anyhow!("{what}: a selector is a dict from tag to value(s), got {}", d.get_type()))?;
        let mut clause = Vec::with_capacity(e.entries.len());
        for (k, vals) in e.entries {
            let vals: Vec<String> = if let Some(v) = vals.unpack_str() {
                vec![v.to_owned()]
            } else {
                let list: UnpackListOrTuple<&str> = UnpackListOrTuple::unpack_value(vals)
                    .map_err(|e| e.into_anyhow())?
                    .ok_or_else(|| anyhow::anyhow!("{what}: tag {k:?} takes a string or a list of strings, got {}", vals.get_type()))?;
                list.items.into_iter().map(str::to_owned).collect()
            };
            clause.push((k.to_owned(), vals));
        }
        Ok(clause)
    };
    if v.unpack_str().is_none()
        && let Some(list) = UnpackListOrTuple::<Value<'_>>::unpack_value(v).map_err(|e| e.into_anyhow())?
    {
        return list.items.into_iter().map(one).collect();
    }
    Ok(vec![one(v)?])
}

/// `tags=` on a rule: a dict from tag to string.
fn tag_arg(v: Value<'_>, what: &str) -> anyhow::Result<Vec<(String, String)>> {
    let e: UnpackDictEntries<&str, &str> = UnpackDictEntries::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: tags= is a dict from tag to string, got {}", v.get_type()))?;
    let mut out = Vec::with_capacity(e.entries.len());
    for (k, val) in e.entries {
        if k.is_empty() {
            anyhow::bail!("{what}: tags= has an empty tag");
        }
        out.push((k.to_owned(), val.to_owned()));
    }
    Ok(out)
}

/// `relax=`: the names of rules this solve leaves out.
fn relaxed(st: &ProblemState, v: Option<Value<'_>>, what: &str) -> anyhow::Result<Vec<usize>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(Vec::new()) };
    let names: UnpackListOrTuple<&str> = UnpackListOrTuple::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: relax= is a list of constraint names, got {}", v.get_type()))?;
    let mut out = Vec::with_capacity(names.items.len());
    for n in names.items {
        let r = st
            .row_index
            .get(n)
            .ok_or_else(|| anyhow::anyhow!("{what}: relax= names {n:?}, which is no constraint of this problem"))?;
        out.push(*r);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn solve<'v>(p: &KProblem, args: SolveArgs<'v>, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
    let SolveArgs { time_limit, gap, count, distinct, fix, relax, drop, objective, max_nodes, start, job } = args;
    let what = format!("{}: solve", p.feature);
    if let Some(n) = max_nodes
        && n < 1
    {
        anyhow::bail!("{what}: max_nodes= is a number of nodes of 1 or more, got {n}");
    }
    if let Some(t) = time_limit
        && t <= 0.0
    {
        anyhow::bail!("{what}: time_limit= is a positive number of seconds, got {}", fmt_num(t));
    }
    let gap = gap.unwrap_or(DEFAULT_MIP_REL_GAP);
    if gap < 0.0 {
        anyhow::bail!("{what}: gap= is a relative gap of 0 or more, got {}", fmt_num(gap));
    }
    if count < 1 || count as usize > Limits::MAX_OPT_SOLUTIONS {
        anyhow::bail!("{what}: count= is from 1 to {}, got {count}", Limits::MAX_OPT_SOLUTIONS);
    }
    let distinct: Vec<usize> = match distinct {
        None => Vec::new(),
        Some(v) if v.is_none() => Vec::new(),
        Some(v) => {
            let items: UnpackListOrTuple<Value<'v>> = UnpackListOrTuple::unpack_value(v)
                .map_err(|e| e.into_anyhow())?
                .ok_or_else(|| anyhow::anyhow!("{what}: distinct= is a list of binary variables"))?;
            let mut out = Vec::new();
            for item in items.items {
                let x = item.downcast_ref::<KVar>().ok_or_else(|| {
                    anyhow::anyhow!("{what}: distinct= lists variables, got {}", item.get_type())
                })?;
                if x.problem != p.index {
                    anyhow::bail!("{what}: distinct= names {} of another problem", x.name);
                }
                if !(x.integer && x.lower >= 0.0 && x.upper <= 1.0) {
                    anyhow::bail!("{what}: distinct= names {}, which is not binary; solutions differ in binaries", x.name);
                }
                out.push(x.index as usize);
            }
            if out.is_empty() {
                anyhow::bail!("{what}: distinct= is empty; leave it out to mean every binary");
            }
            out
        }
    };
    let fix = fixes(p, fix, &what, "fix=")?;
    let start = fixes(p, start, &what, "start=")?;
    let span = call_span(eval);
    let st = store(eval)?;
    let (snapshot, fid) = {
        let problems = st.problems.borrow();
        let s = problems.get(p.index).ok_or_else(|| anyhow::anyhow!("opt problem {} is gone", p.feature))?;
        let mut relax = relaxed(s, relax, &what)?;
        if let Some(d) = drop.filter(|d| !d.is_none()) {
            relax.extend(s.select(&selector(d, &format!("{what}: drop="))?));
            relax.sort_unstable();
            relax.dedup();
        }
        let changes = Changes { fix, relax, objective };
        let snapshot = s.snapshot(&changes);
        for &(j, v) in &start {
            let c = &snapshot.model.columns[j as usize];
            if v < c.lower || v > c.upper {
                anyhow::bail!(
                    "{what}: start= gives {} {}, outside its bounds{}",
                    c.name,
                    fmt_num(v),
                    match (c.lower.is_finite(), c.upper.is_finite()) {
                        (true, true) => format!(" [{}, {}]", fmt_num(c.lower), fmt_num(c.upper)),
                        (true, false) => format!(" (at least {})", fmt_num(c.lower)),
                        (false, true) => format!(" (at most {})", fmt_num(c.upper)),
                        (false, false) => String::new(),
                    }
                );
            }
        }
        (Arc::new(snapshot), s.feature.clone())
    };
    note_counts(p, eval)?;
    let solver = cadpole_core::opt::installed().ok_or_else(|| anyhow::anyhow!("{what}: {NOT_INSTALLED}"))?;
    // Where the solve may run: the host may hand it to a job only
    // for an evaluation that allows it, and never in a drag preview.
    let preview = st.ctx.borrow().preview;
    let options = Options {
        time_limit,
        deadline: None,
        mip_rel_gap: gap,
        count: count as usize,
        distinct,
        max_nodes: max_nodes.map(|n| n as u64),
        start: start.into_iter().map(|(j, v)| (j as usize, v)).collect(),
        job,
        may_start_job: st.limits.solve_jobs && !preview,
        inline_only: false,
        origin: format!("{}{}", p.feature, where_(&span)),
    };
    // The key: the solver, the feature the answer is recorded under, and a
    // digest of everything in the problem and the options, in order.
    let digest = cadpole_core::opt::digest(&snapshot.model, &options, solver.identity());
    let mut key = KeyBuilder::new("optimize");
    key.put(&fid).str(&hex(&digest));
    // The deadline's share is the solver's to honour and not in the key: it
    // only ever stops a solve, and a stopped solve is not kept.
    let run_options = Options { deadline: budget_left(st), ..options.clone() };
    let outcome = {
        let mut ctx = st.ctx.borrow_mut();
        let model = &snapshot.model;
        ctx.produce_value_if::<Vec<Solution>, _>("optimize", &fid, span.clone(), key.finish(), |ctx| {
            let sols = solver
                .solve(model, &run_options)
                .map_err(|e| cadpole_core::CadpoleError::Invalid(format!("{what}: {e}")))?;
            let first = &sols[0];
            ctx.set_feature_arg(&fid, "solver", solver.identity());
            ctx.set_feature_arg(&fid, "status", first.status.as_str());
            match first.objective {
                Some(o) => ctx.set_feature_arg(&fid, "objective", fmt_num(o)),
                None => ctx.set_feature_arg(&fid, "objective", "none"),
            }
            if model.is_mip() {
                ctx.set_feature_arg(&fid, "nodes", first.nodes);
            }
            ctx.set_feature_arg(&fid, "solutions", sols.len());
            let keep = sols.iter().all(|s| s.status.is_repeatable());
            let bytes = sols.iter().map(Solution::bytes).sum::<usize>() + 64;
            Ok((sols, bytes, keep))
        })?
    };
    let heap = eval.heap();
    let question = Arc::new(options.clone());
    let make = |which: usize| KSolution {
        problem: p.index,
        feature: p.feature.clone(),
        snapshot: snapshot.clone(),
        outcome: outcome.clone(),
        which,
        options: question.clone(),
        digest,
    };
    // Say what went wrong, where the call was, without failing: a test must
    // be able to assert infeasibility, and Starlark has no try/except.
    {
        let first = make(0);
        let status = outcome[0].status;
        let message = match status {
            Status::Optimal => None,
            Status::Infeasible => {
                let names = conflict_names(&first);
                Some(if names.is_empty() {
                    format!(
                        "{}: infeasible, while its LP relaxation is not: integrality is part of the conflict; call \
                         sol.conflict() for the rules that clash",
                        p.feature
                    )
                } else {
                    format!("{}: infeasible; these cannot all hold: {}", p.feature, first.described(&names))
                })
            }
            Status::Unbounded => Some(format!("{}: unbounded; the objective can improve without limit", p.feature)),
            Status::Pending if !outcome[0].message.is_empty() => Some(format!("{}: pending: {}", p.feature, outcome[0].message)),
            Status::Pending => Some(format!("{}: being solved as a job; evaluate again to read the answer", p.feature)),
            Status::TimeLimit | Status::NodeLimit | Status::IterationLimit if outcome[0].values.is_some() => Some(format!(
                "{}: stopped at the {} with a point{}{}",
                p.feature,
                status.as_str().replace('_', " "),
                match outcome[0].mip_gap {
                    Some(g) if g.is_finite() => format!(" within {} of the best bound", fmt_percent(g)),
                    _ => String::new(),
                },
                // Where a kept answer came from: "; kept from job 3f9a..., which
                // ran to its 600-s limit".
                if outcome[0].message.is_empty() { String::new() } else { format!("; {}", outcome[0].message) }
            )),
            other => Some(format!("{}: the solver stopped ({}) {}", p.feature, other.as_str(), outcome[0].message).trim_end().to_owned()),
        };
        if let Some(m) = message {
            let mut ctx = st.ctx.borrow_mut();
            // A limit that left a point is the normal end of a big MILP: an
            // info, like an infeasible answer a test may be asserting. A
            // limit with nothing to show, or a failure, is a warning.
            if status.is_final() || status == Status::Pending || outcome[0].values.is_some() {
                ctx.info(m, &fid, span.clone());
            } else {
                ctx.diagnostics.push(Diagnostic::warning(m).with_span(span.clone()).with_feature(fid.clone()));
            }
        }
    }
    if let Some(m) = cut_short(&p.feature, count as usize, &outcome) {
        // Fewer solutions than asked for, and not because there are no
        // more: a warning, since a script reading the list as complete
        // would be wrong about the problem.
        let mut ctx = st.ctx.borrow_mut();
        ctx.diagnostics.push(Diagnostic::warning(m).with_span(span.clone()).with_feature(fid.clone()));
    }
    if count == 1 {
        Ok(heap.alloc(make(0)))
    } else {
        let all: Vec<Value<'v>> = (0..outcome.len()).map(|i| heap.alloc(make(i))).collect();
        Ok(heap.alloc(all))
    }
}

/// What a script is told when `count=` asked for more solutions than came
/// back and the search for the next one was stopped -- by a limit, an
/// interrupt, an error -- rather than finding there were no more: the list
/// then ends with that answer, point or none (`cadpole_opt::solve`), and it
/// may not be every solution there is. `None` for a list that is complete,
/// and for a first solve that stopped, which has its own message.
pub(crate) fn cut_short(feature: &str, count: usize, outcome: &[Solution]) -> Option<String> {
    let last = outcome.last()?;
    if count < 2 || outcome.len() < 2 || last.status == Status::Optimal {
        return None;
    }
    let found = outcome.iter().filter(|s| s.values.is_some()).count();
    let what = match last.status {
        Status::Error if !last.message.is_empty() => format!("a solver error ({})", last.message),
        Status::Error => "a solver error".to_owned(),
        other => format!("the {}", other.as_str().replace('_', " ")),
    };
    Some(format!(
        "{feature}: count={count}: {what} stopped the search after {found} solution{}; there may be more{}",
        if found == 1 { "" } else { "s" },
        if last.values.is_none() { ", and the last entry has no point" } else { "" }
    ))
}

/// A gap as a percentage, to two decimals below 1 % and fewer above.
fn fmt_percent(g: f64) -> String {
    let p = g * 100.0;
    let s = if p >= 10.0 {
        format!("{p:.0}")
    } else if p >= 1.0 {
        format!("{p:.1}")
    } else {
        format!("{p:.2}")
    };
    format!("{s}%")
}

/// `sol.conflict()` for a MILP whose LP relaxation is feasible:
/// `cadpole_core::opt::conflict_by_deletion` over the problem as this solve saw
/// it, families first, bounded by `limits` and what is left of the
/// evaluation's budget, and memoised under the solve's own question unless
/// the clock decided any of it. Only `conflict()` calls this: no message
/// runs it to name a conflict.
fn by_deletion(s: &KSolution, limits: DeletionLimits, eval: &mut Evaluator<'_, '_, '_>) -> anyhow::Result<Vec<String>> {
    let what = format!("{}: conflict", s.feature);
    let span = call_span(eval);
    let st = store(eval)?;
    let solver = cadpole_core::opt::installed().ok_or_else(|| anyhow::anyhow!("{what}: {NOT_INSTALLED}"))?;
    let fid = FeatureId(s.feature.clone());
    // The answer depends on the question, the solver, the probe count and
    // whether it stops at families -- not on the time limit: an answer the
    // clock cut short is never kept, and one it did not is the same for any
    // limit.
    let mut key = KeyBuilder::new("optimize-conflict");
    key.put(&fid).str(&hex(&s.digest)).str(solver.identity());
    match limits.max_probes {
        Some(n) => key.bool(true).u64(n as u64),
        None => key.bool(false),
    };
    key.bool(limits.families_only);
    let options = Options { deadline: budget_left(st), ..(*s.options).clone() };
    let groups = s.snapshot.families();
    let model = &s.snapshot.model;
    let found = {
        let mut ctx = st.ctx.borrow_mut();
        ctx.produce_value_if::<Deleted, _>("optimize-conflict", &fid, span.clone(), key.finish(), |_| {
            let d = conflict_by_deletion(solver, model, &options, &groups, limits);
            let keep = !d.clock;
            Ok((d, 64 + 8 * model.rows.len(), keep))
        })?
    };
    let loose = match found.stop {
        Some(Stop::Probes) => Some(format!("stopped at max_probes={}", found.probes)),
        Some(Stop::Time) => Some(format!("its time ran out after {} probes", found.probes)),
        None if found.unanswered > 0 => {
            Some(format!("{} of its {} probes stopped at a limit before they answered", found.unanswered, found.probes))
        }
        None => None,
    };
    if let Some(why) = loose {
        let mut ctx = st.ctx.borrow_mut();
        ctx.info(
            format!(
                "{what}: {why}: these {} rules cannot all hold, but the set may not be irreducible -- some may not be \
                 needed",
                found.rows.len()
            ),
            &fid,
            span,
        );
    }
    Ok(found.rows.iter().map(|&r| model.rows[r].name.clone()).collect())
}

fn value_of<'v>(s: &KSolution, x: Value<'v>, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
    let what = format!("{}: value", s.feature);
    let point = s.point()?;
    if let Some(v) = x.downcast_ref::<KVar>() {
        if v.problem != s.problem {
            anyhow::bail!("{what}: {} belongs to another problem", v.name);
        }
        let Some(&val) = point.get(v.index as usize) else {
            anyhow::bail!("{what}: {} was declared after this solve", v.name);
        };
        return Ok(if v.integer { heap.alloc(val as i64) } else { heap.alloc(val) });
    }
    let lin = expr_arg(x, &what)?;
    if let Some(q) = lin.problem
        && q != s.problem
    {
        anyhow::bail!("{what}: the expression belongs to another problem");
    }
    let mut total = lin.constant;
    for (j, a) in lin.canonical() {
        let Some(&val) = point.get(j as usize) else {
            anyhow::bail!("{what}: the expression uses a variable declared after this solve");
        };
        total += a * val;
    }
    Ok(heap.alloc(if total == 0.0 { 0.0 } else { total }))
}

// -------------------------------------------------------------- namespace

#[starlark_module]
pub(crate) fn opt_namespace(builder: &mut GlobalsBuilder) {
    /// A new linear or mixed-integer program: declare variables on
    /// it with `p.var(...)` and `p.binary(...)`, write rules with
    /// `p.constrain(...)` and an objective with `p.maximize(...)` or
    /// `p.minimize(...)`, then `p.solve()`. Records a feature of kind
    /// `optimize` named `name` (or `optimize_<n>`), whose args carry the
    /// counts, the solver and the last answer's status and objective.
    ///
    /// Solving needs native Cadpole: the CLI, `cadpole serve` or `cadpole mcp`. The
    /// browser's engine and the Apple apps build a problem but refuse to
    /// solve it.
    ///
    /// ```python
    /// p = opt.problem(name="sizes")
    /// w = p.var("w", min=8, max=20)
    /// d = p.var("d", min=8, max=20)
    /// p.constrain(w + d, max=30, name="perimeter")
    /// p.maximize(2 * w + d)
    /// sol = p.solve()
    /// ```
    fn problem<'v>(
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KProblem> {
        let span = call_span(eval);
        let st = store(eval)?;
        let fid = {
            let mut ctx = st.ctx.borrow_mut();
            let mut args = std::collections::BTreeMap::new();
            args.insert("variables".into(), "0".into());
            args.insert("constraints".into(), "0".into());
            args.insert("nonzeros".into(), "0".into());
            ctx.new_feature("optimize", name.map(str::to_owned), vec![], args, span)?
        };
        let mut problems = st.problems.borrow_mut();
        let index = problems.len();
        problems.push(ProblemState::new(fid.clone()));
        Ok(KProblem { index, feature: fid.0 })
    }

    /// The sum of a list (or tuple) of variables, expressions and numbers,
    /// as one expression. Starlark has no `sum()`, and `a + b + c + ...`
    /// copies the expression at every `+`: for long sums this is the cheap
    /// way. An empty list is the expression 0.
    fn sum<'v>(#[starlark(require = pos)] items: UnpackListOrTuple<Value<'v>>) -> anyhow::Result<KExpr> {
        let mut lin = Lin::constant(0.0);
        for (i, v) in items.items.into_iter().enumerate() {
            let term = expr_arg(v, &format!("opt.sum: item {i}"))?;
            lin = lin.plus(term, 1.0)?;
        }
        Ok(KExpr { lin })
    }

    /// `coeffs[0] * exprs[0] + coeffs[1] * exprs[1] + ...`: numbers times
    /// variables or expressions, the two lists the same length.
    fn dot<'v>(
        #[starlark(require = pos)] coeffs: UnpackListOrTuple<Value<'v>>,
        #[starlark(require = pos)] exprs: UnpackListOrTuple<Value<'v>>,
    ) -> anyhow::Result<KExpr> {
        if coeffs.items.len() != exprs.items.len() {
            anyhow::bail!("opt.dot: {} coefficients for {} expressions", coeffs.items.len(), exprs.items.len());
        }
        let mut lin = Lin::constant(0.0);
        for (i, (c, e)) in coeffs.items.into_iter().zip(exprs.items).enumerate() {
            let k = number(c)?.ok_or_else(|| anyhow::anyhow!("opt.dot: coefficient {i} is {}, not a number", c.get_type()))?;
            let term = expr_arg(e, &format!("opt.dot: expression {i}"))?;
            lin = lin.plus(term.scaled(finite(k, &format!("opt.dot: coefficient {i}"))?), 1.0)?;
        }
        Ok(KExpr { lin })
    }
}

#[starlark_module]
fn problem_methods(builder: &mut MethodsBuilder) {
    /// The problem's feature id: its `name=`, or `optimize_<n>`.
    #[starlark(attribute)]
    fn id<'v>(this: &KProblem) -> anyhow::Result<String> {
        Ok(this.feature.clone())
    }

    /// Declare a continuous variable, or a whole-number one with
    /// `integer=True`. `min=` and `max=` are its bounds; `None` (the
    /// default) is unbounded on that side, as in most modelling languages,
    /// so write `min=0` for a size. Names are unique within a problem and
    /// are what solutions and conflicts print.
    fn var<'v>(
        this: &KProblem,
        name: &str,
        #[starlark(require = named)] min: Option<Value<'v>>,
        #[starlark(require = named)] max: Option<Value<'v>>,
        #[starlark(require = named, default = false)] integer: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KVar> {
        let lower = optional_bound(min, &format!("{}: var {name:?}: min=", this.feature))?;
        let upper = optional_bound(max, &format!("{}: var {name:?}: max=", this.feature))?;
        new_var(this, name, lower, upper, integer, eval)
    }

    /// Declare a yes-or-no variable: an integer from 0 to 1. Binaries are
    /// what `solve(count=...)` tells solutions apart by.
    fn binary<'v>(this: &KProblem, name: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KVar> {
        new_var(this, name, Some(0.0), Some(1.0), true, eval)
    }

    /// Add the rule `min <= expr <= max`, or `expr == equals`. Give
    /// `max=`, `min=`, both (a range) or `equals=`. A bound may be an
    /// expression, which is how to write `a <= b` -- `p.constrain(a,
    /// max=b)` -- since comparison operators cannot build rules in
    /// Starlark; a range takes numbers for both ends. Returns the rule's
    /// name: `name=`, or `constraint_<n>`, which `sol.slack()`,
    /// `sol.dual()`, `sol.binding()` and `sol.conflict()` use. `tags=` is a
    /// dict from tag to string -- `family`, `floor` and `room` are the ones a
    /// conflict groups by -- for `solve(drop=)` and `p.constraints(tags=)`;
    /// with no `tags=`, a name of the form `family:floor:room:what` carries
    /// those three.
    fn constrain<'v>(
        this: &KProblem,
        #[starlark(require = pos)] expr: Value<'v>,
        #[starlark(require = named)] min: Option<Value<'v>>,
        #[starlark(require = named)] max: Option<Value<'v>>,
        #[starlark(require = named)] equals: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] tags: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<String> {
        constrain(this, expr, min, max, equals, name, tags, eval)
    }

    /// The names of this problem's constraints, in the order they were
    /// written: all of them, or those `tags=` selects -- a dict from tag to a
    /// value or a list of values, all of which must match, or a list of such
    /// dicts, any of which may (`{"family": "apart", "floor": ["L1", "L2"]}`).
    fn constraints<'v>(
        this: &KProblem,
        #[starlark(require = named)] tags: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<String>> {
        let what = format!("{}: constraints", this.feature);
        with_problem(this, eval, |s| {
            let rows: Vec<usize> = match tags.filter(|t| !t.is_none()) {
                Some(t) => s.select(&selector(t, &format!("{what}: tags="))?),
                None => (0..s.model.rows.len()).collect(),
            };
            Ok(rows.into_iter().map(|r| s.model.rows[r].name.clone()).collect())
        })
    }

    /// Make `expr` as large as possible. A problem has one objective;
    /// setting a second is an error naming the first one's line. A problem
    /// with none is a feasibility problem: any point that satisfies the
    /// rules.
    fn maximize<'v>(
        this: &KProblem,
        #[starlark(require = pos)] expr: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        set_objective(this, Sense::Maximize, expr, eval)?;
        Ok(NoneType)
    }

    /// Make `expr` as small as possible; see `maximize`.
    fn minimize<'v>(
        this: &KProblem,
        #[starlark(require = pos)] expr: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        set_objective(this, Sense::Minimize, expr, eval)?;
        Ok(NoneType)
    }

    /// Solve the problem as it stands, with HiGHS. Returns an
    /// `OptSolution`, or with `count=` above 1 a list of them: the best,
    /// then the best that differs from every earlier one in the binaries
    /// `distinct=` lists (every binary when it is None), stopping early
    /// when there are no more. A search for the next one that a limit
    /// stops ends the list with that answer instead -- its status says
    /// which limit, and it has no point when the limit found none -- and a
    /// warning says the list may not be complete. That only finds different *designs* when
    /// those binaries decide the design: an either/or rule's binaries can
    /// often flip without anything moving. For real options, add rules
    /// between solves -- `p.constrain()` works after a solve, and the next
    /// solve is the problem as it then stands.
    ///
    /// For this solve alone: `fix={var: number}` holds variables at values
    /// (fix every integer of a MILP and what is left is an LP, with the duals
    /// local to that arrangement); `relax=[names]` leaves rules out, and
    /// `drop=` the rules a tag selector picks (as `p.constraints(tags=)`
    /// takes); `objective=False` asks for any feasible point, which is what a
    /// probe wants. `start={var: number}` is where a MILP search begins --
    /// an earlier solution, or a hand plan -- which HiGHS completes for any
    /// variable it leaves out; a start can change which tied optimum, or which
    /// point a limit stops at, comes back, so it is part of the memo key.
    ///
    /// Never raises for an infeasible or unbounded problem -- a test must be
    /// able to assert one -- but reads of a solution that has no point do,
    /// naming the conflicting rules and their lines. A limit returns the best
    /// point found, its `gap` and `bound`, and a status that says which limit
    /// stopped it: `max_nodes=` counts branch-and-bound nodes and is
    /// deterministic, so its answer is memoised like any other;
    /// `time_limit=` is in seconds, capped by the evaluation's own budget,
    /// and a solve it stops is never memoised. `gap=` is the relative MILP
    /// gap at which the answer counts as optimal (default 1e-4, HiGHS's
    /// own). The same problem gives the same answer on every run on one
    /// machine; when several corners tie, which one comes back can differ
    /// between Linux and macOS, so assert objectives and slacks rather than
    /// tied values.
    ///
    /// A solve too long for the evaluation's budget, or one asked for with
    /// `job=True`, runs as a job where the host has them (`cadpole serve`, `cadpole
    /// solve`): the solution's status is then "pending" -- `ok` is
    /// False and `value()` raises, naming the job -- and the document is
    /// evaluated again when the job lands, with its answer. `job=` never
    /// changes the answer, only where it is computed.
    fn solve<'v>(
        this: &KProblem,
        #[starlark(require = named)] time_limit: Option<Value<'v>>,
        #[starlark(require = named)] gap: Option<Value<'v>>,
        #[starlark(require = named, default = 1)] count: i32,
        #[starlark(require = named)] distinct: Option<Value<'v>>,
        #[starlark(require = named)] fix: Option<Value<'v>>,
        #[starlark(require = named)] relax: Option<Value<'v>>,
        #[starlark(require = named)] drop: Option<Value<'v>>,
        #[starlark(require = named, default = true)] objective: bool,
        #[starlark(require = named)] max_nodes: Option<i32>,
        #[starlark(require = named)] start: Option<Value<'v>>,
        #[starlark(require = named, default = false)] job: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let time_limit = optional_bound(time_limit, &format!("{}: solve: time_limit=", this.feature))?;
        let gap = optional_bound(gap, &format!("{}: solve: gap=", this.feature))?;
        let args = SolveArgs { time_limit, gap, count, distinct, fix, relax, drop, objective, max_nodes, start, job };
        solve(this, args, eval)
    }
}

#[starlark_module]
fn var_methods(builder: &mut MethodsBuilder) {
    /// The variable's name.
    #[starlark(attribute)]
    fn name<'v>(this: &KVar) -> anyhow::Result<String> {
        Ok(this.name.to_string())
    }

    /// Its lower bound, or None when it has none.
    #[starlark(attribute)]
    fn min<'v>(this: &KVar, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(bound(heap, this.lower))
    }

    /// Its upper bound, or None when it has none.
    #[starlark(attribute)]
    fn max<'v>(this: &KVar, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(bound(heap, this.upper))
    }

    /// Whether it takes whole numbers only.
    #[starlark(attribute)]
    fn integer<'v>(this: &KVar) -> anyhow::Result<bool> {
        Ok(this.integer)
    }
}

#[starlark_module]
fn solution_methods(builder: &mut MethodsBuilder) {
    /// How the solve ended: "optimal", "infeasible", "unbounded",
    /// "infeasible_or_unbounded", "time_limit", "node_limit",
    /// "iteration_limit", "interrupted" or "error". A limit may still have
    /// a point: see `feasible`.
    #[starlark(attribute)]
    fn status<'v>(this: &KSolution) -> anyhow::Result<String> {
        Ok(this.sol().status.as_str().to_owned())
    }

    /// True when the status is "optimal".
    #[starlark(attribute)]
    fn ok<'v>(this: &KSolution) -> anyhow::Result<bool> {
        Ok(this.sol().status == Status::Optimal)
    }

    /// The objective at the solution's point; None when it has none.
    #[starlark(attribute)]
    fn objective<'v>(this: &KSolution, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(match this.sol().objective {
            Some(o) => heap.alloc(o),
            None => Value::new_none(),
        })
    }

    /// A MILP's relative gap between its point and its best bound (0 when
    /// proven optimal to within `gap=`); None for an LP.
    #[starlark(attribute)]
    fn gap<'v>(this: &KSolution, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(match this.sol().mip_gap {
            Some(g) => heap.alloc(g),
            None => Value::new_none(),
        })
    }

    /// Branch-and-bound nodes the MILP search visited; 0 for an LP.
    #[starlark(attribute)]
    fn nodes<'v>(this: &KSolution) -> anyhow::Result<i64> {
        Ok(this.sol().nodes as i64)
    }

    /// A MILP's best bound on the objective: no point is better. Equal to
    /// the objective (within `gap=`) when optimal; what a limit's point is
    /// measured against when not. None for an LP.
    #[starlark(attribute)]
    fn bound<'v>(this: &KSolution, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(match this.sol().bound {
            Some(b) => heap.alloc(b),
            None => Value::new_none(),
        })
    }

    /// True when the solution has a point that satisfies every rule:
    /// optimal, or the best point a limit stopped at, so `value()` works.
    #[starlark(attribute)]
    fn feasible<'v>(this: &KSolution) -> anyhow::Result<bool> {
        Ok(this.sol().values.is_some())
    }

    /// The value of a variable (an int for an integer variable) or of an
    /// expression (a float) at the solution. Raises when there is no point,
    /// saying why: for an infeasible problem, which rules conflict.
    fn value<'v>(this: &KSolution, #[starlark(require = pos)] x: Value<'v>, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        value_of(this, x, heap)
    }

    /// How far the named constraint is from its nearest bound at the
    /// solution: 0 for one that binds (within the solver's tolerance),
    /// never negative -- except for a rule this solve left out with
    /// `relax=`, which is measured against its own bounds and is negative by
    /// as much as the point breaks it.
    fn slack<'v>(this: &KSolution, #[starlark(require = pos)] name: &str) -> anyhow::Result<f64> {
        let r = this.row(name)?;
        let x = this.point()?;
        let (s, relaxed) = this.slack_of(r, x);
        Ok(if relaxed { s } else { s.max(0.0) })
    }

    /// The names of the constraints that bind at the solution (zero slack;
    /// an equality always does), in the order they were written. A rule the
    /// solve left out with `relax=` binds nothing.
    fn binding<'v>(this: &KSolution) -> anyhow::Result<Vec<String>> {
        let x = this.point()?;
        let rows = &this.snapshot.model.rows;
        Ok((0..rows.len())
            .filter(|&r| matches!(this.slack_of(r, x), (s, false) if s <= 0.0))
            .map(|r| rows[r].name.clone())
            .collect())
    }

    /// For an LP solved to optimality: how fast the objective changes as the
    /// named constraint's binding bound moves up (positive when raising it
    /// raises the objective; 0 when it does not bind). None for a MILP and
    /// when there is no optimum. A MILP's duals would be local to its
    /// arrangement and can mislead (on a floor-plan model one can be
    /// several times what a re-solve with the rule moved gives); to get them
    /// anyway, solve again with `fix=` every integer at its value, which is
    /// an LP.
    fn dual<'v>(this: &KSolution, #[starlark(require = pos)] name: &str, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let r = this.row(name)?;
        Ok(match &this.sol().row_duals {
            Some(d) => heap.alloc(d[r]),
            None => Value::new_none(),
        })
    }

    /// `dual()` for a variable's bounds: how fast the objective changes as
    /// its binding bound moves up. None for a MILP.
    fn reduced_cost<'v>(this: &KSolution, #[starlark(require = pos)] var: &KVar, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        if var.problem != this.problem {
            anyhow::bail!("{}: reduced_cost: {} belongs to another problem", this.feature, var.name);
        }
        Ok(match &this.sol().reduced_costs {
            Some(d) if (var.index as usize) < d.len() => heap.alloc(d[var.index as usize]),
            Some(_) => anyhow::bail!("{}: reduced_cost: {} was declared after this solve", this.feature, var.name),
            None => Value::new_none(),
        })
    }

    /// For an infeasible problem: an irreducible set of rules that cannot
    /// all hold, any one of which could go -- constraint names, then
    /// variable bounds as "x.min" and "x.max". Empty for any other status.
    /// For an LP, and a MILP whose LP relaxation is infeasible too, it is
    /// HiGHS's IIS, found with the solve. An integer variable whose bounds
    /// hold no whole number is a conflict by itself, `["x.min", "x.max"]`,
    /// also found with the solve. For a MILP whose relaxation is otherwise
    /// feasible -- integrality is part of the conflict, the usual case for a
    /// plan -- it is found by deletion, here and only when asked: whole
    /// families of rules (the `family` tag) are dropped while the rest stays
    /// infeasible, the untagged rules as one family more; then halves of
    /// each family that had to stay, down to single rules. Each probe is a
    /// solve for any feasible point under the solve's own `time_limit=`,
    /// `gap=` and `max_nodes=`; rules only, never variable bounds.
    /// `max_probes=` and `time_limit=` (seconds for the whole search, which
    /// the evaluation's budget bounds as well) stop it early, and then the
    /// rules it returns cannot all hold but some may not be needed, which an
    /// info says. `families_only=True` stops after the families: every rule
    /// of the families that cannot go. Memoised, unless the clock stopped it.
    fn conflict<'v>(
        this: &KSolution,
        #[starlark(require = named)] max_probes: Option<i32>,
        #[starlark(require = named)] time_limit: Option<Value<'v>>,
        #[starlark(require = named, default = false)] families_only: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<String>> {
        let what = format!("{}: conflict", this.feature);
        if let Some(n) = max_probes
            && n < 1
        {
            anyhow::bail!("{what}: max_probes= is a number of probes of 1 or more, got {n}");
        }
        let time_limit = optional_bound(time_limit, &format!("{what}: time_limit="))?;
        if let Some(t) = time_limit
            && t <= 0.0
        {
            anyhow::bail!("{what}: time_limit= is a positive number of seconds, got {}", fmt_num(t));
        }
        let names = conflict_names(this);
        if !names.is_empty() || this.sol().status != Status::Infeasible || !this.snapshot.model.is_mip() {
            return Ok(names);
        }
        let limits = DeletionLimits { max_probes: max_probes.map(|n| n as usize), time_limit, families_only };
        by_deletion(this, limits, eval)
    }
}

#[cfg(test)]
#[path = "opt_tests.rs"]
mod tests;
