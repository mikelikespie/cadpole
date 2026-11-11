//! Property test: randomly generated *well-constrained* systems must all
//! converge to zero residual with zero degrees of freedom.
//!
//! The generator is a Henneberg type-I construction, the standard way to build
//! a framework that is generically rigid in the plane (a Laman graph): start
//! from one edge, then repeatedly add a vertex joined to two existing ones.
//! Such a framework has exactly `2n − 3` independent distance constraints, so
//! pinning one point and one direction leaves zero degrees of freedom *by
//! construction* — the expected answer is known without solving anything,
//! which is what makes this a property test rather than a golden one.
//!
//! The distances are measured from a random target configuration, so a
//! solution certainly exists; the solver is then started from a perturbed
//! configuration and has to find its way back.

use super::{Rng, near};
use crate::{Constraint, PointId, System};

fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn len(a: (f64, f64)) -> f64 {
    (a.0 * a.0 + a.1 * a.1).sqrt()
}

/// Build a random rigid framework of `n` points, started `perturb` mm away
/// from the configuration its dimensions were measured on.
fn random_framework(seed: u64, n: usize, perturb: f64) -> (System, Vec<PointId>) {
    let mut rng = Rng::new(seed);
    // A target configuration. The first edge lies along +x so that pinning it
    // horizontal is consistent.
    let mut target: Vec<(f64, f64)> = vec![(0.0, 0.0), (rng.range(30.0, 60.0), 0.0)];
    // For each added vertex, the two it is tied to.
    let mut links: Vec<(usize, usize)> = Vec::new();

    while target.len() < n {
        let k = target.len();
        let c = (rng.range(-60.0, 60.0), rng.range(-60.0, 60.0));
        if target.iter().any(|q| len(sub(c, *q)) < 12.0) {
            continue;
        }
        // Tie it to the pair it makes the least degenerate triangle with: two
        // nearly parallel distance constraints would leave the vertex free to
        // slide, and the framework would not be rigid after all.
        let mut best = (0.0f64, (0usize, 1usize));
        for a in 0..k {
            for b in (a + 1)..k {
                let (u, v) = (sub(target[a], c), sub(target[b], c));
                let sine = (u.0 * v.1 - u.1 * v.0).abs() / (len(u) * len(v));
                if sine > best.0 {
                    best = (sine, (a, b));
                }
            }
        }
        if best.0 < 0.3 {
            continue;
        }
        target.push(c);
        links.push(best.1);
    }

    let mut s = System::new();
    let ids: Vec<PointId> = target
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // The first point starts on its target, so `fix` pins the frame
            // where the dimensions expect it; every other point starts wrong.
            if i == 0 {
                s.point(p.0, p.1)
            } else {
                s.point(p.0 + rng.range(-perturb, perturb), p.1 + rng.range(-perturb, perturb))
            }
        })
        .collect();

    let d = |a: usize, b: usize| len(sub(target[a], target[b]));

    // Pin the frame: one point, one direction, one length.
    let base = s.line(ids[0], ids[1]);
    s.constrain(Constraint::fix(ids[0])).unwrap();
    s.constrain(Constraint::horizontal(base)).unwrap();
    s.constrain(Constraint::distance(ids[0], ids[1], d(0, 1))).unwrap();

    for (k, &(a, b)) in links.iter().enumerate() {
        let k = k + 2;
        s.constrain(Constraint::distance(ids[a], ids[k], d(a, k))).unwrap();
        s.constrain(Constraint::distance(ids[b], ids[k], d(b, k))).unwrap();
    }
    (s, ids)
}

#[test]
fn random_rigid_frameworks_all_solve_to_zero_dof() {
    for seed in 0..60u64 {
        let n = 3 + (seed as usize % 8);
        let (mut s, _) = random_framework(seed, n, 4.0);

        // Both counts are known before solving: 2n parameters and
        // 2 + 1 + 1 + 2(n − 2) = 2n equations.
        assert_eq!(s.parameters().len(), 2 * n, "seed {seed}");
        assert_eq!(s.diagnose().equations, 2 * n, "seed {seed}");

        let r = s.solve();
        assert!(r.is_converged(), "seed {seed}, n {n}: {r}");
        assert_eq!(r.dof, 0, "seed {seed}, n {n}: {r}");
        // 1e-7 mm on sketches up to ~170 mm across: a relative error of 1e-9.
        assert!(r.max_residual < 1e-7, "seed {seed}: {r}");

        let diag = s.diagnose();
        assert!(diag.is_fully_constrained(), "seed {seed}: {diag}");
        assert!(diag.redundant.is_empty(), "seed {seed}: {diag}");
        assert!(diag.conflicting.is_empty(), "seed {seed}: {diag}");
        assert!(diag.free_entities.is_empty(), "seed {seed}: {diag}");

        // Every individual constraint really is met, not just the aggregate.
        for id in s.constraint_ids().collect::<Vec<_>>() {
            near(s.residual(id), 0.0, 1e-7, &format!("seed {seed} {}", s.label(id)));
        }
    }
}

