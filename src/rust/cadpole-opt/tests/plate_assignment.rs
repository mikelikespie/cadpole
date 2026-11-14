//! The choice of plates as a mixed-integer program, against the planner's.
//!
//! `cadpole_print::plate` chooses plates by costing a handful of greedy
//! fills (`docs/plating.md`, "Which plate"). The same choice is a natural
//! MILP: assign pieces to plates, minimising plates and colour changes,
//! within each plate's area and filament slots. This test builds that
//! program, solves it with HiGHS, hands its assignment back to the planner
//! as `plate=` pins to learn whether it fits and what it really costs, and
//! compares. It prints a table (`--test_output=all`), and fails if the
//! planner's own choice costs more than the program's realised optimum by
//! more than [`TOLERATED`]: the measure of whether a solver would earn its
//! place in the planner.
//!
//! The program knows a piece only by its footprint's area, so it cannot
//! tell whether its plates can be nested; when the planner cannot honour
//! every pin the plate's usable area is cut and the program solved again.

use std::time::Instant;

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::opt::{Column, Model, Options as SolveOptions, Row, Sense, Status};
use cadpole_core::solid::fixtures::box_mesh;
use cadpole_core::tessellate::Mesh;
use cadpole_print::plate::{self, Item, Options, Plan, PlanBody};

fn translated(m: &Mesh, d: Vec3) -> Mesh {
    m.transformed(&Transform::translation(d))
}

/// How far behind the program's realised optimum the planner may fall.
const TOLERATED: f64 = 0.10;

/// One piece as the program sees it: footprint area in mm², and the
/// heights each of its filaments spans.
struct Brief {
    area: f64,
    spans: Vec<(usize, f64, f64)>,
}

fn briefs(plan: &Plan, colours: &mut Vec<String>) -> Vec<Brief> {
    let mut out: Vec<Option<Brief>> = (0..plan.pieces.len()).map(|_| None).collect();
    for pl in &plan.placements {
        let piece = &plan.pieces[pl.piece];
        let spans = pl
            .spans
            .iter()
            .map(|&(c, z0, z1)| {
                let hex = &piece.colours[c];
                let k = colours.iter().position(|h| h == hex).unwrap_or_else(|| {
                    colours.push(hex.clone());
                    colours.len() - 1
                });
                (k, z0, z1)
            })
            .collect();
        out[pl.piece] = Some(Brief { area: pl.area, spans });
    }
    out.into_iter().map(|b| b.expect("every piece placed")).collect()
}

/// What the planner minimises by default.
fn cost(plates: usize, changes: usize, opts: &Options) -> f64 {
    plates as f64 * opts.plate_seconds + changes as f64 * opts.change_seconds
}

fn plan_cost(plan: &Plan, opts: &Options) -> f64 {
    cost(plan.plates.iter().filter(|p| !p.placements.is_empty()).count(), plan.plates.iter().map(|p| p.colour_changes).sum(), opts)
}

