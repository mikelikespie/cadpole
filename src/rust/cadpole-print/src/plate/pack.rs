//! Packing pieces onto plates: raster bottom-left fill.
//!
//! A piece's footprint is its triangles projected onto the bed, rasterised
//! conservatively at `cell` mm and dilated by a disc of half the spacing; a
//! plate is a bitmap of where every nozzle reaches, its excluded corners
//! taken. A piece goes where its bitmap is clear with the lowest top edge,
//! then the leftmost right edge, over the spins tried (0°, 90°, 180°, 270°
//! and the two of the footprint's minimum-area rectangle). Rows are 64-bit
//! words, so a test is a few ANDs per row, and most positions fail on the
//! first row they test.
//!
//! The plates are tried in order of colour affinity -- fewest new
//! filaments, then most shared, then plate number -- and a plate never
//! takes a piece that would put it past `ams` filaments. A plate of more
//! than one filament keeps a free square for the prime tower at its back
//! right corner, the last place a bottom-left fill reaches.

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::Mesh;

use super::{Options, Piece, Placement, Plate, PlanBody, Variant};

/// A bitmap, rows of 64-bit words.
#[derive(Clone, Debug)]
pub struct Raster {
    pub w: usize,
    pub h: usize,
    words: usize,
    rows: Vec<u64>,
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        // One spare word a row, so a shifted footprint word never indexes
        // past the row.
        let words = w.div_ceil(64) + 1;
        Raster { w, h, words, rows: vec![0; words * h] }
    }

    fn row(&self, y: usize) -> &[u64] {
        &self.rows[y * self.words..(y + 1) * self.words]
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.w && y < self.h && self.rows[y * self.words + x / 64] >> (x % 64) & 1 == 1
    }

    /// Set `x0..=x1` on row `y`, clipped to the raster.
    pub fn set_range(&mut self, y: i64, x0: i64, x1: i64) {
        if y < 0 || y as usize >= self.h || x1 < 0 || x0 >= self.w as i64 {
            return;
        }
        let (x0, x1) = (x0.max(0) as usize, (x1 as usize).min(self.w - 1));
        let base = y as usize * self.words;
        for x in x0..=x1 {
            self.rows[base + x / 64] |= 1u64 << (x % 64);
        }
    }

    /// Cells set.
    pub fn count(&self) -> usize {
        self.rows.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Whether `fp` placed with its corner at `(cx, cy)` misses every set
    /// cell. The caller keeps `fp` inside.
    fn clear_at(&self, fp: &Raster, cx: usize, cy: usize) -> bool {
        let (base, shift) = (cx / 64, cx % 64);
        for r in 0..fp.h {
            let prow = self.row(cy + r);
            for (k, &w) in fp.row(r).iter().enumerate() {
                if w == 0 {
                    continue;
                }
                if prow[base + k] & (w << shift) != 0 {
                    return false;
                }
                if shift > 0 && base + k + 1 < self.words && prow[base + k + 1] & (w >> (64 - shift)) != 0 {
                    return false;
                }
            }
        }
        true
    }

    fn stamp(&mut self, fp: &Raster, cx: usize, cy: usize) {
        let (base, shift) = (cx / 64, cx % 64);
        for r in 0..fp.h {
            let words = self.words;
            for k in 0..fp.words {
                let w = fp.rows[r * fp.words + k];
                if w == 0 {
                    continue;
                }
                let at = (cy + r) * words + base + k;
                if base + k < words {
                    self.rows[at] |= w << shift;
                }
                if shift > 0 && base + k + 1 < words {
                    self.rows[at + 1] |= w >> (64 - shift);
                }
            }
        }
    }

    /// The first corner, lowest row first, then leftmost, where `fp` fits.
    fn first_fit(&self, fp: &Raster) -> Option<(usize, usize)> {
        if fp.w > self.w || fp.h > self.h {
            return None;
        }
        for cy in 0..=self.h - fp.h {
            for cx in 0..=self.w - fp.w {
                if self.clear_at(fp, cx, cy) {
                    return Some((cx, cy));
                }
            }
        }
        None
    }
}