/// The same frameworks started 25 mm off, which is twice the minimum spacing
/// between points: every one must converge, possibly onto a mirrored solution,
/// which is correct — the property is that they converge, not that they
/// reproduce the configuration the dimensions happened to be measured on.
#[test]
fn random_frameworks_converge_from_a_poor_guess() {
    for seed in 100..140u64 {
        let n = 4 + (seed as usize % 5);
        let (mut s, _) = random_framework(seed, n, 25.0);
        let r = s.solve();
        assert!(r.is_converged(), "seed {seed}: {r}");
        assert_eq!(r.dof, 0, "seed {seed}: {r}");
        assert!(r.max_residual < 1e-7, "seed {seed}: {r}");
    }
}

/// From *scrambled* geometry — every point displaced by up to 60 mm in a
/// sketch about 120 mm across, so the start bears no resemblance to the answer
/// — a distance system genuinely can have a local least-squares minimum that
/// no descent method escapes. What must never happen is a wrong answer: a
/// solve that reports success must really have succeeded, and one that fails
/// must say so.
#[test]
fn a_scrambled_start_either_converges_or_admits_it() {
    let mut converged = 0;
    for seed in 100..140u64 {
        let n = 4 + (seed as usize % 5);
        let (mut s, _) = random_framework(seed, n, 60.0);
        let r = s.solve();
        if r.is_converged() {
            assert_eq!(r.dof, 0, "seed {seed}: {r}");
            assert!(r.max_residual < 1e-7, "seed {seed}: {r}");
            converged += 1;
        } else {
            assert!(r.max_residual > 1e-7, "a failure must be a real one: {r}");
        }
    }
    assert!(converged >= 39, "only {converged} of 40 recovered from a scramble");
}

/// One more distance on an already rigid framework is always over-constrained,
/// and the constraint that was *added* is the one named.
#[test]
fn one_extra_distance_on_a_rigid_framework_is_always_named() {
    for seed in 200..215u64 {
        let n = 4 + (seed as usize % 5);
        let (mut s, ids) = random_framework(seed, n, 3.0);
        assert!(s.solve().is_converged(), "seed {seed}");

        // Measure something the framework already determines, then constrain
        // it to exactly that: redundant by construction.
        let (p, q) = (ids[0], ids[n - 1]);
        let d = len(sub(s.point_pos(p), s.point_pos(q)));
        s.constrain_named("extra", Constraint::distance(p, q, d)).unwrap();

        let r = s.solve();
        assert!(r.is_converged(), "seed {seed}: {r}");
        let diag = s.diagnose();
        assert_eq!(diag.dof, 0, "seed {seed}: {diag}");
        assert_eq!(diag.redundant.len(), 1, "seed {seed}: {diag}");
        assert_eq!(diag.redundant[0].label, "extra", "seed {seed}: {diag}");
        assert!(diag.conflicting.is_empty(), "seed {seed}: {diag}");
        assert!(!diag.redundant[0].implied_by.is_empty(), "seed {seed}: {diag}");
    }
}

/// The same, with a distance that disagrees: always a conflict, never a silent
/// failure, and the gap says roughly how big the disagreement is.
#[test]
fn one_wrong_distance_on_a_rigid_framework_is_always_a_conflict() {
    for seed in 300..315u64 {
        let n = 4 + (seed as usize % 5);
        let (mut s, ids) = random_framework(seed, n, 3.0);
        assert!(s.solve().is_converged(), "seed {seed}");
        let (p, q) = (ids[0], ids[n - 1]);
        let d = len(sub(s.point_pos(p), s.point_pos(q)));
        s.constrain_named("wrong", Constraint::distance(p, q, d + 7.0)).unwrap();

        let r = s.solve();
        assert!(!r.is_converged(), "seed {seed}: {r}");
        let diag = s.diagnose();
        assert_eq!(diag.conflicting.len(), 1, "seed {seed}: {diag}");
        assert_eq!(diag.conflicting[0].label, "wrong", "seed {seed}: {diag}");
        // The dependency is nonlinear, so the reported gap is the first-order
        // size of the contradiction rather than exactly 7 mm.
        let gap = diag.conflicting[0].gap.abs();
        assert!((2.0..20.0).contains(&gap), "seed {seed}: gap {gap}");
    }
}
