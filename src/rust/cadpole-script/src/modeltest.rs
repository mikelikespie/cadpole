//! Model tests: `test(name, fn, *, sweep=, monotone=)` in a script, and the
//! runner that executes them.
//!
//! The script is the model, so its tests live in the script too, next to the
//! geometry they are about. A test is a function that
//! runs *after* the whole script has built the model, with every body and
//! parameter still in scope, and fails by raising — `assert_*`, `fail()`, or
//! any script error. That is deliberately the same shape as a top-level
//! `assert_solid(body)`, with one difference: a top-level assertion guards
//! every evaluation and its failure is an error diagnostic, while a test
//! runs only when asked (`cadpole test`, the example gate) and its failure is a
//! result with a name. The viewer never pays for a test, and a test may be
//! expensive — a print check, a probe boolean — without slowing the loop.
//!
//! **Sweeps re-evaluate, they do not re-enter.** "For blades in 3, 5, 7, 12
//! the rotor is still a solid" needs the model rebuilt at each value, and
//! rebuilding it from inside a running evaluation would mean a second
//! `Store`, a second deadline and a second heap nested in the first — on
//! wasm, on the same stack. So a sweep is declarative: the script says which
//! values, and [`run_tests`] evaluates the document once per point with the
//! value as an override and runs that one test there, exactly as `cadpole
//! sweep` renders its cells. Points are independent evaluations, which is
//! what lets them run in parallel and what makes "the model failed to build
//! at blades=12" a failure *of that point* rather than of the whole run.
//! What a point cannot see is its neighbours, which is what `monotone=` is
//! for: the runner keeps the number each point returned and checks the
//! sequence afterwards.
//!
//! **Where the function goes.** A `def` at module level is a value on the
//! module's heap until the module is frozen, and the [`Store`] outlives that
//! heap, so the store cannot hold the function itself. `test()` instead
//! stores it back into the module under a name no script would write
//! (`cadpole:test:N`, not a valid identifier, so nothing can shadow it) and the
//! store keeps the name. For the entry file the runner reads the slot back
//! while the module is still live; for a loaded library the loader reads it
//! out of the frozen module the moment the library finishes, and the runner
//! gets a frozen value that any heap may use.

use std::collections::HashMap;

use cadpole_core::ParamValue;
use cadpole_core::context::{Severity, Span, fmt_num, fmt_vec};
use cadpole_core::time::Stopwatch;
use starlark::environment::FrozenModule;
use starlark::eval::Evaluator;
use starlark::values::dict::DictRef;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::{OwnedFrozenValue, UnpackValue, Value};

use crate::check::VecArg;
use crate::limits::Limits;
use crate::store::{Store, call_span, store};
use crate::{EvalInput, Evaluation, evaluate_full, span_of};

/// What an evaluation does with the tests a script declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TestMode {
    /// Record the declarations and run nothing: every ordinary evaluation.
    #[default]
    Skip,
    /// The base run of [`run_tests`]: every test without a `sweep=`, or the
    /// ones of them named in `only`.
    Unswept { only: Option<Vec<String>> },
    /// One sweep point: exactly these tests, whatever their sweep says. The
    /// runner has already applied the point's overrides.
    Exactly(Vec<String>),
}

impl TestMode {
    fn selects(&self, decl: &TestDecl) -> bool {
        match self {
            TestMode::Skip => false,
            TestMode::Unswept { only } => {
                decl.sweep.is_empty() && only.as_ref().is_none_or(|names| names.contains(&decl.name))
            }
            TestMode::Exactly(names) => names.contains(&decl.name),
        }
    }
}

/// The claim `monotone=` makes about the number a swept test returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Monotone {
    Increasing,
    Decreasing,
}

impl Monotone {
    /// Both claims, in documentation order. `parse` reads this and so does
    /// the `monotone` vocabulary, so the two are one list.
    pub const ALL: [Monotone; 2] = [Monotone::Increasing, Monotone::Decreasing];

    pub fn parse(s: &str) -> Option<Monotone> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Monotone::Increasing => "increasing",
            Monotone::Decreasing => "decreasing",
        }
    }

    /// Strict: "more blades, more volume" means *more*, and two equal
    /// volumes along a sweep are exactly the surprise a test is for.
    fn holds(self, prev: f64, next: f64) -> bool {
        match self {
            Monotone::Increasing => next > prev,
            Monotone::Decreasing => next < prev,
        }
    }
}

