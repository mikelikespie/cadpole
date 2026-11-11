//! The `opt` namespace through Starlark, against the CLI's solver: what a
//! script writes, what it reads back, what it is told when it writes
//! something a linear program cannot be, and the memo.

use std::sync::Arc;

use cadpole_core::context::Severity;
use cadpole_core::memo::Memo;

use crate::tests::{errors, run};
use crate::{EvalInput, ScriptFile, evaluate};

fn messages(ctx: &cadpole_core::Context, severity: Severity) -> Vec<String> {
    ctx.diagnostics.iter().filter(|d| d.severity == severity).map(|d| d.message.clone()).collect()
}

fn clean(src: &str) -> cadpole_core::Context {
    let ctx = run(src);
    assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    ctx
}

fn fails_with(src: &str, want: &str) {
    let ctx = run(src);
    let e = errors(&ctx);
    assert!(e.iter().any(|m| m.contains(want)), "want {want:?} in {e:?}");
}

const LP: &str = r#"
p = opt.problem(name="lp")
x = p.var("x", min=0)
y = p.var("y", min=0)
p.constrain(x + 2 * y, max=4, name="a")
p.constrain(3 * x + y, max=6, name="b")
p.constrain(x, max=10, name="loose")
p.maximize(x + y)
sol = p.solve()
"#;