/// Solve the assignment: the plate (0-based) of each piece, and the
/// program's objective.
fn assign(pieces: &[Brief], colours: usize, plates: usize, usable: f64, opts: &Options) -> (Vec<usize>, f64) {
    let lh = opts.profile.layer_height;
    let plate_area = (opts.region[2] - opts.region[0]) * (opts.region[3] - opts.region[1]);
    let tower = opts.tower.map(|s| s * s).unwrap_or(0.0);
    let mut cuts: Vec<f64> = pieces.iter().flat_map(|p| p.spans.iter().flat_map(|s| [s.1, s.2])).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let bands: Vec<(f64, f64)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
    let mut m = Model::new(Sense::Minimize);
    let col = |m: &mut Model, name: String, cost: f64, integer: bool, upper: f64| -> u32 {
        m.columns.push(Column { name, lower: 0.0, upper, integer, cost });
        m.columns.len() as u32 - 1
    };
    let n = pieces.len();
    let x: Vec<Vec<u32>> = (0..n).map(|i| (0..plates).map(|p| col(&mut m, format!("x{i}_{p}"), 0.0, true, 1.0)).collect()).collect();
    let on: Vec<Vec<u32>> = (0..colours).map(|k| (0..plates).map(|p| col(&mut m, format!("c{k}_{p}"), 0.0, true, 1.0)).collect()).collect();
    let used: Vec<u32> = (0..plates).map(|p| col(&mut m, format!("u{p}"), opts.plate_seconds, true, 1.0)).collect();
    let mixed: Vec<u32> = (0..plates).map(|p| col(&mut m, format!("m{p}"), 0.0, true, 1.0)).collect();
    let row = |name: String, lower: f64, upper: f64| Row { name, lower, upper };
    let inf = f64::INFINITY;
    for i in 0..n {
        m.push_row(row(format!("once{i}"), 1.0, 1.0), &x[i].iter().map(|&c| (c, 1.0)).collect::<Vec<_>>());
        for p in 0..plates {
            let mut mine: Vec<usize> = pieces[i].spans.iter().map(|s| s.0).collect();
            mine.sort_unstable();
            mine.dedup();
            for k in mine {
                m.push_row(row(format!("has{i}_{k}_{p}"), -inf, 0.0), &[(x[i][p], 1.0), (on[k][p], -1.0)]);
            }
            m.push_row(row(format!("use{i}_{p}"), -inf, 0.0), &[(x[i][p], 1.0), (used[p], -1.0)]);
        }
    }
    for p in 0..plates {
        let all: Vec<(u32, f64)> = (0..colours).map(|k| (on[k][p], 1.0)).collect();
        if opts.ams > 0 {
            m.push_row(row(format!("slots{p}"), -inf, opts.ams as f64), &all);
        }
        let mut mix = all.clone();
        mix.push((mixed[p], -(colours as f64)));
        m.push_row(row(format!("mixed{p}"), -inf, 1.0), &mix);
        let mut area: Vec<(u32, f64)> = (0..n).map(|i| (x[i][p], pieces[i].area)).collect();
        area.push((mixed[p], tower));
        m.push_row(row(format!("area{p}"), -inf, usable * plate_area), &area);
        if p + 1 < plates {
            m.push_row(row(format!("order{p}"), 0.0, inf), &[(used[p], 1.0), (used[p + 1], -1.0)]);
        }
        for (b, (z0, z1)) in bands.iter().enumerate() {
            let layers = ((z1 - z0) / lh).round();
            if layers < 1.0 {
                continue;
            }
            let mid = (z0 + z1) / 2.0;
            let mut present: Vec<(u32, f64)> = Vec::new();
            for k in 0..colours {
                let here: Vec<usize> = (0..n).filter(|&i| pieces[i].spans.iter().any(|s| s.0 == k && s.1 <= mid && mid <= s.2)).collect();
                if here.is_empty() {
                    continue;
                }
                let y = col(&mut m, format!("y{k}_{p}_{b}"), 0.0, false, 1.0);
                for i in here {
                    m.push_row(row(format!("in{i}_{k}_{p}_{b}"), -inf, 0.0), &[(x[i][p], 1.0), (y, -1.0)]);
                }
                present.push((y, 1.0));
            }
            if present.len() > 1 {
                let e = col(&mut m, format!("e{p}_{b}"), opts.change_seconds * layers, false, inf);
                present.push((e, -1.0));
                m.push_row(row(format!("chg{p}_{b}"), -inf, 1.0), &present);
            }
        }
    }
    let solve = SolveOptions { time_limit: Some(20.0), ..SolveOptions::default() };
    let s = cadpole_opt::solve(&m, &solve).expect("the program is well formed").remove(0);
    assert!(matches!(s.status, Status::Optimal | Status::TimeLimit), "{:?}: {}", s.status, s.message);
    let v = s.values.expect("a point");
    let plate_of = (0..n).map(|i| (0..plates).max_by(|&a, &b| v[x[i][a] as usize].total_cmp(&v[x[i][b] as usize])).unwrap()).collect();
    (plate_of, s.objective.unwrap_or(f64::NAN))
}

struct Case {
    name: &'static str,
    /// `(width, depth, height, colour)` of each block.
    blocks: Vec<(f64, f64, f64, &'static str)>,
}

fn cases() -> Vec<Case> {
    let (w, k, r) = ("#ffffff", "#000000", "#ff0000");
    let mut out = vec![
        Case { name: "two tall blocks, two filaments", blocks: vec![(40.0, 40.0, 40.0, w), (40.0, 40.0, 40.0, k)] },
        Case { name: "two thin panels, two filaments", blocks: vec![(40.0, 40.0, 0.4, w), (40.0, 40.0, 0.4, k)] },
        Case {
            name: "a tall part and short trim",
            blocks: vec![(60.0, 60.0, 60.0, w), (30.0, 30.0, 4.0, w), (30.0, 30.0, 4.0, k), (30.0, 30.0, 4.0, k), (20.0, 20.0, 2.0, r)],
        },
        Case {
            name: "twelve panels, three filaments",
            blocks: (0..12).map(|i| (120.0, 70.0, 0.6 + 0.2 * (i % 3) as f64, [w, k, r][i % 3])).collect(),
        },
        Case {
            name: "a kit: big shells, small clips",
            blocks: {
                let mut b = vec![(150.0, 120.0, 30.0, w), (150.0, 120.0, 30.0, w), (140.0, 110.0, 12.0, k), (100.0, 100.0, 50.0, r)];
                b.extend((0..10).map(|i| (25.0, 12.0, 6.0, [w, k, r][i % 3])));
                b
            },
        },
    ];
    // Random kits, from a fixed seed.
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: u64| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s % n
    };
    for name in ["random kit 1", "random kit 2", "random kit 3", "random kit 4", THIRTY] {
        let n = if name == THIRTY { 30 } else { 6 + next(10) as usize };
        let blocks = (0..n)
            .map(|_| (20.0 + next(130) as f64, 20.0 + next(130) as f64, [0.4, 2.0, 8.0, 20.0, 45.0][next(5) as usize], [w, k, r][next(3) as usize]))
            .collect();
        out.push(Case { name, blocks });
    }
    out
}