/// One `test(...)` call, as the script declared it.
#[derive(Clone, Debug)]
pub struct TestDecl {
    pub name: String,
    /// Parameter name to the values it takes, in declaration order; the
    /// first varies slowest, like nested `for` loops.
    pub sweep: Vec<(String, Vec<ParamValue>)>,
    pub monotone: Option<Monotone>,
    /// The `test(...)` call.
    pub span: Option<Span>,
}

/// One result: a test, or one sweep point of a swept test.
#[derive(Clone, Debug)]
pub struct TestOutcome {
    pub name: String,
    pub ok: bool,
    /// Why it failed; empty on a pass.
    pub message: String,
    /// Where it failed — the raising call — or the declaration.
    pub span: Option<Span>,
    /// The parameter values this was measured at. Empty for an unswept test.
    pub point: Vec<(String, ParamValue)>,
    /// What the function returned, when that was a number.
    pub value: Option<f64>,
    /// How long the evaluation this ran in took, for a sweep point; zero for
    /// a test that ran in the base evaluation, whose time is the document's.
    pub eval_ms: f64,
}

impl TestOutcome {
    fn failed(decl: &TestDecl, message: impl Into<String>) -> TestOutcome {
        TestOutcome {
            name: decl.name.clone(),
            ok: false,
            message: message.into(),
            span: decl.span.clone(),
            point: Vec::new(),
            value: None,
            eval_ms: 0.0,
        }
    }
}

/// `blades=5 wall=2`: how a sweep point is written wherever it is reported.
pub fn point_label(point: &[(String, ParamValue)]) -> String {
    point.iter().map(|(k, v)| format!("{k}={}", value_label(v))).collect::<Vec<_>>().join(" ")
}

pub(crate) fn value_label(v: &ParamValue) -> String {
    match v {
        ParamValue::Number(n) => fmt_num(*n),
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Text(s) => s.clone(),
        ParamValue::Vector(v) => fmt_vec(*v),
    }
}

/// A declaration as the store holds it: the public part, plus where the
/// function went (see the module comment).
pub struct Registered {
    pub decl: TestDecl,
    /// The module variable the function was stored under.
    pub slot: String,
    /// The function, once the file that declared it has been frozen. `None`
    /// while that file is still being evaluated — which, for the entry file,
    /// is when its tests run and the slot is read from the live module.
    pub frozen: Option<OwnedFrozenValue>,
}

// ---------------------------------------------------------------------------
// Declaring
// ---------------------------------------------------------------------------

/// The `test(...)` builtin.
pub(crate) fn register<'v>(
    eval: &mut Evaluator<'v, '_, '_>,
    name: &str,
    func: Value<'v>,
    sweep: Option<Value<'v>>,
    monotone: Option<&str>,
) -> anyhow::Result<()> {
    let span = call_span(eval);
    let st = store(eval)?;
    if st.in_test.get() {
        anyhow::bail!("test {name:?}: a test cannot declare another test");
    }
    if name.trim().is_empty() {
        anyhow::bail!("test: the name is empty");
    }
    if st.tests.borrow().iter().any(|r| r.decl.name == name) {
        anyhow::bail!("test {name:?} declared twice");
    }
    let sweep = match sweep {
        None => Vec::new(),
        Some(v) => sweep_of(name, v)?,
    };
    let monotone = match monotone {
        None => None,
        Some(m) => {
            let Some(m) = Monotone::parse(m) else {
                anyhow::bail!("test {name:?}: monotone= is \"increasing\" or \"decreasing\", got {m:?}");
            };
            // One axis, or "along the sweep" has no meaning.
            if sweep.len() != 1 {
                anyhow::bail!(
                    "test {name:?}: monotone= needs a sweep over exactly one parameter, and this test sweeps {}",
                    sweep.len()
                );
            }
            Some(m)
        }
    };
    let decl = TestDecl { name: name.to_owned(), sweep, monotone, span };
    let slot = format!("cadpole:test:{}", st.tests.borrow().len());
    eval.module().set(&slot, func);
    st.tests.borrow_mut().push(Registered { decl, slot, frozen: None });
    Ok(())
}