#[test]
fn an_lp_solves_and_explains_itself() {
    clean(&format!(
        "{LP}
assert_eq(sol.status, \"optimal\")
assert_true(sol.ok)
assert_true(math.close(sol.objective, 2.8, tol=1e-9))
assert_true(math.close(sol.value(x), 1.6, tol=1e-9))
assert_true(math.close(sol.value(x + y), 2.8, tol=1e-9))
assert_eq(sol.binding(), [\"a\", \"b\"])
assert_eq(sol.slack(\"a\"), 0.0)
assert_true(math.close(sol.slack(\"loose\"), 8.4, tol=1e-9))
assert_true(math.close(sol.dual(\"a\"), 0.4, tol=1e-9))
assert_true(math.close(sol.dual(\"b\"), 0.2, tol=1e-9))
assert_eq(sol.dual(\"loose\"), 0.0)
assert_eq(sol.reduced_cost(x), 0.0)
assert_eq(sol.conflict(), [])
assert_eq(sol.gap, None)
assert_eq(sol.nodes, 0)
"
    ));
}

#[test]
fn expressions_are_linear_and_print_by_name() {
    clean(
        r#"
p = opt.problem()
x = p.var("x")
y = p.var("y")
assert_eq(str(3 + x), "x + 3")
assert_eq(str(x * 2 - y / 4 + 1), "2*x - 0.25*y + 1")
assert_eq(str(-x + 2), "-x + 2")
assert_eq(str(-(x - y)), "-x + y")
assert_eq(str(opt.sum([x, y, 1, 2 * x])), "x + y + 2*x + 1")
assert_eq(str(opt.dot([2, 3], [x, y])), "2*x + 3*y")
assert_eq(str(opt.sum([])), "0")
assert_true(x == x)
assert_true(x != y)
assert_eq({x: 1}[x], 1)
assert_eq((x.name, x.min, x.max, x.integer), ("x", None, None, False))
b = p.binary("b")
assert_eq((b.min, b.max, b.integer), (0.0, 1.0, True))
assert_eq(p.id, "optimize_1")
"#,
    );
    fails_with("p = opt.problem()\nx = p.var(\"x\")\ny = p.var(\"y\")\nx * y\n", "not linear");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\n(x + 1) * (x + 2)\n", "not linear");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\nx <= 3\n", "write p.constrain(expr, max=...)");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\nx / 0\n", "division of an expression by zero");
    // No reflected subtraction in starlark-rust: the language's own error,
    // which docs/buildings.md explains.
    fails_with("p = opt.problem()\nx = p.var(\"x\")\n2 - x\n", "-");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.constrain(x == 3)\n", "Comparisons cannot build rules");
    fails_with("p = opt.problem()\nq = opt.problem()\nx = p.var(\"x\")\ny = q.var(\"y\")\nx + y\n", "two different");
    fails_with(
        "p = opt.problem()\nq = opt.problem()\nx = p.var(\"x\")\nq.constrain(x, max=1)\n",
        "another opt.problem()",
    );
}

#[test]
fn a_bound_may_be_an_expression() {
    clean(
        r#"
p = opt.problem()
a = p.var("a", min=0, max=5)
b = p.var("b", min=0, max=3)
p.constrain(a, max=b + 1, name="a within b")
p.constrain(a + b, equals=2 * b + 1, name="tie")
p.maximize(a + b)
sol = p.solve()
# a <= b + 1 and a + b == 2b + 1, i.e. a == b + 1: the best is b = 3, a = 4.
assert_eq((sol.value(a), sol.value(b)), (4.0, 3.0))
assert_eq(sol.binding(), ["a within b", "tie"])
"#,
    );
    fails_with("p = opt.problem()\na = p.var(\"a\")\np.constrain(a, min=a, max=2)\n", "a range takes numbers");
    fails_with("p = opt.problem()\na = p.var(\"a\")\np.constrain(a)\n", "give max=, min=");
    fails_with("p = opt.problem()\na = p.var(\"a\")\np.constrain(a, min=1, equals=2)\n", "equals= stands alone");
}

#[test]
fn a_milp_returns_whole_numbers_and_distinct_solutions() {
    clean(
        r#"
p = opt.problem(name="sack")
items = [("a", 10, 3), ("b", 13, 4), ("c", 7, 2), ("d", 8, 3)]
take = [p.binary(n) for n, _, _ in items]
p.constrain(opt.dot([w for _, _, w in items], take), max=7, name="weight")
p.maximize(opt.dot([v for _, v, _ in items], take))
best = p.solve()
assert_eq(best.objective, 23.0)
assert_eq([best.value(t) for t in take], [1, 1, 0, 0])
assert_eq(type(best.value(take[0])), "int")
assert_eq(best.dual("weight"), None)
assert_true(best.gap <= 1e-4)
top = p.solve(count=3)
assert_eq([s.objective for s in top], [23.0, 21.0, 20.0])
# Differ in `a` only: the best with a flipped.
pair = p.solve(count=5, distinct=[take[0]])
assert_eq([s.value(take[0]) for s in pair], [1, 0])
"#,
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=4, integer=True)\np.solve(count=2, distinct=[x])\n",
        "not binary",
    );
}

#[test]
fn an_infeasible_problem_says_why_without_failing_the_solve() {
    let ctx = run(r#"
p = opt.problem(name="rooms")
w = p.var("w", min=0)
p.constrain(w, min=12, name="wide enough")
p.constrain(w, max=10, name="lot")
sol = p.solve()
assert_eq(sol.status, "infeasible")
assert_true(not sol.ok)
assert_eq(sol.conflict(), ["wide enough", "lot"])
assert_eq(sol.objective, None)
sol.value(w)
"#);
    let e = errors(&ctx);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(
        e[0].contains(
            "rooms: no solution to read: it is infeasible; these cannot all hold: \"wide enough\" (main.star:4), \"lot\" (main.star:5)"
        ),
        "{e:?}"
    );
    let info = messages(&ctx, Severity::Info);
    assert!(info.iter().any(|m| m.contains("rooms: infeasible; these cannot all hold")), "{info:?}");
    // Variable bounds take part as `name.min` / `name.max`.
    clean(
        r#"
p = opt.problem()
x = p.var("x", min=0, max=1)
y = p.var("y", min=0, max=1)
p.constrain(x + y, min=3, name="three")
assert_eq(p.solve().conflict(), ["three", "x.max", "y.max"])
"#,
    );
    // Unbounded is an answer too.
    clean("p = opt.problem()\nx = p.var(\"x\", min=0)\np.maximize(x)\nassert_eq(p.solve().status, \"unbounded\")\n");
}

#[test]
fn names_are_unique_and_a_problem_has_one_objective() {
    fails_with("p = opt.problem()\np.var(\"x\")\np.var(\"x\")\n", "variable \"x\" is already declared (main.star:2)");
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\")\np.constrain(x, max=1, name=\"r\")\np.constrain(x, min=0, name=\"r\")\n",
        "a constraint is already named \"r\" (main.star:3)",
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\")\np.maximize(x)\np.minimize(x)\n",
        "the objective is already set by maximize() (main.star:3)",
    );
    clean(
        r#"
p = opt.problem()
x = p.var("x")
assert_eq(p.constrain(x, max=1), "constraint_1")
assert_eq(p.constrain(x, min=0, name="constraint_2"), "constraint_2")
assert_eq(p.constrain(x, min=-1), "constraint_3")
"#,
    );
}

#[test]
fn a_solution_keeps_the_problem_it_solved() {
    clean(
        r#"
p = opt.problem()
x = p.var("x", min=0, max=10)
p.maximize(x)
first = p.solve()
p.constrain(x, max=4, name="later")
y = p.var("y", min=0, max=1)
second = p.solve()
assert_eq((first.value(x), second.value(x)), (10.0, 4.0))
assert_eq(first.binding(), [])
assert_eq(second.binding(), ["later"])
"#,
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=1)\ns = p.solve()\ny = p.var(\"y\")\ns.value(y)\n",
        "y was declared after this solve",
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=1)\ns = p.solve()\ns.slack(\"nope\")\n",
        "no constraint named \"nope\"",
    );
}

#[test]
fn the_feature_records_the_problem_and_its_answer() {
    let ctx = clean(LP);
    let f = ctx.features.iter().find(|f| f.id.0 == "lp").expect("the optimize feature");
    assert_eq!(f.kind, "optimize");
    assert_eq!(f.args.get("variables").map(String::as_str), Some("2"));
    assert_eq!(f.args.get("constraints").map(String::as_str), Some("3"));
    assert_eq!(f.args.get("nonzeros").map(String::as_str), Some("5"));
    assert_eq!(f.args.get("status").map(String::as_str), Some("optimal"));
    assert_eq!(f.args.get("objective").map(String::as_str), Some("2.8"));
    assert!(f.args.get("solver").is_some_and(|s| s.starts_with("HiGHS 1.15.1")), "{:?}", f.args);
}

/// The same problem in the next evaluation is a memo hit, with the
/// feature's args replayed; a changed coefficient is a new solve.
#[test]
fn a_solve_is_memoised_until_the_problem_changes() {
    cadpole_opt::install();
    let memo = Arc::new(Memo::new());
    let eval = |src: &str| {
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            memo: Some(memo.clone()),
            ..Default::default()
        })
    };
    let first = eval(LP);
    assert!(errors(&first).is_empty(), "{:?}", errors(&first));
    assert!(first.memo_counts.runs.iter().any(|r| r.op == "optimize" && r.feature == "lp"), "{:?}", first.memo_counts.runs);
    let second = eval(LP);
    assert!(second.memo_counts.hits.iter().any(|r| r.op == "optimize" && r.feature == "lp"), "{:?}", second.memo_counts.hits);
    assert!(!second.memo_counts.runs.iter().any(|r| r.op == "optimize"), "{:?}", second.memo_counts.runs);
    let f = second.features.iter().find(|f| f.id.0 == "lp").unwrap();
    assert_eq!(f.args.get("status").map(String::as_str), Some("optimal"), "args replayed on a hit");
    let changed = eval(&LP.replace("max=4", "max=4.5"));
    assert!(changed.memo_counts.runs.iter().any(|r| r.op == "optimize"), "{:?}", changed.memo_counts.runs);
}

#[test]
fn bad_arguments_say_what_they_take() {
    fails_with("p = opt.problem()\np.var(\"x\", min=\"0\")\n", "expected a number or None");
    fails_with("p = opt.problem()\np.solve(time_limit=0)\n", "positive number of seconds");
    fails_with("p = opt.problem()\np.solve(gap=-1)\n", "0 or more");
    fails_with("p = opt.problem()\np.solve(count=0)\n", "count= is from 1");
    fails_with("p = opt.problem()\np.var(\"\")\n", "needs a name");
    fails_with("opt.dot([1], [])\n", "1 coefficients for 0 expressions");
    fails_with("p = opt.problem()\np.solve(max_nodes=0)\n", "max_nodes= is a number of nodes of 1 or more");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.solve(fix={x: \"1\"})\n", "fix= gives x string, not a number");
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=3, integer=True)\np.solve(fix={x: 1.5})\n",
        "fix= gives x 1.5, but it takes whole numbers",
    );
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.solve(relax=[\"nope\"])\n", "relax= names \"nope\", which is no constraint");
}

/// What a floor-plan library needs: a solve's own fixes and
/// relaxations, which leave the problem as written; a probe without the
/// objective; and a MILP's conflict found by deletion where HiGHS's LP-only
/// IIS has nothing to say.
#[test]
fn a_solve_can_fix_relax_and_probe_without_touching_the_problem() {
    clean(
        r#"
p = opt.problem(name="room")
w = p.var("w", min=8, max=20)
d = p.var("d", min=8, max=20)
p.constrain(w + d, max=30, name="perimeter")
p.constrain(w, max=14, name="lot")
p.maximize(2 * w + d)
free = p.solve()
assert_eq((free.value(w), free.value(d)), (14.0, 16.0))
held = p.solve(fix={w: 10})
assert_eq((held.value(w), held.value(d)), (10.0, 20.0))
loose = p.solve(relax=["lot"])
assert_eq(loose.value(w), 20.0)
# Left out of this solve, the lot is measured against its own 14: 6 over.
assert_eq(loose.slack("lot"), -6.0)
assert_eq(loose.binding(), ["perimeter"])
# None of that stayed: the next solve is the problem as written.
assert_eq(p.solve().value(w), 14.0)
# Any feasible point, for a probe: the objective is left out.
probe = p.solve(objective=False)
assert_true(probe.feasible)
assert_eq(probe.objective, 0.0)
"#,
    );
    // A MILP only integrality makes infeasible: HiGHS's IIS (of the LP
    // relaxation) is empty, and a deletion filter in Starlark names the rule.
    clean(
        r#"
p = opt.problem(name="halves")
a = p.binary("a")
b = p.binary("b")
c = p.binary("c")
p.constrain(2 * a + 2 * b, equals=1, name="half")
p.constrain(a + c, max=1, name="harmless")
p.maximize(a + b + c)
sol = p.solve()
assert_eq(sol.status, "infeasible")
# HiGHS's IIS is empty here (the LP relaxation is feasible), so conflict()
# finds the rule by deletion -- and a library can do the same with relax=.
assert_eq(sol.conflict(), ["half"])
def by_deletion(p, names):
    needed = []
    dropped = []
    for n in names:
        if p.solve(relax=dropped + [n], objective=False).status == "infeasible":
            dropped.append(n)
        else:
            needed.append(n)
    return needed
assert_eq(by_deletion(p, ["harmless", "half"]), ["half"])
"#,
    );
}

/// MILP duals: none by default, and the
/// local ones on request, by fixing the arrangement, which leaves an LP.
#[test]
fn a_milps_local_duals_come_from_fixing_its_integers() {
    clean(
        r#"
p = opt.problem(name="sack")
take = [p.binary(n) for n in ("a", "b", "c", "d")]
spare = p.var("spare", min=0)
p.constrain(opt.dot([3, 4, 2, 3], take) + spare, max=7.5, name="weight")
p.maximize(opt.dot([10, 13, 7, 8], take) + 0.5 * spare)
best = p.solve()
assert_eq(best.dual("weight"), None)
assert_eq([best.value(t) for t in take], [1, 1, 0, 0])
local = p.solve(fix={t: best.value(t) for t in take})
assert_eq(local.nodes, 0)
assert_eq(local.gap, None)
assert_true(math.close(local.dual("weight"), 0.5, tol=1e-9))
assert_true(math.close(local.objective, best.objective, tol=1e-9))
"#,
    );
}

/// A node limit returns the best point found, its gap and bound, and the
/// same answer every time -- so, unlike a time limit, it is memoised.
#[test]
fn a_node_limit_returns_its_incumbent_and_is_memoised() {
    cadpole_opt::install();
    // A market split (Cornuejols and Dawande) minimising its miss: rare to
    // split perfectly, hard to prove it cannot, and always a point to show.
    let src = r#"
p = opt.problem(name="split")
state = [11]
def draw():
    state[0] = (state[0] * 1103515245 + 12345) % 2147483648
    return (state[0] // 65536) % 100
x = [p.binary("x%d" % j) for j in range(30)]
miss = []
for i in range(4):
    a = [draw() for _ in range(30)]
    total = 0
    for v in a:
        total += v
    over = p.var("over%d" % i, min=0)
    under = p.var("under%d" % i, min=0)
    p.constrain(opt.dot(a, x) + over - under, equals=total // 2, name="market %d" % i)
    miss.append(over + under)
p.minimize(opt.sum(miss))
sol = p.solve(max_nodes=10, gap=0)
assert_eq(sol.status, "node_limit")
assert_true(sol.feasible)
assert_true(not sol.ok)
assert_true(sol.bound <= sol.objective)
assert_true(sol.gap > 0)
"#;
    let memo = Arc::new(Memo::new());
    let eval = || {
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            memo: Some(memo.clone()),
            ..Default::default()
        })
    };
    let first = eval();
    assert!(errors(&first).is_empty(), "{:?}", errors(&first));
    let info = messages(&first, Severity::Info);
    assert!(info.iter().any(|m| m.contains("split: stopped at the node limit with a point within")), "{info:?}");
    let second = eval();
    assert!(second.memo_counts.hits.iter().any(|r| r.op == "optimize"), "{:?}", second.memo_counts.hits);
}

/// Row tags: given, or read from a `family:floor:room:what` name;
/// selected by `p.constraints(tags=)` and dropped for one solve by `drop=`.
#[test]
fn rules_carry_tags_a_solve_can_drop_by() {
    clean(
        r#"
p = opt.problem(name="plan")
x = p.var("x", min=0, max=10)
p.constrain(x, min=1, name="area:L1:kitchen:at least")
p.constrain(x, max=8, name="apart:L1:kitchen:west")
p.constrain(x, max=6, name="apart:L2:bath:west")
p.constrain(x, max=9, name="door one", tags={"family": "door", "room": "hall"})
p.constrain(x, max=7, name="a:b", tags={})
p.constrain(x, max=5, name="two:parts")
p.constrain(x, max=4, name="gap::stair:landing")
p.maximize(x)
assert_eq(p.constraints(tags={"family": "apart"}), ["apart:L1:kitchen:west", "apart:L2:bath:west"])
assert_eq(p.constraints(tags={"family": "apart", "floor": "L2"}), ["apart:L2:bath:west"])
assert_eq(p.constraints(tags={"room": ["kitchen", "hall"]}), ["area:L1:kitchen:at least", "apart:L1:kitchen:west", "door one"])
assert_eq(p.constraints(tags=[{"family": "door"}, {"room": "bath"}]), ["apart:L2:bath:west", "door one"])
assert_eq(p.constraints(tags={"floor": "L1", "room": "stair"}), [])
assert_eq(p.constraints(tags={"room": "stair"}), ["gap::stair:landing"])
assert_eq(len(p.constraints()), 7)
# The tightest rule is the gap's 4; drop its family and the parts' 5 binds;
# drop by room, two families at once.
assert_eq(p.solve().value(x), 4.0)
assert_eq(p.solve(drop={"family": "gap"}).value(x), 5.0)
assert_eq(p.solve(drop={"family": ["gap", "apart"]}, relax=["two:parts", "a:b"]).value(x), 9.0)
assert_eq(p.solve(drop=[{"room": "stair"}, {"family": "door"}], relax=["two:parts", "a:b"]).value(x), 6.0)
# None of that stayed.
assert_eq(p.solve().value(x), 4.0)
"#,
    );
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.constraints(tags={\"family\": 3})\n", "tag \"family\" takes a string or a list");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.constrain(x, max=1, tags={\"\": \"a\"})\n", "tags= has an empty tag");
    fails_with("p = opt.problem()\nx = p.var(\"x\")\np.solve(drop=3)\n", "a selector is a dict");
}

/// A MILP that only integrality makes infeasible: HiGHS's IIS (an LP's)
/// is empty, and `conflict()` finds the rules by deletion, a family at a
/// time and then a rule at a time.
#[test]
fn a_milps_conflict_is_found_by_deletion_family_first() {
    let ctx = run(r#"
p = opt.problem(name="stairs")
x = p.var("x", min=0, max=3, integer=True)
y = p.var("y", min=0, max=3, integer=True)
p.constrain(x + y, max=1.4, name="cap:L1:hall:width")
p.constrain(x + y, min=1.2, name="need:L1:hall:width")
p.constrain(x, max=3, name="room:L1:bed:x")
p.constrain(y, max=3, name="room:L1:bed:y")
p.constrain(x - y, max=2, name="cap:L1:bath:slope")
p.maximize(x + y)
sol = p.solve()
assert_eq(sol.status, "infeasible")
# Either rule alone holds; together, x + y in [1.2, 1.4] has no whole number.
assert_eq(sol.conflict(), ["cap:L1:hall:width", "need:L1:hall:width"])
# An LP keeps HiGHS's IIS.
q = opt.problem(name="lp")
w = q.var("w", min=0)
q.constrain(w, min=12, name="wide")
q.constrain(w, max=10, name="lot")
assert_eq(q.solve().conflict(), ["wide", "lot"])
sol.value(x)
"#);
    let e = errors(&ctx);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(
        e[0].contains("its LP relaxation is not: integrality is part of the conflict; call sol.conflict() for the rules that clash"),
        "{e:?}"
    );
    // The solve's info says the same, and neither ran the filter: the only
    // conflict search in the memo is the one the script asked for.
    let info = messages(&ctx, Severity::Info);
    assert!(info.iter().any(|m| m.contains("stairs: infeasible, while its LP relaxation is not") && m.contains("call sol.conflict()")), "{info:?}");
}

/// The trap: a filter that probes every untagged rule on its
/// own, and never looks at the clock. Untagged rules are one family now, a
/// kept family is searched by halves, and `max_probes=`, `time_limit=` and
/// `families_only=` bound it.
#[test]
fn a_conflict_search_is_bounded() {
    let ctx = run(r#"
p = opt.problem(name="stairs")
x = p.var("x", min=0, max=3, integer=True)
y = p.var("y", min=0, max=3, integer=True)
p.constrain(x + y, max=1.4, name="cap:L1:hall:width")
p.constrain(x + y, min=1.2, name="need:L1:hall:width")
p.constrain(x, max=3, name="room:L1:bed:x")
p.constrain(y, max=3, name="room:L1:bed:y")
p.constrain(x - y, max=2, name="cap:L1:bath:slope")
for i in range(400):
    p.constrain(x + y, max=10 + i, name="loose %d" % i)
p.maximize(x + y)
sol = p.solve()
# Four family probes (cap, need, room, the 400 untagged at once) and two
# for cap's halves.
assert_eq(sol.conflict(), ["cap:L1:hall:width", "need:L1:hall:width"])
# Every rule of the families that cannot go.
assert_eq(sol.conflict(families_only=True), ["cap:L1:hall:width", "need:L1:hall:width", "cap:L1:bath:slope"])
# Two probes kept cap and need, and asked nothing else: all 405 rules are
# still in, infeasible together and not irreducible.
assert_eq(len(sol.conflict(max_probes=2)), 405)
assert_eq(len(sol.conflict(max_probes=3)), 403)
assert_eq(sol.conflict(time_limit=60), ["cap:L1:hall:width", "need:L1:hall:width"])
"#);
    assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    let info = messages(&ctx, Severity::Info);
    assert!(
        info.iter().any(|m| m.contains("stairs: conflict: stopped at max_probes=2: these 405 rules cannot all hold, but the set may not be irreducible")),
        "{info:?}"
    );
    assert!(!info.iter().any(|m| m.contains("families_only") || m.contains("its time ran out")), "{info:?}");
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", max=1, integer=True)\np.constrain(x, min=0.5, max=0.6)\np.solve().conflict(max_probes=0)\n",
        "max_probes= is a number of probes of 1 or more, got 0",
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", max=1, integer=True)\np.solve().conflict(time_limit=0)\n",
        "time_limit= is a positive number of seconds, got 0",
    );
}

/// `start=`, a MIP start: whole or partial, checked against the bounds the
/// solve sees (a fix= included), and where the search begins.
#[test]
fn a_start_is_where_the_search_begins() {
    clean(
        r#"
p = opt.problem(name="sack")
take = [p.binary(n) for n in ("a", "b", "c", "d")]
p.constrain(opt.dot([3, 4, 2, 3], take), max=7, name="weight")
p.maximize(opt.dot([10, 13, 7, 8], take))
best = p.solve()
again = p.solve(start={t: best.value(t) for t in take})
assert_eq(again.objective, best.objective)
partial = p.solve(start={take[0]: 1})
assert_eq(partial.objective, 23.0)
"#,
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=3, integer=True)\np.solve(start={x: 5})\n",
        "start= gives x 5, outside its bounds [0, 3]",
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=3, integer=True)\np.solve(start={x: 1.5})\n",
        "start= gives x 1.5, but it takes whole numbers",
    );
    fails_with(
        "p = opt.problem()\nx = p.var(\"x\", min=0, max=3)\np.solve(fix={x: 1}, start={x: 2})\n",
        "start= gives x 2, outside its bounds [1, 1]",
    );
}

/// `count=` cut short between two solves -- by the clock, a node limit, an
/// error -- ends its list with that answer, and the script is told the list
/// may not be all there is. A list that ran out of solutions is not.
#[test]
fn a_count_cut_short_says_so() {
    use cadpole_core::opt::{Solution, Status};
    let point = |v: f64| {
        let mut s = Solution::without_point(Status::Optimal);
        s.values = Some(vec![v]);
        s
    };
    let cut = [point(1.0), point(0.0), Solution::without_point(Status::TimeLimit)];
    let m = crate::opt::cut_short("pick", 200, &cut).expect("a cut-short list says so");
    assert!(m.contains("count=200") && m.contains("time limit") && m.contains("after 2 solutions"), "{m}");
    assert!(m.contains("may be more"), "{m}");
    // A limit that left a point still cut the list short.
    let mut limited = point(2.0);
    limited.status = Status::NodeLimit;
    let m = crate::opt::cut_short("pick", 5, &[point(1.0), limited]).expect("says so");
    assert!(m.contains("node limit") && m.contains("after 2 solutions"), "{m}");
    // Every solution there is, or the one asked for: nothing to say.
    assert_eq!(crate::opt::cut_short("pick", 200, &[point(1.0), point(0.0)]), None);
    assert_eq!(crate::opt::cut_short("pick", 2, &[point(1.0), point(0.0)]), None);
    // A first solve the limit stopped has its own message.
    assert_eq!(crate::opt::cut_short("pick", 5, &[Solution::without_point(Status::TimeLimit)]), None);
}

/// An integer with no whole number between its bounds is named as the
/// conflict, rather than the solve sending the script to a `conflict()`
/// that comes back empty.
#[test]
fn an_integer_with_a_fractional_range_names_its_bounds() {
    let ctx = clean(
        r#"
p = opt.problem(name="frac")
k = p.var("k", min=0.2, max=0.8, integer=True)
y = p.var("y", min=0)
p.constrain(k + y, max=5, name="cap")
sol = p.solve()
assert_eq(sol.status, "infeasible")
assert_eq(sol.conflict(), ["k.min", "k.max"])
"#,
    );
    let info = messages(&ctx, Severity::Info);
    assert!(info.iter().any(|m| m.contains("frac: infeasible; these cannot all hold: \"k.min\", \"k.max\"")), "{info:?}");
    assert!(!info.iter().any(|m| m.contains("call sol.conflict()")), "{info:?}");
}

/// An unbounded MILP is reported unbounded, as an LP is.
#[test]
fn an_unbounded_milp_is_unbounded() {
    let ctx = clean(
        r#"
p = opt.problem(name="unb")
x = p.var("x", min=0, integer=True)
y = p.var("y", min=0)
p.constrain(x - y, max=3)
p.maximize(x + y)
assert_eq(p.solve().status, "unbounded")
"#,
    );
    let info = messages(&ctx, Severity::Info);
    assert!(info.iter().any(|m| m.contains("unb: unbounded")), "{info:?}");
}