/// The XY of every vertex.
pub fn plan_points(m: &Mesh) -> Vec<(f64, f64)> {
    m.positions.chunks_exact(3).map(|p| (p[0] as f64, p[1] as f64)).collect()
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
pub fn hull(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut p: Vec<(f64, f64)> = points.to_vec();
    p.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The minimum-area rectangle round `points`: the turn about z, in degrees
/// in `[0, 90)`, that lays it along the axes, and its two sides there.
pub fn min_rect(points: &[(f64, f64)]) -> (f64, f64, f64) {
    let h = hull(points);
    let extent = |a: f64| {
        let (s, c) = a.sin_cos();
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for &(x, y) in &h {
            let (u, v) = (x * c - y * s, x * s + y * c);
            x0 = x0.min(u);
            x1 = x1.max(u);
            y0 = y0.min(v);
            y1 = y1.max(v);
        }
        if x0 > x1 { (0.0, 0.0) } else { (x1 - x0, y1 - y0) }
    };
    let (w0, d0) = extent(0.0);
    let mut best = (0.0, w0, d0);
    for i in 0..h.len() {
        let (a, b) = (h[i], h[(i + 1) % h.len()]);
        let edge = (b.1 - a.1).atan2(b.0 - a.0);
        // The turn that lays this edge along x, folded into [0, 90).
        let mut turn = (-edge).to_degrees().rem_euclid(90.0);
        if turn > 90.0 - 1e-9 {
            turn = 0.0;
        }
        let (w, d) = extent(turn.to_radians());
        if w * d < best.1 * best.2 - 1e-6 {
            best = (turn, w, d);
        }
    }
    best
}

/// A piece's footprint at one spin: the raster, where its corner cell is
/// in model mm, and the core's extent `[x0, y0, x1, y1]` in mm.
#[derive(Clone, Debug)]
pub struct Footprint {
    pub raster: Raster,
    pub origin: (f64, f64),
    pub extent: [f64; 4],
    pub spin: f64,
}

/// The raster of `mesh` seen from above, dilated by `dilate` mm.
pub fn footprint(mesh: &Mesh, cell: f64, dilate: f64, spin: f64) -> Footprint {
    let Some((lo, hi)) = mesh.bbox() else {
        return Footprint { raster: Raster::new(1, 1), origin: (0.0, 0.0), extent: [0.0; 4], spin };
    };
    let pad = (dilate / cell).ceil() as i64;
    let origin = (lo.x - pad as f64 * cell, lo.y - pad as f64 * cell);
    let w = (((hi.x - lo.x) / cell).floor() as i64 + 1 + 2 * pad) as usize;
    let h = (((hi.y - lo.y) / cell).floor() as i64 + 1 + 2 * pad) as usize;
    let mut core = Raster::new(w, h);
    for t in 0..mesh.triangle_count() {
        let mut p = [(0.0f64, 0.0f64); 3];
        for (k, q) in p.iter_mut().enumerate() {
            let i = mesh.indices[3 * t + k] as usize;
            *q = (
                (mesh.positions[3 * i] as f64 - origin.0) / cell,
                (mesh.positions[3 * i + 1] as f64 - origin.1) / cell,
            );
        }
        fill_triangle(&mut core, &p);
    }
    let raster = if pad > 0 { dilated(&core, dilate / cell) } else { core };
    Footprint { raster, origin, extent: [lo.x, lo.y, hi.x, hi.y], spin }
}

/// Mark every cell the triangle covers any part of, row band by row band.
fn fill_triangle(r: &mut Raster, p: &[(f64, f64); 3]) {
    let ymin = p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    let ymax = p.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max);
    if !ymin.is_finite() {
        return;
    }
    for row in (ymin.floor() as i64)..=(ymax.floor() as i64) {
        let (b0, b1) = (row as f64, row as f64 + 1.0);
        let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
        for q in p {
            if q.1 >= b0 && q.1 <= b1 {
                x0 = x0.min(q.0);
                x1 = x1.max(q.0);
            }
        }
        for k in 0..3 {
            let (a, b) = (p[k], p[(k + 1) % 3]);
            if (a.1 - b.1).abs() < 1e-12 {
                continue;
            }
            for y in [b0, b1] {
                let t = (y - a.1) / (b.1 - a.1);
                if (0.0..=1.0).contains(&t) {
                    let x = a.0 + t * (b.0 - a.0);
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                }
            }
        }
        if x0 <= x1 {
            r.set_range(row, x0.floor() as i64, x1.floor() as i64);
        }
    }
}

/// `r` grown by a disc of `radius` cells.
fn dilated(r: &Raster, radius: f64) -> Raster {
    let mut out = Raster::new(r.w, r.h);
    let reach = radius.ceil() as i64;
    let halves: Vec<(i64, i64)> = (-reach..=reach)
        .filter_map(|dy| {
            let s = radius * radius - (dy * dy) as f64;
            (s >= 0.0).then(|| (dy, s.sqrt().floor() as i64))
        })
        .collect();
    for y in 0..r.h {
        let mut x = 0usize;
        while x < r.w {
            if !r.get(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < r.w && r.get(x, y) {
                x += 1;
            }
            for &(dy, half) in &halves {
                out.set_range(y as i64 + dy, start as i64 - half, x as i64 - 1 + half);
            }
        }
    }
    out
}

/// One plate while it fills.
struct Open {
    set: Option<usize>,
    variant: Option<Variant>,
    index_in_set: u32,
    bitmap: Raster,
    colours: Vec<String>,
    tower: bool,
    placements: Vec<usize>,
}

/// What the packer keeps per piece: per candidate it may use, the oriented
/// mesh's footprints at each spin.
struct Shapes {
    /// `(candidate, footprints)`, the chosen candidate first.
    by_candidate: Vec<(usize, Vec<Footprint>)>,
}

fn spins(pin: &super::Pin, mesh: &Mesh) -> Vec<f64> {
    if pin.no_spin {
        return vec![0.0];
    }
    if let Some(a) = pin.spin {
        return vec![a];
    }
    let (turn, _, _) = min_rect(&plan_points(mesh));
    let mut out: Vec<f64> = Vec::new();
    for a in [0.0, 90.0, 180.0, 270.0, turn, turn + 90.0, turn + 180.0, turn + 270.0] {
        let a = a.rem_euclid(360.0);
        if !out.iter().any(|b| ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 0.5) {
            out.push(a);
        }
    }
    out
}

fn spun(mesh: &Mesh, spin: f64) -> (Mesh, Transform) {
    let s = Transform::rotation(Vec3::ZERO, Vec3::Z, spin.to_radians());
    (mesh.transformed(&s), s)
}

fn shapes(bodies: &[PlanBody<'_>], piece: &Piece, opts: &Options) -> Shapes {
    let mesh = super::concat(piece.bodies.iter().map(|&b| bodies[b].mesh));
    let mut by_candidate = Vec::new();
    for c in std::iter::once(piece.chosen).chain(piece.alternatives.iter().copied()) {
        let oriented = mesh.transformed(&piece.candidates[c].transform);
        let fps = spins(&piece.pin, &oriented)
            .into_iter()
            .map(|a| footprint(&spun(&oriented, a).0, opts.cell, opts.spacing / 2.0, a))
            .collect();
        by_candidate.push((c, fps));
    }
    Shapes { by_candidate }
}

/// The plate grid: the region at `cell`, with a margin of the dilation all
/// round so a piece may stand at the edge of the region.
fn empty_plate(opts: &Options) -> (Raster, usize) {
    let pad = (opts.spacing / 2.0 / opts.cell).ceil() as usize;
    let w = ((opts.region[2] - opts.region[0]) / opts.cell).floor() as usize;
    let h = ((opts.region[3] - opts.region[1]) / opts.cell).floor() as usize;
    // The margin is left free: a footprint's own cells start `pad` in from
    // its corner, so a footprint inside the grid has its core inside the
    // region and only its spacing in the margin.
    let mut free = Raster::new(w + 2 * pad, h + 2 * pad);
    for e in &opts.exclude {
        let x0 = ((e[0] - opts.region[0]) / opts.cell).floor() as i64 + pad as i64;
        let x1 = ((e[2] - opts.region[0]) / opts.cell).ceil() as i64 + pad as i64 - 1;
        let y0 = ((e[1] - opts.region[1]) / opts.cell).floor() as i64 + pad as i64;
        let y1 = ((e[3] - opts.region[1]) / opts.cell).ceil() as i64 + pad as i64 - 1;
        for y in y0..=y1 {
            free.set_range(y, x0, x1);
        }
    }
    (free, pad)
}

/// The square kept for the prime tower, in grid cells: the back right of
/// the region.
fn tower_cells(opts: &Options, pad: usize) -> Option<(usize, usize, usize, usize)> {
    let side = opts.tower?;
    let s = (side / opts.cell).ceil() as usize;
    let w = ((opts.region[2] - opts.region[0]) / opts.cell).floor() as usize;
    let h = ((opts.region[3] - opts.region[1]) / opts.cell).floor() as usize;
    if s > w || s > h {
        return None;
    }
    Some((pad + w - s, pad + h - s, s, s))
}

fn rect_clear(r: &Raster, (x, y, w, h): (usize, usize, usize, usize)) -> bool {
    (y..y + h).all(|yy| (x..x + w).all(|xx| !r.get(xx, yy)))
}

fn rect_fill(r: &mut Raster, (x, y, w, h): (usize, usize, usize, usize)) {
    for yy in y..y + h {
        r.set_range(yy as i64, x as i64, (x + w) as i64 - 1);
    }
}

/// Pack every piece, coupons first, then one set of plates per variant.
pub fn pack(bodies: &[PlanBody<'_>], pieces: &[Piece], opts: &Options, notes: &mut Vec<String>) -> (Vec<Plate>, Vec<Placement>) {
    let shapes: Vec<Shapes> = pieces.iter().map(|p| shapes(bodies, p, opts)).collect();
    let area = |i: usize| -> f64 {
        let fp = &shapes[i].by_candidate[0].1[0];
        (fp.extent[2] - fp.extent[0]) * (fp.extent[3] - fp.extent[1])
    };
    // Order: groups by their total area, larger first; inside a group,
    // larger first; ties by input order.
    let order_of = |which: &dyn Fn(&Piece) -> bool| -> Vec<usize> {
        let idx: Vec<usize> = (0..pieces.len()).filter(|&i| which(&pieces[i])).collect();
        let mut units: Vec<(f64, usize, Vec<usize>)> = Vec::new();
        for &i in &idx {
            match &pieces[i].pin.group {
                Some(g) => match units.iter_mut().find(|u| pieces[u.2[0]].pin.group.as_ref() == Some(g)) {
                    Some(u) => {
                        u.0 += area(i);
                        u.2.push(i);
                    }
                    None => units.push((area(i), i, vec![i])),
                },
                None => units.push((area(i), i, vec![i])),
            }
        }
        for u in units.iter_mut() {
            u.2.sort_by(|&a, &b| area(b).total_cmp(&area(a)).then(a.cmp(&b)));
        }
        units.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        units.into_iter().flat_map(|u| u.2).collect()
    };
    let coupons = order_of(&|p: &Piece| p.pin.coupon);
    let rest = order_of(&|p: &Piece| !p.pin.coupon);

    let mut plates: Vec<Open> = Vec::new();
    let mut placements: Vec<Placement> = Vec::new();
    let colour_first = opts.variants.first().copied().unwrap_or(Variant::Colour);
    let mut sets: Vec<(Option<Variant>, Vec<usize>)> = Vec::new();
    if !coupons.is_empty() {
        sets.push((None, coupons));
    }
    for v in &opts.variants {
        if !rest.is_empty() {
            sets.push((Some(*v), rest.clone()));
        }
    }
    for (set, (variant, order)) in sets.into_iter().enumerate() {
        let mono = variant.unwrap_or(colour_first) == Variant::Mono;
        let mut group_plate: Vec<(String, usize)> = Vec::new();
        for &i in &order {
            let piece = &pieces[i];
            let colours: Vec<String> = if mono { Vec::new() } else { piece.colours.clone() };
            // The plates this piece may join, in order of preference.
            let mut tries: Vec<usize> = (0..plates.len()).filter(|&k| plates[k].set == Some(set)).collect();
            let affinity = |k: usize| {
                let have = &plates[k].colours;
                let new = colours.iter().filter(|c| !have.contains(c)).count();
                let shared = colours.len() - new;
                (new, usize::MAX - shared, k)
            };
            tries.sort_by_key(|&k| affinity(k));
            if let Some(g) = &piece.pin.group {
                if let Some(&(_, k)) = group_plate.iter().find(|(name, _)| name == g) {
                    tries.retain(|&t| t != k);
                    tries.insert(0, k);
                }
            }
            if let Some(n) = piece.pin.plate {
                while (plates.iter().filter(|p| p.set == Some(set)).count() as u32) < n {
                    let k = plates.iter().filter(|p| p.set == Some(set)).count() as u32 + 1;
                    let (bitmap, _) = empty_plate(opts);
                    plates.push(Open { set: Some(set), variant, index_in_set: k, bitmap, colours: Vec::new(), tower: false, placements: Vec::new() });
                }
                let k = plates.iter().position(|p| p.set == Some(set) && p.index_in_set == n).expect("made above");
                tries.retain(|&t| t != k);
                tries.insert(0, k);
            }
            let mut placed = None;
            for k in tries {
                if let Some(p) = try_place(&mut plates[k], piece, &shapes[i], &colours, mono, opts) {
                    placed = Some((k, p));
                    break;
                }
            }
            let (k, (cand, fp, cx, cy)) = match placed {
                Some(x) => x,
                None => {
                    let n = plates.iter().filter(|p| p.set == Some(set)).count() as u32 + 1;
                    let (bitmap, _) = empty_plate(opts);
                    plates.push(Open { set: Some(set), variant, index_in_set: n, bitmap, colours: Vec::new(), tower: false, placements: Vec::new() });
                    let k = plates.len() - 1;
                    match try_place(&mut plates[k], piece, &shapes[i], &colours, mono, opts) {
                        Some(p) => (k, p),
                        None => {
                            // It fits no plate: on a plate of its own, where
                            // the export's own check will say why.
                            notes.push(format!(
                                "{} does not fit the {} in any orientation and spin tried; it is put on a plate of its own",
                                piece.name, opts.printer
                            ));
                            let (c, fps) = &shapes[i].by_candidate[0];
                            for col in &colours {
                                if !plates[k].colours.contains(col) {
                                    plates[k].colours.push(col.clone());
                                }
                            }
                            (k, (*c, fps[0].clone(), 0, 0))
                        }
                    }
                }
            };
            if let Some(g) = &piece.pin.group {
                if !group_plate.iter().any(|(name, _)| name == g) {
                    group_plate.push((g.clone(), k));
                }
            }
            let pad = pad_of(opts);
            let px = opts.region[0] + (cx as f64 - pad as f64) * opts.cell;
            let py = opts.region[1] + (cy as f64 - pad as f64) * opts.cell;
            let shift = Vec3::new(px - fp.origin.0, py - fp.origin.1, 0.0);
            let spin = Transform::rotation(Vec3::ZERO, Vec3::Z, fp.spin.to_radians());
            let transform = piece.candidates[cand].transform.then(spin).then(Transform::translation(shift));
            let footprint = [fp.extent[0] + shift.x, fp.extent[1] + shift.y, fp.extent[2] + shift.x, fp.extent[3] + shift.y];
            placements.push(Placement { piece: i, plate: k, transform, candidate: cand, spin: fp.spin, footprint });
            plates[k].placements.push(placements.len() - 1);
        }
    }

    // Plates in set order, then number; names from the set.
    let mut order: Vec<usize> = (0..plates.len()).collect();
    order.sort_by_key(|&k| (plates[k].set, plates[k].index_in_set));
    let mut remap = vec![0usize; plates.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    for p in placements.iter_mut() {
        p.plate = remap[p.plate];
    }
    let several_sets = opts.variants.len() > 1;
    let out: Vec<Plate> = order
        .iter()
        .map(|&k| {
            let p = &plates[k];
            let count = plates.iter().filter(|q| q.set == p.set).count();
            let name = match p.variant {
                None if count == 1 => "Test coupons".to_owned(),
                None => format!("Test coupons {}", p.index_in_set),
                Some(v) if several_sets => format!("{} {}", v.title(), p.index_in_set),
                Some(_) if count == 1 => "Parts".to_owned(),
                Some(_) => format!("Parts {}", p.index_in_set),
            };
            Plate {
                name,
                variant: p.variant,
                placements: p.placements.clone(),
                colours: p.colours.clone(),
                colour_changes: 0,
                layers: 0,
                tower: p.tower,
            }
        })
        .collect();
    (out, placements)
}

fn pad_of(opts: &Options) -> usize {
    (opts.spacing / 2.0 / opts.cell).ceil() as usize
}

/// Place `piece` on `plate` if it fits: its best orientation at its best
/// spin, or one of its equally good alternatives. Returns the candidate,
/// the footprint and the corner cell.
fn try_place(
    plate: &mut Open,
    piece: &Piece,
    shapes: &Shapes,
    colours: &[String],
    mono: bool,
    opts: &Options,
) -> Option<(usize, Footprint, usize, usize)> {
    let mut union = plate.colours.clone();
    for c in colours {
        if !union.contains(c) {
            union.push(c.clone());
        }
    }
    if !mono && opts.ams > 0 && union.len() as u32 > opts.ams && !plate.placements.is_empty() {
        return None;
    }
    let pad = pad_of(opts);
    let needs_tower = !mono && union.len() > 1 && !plate.tower;
    let mut grid = plate.bitmap.clone();
    if needs_tower {
        if let Some(t) = tower_cells(opts, pad) {
            if !rect_clear(&grid, t) {
                return None;
            }
            rect_fill(&mut grid, t);
        }
    }
    let _ = piece;
    for (cand, fps) in &shapes.by_candidate {
        let mut best: Option<(usize, usize, usize, usize, usize)> = None; // (top, right, k, cx, cy)
        for (k, fp) in fps.iter().enumerate() {
            if let Some((cx, cy)) = grid.first_fit(&fp.raster) {
                let key = (cy + fp.raster.h, cx + fp.raster.w, k, cx, cy);
                // A later spin has to gain more than a cell to be worth
                // the turn: a round part is not turned 47.8° for nothing.
                let better = |b: &(usize, usize, usize, usize, usize)| {
                    key.0 + 1 < b.0 || (key.0 <= b.0 + 1 && key.1 + 1 < b.1)
                };
                if best.as_ref().is_none_or(better) {
                    best = Some(key);
                }
            }
        }
        if let Some((_, _, k, cx, cy)) = best {
            let fp = fps[k].clone();
            grid.stamp(&fp.raster, cx, cy);
            plate.bitmap = grid;
            plate.colours = union;
            plate.tower |= needs_tower;
            return Some((*cand, fp, cx, cy));
        }
    }
    None
}