/// `{"blades": [3, 5, 7], "wall": [1.6, 2.4]}` as declared sweep axes. The
/// values take the same forms a `param` default does, and a sweep is capped
/// at [`Limits::MAX_SWEEP_POINTS`] evaluations because every point is one.
fn sweep_of<'v>(name: &str, v: Value<'v>) -> anyhow::Result<Vec<(String, Vec<ParamValue>)>> {
    let Some(dict) = DictRef::from_value(v) else {
        anyhow::bail!("test {name:?}: sweep= is a dict of parameter name to a list of values, got {}", v.get_type());
    };
    let mut out = Vec::new();
    let mut points: usize = 1;
    for (k, values) in dict.iter() {
        let Some(param) = k.unpack_str() else {
            anyhow::bail!("test {name:?}: sweep= keys are parameter names, got {}", k.get_type());
        };
        let Some(list) = <UnpackListOrTuple<Value<'v>>>::unpack_value(values).map_err(|e| e.into_anyhow())? else {
            anyhow::bail!("test {name:?}: sweep= {param:?} needs a list of values, got {}", values.get_type());
        };
        if list.items.is_empty() {
            anyhow::bail!("test {name:?}: sweep= {param:?} has no values");
        }
        let vals = list
            .items
            .iter()
            .map(|item| param_value_of(*item).ok_or_else(|| {
                anyhow::anyhow!(
                    "test {name:?}: sweep= {param:?} values must be numbers, bools, strings or (x, y, z), got {}",
                    item.get_type()
                )
            }))
            .collect::<anyhow::Result<Vec<_>>>()?;
        points = points.saturating_mul(vals.len());
        if points > Limits::MAX_SWEEP_POINTS {
            anyhow::bail!(
                "test {name:?}: the sweep has more than {} points; every point is a whole evaluation, so split \
                 it into several tests",
                Limits::MAX_SWEEP_POINTS
            );
        }
        out.push((param.to_owned(), vals));
    }
    if out.is_empty() {
        anyhow::bail!("test {name:?}: sweep= is empty");
    }
    Ok(out)
}

pub(crate) fn param_value_of(v: Value<'_>) -> Option<ParamValue> {
    if let Some(b) = v.unpack_bool() {
        return Some(ParamValue::Bool(b));
    }
    if let Some(s) = v.unpack_str() {
        return Some(ParamValue::Text(s.to_owned()));
    }
    if let Ok(Some(n)) = UnpackFloat::unpack_value(v) {
        return Some(ParamValue::Number(n.0));
    }
    let vec: VecArg = VecArg::unpack_value(v).ok().flatten()?;
    if vec.items.len() != 3 {
        return None;
    }
    crate::check::vec3(&vec, "sweep value").ok().map(ParamValue::Vector)
}

// ---------------------------------------------------------------------------
// Running, inside an evaluation
// ---------------------------------------------------------------------------

/// Run the selected tests of `store` against the model the entry module has
/// just built. Called by the loader after the entry file's body, while its
/// module is still live. Every outcome goes on the store; nothing here is a
/// diagnostic, because a failed test is a result, not a broken model.
pub(crate) fn run_declared<'v>(eval: &mut Evaluator<'v, '_, '_>, store: &Store) {
    // Copy what the loop needs out of the registry first: a test body may
    // legitimately call builtins that read the store, and `test()` inside
    // one has to be able to look at the registry to refuse.
    let selected: Vec<(TestDecl, String, Option<OwnedFrozenValue>)> = store
        .tests
        .borrow()
        .iter()
        .filter(|r| store.test_mode.selects(&r.decl))
        .map(|r| (r.decl.clone(), r.slot.clone(), r.frozen.clone()))
        .collect();
    let mut expired: Option<String> = None;
    for (decl, slot, frozen) in selected {
        if let Some(msg) = &expired {
            store.outcomes.borrow_mut().push(TestOutcome::failed(&decl, msg.clone()));
            continue;
        }
        let func: Option<Value<'v>> = match &frozen {
            // A frozen value lives as long as its heap, which the loader's
            // cache holds until the whole evaluation is over — longer than
            // this module — so handing it to this module's evaluator is
            // sound; the borrow-tied `value()` cannot say so.
            Some(o) => Some(unsafe { o.unchecked_frozen_value() }.to_value()),
            None => eval.module().get(&slot),
        };
        let Some(func) = func else {
            store.outcomes.borrow_mut().push(TestOutcome::failed(&decl, "the test's function was not found"));
            continue;
        };
        let sw = Stopwatch::start();
        store.in_test.set(true);
        let result = eval.eval_function(func, &[], &[]);
        store.in_test.set(false);
        let outcome = match result {
            Ok(v) => TestOutcome {
                name: decl.name.clone(),
                ok: true,
                message: String::new(),
                span: decl.span.clone(),
                point: Vec::new(),
                value: UnpackFloat::unpack_value(v).ok().flatten().map(|n| n.0),
                eval_ms: sw.elapsed_ms(),
            },
            Err(e) => {
                // Past the deadline every remaining test would fail the same
                // way; say so once each rather than pretending to run them.
                let message = if store.deadline.fired() {
                    let m = store.deadline.message();
                    expired = Some(m.clone());
                    m
                } else {
                    // A `kind.cylnder` inside the test body reads the same
                    // as one outside it.
                    crate::vocab::missing_member_message(&e).unwrap_or_else(|| e.kind().to_string())
                };
                // Raised inside a library function: name the library line and
                // the call in the document, and point at the document.
                let span = span_of(&e);
                let (message, span) = match crate::stdlib::attribute(&e, &message, span.as_ref()) {
                    Some((m, s)) => (m, Some(s)),
                    None => (message, span),
                };
                TestOutcome {
                    name: decl.name.clone(),
                    ok: false,
                    message,
                    span: span.or_else(|| decl.span.clone()),
                    point: Vec::new(),
                    value: None,
                    eval_ms: sw.elapsed_ms(),
                }
            }
        };
        store.outcomes.borrow_mut().push(outcome);
    }
}

