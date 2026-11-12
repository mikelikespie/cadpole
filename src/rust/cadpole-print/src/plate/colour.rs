//! Filaments per layer, and an estimate of the colour changes a plate takes.
//!
//! Each body on the plate contributes, per filament it prints in, the
//! height range its triangles of that filament span (its own colour, and
//! each paint). A layer's filaments are those whose ranges contain its
//! middle. A layer of *k* filaments costs *k − 1* changes, plus one when
//! none of them is the filament the previous layer ended on; each layer
//! starts on the previous layer's last filament when it can and ends on
//! one the next layer needs when it can. The slicer orders filaments itself
//! and a machine with several hotends swaps without purging, so this is a
//! proxy -- but it ranks layouts the right way round.

use super::{Options, Plan, PlanBody, Variant};

/// Fill in each plate's `layers` and `colour_changes`.
pub fn count(bodies: &[PlanBody<'_>], plan: &mut Plan, opts: &Options) {
    let lh = if opts.profile.layer_height > 0.0 { opts.profile.layer_height } else { 0.2 };
    for p in 0..plan.plates.len() {
        let plate = &plan.plates[p];
        let mut ranges: Vec<(usize, f64, f64)> = Vec::new(); // (colour index in plate, z0, z1)
        let mut top = 0.0f64;
        for &k in &plate.placements {
            let pl = &plan.placements[k];
            for &b in &plan.pieces[pl.piece].bodies {
                let body = &bodies[b];
                let m = body.mesh.transformed(&pl.transform);
                let mut span: Vec<(f64, f64)> = vec![(f64::INFINITY, f64::NEG_INFINITY); body.colours.len().max(1)];
                for t in 0..m.triangle_count() {
                    let c = body.tri_colours.get(t).copied().unwrap_or(0) as usize;
                    let c = c.min(span.len() - 1);
                    for j in 0..3 {
                        let z = m.positions[3 * m.indices[3 * t + j] as usize + 2] as f64;
                        span[c].0 = span[c].0.min(z);
                        span[c].1 = span[c].1.max(z);
                    }
                }
                for (c, (z0, z1)) in span.into_iter().enumerate() {
                    if z0 > z1 {
                        continue;
                    }
                    top = top.max(z1);
                    let idx = body.colours.get(c).and_then(|hex| plate.colours.iter().position(|h| h == hex)).unwrap_or(0);
                    ranges.push((idx, z0, z1));
                }
            }
        }
        let layers = (top / lh - 1e-6).ceil().max(0.0) as usize;
        let mono = plate.variant == Some(Variant::Mono) || plate.colours.len() <= 1;
        let changes = if mono { 0 } else { changes(&ranges, layers, lh, plate.colours.len()) };
        let plate = &mut plan.plates[p];
        plate.layers = layers;
        plate.colour_changes = changes;
    }
}

/// The estimate, from `(filament, z0, z1)` ranges over `layers` layers.
pub fn changes(ranges: &[(usize, f64, f64)], layers: usize, lh: f64, filaments: usize) -> usize {
    let sets: Vec<Vec<usize>> = (0..layers)
        .map(|l| {
            let z = (l as f64 + 0.5) * lh;
            let mut s: Vec<usize> = (0..filaments).filter(|&f| ranges.iter().any(|r| r.0 == f && r.1 <= z && z <= r.2)).collect();
            s.dedup();
            s
        })
        .collect();
    let mut total = 0;
    let mut last: Option<usize> = None;
    for (l, s) in sets.iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        total += s.len() - 1;
        let first = match last {
            Some(f) if s.contains(&f) => f,
            Some(_) => {
                total += 1;
                s[0]
            }
            None => s[0],
        };
        last = Some(if s.len() == 1 {
            first
        } else {
            let next = sets[l + 1..].iter().find(|n| !n.is_empty());
            s.iter()
                .copied()
                .filter(|&f| f != first)
                .find(|f| next.is_some_and(|n| n.contains(f)))
                .unwrap_or_else(|| s.iter().copied().find(|&f| f != first).unwrap_or(first))
        });
    }
    total
}
