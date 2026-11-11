//! Timing harness for the sketch solver.
//!
//! `bazel run --config=opt //src/rust/cadpole-sketch-solve:bench`
//!
//! The number that matters is the drag: a sketch UI calls `drag` on every
//! pointer move, so it has a frame's budget, and at 60 Hz that is 16 ms for
//! everything including redraw.

use cadpole_sketch_solve::{Constraint, PointId, System};
use std::time::Instant;

/// A ladder truss: `bays` square bays with a diagonal brace in each, every
/// member dimensioned. It is rigid, fully dimensioned, and grows linearly, so
/// it is a fair stand-in for a real sketch of the same size.
fn truss(bays: usize, pitch: f64, height: f64, jitter: f64) -> (System, Vec<PointId>) {
    let mut s = System::new();
    let mut bottom: Vec<PointId> = Vec::new();
    let mut top: Vec<PointId> = Vec::new();
    // A deterministic wobble, so the solver has real work to do.
    let wob = |i: usize, k: usize| ((i * 7 + k * 13) % 11) as f64 / 11.0 - 0.5;
    for i in 0..=bays {
        let x = i as f64 * pitch;
        bottom.push(s.point(x + jitter * wob(i, 0), jitter * wob(i, 1)));
        top.push(s.point(x + jitter * wob(i, 2), height + jitter * wob(i, 3)));
    }

    let base = s.line(bottom[0], bottom[1]);
    s.constrain(Constraint::fix(bottom[0])).unwrap();
    s.constrain(Constraint::horizontal(base)).unwrap();

    let diag = (pitch * pitch + height * height).sqrt();
    for i in 0..bays {
        s.constrain(Constraint::distance(bottom[i], bottom[i + 1], pitch)).unwrap();
        s.constrain(Constraint::distance(top[i], top[i + 1], pitch)).unwrap();
        s.constrain(Constraint::distance(bottom[i], top[i], height)).unwrap();
        s.constrain(Constraint::distance(bottom[i], top[i + 1], diag)).unwrap();
    }
    s.constrain(Constraint::distance(bottom[bays], top[bays], height)).unwrap();

    let mut all = bottom;
    all.extend(top);
    (s, all)
}

fn ms(t: std::time::Duration) -> f64 {
    t.as_secs_f64() * 1e3
}

fn report(name: &str, entities: usize, samples: &[f64]) {
    let mut v = samples.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = v[v.len() / 2];
    let worst = v[v.len() - 1];
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    println!(
        "{name:<28} entities {entities:>4}  median {median:>8.3} ms  mean {mean:>8.3} ms  \
         worst {worst:>8.3} ms  (n={})",
        v.len()
    );
}

fn main() {
    println!("cadpole-sketch-solve timings (build with --config=opt; a debug build is ~15x slower)\n");

    for &bays in &[4usize, 12, 24, 49] {
        let entities = 2 * (bays + 1) + 1;

        // Cold solve: from the wobbled starting geometry.
        let mut cold = Vec::new();
        for _ in 0..20 {
            let (mut s, _) = truss(bays, 40.0, 30.0, 6.0);
            let t = Instant::now();
            let r = s.solve();
            cold.push(ms(t.elapsed()));
            assert!(r.is_converged(), "{r}");
            assert_eq!(r.dof, 0, "{r}");
        }
        report("cold solve", entities, &cold);

        // Warm solve: already solved, nothing to do but confirm it.
        let (mut s, pts) = truss(bays, 40.0, 30.0, 6.0);
        assert!(s.solve().is_converged());
        let mut warm = Vec::new();
        for _ in 0..50 {
            let t = Instant::now();
            s.solve();
            warm.push(ms(t.elapsed()));
        }
        report("warm solve", entities, &warm);

        // Drag: what the UI calls on every pointer move.
        let handle = pts[pts.len() / 2];
        let start = s.point_pos(handle);
        let mut drag = Vec::new();
        for i in 0..60 {
            let d = (i % 20) as f64 * 0.25;
            let t = Instant::now();
            s.drag(handle, (start.0 + d, start.1 + d * 0.5));
            drag.push(ms(t.elapsed()));
        }
        report("drag", entities, &drag);

        // Diagnose: DOF, redundancy and conflict, which a UI wants after every
        // edit rather than every frame.
        let mut diag = Vec::new();
        for _ in 0..20 {
            let t = Instant::now();
            let d = s.diagnose();
            diag.push(ms(t.elapsed()));
            assert_eq!(d.dof, 0);
        }
        report("diagnose", entities, &diag);
        println!();
    }
}