/// A library has just been frozen: the functions its `test()` calls stored
/// are now frozen values, which any later evaluator may call. `first` is the
/// registry length before the library ran; entries after it that still have
/// no frozen value are this file's own (a nested load's are already set).
pub(crate) fn adopt_frozen(store: &Store, first: usize, module: &FrozenModule) {
    for r in store.tests.borrow_mut().iter_mut().skip(first).filter(|r| r.frozen.is_none()) {
        r.frozen = module.get(&r.slot).ok();
    }
}

// ---------------------------------------------------------------------------
// The runner: sweeps and the report
// ---------------------------------------------------------------------------

/// What [`run_tests`] produced.
pub struct TestRun {
    /// The document at its declared defaults plus the caller's overrides:
    /// where the unswept tests ran, and whose diagnostics say whether the
    /// model built at all. An error there means nothing ran.
    pub base: Evaluation,
    /// One per unswept test, one per point of a swept test, plus one per
    /// failed `monotone=` claim, in declaration order.
    pub outcomes: Vec<TestOutcome>,
}

impl TestRun {
    pub fn ok(&self) -> bool {
        self.outcomes.iter().all(|o| o.ok)
    }
}

/// Run the tests of a document: the unswept ones in one evaluation, each
/// sweep point in an evaluation of its own with the point as an override,
/// then the `monotone=` claims over what the points returned. `names` limits
/// the run to those tests; empty means all of them. `threads` bounds how
/// many sweep points evaluate at once (0 = as many as the machine has, up to
/// 8; always 1 on wasm).
pub fn run_tests(input: EvalInput, names: &[String], threads: usize) -> TestRun {
    let only = if names.is_empty() { None } else { Some(names.to_vec()) };
    let base = evaluate_full(EvalInput { tests: TestMode::Unswept { only: only.clone() }, ..input.clone() });
    if base.ctx.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return TestRun { base, outcomes: Vec::new() };
    }
    let mut outcomes = base.outcomes.clone();
    for name in names {
        if !base.tests.iter().any(|t| &t.name == name) {
            let declared: Vec<&str> = base.tests.iter().map(|t| t.name.as_str()).collect();
            outcomes.push(TestOutcome {
                name: name.clone(),
                ok: false,
                message: format!(
                    "no test named {name:?}; the document declares {}",
                    if declared.is_empty() { "none".to_owned() } else { declared.join(", ") }
                ),
                span: None,
                point: Vec::new(),
                value: None,
                eval_ms: 0.0,
            });
        }
    }

    // Every sweep point of every swept test is one job; they are all
    // independent, so one pool runs the lot.
    let swept: Vec<&TestDecl> = base
        .tests
        .iter()
        .filter(|t| !t.sweep.is_empty() && only.as_ref().is_none_or(|n| n.contains(&t.name)))
        .collect();
    let mut jobs: Vec<(usize, Vec<(String, ParamValue)>)> = Vec::new();
    let mut refused: Vec<(usize, TestOutcome)> = Vec::new();
    for (i, decl) in swept.iter().enumerate() {
        match check_sweep(decl, &base.ctx) {
            Ok(()) => jobs.extend(points(&decl.sweep).into_iter().map(|p| (i, p))),
            Err(why) => refused.push((i, TestOutcome::failed(decl, why))),
        }
    }
    let results = run_points(&input, &swept, &jobs, threads);

    // Report per test, in declaration order, with the points in sweep order.
    for (i, decl) in swept.iter().enumerate() {
        if let Some((_, o)) = refused.iter().find(|(j, _)| *j == i) {
            outcomes.push(o.clone());
            continue;
        }
        let mine: Vec<&TestOutcome> = jobs.iter().zip(&results).filter(|((j, _), _)| *j == i).map(|(_, o)| o).collect();
        outcomes.extend(mine.iter().map(|o| (*o).clone()));
        if let Some(m) = decl.monotone
            && mine.iter().all(|o| o.ok)
        {
            if let Some(o) = check_monotone(decl, m, &mine) {
                outcomes.push(o);
            }
        }
    }
    TestRun { base, outcomes }
}