/// The largest case: thirty parts over five plates. Run on its own, since
/// unoptimised it takes minutes.
const THIRTY: &str = "random kit of thirty";

#[test]
fn the_planners_choice_of_plates_against_the_programs() {
    compare(&|c: &Case| c.name != THIRTY);
}

/// `bazel test --config=opt //src/rust/cadpole-opt:plate_assignment_test
/// --test_arg=--ignored --test_arg=--nocapture --test_output=all`
#[test]
#[ignore = "minutes when unoptimised; run with --config=opt"]
fn thirty_parts_against_the_program() {
    compare(&|c: &Case| c.name == THIRTY);
}

fn compare(which: &dyn Fn(&Case) -> bool) {
    println!("{:<34} {:>3} | {:>18} {:>8} | {:>18} {:>8} {:>6}", "case", "n", "planner (pl, chg)", "s", "program (pl, chg)", "s", "fits");
    let mut behind = Vec::new();
    for case in cases().into_iter().filter(|c| which(c)) {
        let meshes: Vec<Mesh> =
            case.blocks.iter().enumerate().map(|(i, b)| translated(&box_mesh(b.0, b.1, b.2), Vec3::new(500.0 * i as f64, 0.0, 0.0))).collect();
        let bodies: Vec<PlanBody<'_>> = meshes
            .iter()
            .zip(&case.blocks)
            .enumerate()
            .map(|(i, (m, b))| PlanBody { name: format!("p{i}"), mesh: m, topo: None, colours: vec![b.3.to_owned()], tri_colours: Vec::new() })
            .collect();
        let items: Vec<Item> = (0..bodies.len()).map(|i| Item { bodies: vec![i], ..Item::default() }).collect();
        let opts = Options::default();
        let t = Instant::now();
        let mine = plate::plan(&bodies, &items, &opts);
        let mine_s = t.elapsed().as_secs_f64();
        let mut colours = Vec::new();
        let brief = briefs(&mine, &mut colours);
        let plates_now = mine.plates.len();
        // The program, its assignment pinned and planned; less usable area
        // each time the pins cannot all be honoured.
        let t = Instant::now();
        let mut theirs: Option<Plan> = None;
        let mut fits = "no";
        for usable in [0.9, 0.75, 0.6, 0.45] {
            let (plate_of, _) = assign(&brief, colours.len(), plates_now + 1, usable, &opts);
            let pinned: Vec<Item> = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let mut it = it.clone();
                    it.pin.plate = Some(plate_of[i] as u32 + 1);
                    it
                })
                .collect();
            let p = plate::plan(&bodies, &pinned, &opts);
            let ok = p.unpinned.is_empty();
            theirs = Some(p);
            if ok {
                fits = if usable == 0.9 { "yes" } else { "cut" };
                break;
            }
        }
        let theirs_s = t.elapsed().as_secs_f64();
        let theirs = theirs.unwrap();
        let count = |p: &Plan| (p.plates.iter().filter(|q| !q.placements.is_empty()).count(), p.plates.iter().map(|q| q.colour_changes).sum::<usize>());
        let (a, b) = (count(&mine), count(&theirs));
        println!(
            "{:<34} {:>3} | {:>3} pl {:>5} chg {:>4.0}s {:>7.3} | {:>3} pl {:>5} chg {:>4.0}s {:>7.3} {:>6}",
            case.name,
            bodies.len(),
            a.0,
            a.1,
            plan_cost(&mine, &opts),
            mine_s,
            b.0,
            b.1,
            plan_cost(&theirs, &opts),
            theirs_s,
            fits
        );
        if fits != "no" && plan_cost(&mine, &opts) > plan_cost(&theirs, &opts) * (1.0 + TOLERATED) + 1e-9 {
            behind.push(format!("{}: {} against {}", case.name, plan_cost(&mine, &opts), plan_cost(&theirs, &opts)));
        }
    }
    assert!(behind.is_empty(), "the planner's plates cost more than the program's: {behind:?}");
}
