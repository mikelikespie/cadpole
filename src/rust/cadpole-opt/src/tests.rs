//! The wrapper against problems whose answers are known by hand or by brute
//! force: an LP with its duals, a knapsack MILP, infeasible problems and
//! their conflicts, an unbounded one, k distinct solutions, and the same
//! solves on many threads at once.

use cadpole_core::opt::{Column, Conflict, Model, Options, Row, Sense, Side, Solution, Solver, Status};

use super::*;

const INF: f64 = f64::INFINITY;

fn col(name: &str, lower: f64, upper: f64, cost: f64) -> Column {
    Column { name: name.into(), lower, upper, integer: false, cost }
}

fn binary(name: &str, cost: f64) -> Column {
    Column { name: name.into(), lower: 0.0, upper: 1.0, integer: true, cost }
}

fn row(name: &str, lower: f64, upper: f64) -> Row {
    Row { name: name.into(), lower, upper }
}

fn one(model: &Model) -> Solution {
    let mut s = solve(model, &Options::default()).expect("solves");
    assert_eq!(s.len(), 1);
    s.pop().unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

/// max x + y  s.t.  x + 2y <= 4,  3x + y <= 6,  x, y >= 0:
/// the corner (1.6, 1.2), objective 2.8, row duals 0.4 and 0.2.
fn lp(sense: Sense) -> Model {
    let s = if sense == Sense::Maximize { 1.0 } else { -1.0 };
    let mut m = Model::new(sense);
    m.columns.push(col("x", 0.0, INF, s));
    m.columns.push(col("y", 0.0, INF, s));
    m.push_row(row("a", -INF, 4.0), &[(0, 1.0), (1, 2.0)]);
    m.push_row(row("b", -INF, 6.0), &[(0, 3.0), (1, 1.0)]);
    m
}

/// Ten, thirteen, seven and eight for weights three, four, two and three in
/// a sack of seven.
fn knapsack() -> Model {
    let mut m = Model::new(Sense::Maximize);
    for (name, v) in [("a", 10.0), ("b", 13.0), ("c", 7.0), ("d", 8.0)] {
        m.columns.push(binary(name, v));
    }
    m.push_row(row("weight", -INF, 7.0), &[(0, 3.0), (1, 4.0), (2, 2.0), (3, 3.0)]);
    m
}

/// Every feasible subset of the knapsack with its value, best first; ties
/// in the order HiGHS has no reason to prefer, so only values are compared.
fn knapsack_by_brute_force() -> Vec<f64> {
    let (v, w) = ([10.0, 13.0, 7.0, 8.0], [3.0, 4.0, 2.0, 3.0]);
    let mut values: Vec<f64> = (0u32..16)
        .filter(|s| (0..4).filter(|i| s & (1 << i) != 0).map(|i| w[i]).sum::<f64>() <= 7.0)
        .map(|s| (0..4).filter(|i| s & (1 << i) != 0).map(|i| v[i]).sum())
        .collect();
    values.sort_by(|a, b| b.partial_cmp(a).unwrap());
    values
}

#[test]
fn the_c_api_is_the_one_declared() {
    assert_eq!(Highs::version(), "1.15.1");
    assert_eq!(Highs::new().infinity(), f64::INFINITY);
    let s = HighsSolver::new();
    assert!(s.identity().starts_with("HiGHS 1.15.1 (threads=1"), "{}", s.identity());
}

#[test]
fn an_lp_finds_its_corner_with_duals_and_activities() {
    let s = one(&lp(Sense::Maximize));
    assert_eq!(s.status, Status::Optimal);
    let x = s.values.as_ref().unwrap();
    assert!(close(x[0], 1.6) && close(x[1], 1.2), "{x:?}");
    assert!(close(s.objective.unwrap(), 2.8));
    let act = s.activities.as_ref().unwrap();
    assert!(close(act[0], 4.0) && close(act[1], 6.0), "{act:?}");
    let d = s.row_duals.as_ref().unwrap();
    assert!(close(d[0], 0.4) && close(d[1], 0.2), "{d:?}");
    assert_eq!(s.nodes, 0);
    assert_eq!(s.mip_gap, None);
    assert!(s.conflict.is_none());
}

/// A dual is the objective's rate of change as the row's binding bound
/// moves up, in either sense, for an upper bound, a lower bound and an
/// equality; a reduced cost is the same for a column's bound. Checked
/// against finite differences of the solver's own answers.
#[test]
fn duals_are_derivatives_of_the_objective() {
    let h = 1e-4;
    for sense in [Sense::Maximize, Sense::Minimize] {
        let base = lp(sense);
        let s0 = one(&base);
        for r in 0..2 {
            let mut moved = base.clone();
            moved.rows[r].upper += h;
            let slope = (one(&moved).objective.unwrap() - s0.objective.unwrap()) / h;
            let dual = s0.row_duals.as_ref().unwrap()[r];
            assert!((slope - dual).abs() < 1e-6, "{sense:?} row {r}: slope {slope}, dual {dual}");
        }
    }
    // A binding lower bound and an equality: min x + y with x + y >= 2 and
    // x - y == 1 is (1.5, 0.5); raising the 2 costs 1 per unit, raising the
    // equality's 1 costs nothing (the sum is fixed by the other row).
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(col("x", -INF, INF, 1.0));
    m.columns.push(col("y", -INF, INF, 1.0));
    m.push_row(row("sum", 2.0, INF), &[(0, 1.0), (1, 1.0)]);
    m.push_row(row("diff", 1.0, 1.0), &[(0, 1.0), (1, -1.0)]);
    let s = one(&m);
    let d = s.row_duals.unwrap();
    assert!(close(d[0], 1.0) && close(d[1], 0.0), "{d:?}");
    // A column at its upper bound: max 2x + y with x <= 1 (a bound) and
    // x + y <= 3. Raising x's bound gains 2 - 1 = 1 per unit.
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(col("x", 0.0, 1.0, 2.0));
    m.columns.push(col("y", 0.0, INF, 1.0));
    m.push_row(row("cap", -INF, 3.0), &[(0, 1.0), (1, 1.0)]);
    let s = one(&m);
    assert!(close(s.objective.unwrap(), 4.0));
    let rc = s.reduced_costs.unwrap();
    assert!(close(rc[0], 1.0) && close(rc[1], 0.0), "{rc:?}");
}

#[test]
fn a_milp_rounds_its_integers_and_reports_its_search() {
    let s = one(&knapsack());
    assert_eq!(s.status, Status::Optimal);
    assert_eq!(s.values.as_deref(), Some(&[1.0, 1.0, 0.0, 0.0][..]));
    // Recomputed from the rounded point, so a whole number exactly.
    assert_eq!(s.objective, Some(23.0));
    assert_eq!(s.activities.as_deref(), Some(&[7.0][..]));
    assert!(s.mip_gap.unwrap() <= 1e-4);
    assert!(s.row_duals.is_none() && s.reduced_costs.is_none(), "a MILP has no duals");
}

#[test]
fn an_infeasible_lp_names_its_conflict_and_nothing_else() {
    // x + y >= 5 against x <= 1 and y <= 1, beside a row that is fine.
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(col("x", 0.0, INF, 1.0));
    m.columns.push(col("y", 0.0, INF, 1.0));
    m.columns.push(col("z", 0.0, INF, 1.0));
    m.push_row(row("innocent", -INF, 10.0), &[(2, 1.0)]);
    m.push_row(row("need", 5.0, INF), &[(0, 1.0), (1, 1.0)]);
    m.push_row(row("x cap", -INF, 1.0), &[(0, 1.0)]);
    m.push_row(row("y cap", -INF, 1.0), &[(1, 1.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible);
    assert!(s.values.is_none() && s.objective.is_none());
    let c = s.conflict.unwrap();
    assert_eq!(c.rows, vec![(1, Side::Lower), (2, Side::Upper), (3, Side::Upper)], "{c:?}");
    assert!(c.columns.is_empty(), "{c:?}");
}

#[test]
fn a_conflict_can_be_variable_bounds() {
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(col("x", 0.0, 1.0, 1.0));
    m.columns.push(col("y", 0.0, 1.0, 1.0));
    m.push_row(row("three", 3.0, INF), &[(0, 1.0), (1, 1.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible);
    let c = s.conflict.unwrap();
    assert_eq!(c, Conflict { rows: vec![(0, Side::Lower)], columns: vec![(0, Side::Upper), (1, Side::Upper)] });
}

#[test]
fn a_milp_infeasible_only_by_integrality_has_an_empty_conflict() {
    // 2x == 1 with x whole: the relaxation is fine, so no subset of the
    // linear rules conflicts on its own.
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(Column { name: "x".into(), lower: 0.0, upper: 5.0, integer: true, cost: 1.0 });
    m.push_row(row("half", 1.0, 1.0), &[(0, 2.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible);
    assert_eq!(s.conflict, Some(Conflict::default()));
}

#[test]
fn crossed_bounds_are_an_infeasible_answer_not_an_error() {
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(col("x", 0.0, INF, 1.0));
    m.push_row(row("crossed", 3.0, 2.0), &[(0, 1.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible);
    assert_eq!(s.conflict.unwrap().rows, vec![(0, Side::Both)]);
}

#[test]
fn an_unbounded_lp_says_so_with_no_point() {
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(col("x", 0.0, INF, 1.0));
    m.columns.push(col("y", 0.0, INF, 0.0));
    m.push_row(row("y small", -INF, 1.0), &[(1, 1.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Unbounded);
    assert!(s.values.is_none() && s.conflict.is_none());
}

#[test]
fn k_distinct_solutions_come_best_first() {
    let all = knapsack_by_brute_force();
    let sols = solve(&knapsack(), &Options { count: 4, ..Options::default() }).unwrap();
    assert_eq!(sols.len(), 4);
    let objectives: Vec<f64> = sols.iter().map(|s| s.objective.unwrap()).collect();
    assert_eq!(objectives, all[..4].to_vec(), "the four best values, in order");
    for (i, a) in sols.iter().enumerate() {
        assert_eq!(a.status, Status::Optimal);
        for b in &sols[..i] {
            assert_ne!(a.values, b.values, "solution {i} repeats an earlier one");
        }
        // The cuts are Cadpole's, not the model's: activities cover its rows only.
        assert_eq!(a.activities.as_ref().unwrap().len(), 1);
    }
    // Asking for more than exist stops when they run out.
    let every = solve(&knapsack(), &Options { count: 100, ..Options::default() }).unwrap();
    assert_eq!(every.len(), all.len());
}

#[test]
fn distinct_in_named_binaries_only() {
    // Differ in `a` alone: the second answer is the best with a flipped,
    // whatever the others do.
    let sols = solve(&knapsack(), &Options { count: 3, distinct: vec![0], ..Options::default() }).unwrap();
    assert_eq!(sols.len(), 2, "a has two values, so two solutions");
    let a: Vec<f64> = sols.iter().map(|s| s.values.as_ref().unwrap()[0]).collect();
    assert_eq!(a, vec![1.0, 0.0]);
    // b + c is 20 and b + d is 21: the best without a.
    assert_eq!(sols[1].objective, Some(21.0));
    let err = solve(&lp(Sense::Maximize), &Options { count: 2, ..Options::default() }).unwrap_err();
    assert!(err.contains("binary"), "{err}");
    let mut m = knapsack();
    m.columns[2].upper = 2.0;
    let err = solve(&m, &Options { count: 2, distinct: vec![2], ..Options::default() }).unwrap_err();
    assert!(err.contains("\"c\" is not a binary"), "{err}");
}

#[test]
fn a_model_with_no_variables_is_its_constant_rows() {
    let mut m = Model::new(Sense::Minimize);
    m.offset = 5.0;
    m.push_row(row("zero fits", -1.0, 1.0), &[]);
    let s = one(&m);
    assert_eq!((s.status, s.objective), (Status::Optimal, Some(5.0)));
    m.push_row(row("zero does not", 1.0, 2.0), &[]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible);
    assert_eq!(s.conflict.unwrap().rows, vec![(1, Side::Lower)]);
}

#[test]
fn no_value_is_negative_zero() {
    // min -x over x in [-1, 0] ends at x = 0, which a simplex is free to
    // write as -0.0.
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(col("x", -1.0, 0.0, -1.0));
    m.columns.push(Column { name: "n".into(), lower: -3.0, upper: 0.0, integer: true, cost: -1.0 });
    let s = one(&m);
    for v in s.values.unwrap().into_iter().chain(s.objective) {
        assert_ne!(v.to_bits(), (-0.0f64).to_bits());
    }
}

#[test]
fn bad_input_is_an_error_not_a_crash() {
    let mut m = lp(Sense::Maximize);
    m.columns[0].lower = f64::NAN;
    assert!(solve(&m, &Options::default()).unwrap_err().contains("not a number"));
    let m = lp(Sense::Maximize);
    assert!(solve(&m, &Options { count: 0, ..Options::default() }).is_err());
    assert!(solve(&m, &Options { mip_rel_gap: -1.0, ..Options::default() }).is_err());
    assert!(solve(&m, &Options { time_limit: Some(0.0), ..Options::default() }).is_err());
    assert!(solve(&m, &Options { deadline: Some(f64::NAN), ..Options::default() }).is_err());
}

// ------------------------------------------------------------------ threads

/// A strongly correlated knapsack (weights w, values w + 10, a sack of half
/// the total): hard enough that branch and bound has a real search to do,
/// built from a fixed linear congruential sequence so every run is the same.
fn hard_knapsack(n: usize, seed: u64) -> Model {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as f64 / (1u64 << 31) as f64
    };
    let mut m = Model::new(Sense::Maximize);
    let mut terms = Vec::new();
    let mut total = 0.0;
    for i in 0..n {
        let w = (1.0 + 99.0 * next()).floor();
        total += w;
        m.columns.push(binary(&format!("item {i}"), w + 10.0));
        terms.push((i as u32, w));
    }
    m.push_row(row("sack", -INF, (total / 2.0).floor()), &terms);
    m
}

/// Cornuéjols and Dawande's market split as an optimisation: `m` rows
/// `sum(a_ij x_j) + over_i - under_i == d_i`, `d_i` half the row's total,
/// minimising the miss. Notoriously hard for branch and bound -- with
/// `n = 10 (m - 1)` binaries a perfect split is rare and proving there is
/// none takes far more than a handful of nodes -- and always has a point.
fn market_split(rows: usize, n: usize, seed: u64) -> Model {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) % 100) as f64
    };
    let mut m = Model::new(Sense::Minimize);
    for j in 0..n {
        m.columns.push(Column { name: format!("x{j}"), lower: 0.0, upper: 1.0, integer: true, cost: 0.0 });
    }
    for i in 0..rows {
        m.columns.push(col(&format!("over{i}"), 0.0, INF, 1.0));
        m.columns.push(col(&format!("under{i}"), 0.0, INF, 1.0));
    }
    for i in 0..rows {
        let a: Vec<f64> = (0..n).map(|_| next()).collect();
        let d = (a.iter().sum::<f64>() / 2.0).floor();
        let mut terms: Vec<(u32, f64)> = Vec::with_capacity(n + 2);
        for (j, &v) in a.iter().enumerate() {
            if v != 0.0 {
                terms.push((j as u32, v));
            }
        }
        terms.push(((n + 2 * i) as u32, 1.0));
        terms.push(((n + 2 * i + 1) as u32, -1.0));
        m.push_row(row(&format!("market {i}"), d, d), &terms);
    }
    m
}

/// A transport LP: 30 sources, 30 sinks, costs from the same sequence.
fn transport(n: usize) -> Model {
    let mut m = Model::new(Sense::Minimize);
    for i in 0..n {
        for j in 0..n {
            m.columns.push(col(&format!("ship {i}->{j}"), 0.0, INF, (((i * 7 + j * 13) % 17) + 1) as f64));
        }
    }
    for i in 0..n {
        let terms: Vec<(u32, f64)> = (0..n).map(|j| ((i * n + j) as u32, 1.0)).collect();
        m.push_row(row(&format!("supply {i}"), -INF, 10.0 + (i % 5) as f64), &terms);
    }
    for j in 0..n {
        let terms: Vec<(u32, f64)> = (0..n).map(|i| ((i * n + j) as u32, 1.0)).collect();
        m.push_row(row(&format!("demand {j}"), 8.0 + (j % 3) as f64, INF), &terms);
    }
    m
}

fn bits(s: &[Solution]) -> Vec<(Status, Vec<u64>, Option<u64>, u64, u64)> {
    s.iter()
        .map(|s| {
            let v = s.values.as_ref().map(|v| v.iter().map(|x| x.to_bits()).collect()).unwrap_or_default();
            (s.status, v, s.objective.map(f64::to_bits), s.nodes, s.iterations)
        })
        .collect()
}

/// `cadpole serve` solves on whichever worker thread runs the evaluation, so
/// several solves can run at once in one process. HiGHS keeps its scheduler
/// per thread; this runs the same problems on eight threads side by side
/// and asks for the same bits, node counts and iteration counts as one
/// thread running them alone.
#[test]
fn solves_on_many_threads_agree_with_one() {
    let problems: Vec<(Model, Options)> = vec![
        (hard_knapsack(40, 1), Options::default()),
        (hard_knapsack(35, 2), Options { count: 3, ..Options::default() }),
        (transport(30), Options::default()),
        (knapsack(), Options { count: 5, ..Options::default() }),
    ];
    let alone: Vec<_> = problems.iter().map(|(m, o)| bits(&solve(m, o).unwrap())).collect();
    for (k, a) in alone.iter().enumerate() {
        assert!(!a.is_empty() && a[0].0 == Status::Optimal, "problem {k} solves alone");
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let problems = &problems;
                scope.spawn(move || {
                    let mut seen = Vec::new();
                    for round in 0..3 {
                        // Each thread walks the problems from a different
                        // place, so different solves overlap.
                        for k in 0..problems.len() {
                            let i = (k + t + round) % problems.len();
                            seen.push((i, bits(&solve(&problems[i].0, &problems[i].1).unwrap())));
                        }
                    }
                    seen
                })
            })
            .collect();
        for h in handles {
            for (i, b) in h.join().expect("no solver thread panics") {
                assert_eq!(b, alone[i], "problem {i} differs on a thread from alone");
            }
        }
    });
}

#[test]
fn the_installed_solver_is_highs() {
    install();
    install();
    let s = cadpole_core::opt::installed().expect("installed");
    assert!(s.identity().starts_with("HiGHS 1.15.1"));
    let sols = s.solve(&knapsack(), &Options::default()).unwrap();
    assert_eq!(sols[0].objective, Some(23.0));
}

#[test]
fn a_time_limit_stops_a_search_and_says_so() {
    // Big enough that one millisecond cannot be enough: the answer is a
    // limit status, never definitive, never mistaken for optimal.
    let m = hard_knapsack(400, 7);
    let s = solve(&m, &Options { time_limit: Some(1e-3), mip_rel_gap: 0.0, ..Options::default() }).unwrap();
    assert_eq!(s[0].status, Status::TimeLimit, "{:?}", s[0].status);
    assert!(!s[0].status.is_repeatable());
}

/// A node limit counts work, not time: the same limit stops the same search
/// at the same point with the same bound on every run, so its answer is
/// repeatable, and it comes back with the best point found and how far from
/// optimal that can be.
#[test]
fn a_node_limit_stops_the_same_search_every_time_with_its_best_point_and_bound() {
    let m = market_split(4, 30, 11);
    let opts = Options { max_nodes: Some(20), mip_rel_gap: 0.0, ..Options::default() };
    let a = solve(&m, &opts).unwrap();
    let b = solve(&m, &opts).unwrap();
    assert_eq!(a[0].status, Status::NodeLimit, "{:?}", a[0].status);
    assert!(a[0].status.is_repeatable());
    assert_eq!(bits(&a), bits(&b), "the same nodes, the same point");
    let (x, bound) = (a[0].objective.expect("an incumbent"), a[0].bound.expect("a bound"));
    assert!(bound <= x + 1e-9, "a minimisation's bound is below its incumbent: {bound} vs {x}");
    assert!(a[0].mip_gap.unwrap() > 0.0, "stopped short of proving it");
    assert!(a[0].nodes >= 20, "{}", a[0].nodes);
    // Solved to the end, the bound meets the objective.
    let done = solve(&knapsack(), &Options::default()).unwrap();
    assert!((done[0].bound.unwrap() - 23.0).abs() < 1e-6, "{:?}", done[0].bound);
}

/// A start is where the search begins: given a good point, a search the
/// node limit stops at once still has it, where one with no start may not.
#[test]
fn a_start_is_where_a_milp_search_begins() {
    let m = market_split(4, 30, 11);
    let long = solve(&m, &Options { max_nodes: Some(2000), mip_rel_gap: 0.0, ..Options::default() }).unwrap();
    let good = long[0].objective.unwrap();
    let start: Vec<(usize, f64)> =
        long[0].values.as_ref().unwrap().iter().enumerate().take(30).map(|(j, &v)| (j, v)).collect();
    let short = Options { max_nodes: Some(1), mip_rel_gap: 0.0, ..Options::default() };
    let begun = solve(&m, &Options { start: start.clone(), ..short.clone() }).unwrap();
    assert!(begun[0].objective.unwrap() <= good + 1e-9, "{:?} vs {good}", begun[0].objective);
    // The same start, the same answer.
    assert_eq!(bits(&begun), bits(&solve(&m, &Options { start, ..short }).unwrap()));
    let err = solve(&m, &Options { start: vec![(0, 2.0)], ..Options::default() }).unwrap_err();
    assert!(err.contains("outside its bounds"), "{err}");
}

/// Fixing every integer of a MILP leaves an LP: solved as one, it has duals
/// and a real conflict, the ones local to that arrangement.
#[test]
fn a_milp_with_its_integers_fixed_is_an_lp_with_duals() {
    let mut m = knapsack();
    for (c, v) in m.columns.iter_mut().zip([1.0, 1.0, 0.0, 0.0]) {
        c.lower = v;
        c.upper = v;
    }
    // A continuous slack in the sack, so the weight row has something to
    // price: half a unit of room left, so the point is not degenerate (with
    // the sack exactly full, any dual from 0.5 up would be a valid one, and
    // a fixed arrangement makes that common -- which is why such duals
    // are called local).
    m.columns.push(col("spare", 0.0, INF, 0.5));
    m.index.push(4);
    m.value.push(1.0);
    *m.start.last_mut().unwrap() += 1;
    m.rows[0].upper = 7.5;
    m.validate().unwrap();
    let s = one(&m);
    assert_eq!(s.status, Status::Optimal);
    assert_eq!(s.nodes, 0, "solved as an LP");
    assert_eq!(s.mip_gap, None);
    let d = s.row_duals.expect("an LP's duals");
    // The spare room is worth 0.5 a unit, so the sack's limit is too.
    assert!(close(d[0], 0.5), "{d:?}");
    // A fractional fix of an integer is not a constant: it stays a MILP, and
    // an infeasible one.
    let mut m = knapsack();
    m.columns[0].lower = 0.5;
    m.columns[0].upper = 0.5;
    assert_eq!(one(&m).status, Status::Infeasible);
}

/// A clock that reads 0 for its first `reads` readings and then a day: the
/// time limit runs out at a reading of the test's choosing.
fn runs_out_after(reads: u32) -> impl Fn() -> f64 {
    let n = std::cell::Cell::new(0u32);
    move || {
        n.set(n.get() + 1);
        if n.get() <= reads { 0.0 } else { 86_400.0 }
    }
}

/// `count=` cut short by the clock between two solves must say so: the list
/// ends with the limit's status rather than looking like every solution
/// there is, and so is never kept as a repeatable answer.
#[test]
fn a_count_the_clock_cuts_short_ends_with_the_limit_and_is_not_repeatable() {
    let opts = Options { count: 5, time_limit: Some(60.0), ..Options::default() };
    // The first solve reads the clock once, then the clock is spent.
    let sols = solve_on(&knapsack(), &opts, &runs_out_after(1)).unwrap();
    assert_eq!(sols.len(), 2, "{:?}", sols.iter().map(|s| s.status).collect::<Vec<_>>());
    assert_eq!(sols[0].status, Status::Optimal);
    assert_eq!(sols[1].status, Status::TimeLimit);
    assert!(sols[1].values.is_none(), "the limit found no next point");
    assert!(!sols.iter().all(|s| s.status.is_repeatable()), "a cut-short list is not repeatable");
    // Two solves in, the same: two points, then the limit.
    let sols = solve_on(&knapsack(), &opts, &runs_out_after(2)).unwrap();
    let statuses: Vec<Status> = sols.iter().map(|s| s.status).collect();
    assert_eq!(statuses, vec![Status::Optimal, Status::Optimal, Status::TimeLimit]);
    // A clock that never runs out still gives all five, all optimal.
    let sols = solve_on(&knapsack(), &opts, &|| 0.0).unwrap();
    assert_eq!(sols.len(), 5);
    assert!(sols.iter().all(|s| s.status == Status::Optimal));
}

fn integer(name: &str, lower: f64, upper: f64, cost: f64) -> Column {
    Column { name: name.into(), lower, upper, integer: true, cost }
}

/// HiGHS's MIP solver answers "infeasible or unbounded" for a MILP whose
/// relaxation is unbounded, whatever `allow_unbounded_or_infeasible` says;
/// the wrapper tells the two apart, as it does for an LP.
#[test]
fn an_unbounded_milp_says_so_and_an_infeasible_one_too() {
    // x whole, y not, x - y <= 3: (0, 0) is feasible and x + y grows for ever.
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(integer("x", 0.0, INF, 1.0));
    m.columns.push(col("y", 0.0, INF, 1.0));
    m.push_row(row("lead", -INF, 3.0), &[(0, 1.0), (1, -1.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Unbounded, "{:?}", s.status);
    assert!(s.values.is_none() && s.conflict.is_none());
    // One whole number and nothing else.
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(integer("x", 0.0, INF, 1.0));
    assert_eq!(one(&m).status, Status::Unbounded);
    // Unbounded relaxation, but 2x == 1 has no whole x: infeasible, not
    // unbounded, and it says what clashes as far as it can.
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(integer("x", 0.0, 5.0, 0.0));
    m.columns.push(col("y", 0.0, INF, 1.0));
    m.push_row(row("half", 1.0, 1.0), &[(0, 2.0)]);
    let s = one(&m);
    assert_eq!(s.status, Status::Infeasible, "{:?}", s.status);
    assert!(s.conflict.is_some());
}

/// Telling "infeasible or unbounded" apart takes a second solve; when the
/// clock stops that one, the answer depended on the time left, so it is the
/// time limit, not a repeatable "infeasible or unbounded" a cache keeps.
#[test]
fn an_unbounded_milp_the_clock_cuts_short_is_the_time_limit() {
    let mut m = Model::new(Sense::Maximize);
    m.columns.push(integer("x", 0.0, INF, 1.0));
    m.columns.push(col("y", 0.0, INF, 1.0));
    m.push_row(row("lead", -INF, 3.0), &[(0, 1.0), (1, -1.0)]);
    let opts = Options { time_limit: Some(60.0), ..Options::default() };
    let sols = solve_on(&m, &opts, &runs_out_after(1)).unwrap();
    assert_eq!(sols.len(), 1);
    assert!(!sols[0].status.is_repeatable(), "{:?}", sols[0].status);
    let sols = solve_on(&m, &opts, &|| 0.0).unwrap();
    assert_eq!(sols[0].status, Status::Unbounded);
}

/// An integer whose bounds hold no whole number is a conflict on its own:
/// its two bounds, which the solve names, where the LP relaxation (which
/// is feasible) has nothing to say.
#[test]
fn an_integer_with_no_whole_number_in_its_range_is_the_conflict() {
    for with_a_row in [false, true] {
        let mut m = Model::new(Sense::Minimize);
        m.columns.push(col("y", 0.0, INF, 1.0));
        m.columns.push(integer("k", 0.2, 0.8, 1.0));
        if with_a_row {
            m.push_row(row("cap", -INF, 5.0), &[(0, 1.0), (1, 1.0)]);
        }
        let s = one(&m);
        assert_eq!(s.status, Status::Infeasible);
        assert_eq!(s.conflict, Some(Conflict { rows: Vec::new(), columns: vec![(1, Side::Both)] }), "row: {with_a_row}");
    }
}