/// A sweep names parameters the document declares, with values of the
/// declared kind that the parameter would take as given. An override for an
/// unknown parameter is silently ignored by `param()` (there is nothing to
/// attach it to), one outside `choices` falls back to the default, and a
/// number outside `min`/`max` is clamped: each runs the model at some other
/// value than the point it is reported under, so a sweep like that would
/// pass for the wrong reason. The same rule `animation` keyframes follow.
fn check_sweep(decl: &TestDecl, ctx: &cadpole_core::Context) -> Result<(), String> {
    for (param, values) in &decl.sweep {
        let Some((d, _)) = ctx.params.iter().find(|(d, _)| &d.name == param) else {
            let known: Vec<&str> = ctx.params.iter().map(|(d, _)| d.name.as_str()).collect();
            return Err(format!(
                "sweep parameter {param:?} is not a param of this document (it declares {})",
                if known.is_empty() { "none".to_owned() } else { known.join(", ") }
            ));
        };
        if let Some(v) = values.iter().find(|v| v.kind_name() != d.default.kind_name()) {
            return Err(format!(
                "sweep parameter {param:?} is a {} but the sweep gives it a {} ({})",
                d.default.kind_name(),
                v.kind_name(),
                value_label(v)
            ));
        }
        for v in values {
            match v {
                ParamValue::Number(n) => {
                    if let Some(lo) = d.min.filter(|lo| n < lo) {
                        return Err(format!("sweep parameter {param:?}: {} is below its min {}", fmt_num(*n), fmt_num(lo)));
                    }
                    if let Some(hi) = d.max.filter(|hi| n > hi) {
                        return Err(format!("sweep parameter {param:?}: {} is above its max {}", fmt_num(*n), fmt_num(hi)));
                    }
                }
                ParamValue::Text(t) if !d.choices.is_empty() && !d.choices.contains(t) => {
                    return Err(format!("sweep parameter {param:?}: {t:?} is not one of its choices {:?}", d.choices));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The cartesian product of the axes, first axis slowest: nested `for`
/// loops in the order the script wrote them.
fn points(sweep: &[(String, Vec<ParamValue>)]) -> Vec<Vec<(String, ParamValue)>> {
    let mut out: Vec<Vec<(String, ParamValue)>> = vec![Vec::new()];
    for (name, values) in sweep {
        let mut next = Vec::with_capacity(out.len() * values.len());
        for prefix in &out {
            for v in values {
                let mut p = prefix.clone();
                p.push((name.clone(), v.clone()));
                next.push(p);
            }
        }
        out = next;
    }
    out
}

/// Evaluate every job — the document with the point applied, running that
/// one test — and hand back its outcome in job order.
fn run_points(
    input: &EvalInput,
    swept: &[&TestDecl],
    jobs: &[(usize, Vec<(String, ParamValue)>)],
    threads: usize,
) -> Vec<TestOutcome> {
    let run_one = |(i, point): &(usize, Vec<(String, ParamValue)>)| -> TestOutcome {
        let decl = swept[*i];
        let mut overrides: HashMap<String, ParamValue> = input.overrides.clone();
        overrides.extend(point.iter().cloned());
        let sw = Stopwatch::start();
        let ev = evaluate_full(EvalInput {
            overrides,
            tests: TestMode::Exactly(vec![decl.name.clone()]),
            ..input.clone()
        });
        let eval_ms = sw.elapsed_ms();
        let mut o = match ev.ctx.diagnostics.iter().find(|d| d.severity == Severity::Error) {
            // The model did not build at this point. That is what a sweep is
            // for finding, and the point is the whole message.
            Some(d) => TestOutcome {
                span: d.span.clone().or_else(|| decl.span.clone()),
                ..TestOutcome::failed(decl, format!("the model did not build: {}", d.message))
            },
            None => match ev.outcomes.into_iter().next() {
                Some(o) => o,
                None => TestOutcome::failed(decl, "the test did not run at this point"),
            },
        };
        o.point = point.clone();
        o.eval_ms = eval_ms;
        o
    };
    let threads = if cfg!(target_arch = "wasm32") {
        1
    } else if threads == 0 {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
    } else {
        threads
    }
    .clamp(1, jobs.len().max(1));
    if threads == 1 {
        return jobs.iter().map(run_one).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::Mutex<Option<TestOutcome>>> = jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= jobs.len() {
                        break;
                    }
                    let o = run_one(&jobs[i]);
                    *out[i].lock().unwrap() = Some(o);
                }
            });
        }
    });
    out.into_iter().map(|m| m.into_inner().unwrap().expect("every job ran")).collect()
}

/// The `monotone=` verdict over one swept test's points, all of which
/// passed. `None` when the claim holds.
fn check_monotone(decl: &TestDecl, m: Monotone, points: &[&TestOutcome]) -> Option<TestOutcome> {
    let mut prev: Option<&TestOutcome> = None;
    for o in points {
        let Some(v) = o.value else {
            return Some(TestOutcome {
                point: o.point.clone(),
                ..TestOutcome::failed(
                    decl,
                    format!(
                        "monotone=\"{}\" needs the test to return a number, and it returned none at {}",
                        m.as_str(),
                        point_label(&o.point)
                    ),
                )
            });
        };
        if let Some(p) = prev
            && !m.holds(p.value.unwrap_or(f64::NAN), v)
        {
            return Some(TestOutcome {
                point: o.point.clone(),
                value: Some(v),
                ..TestOutcome::failed(
                    decl,
                    format!(
                        "not {}: {} at {}, then {} at {}",
                        m.as_str(),
                        fmt_num(p.value.unwrap_or(f64::NAN)),
                        point_label(&p.point),
                        fmt_num(v),
                        point_label(&o.point)
                    ),
                )
            });
        }
        prev = Some(o);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScriptFile;

    fn run(src: &str) -> TestRun {
        run_tests(EvalInput { scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }], ..Default::default() }, &[], 1)
    }

    /// A sweep value the param would not take -- outside its choices, or
    /// outside its min/max where `param()` clamps -- runs the model at some
    /// other value, so a PASS under the requested label would be a pass for
    /// the wrong reason. The runner refuses it by name, as it refuses an
    /// undeclared sweep parameter, and as `animation` refuses such a
    /// keyframe.
    #[test]
    fn a_sweep_value_the_param_would_not_take_is_refused() {
        let head = "s = param(\"s\", \"a\", choices=[\"a\", \"b\"])\nn = param(\"n\", 4, min=2, max=10)\nb = box(n, 1, 1)\ndef t():\n    return n\n";
        let refused = |sweep: &str, want: &str| {
            let r = run(&format!("{head}test(\"t\", t, sweep={sweep})\n"));
            assert_eq!(r.outcomes.len(), 1, "{sweep}: one refusal, no points: {:?}", r.outcomes);
            assert!(!r.outcomes[0].ok, "{sweep}");
            assert!(r.outcomes[0].message.contains(want), "{sweep}: {}", r.outcomes[0].message);
        };
        refused("{\"s\": [\"a\", \"c\"]}", "sweep parameter \"s\": \"c\" is not one of its choices [\"a\", \"b\"]");
        refused("{\"n\": [2, 100]}", "sweep parameter \"n\": 100 is above its max 10");
        refused("{\"n\": [1, 4]}", "sweep parameter \"n\": 1 is below its min 2");
        // In range and among the choices, every point runs and passes.
        let r = run(&format!("{head}test(\"t\", t, sweep={{\"n\": [2, 10], \"s\": [\"a\", \"b\"]}})\n"));
        assert_eq!(r.outcomes.len(), 4, "{:?}", r.outcomes);
        assert!(r.outcomes.iter().all(|o| o.ok), "{:?}", r.outcomes);
    }
}
