//! Packing pieces onto plates by their true outlines.
//!
//! `docs/plating.md` ("Packing") is the design. In short:
//!
//! * **Footprints.** A piece's footprint is its triangles projected onto
//!   the bed and rasterised at `cell` mm, a cell marked when the piece
//!   touches any part of it. A plate is a bitmap of where every nozzle
//!   reaches holding the footprints of its pieces *grown by the spacing*,
//!   its excluded corners and its prime tower's square; a piece may stand
//!   where its own cells miss them all, which puts it at least the spacing
//!   from every other piece. Rows are 64-bit words, so a test is a few ANDs
//!   per row.
//! * **Which plate** ([`fill`], [`dissolve`], [`Cost`]). The same greedy
//!   fill is run in three orders and with two rules for joining a plate,
//!   each result improved by emptying plates into the others, and the
//!   cheapest kept: plates against colour changes, by `Options::prefer`.
//! * **Where on the plate** ([`settle`], [`centred`]). Each plate is laid
//!   out again from the middle of the bed outwards, the pieces likeliest to
//!   warp first and nearest it, each at the clear position nearest its
//!   target; the prime tower keeps a corner.
//!
//! Nothing here is trusted: [`super::verify`] measures the finished layout
//! on the pieces' triangles.

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::Mesh;

use super::{Options, Piece, Placement, Plate, PlanBody, Variant};

/// A bitmap, rows of 64-bit words.
#[derive(Clone, Debug, PartialEq)]
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
        let (s, c) = cadpole_core::fmath::sin_cos(a);
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
        let edge = cadpole_core::fmath::atan2(b.1 - a.1, b.0 - a.0);
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

/// A piece's footprint at one spin: the cells it touches (`core`), those
/// cells grown by the spacing (`raster`, the same size and corner), where
/// the corner cell is in model mm, the core's extent `[x0, y0, x1, y1]` in
/// mm, the core's centroid in cells from the corner, and the core's area
/// in mm². A plate's bitmap holds the grown rasters of the pieces on it,
/// and a piece may stand where its core misses them all.
#[derive(Clone, Debug)]
pub struct Footprint {
    pub core: Raster,
    pub raster: Raster,
    pub origin: (f64, f64),
    pub extent: [f64; 4],
    pub spin: f64,
    pub centroid: (f64, f64),
    pub area: f64,
}

/// The cells a footprint is grown by on every side for a clearance of
/// `spacing` mm.
pub fn margin(spacing: f64, cell: f64) -> usize {
    (spacing / cell - 1e-9).ceil().max(0.0) as usize
}

/// The raster of `mesh` seen from above, and the same grown so that a core
/// that misses another footprint's grown raster is a shape at least
/// `dilate` mm from that footprint's.
pub fn footprint(mesh: &Mesh, cell: f64, dilate: f64, spin: f64) -> Footprint {
    let Some((lo, hi)) = mesh.bbox() else {
        return Footprint { core: Raster::new(1, 1), raster: Raster::new(1, 1), origin: (0.0, 0.0), extent: [0.0; 4], spin, centroid: (0.0, 0.0), area: 0.0 };
    };
    let pad = margin(dilate, cell) as i64;
    let origin = (lo.x - pad as f64 * cell, lo.y - pad as f64 * cell);
    let across = |extent: f64| ((extent / cell - 1e-9).ceil() as i64).max(1);
    let w = (across(hi.x - lo.x) + 2 * pad) as usize;
    let h = (across(hi.y - lo.y) + 2 * pad) as usize;
    let mut core = Raster::new(w, h);
    let (mut flat, mut edge_on) = (true, Vec::new());
    for t in 0..mesh.triangle_count() {
        let mut p = [(0.0f64, 0.0f64); 3];
        for (k, q) in p.iter_mut().enumerate() {
            let i = mesh.indices[3 * t + k] as usize;
            *q = (
                (mesh.positions[3 * i] as f64 - origin.0) / cell,
                (mesh.positions[3 * i + 1] as f64 - origin.1) / cell,
            );
        }
        // A wall seen edge on has no area from above; it lies on the edge
        // of the faces it joins, which mark its cells.
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
        if area.abs() > 1e-9 {
            fill_triangle(&mut core, &p);
            flat = false;
        } else {
            edge_on.push(p);
        }
    }
    if flat {
        // Nothing but walls: a sheet standing on edge. Mark those.
        for p in &edge_on {
            fill_triangle(&mut core, p);
        }
    }
    let (mut n, mut sx, mut sy) = (0usize, 0.0f64, 0.0f64);
    for y in 0..h {
        for x in 0..w {
            if core.get(x, y) {
                n += 1;
                sx += x as f64 + 0.5;
                sy += y as f64 + 0.5;
            }
        }
    }
    let centroid = if n > 0 { (sx / n as f64, sy / n as f64) } else { (w as f64 / 2.0, h as f64 / 2.0) };
    let raster = grown(&core, dilate / cell);
    Footprint { core, raster, origin, extent: [lo.x, lo.y, hi.x, hi.y], spin, centroid, area: n as f64 * cell * cell }
}

/// Mark every cell the triangle covers any part of, row band by row band.
/// A cell is its closed square, so a triangle that only touches a cell's
/// low edge or corner is already inside the cell below or beside it and
/// does not mark this one.
fn fill_triangle(r: &mut Raster, p: &[(f64, f64); 3]) {
    let ymin = p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    let ymax = p.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max);
    if !ymin.is_finite() {
        return;
    }
    for row in (ymin.floor() as i64)..=((ymax - 1e-9).floor() as i64).max(ymin.floor() as i64) {
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
            r.set_range(row, x0.floor() as i64, ((x1 - 1e-9).floor() as i64).max(x0.floor() as i64));
        }
    }
}

/// `r` and every cell nearer than `radius` cells to one of its cells,
/// measured between the cells' closed squares, not their centres. A shape
/// lies inside its cells, so a point nearer than `radius` to the shape is
/// in a cell of the result: a second shape whose own cells miss the result
/// has no point nearer than `radius` to the first.
fn grown(r: &Raster, radius: f64) -> Raster {
    let mut out = r.clone();
    let reach = radius.ceil() as i64 + 1;
    let halves: Vec<(i64, i64)> = (-reach..=reach)
        .filter_map(|dy| {
            let t = (dy.abs() - 1).max(0) as f64;
            (t < radius - 1e-9).then(|| (dy, ((radius * radius - t * t).sqrt() - 1e-9).ceil() as i64))
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

/// Where one piece stands on a plate's grid: the candidate orientation,
/// which of its spins, and the corner cell of that footprint's raster.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    cand: usize,
    spin: usize,
    cx: i64,
    cy: i64,
}

type Cells = (usize, usize, usize, usize);

/// One plate while it fills.
#[derive(Clone)]
struct Open {
    set: usize,
    variant: Option<Variant>,
    number: u32,
    bitmap: Raster,
    colours: Vec<String>,
    tower: Option<Cells>,
    /// `(piece, where)`, in the order placed.
    placed: Vec<(usize, Spot)>,
    /// Holds one piece too large for the plate, and nothing else.
    oversize: bool,
    /// The area its pieces' footprints cover, in cells.
    used: f64,
}

/// What the packer keeps per piece.
struct Shapes {
    /// `(candidate, footprints per spin)`, the chosen candidate first.
    by_candidate: Vec<(usize, Vec<Footprint>)>,
    /// The chosen candidate's footprint area, longest side and height.
    area: f64,
    /// The least cells any of its footprints covers.
    cells: f64,
    long: f64,
    height: f64,
    /// How much the piece wants the middle of the bed, 0..1 ([`warp_need`]).
    need: f64,
    /// Per filament the piece prints in, the heights it spans as it
    /// stands: `(index into the piece's colours, z0, z1)`.
    spans: Vec<(usize, f64, f64)>,
}

/// The spins tried for a piece the script has not pinned: the quarter
/// turns, the quarter turns of its minimum-area rectangle, and, for a piece
/// nearly as long as the plate's shorter side, its long side along either
/// diagonal of the plate.
fn spins(pin: &super::Pin, mesh: &Mesh, opts: &Options) -> Vec<f64> {
    if pin.no_spin {
        return vec![0.0];
    }
    if let Some(a) = pin.spin {
        return vec![a];
    }
    let (turn, w, d) = min_rect(&plan_points(mesh));
    let mut all = vec![0.0, 90.0, 180.0, 270.0, turn, turn + 90.0, turn + 180.0, turn + 270.0];
    let (rw, rh) = (opts.region[2] - opts.region[0], opts.region[3] - opts.region[1]);
    if w.max(d) > DIAGONAL_FROM * rw.min(rh) {
        let along_x = if w >= d { turn } else { turn + 90.0 };
        let diagonal = cadpole_core::fmath::atan2(rh, rw).to_degrees();
        all.extend([along_x + diagonal, along_x - diagonal, along_x + diagonal + 180.0, along_x - diagonal + 180.0]);
    }
    let mut out: Vec<f64> = Vec::new();
    for a in all {
        let a = a.rem_euclid(360.0);
        if !out.iter().any(|b| ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 0.5) {
            out.push(a);
        }
    }
    out
}

/// A piece longer than this share of the plate's shorter side is also
/// tried along the plate's diagonals.
pub const DIAGONAL_FROM: f64 = 0.8;

fn spun(mesh: &Mesh, spin: f64) -> (Mesh, Transform) {
    let s = Transform::rotation(Vec3::ZERO, Vec3::Z, spin.to_radians());
    (mesh.transformed(&s), s)
}

/// How much a piece gains from the even heat in the middle of the bed,
/// 0..1: the risk that its first layer lifts. Long pieces, pieces with a
/// large first layer, pieces low for their length (a two-layer panel) and
/// pieces with sharp corners rank high; a small chunky part ranks near 0.
pub fn warp_need(long: f64, bed_contact: f64, height: f64, sharpest_corner_degrees: f64) -> f64 {
    let sat = |v: f64| v.clamp(0.0, 1.0);
    let length = sat(long / 250.0);
    let contact = sat(bed_contact / 15000.0);
    let flat = if long > 0.0 { sat(1.0 - height / (0.2 * long)) } else { 0.0 };
    let corner = sat((120.0 - sharpest_corner_degrees) / 60.0);
    0.4 * length + 0.3 * contact + 0.2 * flat * length.max(contact) + 0.1 * corner * length.max(contact)
}

/// The sharpest corner of the hull of `points`, in degrees, ignoring edges
/// under 5 mm (a chamfer or an arc's facets); 180 for no corner.
fn sharpest_corner(points: &[(f64, f64)]) -> f64 {
    let mut h = hull(points);
    // Drop short edges: keep a vertex only when it is 5 mm from the last kept.
    let mut kept: Vec<(f64, f64)> = Vec::new();
    for p in h.drain(..) {
        if kept.last().is_none_or(|q| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt() >= 5.0) {
            kept.push(p);
        }
    }
    if kept.len() < 3 {
        return 180.0;
    }
    let mut sharpest = 180.0f64;
    for i in 0..kept.len() {
        let (a, b, c) = (kept[(i + kept.len() - 1) % kept.len()], kept[i], kept[(i + 1) % kept.len()]);
        let (u, v) = ((a.0 - b.0, a.1 - b.1), (c.0 - b.0, c.1 - b.1));
        let (lu, lv) = ((u.0 * u.0 + u.1 * u.1).sqrt(), (v.0 * v.0 + v.1 * v.1).sqrt());
        if lu > 0.0 && lv > 0.0 {
            let cos = ((u.0 * v.0 + u.1 * v.1) / (lu * lv)).clamp(-1.0, 1.0);
            sharpest = sharpest.min(cos.acos().to_degrees());
        }
    }
    sharpest
}

fn shapes(bodies: &[PlanBody<'_>], piece: &Piece, opts: &Options, extra: usize) -> Shapes {
    let mesh = super::concat(piece.bodies.iter().map(|&b| bodies[b].mesh));
    let dilate = opts.spacing + extra as f64 * opts.cell;
    let mut by_candidate = Vec::new();
    let (mut long, mut corner) = (0.0, 180.0);
    for (n, c) in std::iter::once(piece.chosen).chain(piece.alternatives.iter().copied()).enumerate() {
        let oriented = mesh.transformed(&piece.candidates[c].transform);
        if n == 0 {
            let points = plan_points(&oriented);
            let (_, w, d) = min_rect(&points);
            long = w.max(d);
            corner = sharpest_corner(&points);
        }
        // A spin that shows the same outline as an earlier one (a block
        // turned half way round) is not tried twice.
        let mut fps: Vec<Footprint> = Vec::new();
        for a in spins(&piece.pin, &oriented, opts) {
            let fp = footprint(&spun(&oriented, a).0, opts.cell, dilate, a);
            if !fps.iter().any(|f| f.core == fp.core) {
                fps.push(fp);
            }
        }
        by_candidate.push((c, fps));
    }
    let best = piece.best();
    let mut spans: Vec<(usize, f64, f64)> = Vec::new();
    for &b in &piece.bodies {
        let body = &bodies[b];
        let m = body.mesh.transformed(&best.transform);
        let mut span = vec![(f64::INFINITY, f64::NEG_INFINITY); body.colours.len().max(1)];
        for t in 0..m.triangle_count() {
            let c = (body.tri_colours.get(t).copied().unwrap_or(0) as usize).min(span.len() - 1);
            for j in 0..3 {
                let z = m.positions[3 * m.indices[3 * t + j] as usize + 2] as f64;
                span[c] = (span[c].0.min(z), span[c].1.max(z));
            }
        }
        for (c, (z0, z1)) in span.into_iter().enumerate() {
            if let (true, Some(k)) = (z0 <= z1, body.colours.get(c).and_then(|hex| piece.colours.iter().position(|h| h == hex))) {
                spans.push((k, z0, z1));
            }
        }
    }
    let area = by_candidate[0].1[0].area;
    let contact = if best.full && best.bed_contact > 0.0 { best.bed_contact } else { area };
    let cells = by_candidate.iter().flat_map(|(_, fps)| fps.iter()).map(|f| f.area).fold(f64::INFINITY, f64::min) / (opts.cell * opts.cell);
    Shapes { by_candidate, area, cells, long, height: best.height, need: warp_need(long, contact, best.height, corner), spans }
}

/// The plate's grid: the region at `cell`, with a margin all round so a
/// piece may stand at the edge of the region with its spacing outside it.
struct Grid {
    pad: usize,
    /// The region, in cells.
    w: usize,
    h: usize,
    cell: f64,
    region: [f64; 4],
    exclude: Vec<[f64; 4]>,
    /// The side of the prime tower's square, in cells.
    tower: Option<usize>,
}

impl Grid {
    fn new(opts: &Options, extra: usize) -> Grid {
        let w = ((opts.region[2] - opts.region[0]) / opts.cell).floor() as usize;
        let h = ((opts.region[3] - opts.region[1]) / opts.cell).floor() as usize;
        let tower = opts.tower.map(|side| (side / opts.cell).ceil() as usize).filter(|s| *s <= w && *s <= h);
        Grid {
            pad: margin(opts.spacing, opts.cell) + extra,
            w,
            h,
            cell: opts.cell,
            region: opts.region,
            exclude: opts.exclude.clone(),
            tower,
        }
    }

    /// An empty plate: the excluded rectangles taken. The margin round
    /// the region is free and stays so for cores: a footprint's own cells
    /// start `pad` in from its corner, so a footprint inside the grid has
    /// its core inside the region.
    fn empty(&self) -> Raster {
        let pad = self.pad as i64;
        let mut free = Raster::new(self.w + 2 * self.pad, self.h + 2 * self.pad);
        for e in &self.exclude {
            let x0 = ((e[0] - self.region[0]) / self.cell).floor() as i64 + pad;
            let x1 = ((e[2] - self.region[0]) / self.cell).ceil() as i64 + pad - 1;
            let y0 = ((e[1] - self.region[1]) / self.cell).floor() as i64 + pad;
            let y1 = ((e[3] - self.region[1]) / self.cell).ceil() as i64 + pad - 1;
            for y in y0..=y1 {
                free.set_range(y, x0, x1);
            }
        }
        free
    }

    /// Where the prime tower's square may stand, in order of preference:
    /// the back right corner, the back left, the front right, the front
    /// left. A corner, never the middle: the middle is where the bed's heat
    /// is most even, and it is kept for the pieces.
    fn tower_corners(&self) -> Vec<Cells> {
        let Some(s) = self.tower else { return Vec::new() };
        let (p, w, h) = (self.pad, self.w, self.h);
        vec![(p + w - s, p + h - s, s, s), (p, p + h - s, s, s), (p + w - s, p, s, s), (p, p, s, s)]
    }

    fn cells_mm(&self, (x, y, w, h): Cells) -> [f64; 4] {
        let at = |c: usize, o: f64| o + (c as f64 - self.pad as f64) * self.cell;
        [at(x, self.region[0]), at(y, self.region[1]), at(x + w, self.region[0]), at(y + h, self.region[1])]
    }

    /// The middle of the bed, in grid cells.
    fn middle(&self, opts: &Options) -> (f64, f64) {
        let (x, y) = opts.centre.unwrap_or(((self.region[0] + self.region[2]) / 2.0, (self.region[1] + self.region[3]) / 2.0));
        (self.pad as f64 + (x - self.region[0]) / self.cell, self.pad as f64 + (y - self.region[1]) / self.cell)
    }

    /// Whether a raster of `fp`'s size with its corner at `(cx, cy)` is
    /// inside the grid.
    fn holds(&self, fp: &Raster, cx: i64, cy: i64) -> bool {
        cx >= 0 && cy >= 0 && cx as usize + fp.w <= self.w + 2 * self.pad && cy as usize + fp.h <= self.h + 2 * self.pad
    }
}

fn rect_clear(r: &Raster, (x, y, w, h): Cells) -> bool {
    (y..y + h).all(|yy| (x..x + w).all(|xx| !r.get(xx, yy)))
}

fn rect_fill(r: &mut Raster, (x, y, w, h): Cells) {
    for yy in y..y + h {
        r.set_range(yy as i64, x as i64, (x + w) as i64 - 1);
    }
}

/// Every grid offset, nearest first (ties: lower row, then lower column),
/// by a counting sort on the squared distance.
fn offsets_by_distance(w: i64, h: i64) -> Vec<(i32, i32)> {
    let max = (w * w + h * h) as usize;
    let mut count = vec![0u32; max + 2];
    for dy in -h..=h {
        for dx in -w..=w {
            count[(dx * dx + dy * dy) as usize + 1] += 1;
        }
    }
    for i in 1..count.len() {
        count[i] += count[i - 1];
    }
    let mut out = vec![(0i32, 0i32); ((2 * w + 1) * (2 * h + 1)) as usize];
    for dy in -h..=h {
        for dx in -w..=w {
            let slot = &mut count[(dx * dx + dy * dy) as usize];
            out[*slot as usize] = (dx as i32, dy as i32);
            *slot += 1;
        }
    }
    out
}

/// The clear position of `fp` nearest to the corner `want`, when one lies
/// within a squared distance of `bound` cells: `(distance², cx, cy)`.
fn nearest_fit(bitmap: &Raster, fp: &Raster, want: (f64, f64), offsets: &[(i32, i32)], bound: i64) -> Option<(i64, usize, usize)> {
    if fp.w > bitmap.w || fp.h > bitmap.h {
        return None;
    }
    let (bx, by) = (want.0.round() as i64, want.1.round() as i64);
    let (mx, my) = ((bitmap.w - fp.w) as i64, (bitmap.h - fp.h) as i64);
    for &(dx, dy) in offsets {
        let d2 = dx as i64 * dx as i64 + dy as i64 * dy as i64;
        if d2 >= bound {
            return None;
        }
        let (cx, cy) = (bx + dx as i64, by + dy as i64);
        if cx < 0 || cy < 0 || cx > mx || cy > my {
            continue;
        }
        if bitmap.clear_at(fp, cx as usize, cy as usize) {
            return Some((d2, cx as usize, cy as usize));
        }
    }
    None
}

/// What [`pack`] returns.
pub struct Packed {
    pub plates: Vec<Plate>,
    pub placements: Vec<Placement>,
    /// Pieces whose pinned plate or position could not be honoured (each
    /// has a note saying why).
    pub unpinned: Vec<usize>,
    /// Per set of plates, the least number of plates the pieces' area and
    /// filaments allow.
    pub floors: Vec<usize>,
}

/// The pieces in packing order: position-pinned pieces first, then units
/// (a group, or a piece) by `key`, larger first, each unit's pieces by
/// `key`; ties by input order.
fn order_of(pieces: &[Piece], which: &dyn Fn(&Piece) -> bool, key: &dyn Fn(usize) -> f64) -> Vec<usize> {
    let idx: Vec<usize> = (0..pieces.len()).filter(|&i| which(&pieces[i])).collect();
    let mut units: Vec<(f64, usize, Vec<usize>)> = Vec::new();
    for &i in &idx {
        match &pieces[i].pin.group {
            Some(g) => match units.iter_mut().find(|u| pieces[u.2[0]].pin.group.as_ref() == Some(g)) {
                Some(u) => {
                    u.0 += key(i);
                    u.2.push(i);
                }
                None => units.push((key(i), i, vec![i])),
            },
            None => units.push((key(i), i, vec![i])),
        }
    }
    for u in units.iter_mut() {
        u.2.sort_by(|&a, &b| key(b).total_cmp(&key(a)).then(a.cmp(&b)));
    }
    units.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<usize> = units.into_iter().flat_map(|u| u.2).collect();
    out.sort_by_key(|&i| pieces[i].pin.at.is_none());
    out
}

struct Filled {
    plates: Vec<Open>,
    notes: Vec<String>,
    unpinned: Vec<usize>,
}

/// Fill one set's plates with `order`, each piece at the lowest, then
/// leftmost, clear position of the first plate that takes it.
#[allow(clippy::too_many_arguments)]
fn fill(set: usize, variant: Option<Variant>, mono: bool, pure: bool, order: &[usize], pieces: &[Piece], shapes: &[Shapes], grid: &Grid, opts: &Options) -> Filled {
    let mut plates: Vec<Open> = Vec::new();
    let mut notes = Vec::new();
    let mut unpinned = Vec::new();
    let mut group_plate: Vec<(String, usize)> = Vec::new();
    let new_plate = |plates: &mut Vec<Open>| -> usize {
        let number = plates.len() as u32 + 1;
        plates.push(Open { set, variant, number, bitmap: grid.empty(), colours: Vec::new(), tower: None, placed: Vec::new(), oversize: false, used: 0.0 });
        plates.len() - 1
    };
    for &i in order {
        let piece = &pieces[i];
        let colours: Vec<String> = if mono { Vec::new() } else { piece.colours.clone() };
        let place = |plates: &mut Vec<Open>, honour: bool| -> Option<(usize, Spot)> {
            let at = if honour { piece.pin.at } else { None };
            if let (Some(n), true) = (piece.pin.plate, honour) {
                while (plates.len() as u32) < n {
                    new_plate(plates);
                }
                let k = n as usize - 1;
                return try_place(&mut plates[k], &shapes[i], &colours, mono, pure, opts, grid, at).map(|s| (k, s));
            }
            // The plates this piece may join, in order of preference:
            // fewest new filaments, then most shared, then plate number.
            let mut tries: Vec<usize> = (0..plates.len()).filter(|&k| !plates[k].oversize).collect();
            let affinity = |k: usize| {
                let have = &plates[k].colours;
                let new = colours.iter().filter(|c| !have.contains(c)).count();
                // Of two plates that already have its filaments, the one
                // with fewer others: a one-filament piece goes to a
                // one-filament plate before a mixed one.
                (new, usize::MAX - (colours.len() - new), have.len(), k)
            };
            tries.sort_by_key(|&k| affinity(k));
            if let Some(g) = &piece.pin.group {
                if let Some(&(_, k)) = group_plate.iter().find(|(name, _)| name == g) {
                    tries.retain(|&t| t != k);
                    tries.insert(0, k);
                }
            }
            for k in tries {
                if let Some(s) = try_place(&mut plates[k], &shapes[i], &colours, mono, pure, opts, grid, at) {
                    return Some((k, s));
                }
            }
            if plates.last().is_some_and(|p| p.placed.is_empty()) {
                return None;
            }
            let k = new_plate(plates);
            try_place(&mut plates[k], &shapes[i], &colours, mono, pure, opts, grid, at).map(|s| (k, s))
        };
        let mut placed = place(&mut plates, true);
        if placed.is_none() && (piece.pin.plate.is_some() || piece.pin.at.is_some()) {
            let what = match (piece.pin.plate, piece.pin.at) {
                (Some(n), Some((x, y))) => format!("at ({}, {}) on plate {n}", crate::num(x), crate::num(y)),
                (Some(n), None) => format!("on plate {n}"),
                (None, Some((x, y))) => format!("at ({}, {})", crate::num(x), crate::num(y)),
                (None, None) => unreachable!(),
            };
            notes.push(format!(
                "{} is pinned {what} and does not fit there clear of the other pieces, the plate's edge and the prime \
                 tower; it is placed where it fits instead",
                piece.name
            ));
            unpinned.push(i);
            placed = place(&mut plates, false);
        }
        let (k, spot) = match placed {
            Some((k, spot)) => {
                plates[k].placed.push((i, spot));
                (k, spot)
            }
            None => {
                // It fits no plate: alone on a plate nothing else may join,
                // centred, where the export's own check will say why.
                notes.push(format!(
                    "{} does not fit the {} in any orientation and spin tried; it is put on a plate of its own",
                    piece.name, opts.printer
                ));
                let k = if plates.last().is_some_and(|p| p.placed.is_empty()) { plates.len() - 1 } else { new_plate(&mut plates) };
                plates[k].oversize = true;
                for col in &colours {
                    if !plates[k].colours.contains(col) {
                        plates[k].colours.push(col.clone());
                    }
                }
                let fp = &shapes[i].by_candidate[0].1[0];
                let full = (grid.w + 2 * grid.pad, grid.h + 2 * grid.pad);
                let spot = Spot {
                    cand: shapes[i].by_candidate[0].0,
                    spin: 0,
                    cx: (full.0 as i64 - fp.raster.w as i64) / 2,
                    cy: (full.1 as i64 - fp.raster.h as i64) / 2,
                };
                plates[k].placed.push((i, spot));
                (k, spot)
            }
        };
        let _ = spot;
        if let Some(g) = &piece.pin.group {
            if !group_plate.iter().any(|(name, _)| name == g) {
                group_plate.push((g.clone(), k));
            }
        }
    }
    Filled { plates, notes, unpinned }
}

/// The filaments `plate` would hold with `colours` added, or `None` when
/// that is more than the AMS has slots.
fn joined_colours(plate: &Open, colours: &[String], mono: bool, opts: &Options) -> Option<Vec<String>> {
    let mut union = plate.colours.clone();
    for c in colours {
        if !union.contains(c) {
            union.push(c.clone());
        }
    }
    if !mono && opts.ams > 0 && union.len() as u32 > opts.ams && !plate.placed.is_empty() {
        return None;
    }
    Some(union)
}

/// Place a piece on `plate` if it fits: its best orientation at its best
/// spin, or one of its equally good alternatives; at the pinned position
/// `at` (the middle of its footprint, in plate mm) when there is one.
#[allow(clippy::too_many_arguments)]
fn try_place(plate: &mut Open, shapes: &Shapes, colours: &[String], mono: bool, pure: bool, opts: &Options, grid: &Grid, at: Option<(f64, f64)>) -> Option<Spot> {
    if plate.oversize {
        return None;
    }
    // No search can find room for more area than the plate has left.
    if plate.used + shapes.cells > (grid.w * grid.h) as f64 {
        return None;
    }
    let union = joined_colours(plate, colours, mono, opts)?;
    if pure && !plate.placed.is_empty() && union.len() > plate.colours.len() {
        return None;
    }
    let needs_tower = !mono && union.len() > 1 && plate.tower.is_none() && grid.tower.is_some();
    let mut bitmap = plate.bitmap.clone();
    let mut tower = plate.tower;
    if needs_tower {
        let t = grid.tower_corners().into_iter().find(|t| rect_clear(&bitmap, *t))?;
        rect_fill(&mut bitmap, t);
        tower = Some(t);
    }
    for (cand, fps) in &shapes.by_candidate {
        let mut best: Option<(usize, usize, usize, usize, usize)> = None; // (top, right, k, cx, cy)
        for (k, fp) in fps.iter().enumerate() {
            let found = match at {
                Some((x, y)) => {
                    // The middle of the core's box at (x, y): the core's
                    // low corner sits `pad` cells in from the raster's.
                    let lo = (x - (fp.extent[2] - fp.extent[0]) / 2.0, y - (fp.extent[3] - fp.extent[1]) / 2.0);
                    let cx = ((lo.0 - opts.region[0]) / opts.cell).round() as i64;
                    let cy = ((lo.1 - opts.region[1]) / opts.cell).round() as i64;
                    (grid.holds(&fp.raster, cx, cy) && bitmap.clear_at(&fp.core, cx as usize, cy as usize)).then_some((cx as usize, cy as usize))
                }
                None => bitmap.first_fit(&fp.core),
            };
            if let Some((cx, cy)) = found {
                let key = (cy + fp.raster.h, cx + fp.raster.w, k, cx, cy);
                // A later spin has to gain more than a cell to be worth
                // the turn: a round part is not turned 47.8° for nothing.
                let better = |b: &(usize, usize, usize, usize, usize)| key.0 + 1 < b.0 || (key.0 <= b.0 + 1 && key.1 + 1 < b.1);
                if best.as_ref().is_none_or(better) {
                    best = Some(key);
                }
                if at.is_some() {
                    break;
                }
            }
        }
        if let Some((_, _, k, cx, cy)) = best {
            bitmap.stamp(&fps[k].raster, cx, cy);
            plate.bitmap = bitmap;
            plate.colours = union;
            plate.tower = tower;
            plate.used += shapes.cells;
            let spot = Spot { cand: *cand, spin: k, cx: cx as i64, cy: cy as i64 };
            return Some(spot);
        }
    }
    None
}

fn footprint_of<'a>(shapes: &'a Shapes, spot: &Spot) -> &'a Footprint {
    &shapes.by_candidate.iter().find(|(c, _)| *c == spot.cand).expect("a candidate the packer offered").1[spot.spin]
}

/// How strongly a piece is drawn to the pieces of its own filament already
/// on the plate, against the pull of the middle of the bed (which is
/// `centre_bias * (CENTRE_FLOOR + need)`).
const SAME_FILAMENT_PULL: f64 = 0.25;
/// And to the pieces of its own `group=`.
const GROUP_PULL: f64 = 1.5;
/// The pull to the middle a piece with no warp risk still has.
const CENTRE_FLOOR: f64 = 0.3;

/// Lay a plate's pieces out again from the middle of the bed outwards: the
/// pieces that most need even heat first and nearest the middle, each at
/// the clear position nearest its target, which is the middle of the plate
/// drawn towards the pieces of its own filament and group already down.
/// Position-pinned pieces stay where they are. Returns `None` when a piece
/// does not fit that way.
fn settle(plate: &Open, by_need: bool, pieces: &[Piece], shapes: &[Shapes], unpinned: &[usize], grid: &Grid, opts: &Options, offsets: &[(i32, i32)]) -> Option<Open> {
    let mut out = Open { bitmap: grid.empty(), placed: Vec::new(), ..plate.clone() };
    if let Some(t) = out.tower {
        rect_fill(&mut out.bitmap, t);
    }
    let fixed = |i: usize| pieces[i].pin.at.is_some() && !unpinned.contains(&i);
    // (piece, centroid in grid cells, area)
    let mut down: Vec<(usize, (f64, f64), f64)> = Vec::new();
    let mut slots: Vec<Option<Spot>> = vec![None; plate.placed.len()];
    for (n, (i, spot)) in plate.placed.iter().enumerate() {
        if fixed(*i) {
            let fp = footprint_of(&shapes[*i], spot);
            out.bitmap.stamp(&fp.raster, spot.cx as usize, spot.cy as usize);
            down.push((*i, (spot.cx as f64 + fp.centroid.0, spot.cy as f64 + fp.centroid.1), fp.area));
            slots[n] = Some(*spot);
        }
    }
    let mut rest: Vec<usize> = (0..plate.placed.len()).filter(|&n| slots[n].is_none()).collect();
    let key = |n: usize| {
        let s = &shapes[plate.placed[n].0];
        if by_need { s.area * (0.25 + s.need) } else { s.area }
    };
    rest.sort_by(|&a, &b| key(b).total_cmp(&key(a)).then(a.cmp(&b)));
    let middle = grid.middle(opts);
    for n in rest {
        let i = plate.placed[n].0;
        let piece = &pieces[i];
        // The target: the middle, drawn towards its own kind.
        let pull = |which: &dyn Fn(usize) -> bool| {
            let (mut a, mut x, mut y) = (0.0, 0.0, 0.0);
            for (j, c, area) in &down {
                if which(*j) {
                    a += area;
                    x += c.0 * area;
                    y += c.1 * area;
                }
            }
            (a > 0.0).then(|| (x / a, y / a))
        };
        let mut target = (middle.0, middle.1);
        let mut weight = opts.centre_bias * (CENTRE_FLOOR + shapes[i].need);
        let mut draw = |to: Option<(f64, f64)>, w: f64| {
            if let Some(p) = to {
                target = ((target.0 * weight + p.0 * w) / (weight + w), (target.1 * weight + p.1 * w) / (weight + w));
                weight += w;
            }
        };
        draw(pull(&|j| piece.colours.first().is_some() && pieces[j].colours.first() == piece.colours.first()), SAME_FILAMENT_PULL);
        if piece.pin.group.is_some() {
            draw(pull(&|j| pieces[j].pin.group == piece.pin.group), GROUP_PULL);
        }
        let mut found: Option<Spot> = None;
        for (cand, fps) in &shapes[i].by_candidate {
            let mut best: Option<(i64, usize, usize, usize)> = None; // (distance², k, cx, cy)
            for (k, fp) in fps.iter().enumerate() {
                let want = (target.0 - fp.centroid.0, target.1 - fp.centroid.1);
                // A later spin must be nearer by more than a cell and a
                // half to be worth the turn.
                let bound = match best {
                    Some((d2, ..)) => {
                        let d = (d2 as f64).sqrt() - 1.5;
                        if d <= 0.0 { 0 } else { (d * d).floor() as i64 }
                    }
                    None => i64::MAX,
                };
                if let Some((d2, cx, cy)) = nearest_fit(&out.bitmap, &fp.core, want, offsets, bound) {
                    best = Some((d2, k, cx, cy));
                }
            }
            if let Some((_, k, cx, cy)) = best {
                found = Some(Spot { cand: *cand, spin: k, cx: cx as i64, cy: cy as i64 });
                break;
            }
        }
        let spot = found?;
        let fp = footprint_of(&shapes[i], &spot);
        out.bitmap.stamp(&fp.raster, spot.cx as usize, spot.cy as usize);
        down.push((i, (spot.cx as f64 + fp.centroid.0, spot.cy as f64 + fp.centroid.1), fp.area));
        slots[n] = Some(spot);
    }
    out.placed = plate.placed.iter().zip(slots).map(|((i, _), s)| (*i, s.expect("every piece placed"))).collect();
    Some(out)
}

/// Move a plate's pieces as a group so their box is centred on the plate,
/// as far as that keeps them clear of the tower and the excluded corners.
fn centred(plate: &Open, pieces: &[Piece], shapes: &[Shapes], unpinned: &[usize], grid: &Grid, opts: &Options) -> Option<Open> {
    if plate.placed.is_empty() || plate.placed.iter().any(|(i, _)| pieces[*i].pin.at.is_some() && !unpinned.contains(i)) {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (i, spot) in &plate.placed {
        let fp = footprint_of(&shapes[*i], spot);
        x0 = x0.min(spot.cx);
        y0 = y0.min(spot.cy);
        x1 = x1.max(spot.cx + fp.raster.w as i64);
        y1 = y1.max(spot.cy + fp.raster.h as i64);
    }
    let middle = grid.middle(opts);
    let (dx, dy) = ((middle.0 - (x0 + x1) as f64 / 2.0).round() as i64, (middle.1 - (y0 + y1) as f64 / 2.0).round() as i64);
    for (fx, fy) in [(2, 2), (2, 1), (1, 2), (1, 1), (2, 0), (0, 2), (1, 0), (0, 1)] {
        let (sx, sy) = (dx * fx / 2, dy * fy / 2);
        if sx == 0 && sy == 0 {
            continue;
        }
        let mut out = Open { bitmap: grid.empty(), placed: Vec::new(), ..plate.clone() };
        if let Some(t) = out.tower {
            rect_fill(&mut out.bitmap, t);
        }
        let ok = plate.placed.iter().all(|(i, spot)| {
            let fp = footprint_of(&shapes[*i], spot);
            let moved = Spot { cx: spot.cx + sx, cy: spot.cy + sy, ..*spot };
            if !grid.holds(&fp.raster, moved.cx, moved.cy) || !out.bitmap.clear_at(&fp.core, moved.cx as usize, moved.cy as usize) {
                return false;
            }
            out.bitmap.stamp(&fp.raster, moved.cx as usize, moved.cy as usize);
            out.placed.push((*i, moved));
            true
        });
        if ok {
            return Some(out);
        }
    }
    None
}

/// The least number of plates `order`'s pieces can take: by their
/// footprints' area against the plate's, and by their filaments against
/// the AMS's slots.
fn floor_of(order: &[usize], pieces: &[Piece], shapes: &[Shapes], mono: bool, grid: &Grid, opts: &Options) -> usize {
    if order.is_empty() {
        return 0;
    }
    let plate = (grid.w * grid.h) as f64 * grid.cell * grid.cell
        - opts.exclude.iter().map(|e| (e[2] - e[0]) * (e[3] - e[1])).sum::<f64>();
    let area: f64 = order.iter().map(|&i| shapes[i].area).sum();
    let by_area = if plate > 0.0 { (area / plate).ceil() as usize } else { 1 };
    let mut colours: Vec<&String> = Vec::new();
    for &i in order {
        for c in &pieces[i].colours {
            if !colours.contains(&c) {
                colours.push(c);
            }
        }
    }
    let by_colour = if mono || opts.ams == 0 { 1 } else { colours.len().div_ceil(opts.ams as usize) };
    by_area.max(by_colour).max(1)
}

/// What a fill costs: plates to tend, and colour changes to wait for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cost {
    unpinned: usize,
    plates: usize,
    changes: usize,
    /// The changes' time: each at what its plate's changes cost.
    seconds: f64,
}

impl Cost {
    /// The order fills are compared in: pins honoured first, then by
    /// `opts.prefer`.
    fn key(&self, opts: &Options) -> (usize, f64, f64) {
        let (p, c) = (self.plates as f64, self.changes as f64);
        match opts.prefer {
            super::Prefer::Plates => (self.unpinned, p, c),
            super::Prefer::Waste => (self.unpinned, c, p),
            super::Prefer::Balanced => (self.unpinned, p * opts.plate_seconds + self.seconds, p),
        }
    }
}

/// The filament spans of a plate's pieces, numbered by the plate's
/// filaments, and the layers it prints.
fn plate_spans(plate: &Open, pieces: &[Piece], shapes: &[Shapes], lh: f64) -> (Vec<(usize, f64, f64)>, usize) {
    let mut ranges = Vec::new();
    let mut top = 0.0f64;
    for (i, _) in &plate.placed {
        for &(c, z0, z1) in &shapes[*i].spans {
            let k = plate.colours.iter().position(|h| h == &pieces[*i].colours[c]).unwrap_or(0);
            ranges.push((k, z0, z1));
            top = top.max(z1);
        }
    }
    (ranges, (top / lh - 1e-6).ceil().max(0.0) as usize)
}

fn cost_of(f: &Filled, pieces: &[Piece], shapes: &[Shapes], mono: bool, opts: &Options) -> Cost {
    let lh = if opts.profile.layer_height > 0.0 { opts.profile.layer_height } else { 0.2 };
    let (mut changes, mut seconds) = (0, 0.0);
    if !mono {
        for plate in f.plates.iter().filter(|p| p.colours.len() > 1) {
            let (ranges, layers) = plate_spans(plate, pieces, shapes, lh);
            let n = super::colour::changes(&ranges, layers, lh, plate.colours.len());
            changes += n;
            seconds += n as f64 * opts.seconds_per_change(plate.colours.len());
        }
    }
    Cost { unpinned: f.unpinned.len(), plates: f.plates.iter().filter(|p| !p.placed.is_empty()).count(), changes, seconds }
}

/// Empty a plate into the others wherever that is cheaper by
/// `opts.prefer`, the smallest plate first, until no plate can go: a short
/// piece in another filament costs a few colour changes on a plate of tall
/// ones, and a plate of its own costs more. A set with a `plate=` pin is
/// left as filled: its plates' numbers mean something.
fn dissolve(f: Filled, mono: bool, pieces: &[Piece], shapes: &[Shapes], grid: &Grid, opts: &Options) -> Filled {
    if f.plates.iter().any(|p| p.placed.iter().any(|(i, _)| pieces[*i].pin.plate.is_some())) {
        return f;
    }
    let area = |p: &Open| p.placed.iter().map(|(i, _)| shapes[*i].area).sum::<f64>();
    let mut best = f;
    let mut best_cost = cost_of(&best, pieces, shapes, mono, opts);
    loop {
        let mut order: Vec<usize> = (0..best.plates.len()).filter(|&k| !best.plates[k].oversize && !best.plates[k].placed.is_empty()).collect();
        order.sort_by(|&a, &b| area(&best.plates[a]).total_cmp(&area(&best.plates[b])).then(a.cmp(&b)));
        let mut better: Option<(Filled, Cost)> = None;
        for k in order {
            let mut trial = best.plates.clone();
            let gone = trial.remove(k);
            let mut moving: Vec<usize> = gone.placed.iter().map(|(i, _)| *i).collect();
            moving.sort_by(|&a, &b| shapes[b].area.total_cmp(&shapes[a].area).then(a.cmp(&b)));
            let all = moving.iter().all(|&i| {
                let colours: Vec<String> = if mono { Vec::new() } else { pieces[i].colours.clone() };
                let at = if best.unpinned.contains(&i) { None } else { pieces[i].pin.at };
                let mut tries: Vec<usize> = (0..trial.len()).filter(|&t| !trial[t].oversize && !trial[t].placed.is_empty()).collect();
                tries.sort_by_key(|&t| {
                    let have = &trial[t].colours;
                    let new = colours.iter().filter(|c| !have.contains(c)).count();
                    (new, usize::MAX - (colours.len() - new), have.len(), t)
                });
                tries.into_iter().any(|t| match try_place(&mut trial[t], &shapes[i], &colours, mono, false, opts, grid, at) {
                    Some(spot) => {
                        trial[t].placed.push((i, spot));
                        true
                    }
                    None => false,
                })
            });
            if !all {
                continue;
            }
            for (n, p) in trial.iter_mut().enumerate() {
                p.number = n as u32 + 1;
            }
            let moved = Filled { plates: trial, notes: best.notes.clone(), unpinned: best.unpinned.clone() };
            let cost = cost_of(&moved, pieces, shapes, mono, opts);
            if cost.key(opts) < best_cost.key(opts) {
                better = Some((moved, cost));
                break;
            }
        }
        match better {
            Some((f, c)) => {
                best = f;
                best_cost = c;
            }
            None => return best,
        }
    }
}

/// Pack every piece, coupons first, then one set of plates per variant.
/// `extra` widens every footprint's margin by that many cells, for a
/// second try when the check on the real geometry finds a breach.
pub fn pack(bodies: &[PlanBody<'_>], pieces: &[Piece], opts: &Options, extra: usize, notes: &mut Vec<String>) -> Packed {
    let shapes: Vec<Shapes> = pieces.iter().map(|p| shapes(bodies, p, opts, extra)).collect();
    let grid = Grid::new(opts, extra);
    let colour_first = opts.variants.first().copied().unwrap_or(Variant::Colour);
    let coupon = |p: &Piece| p.pin.coupon;
    let part = |p: &Piece| !p.pin.coupon;
    let mut sets: Vec<(Option<Variant>, &dyn Fn(&Piece) -> bool)> = Vec::new();
    if pieces.iter().any(coupon) {
        sets.push((None, &coupon));
    }
    if pieces.iter().any(part) {
        for v in &opts.variants {
            sets.push((Some(*v), &part));
        }
    }
    let offsets = if opts.centre_bias > 0.0 {
        offsets_by_distance((grid.w + 2 * grid.pad) as i64, (grid.h + 2 * grid.pad) as i64)
    } else {
        Vec::new()
    };

    let mut plates: Vec<Open> = Vec::new();
    let mut unpinned: Vec<usize> = Vec::new();
    let mut floors: Vec<usize> = Vec::new();
    for (set, (variant, which)) in sets.into_iter().enumerate() {
        let mono = variant.unwrap_or(colour_first) == Variant::Mono;
        // The same fill in three orders -- largest footprint first,
        // longest first, tallest first -- and two ways of choosing a
        // plate: wherever the piece fits (fewest new filaments first), and
        // only where it brings no new filament. The cheapest by
        // `opts.prefer` is kept, the first found among equals.
        let keys: [&dyn Fn(usize) -> f64; 3] = [&|i| shapes[i].area, &|i| shapes[i].long, &|i| shapes[i].height * 1e6 + shapes[i].area];
        let mut orders: Vec<Vec<usize>> = Vec::new();
        for key in keys {
            let o = order_of(pieces, which, key);
            if !orders.contains(&o) {
                orders.push(o);
            }
        }
        let floor = floor_of(&orders[0], pieces, &shapes, mono, &grid, opts);
        let several = !mono && orders[0].iter().any(|&i| orders[0].iter().any(|&j| pieces[i].colours != pieces[j].colours));
        let mut best: Option<(Filled, Cost)> = None;
        let mut densest: Option<Cost> = None;
        for pure in [false, true] {
            if pure && !several {
                continue;
            }
            for order in &orders {
                let f = dissolve(fill(set, variant, mono, pure, order, pieces, &shapes, &grid, opts), mono, pieces, &shapes, &grid, opts);
                let cost = cost_of(&f, pieces, &shapes, mono, opts);
                if !pure && densest.as_ref().is_none_or(|d| (cost.unpinned, cost.plates, cost.changes) < (d.unpinned, d.plates, d.changes)) {
                    densest = Some(cost);
                }
                if best.as_ref().is_none_or(|(_, b)| cost.key(opts) < b.key(opts)) {
                    best = Some((f, cost));
                }
                if !several && best.as_ref().is_some_and(|(_, b)| b.unpinned == 0 && b.plates <= floor) {
                    break;
                }
            }
        }
        let (filled, cost) = best.expect("at least one order");
        if let Some(d) = densest {
            if d.plates < cost.plates {
                notes.push(format!(
                    "{} plates, not the {} the pieces fit on: keeping filaments on plates of their own saves about {} \
                     colour changes (prefer=\"plates\" packs the fewest plates)",
                    cost.plates,
                    d.plates,
                    d.changes.saturating_sub(cost.changes)
                ));
            }
        }
        // What the plate= pins cost, when leaving them free would mix fewer
        // filaments.
        if orders[0].iter().any(|&i| pieces[i].pin.plate.is_some()) {
            let free: Vec<Piece> = pieces
                .iter()
                .map(|p| Piece { pin: super::Pin { plate: None, at: None, ..p.pin.clone() }, ..p.clone() })
                .collect();
            let mut least: Option<Cost> = None;
            for pure in [false, true] {
                for order in &orders {
                    let c = cost_of(&fill(set, variant, mono, pure, order, &free, &shapes, &grid, opts), &free, &shapes, mono, opts);
                    if least.as_ref().is_none_or(|l| c.key(opts) < l.key(opts)) {
                        least = Some(c);
                    }
                }
            }
            if let Some(l) = least.filter(|l| l.changes < cost.changes && l.key(opts) < cost.key(opts)) {
                notes.push(format!(
                    "the plate= pins cost about {} colour changes: left free, these pieces take {} plate{} and about {} \
                     changes, not {}",
                    cost.changes - l.changes,
                    l.plates,
                    if l.plates == 1 { "" } else { "s" },
                    l.changes,
                    cost.changes
                ));
            }
        }
        notes.extend(filled.notes);
        // Then quality, a plate at a time, the plate count settled: out
        // from the middle by warp risk; failing that by size; failing that
        // the fill as it is, centred as a group.
        for plate in filled.plates {
            let settled = if plate.oversize || plate.placed.is_empty() {
                None
            } else if opts.centre_bias > 0.0 {
                settle(&plate, true, pieces, &shapes, &filled.unpinned, &grid, opts, &offsets)
                    .or_else(|| settle(&plate, false, pieces, &shapes, &filled.unpinned, &grid, opts, &offsets))
            } else {
                None
            };
            // Whichever layout it is, its box goes to the middle of the
            // bed when it can.
            let plate = settled.unwrap_or(plate);
            let plate = centred(&plate, pieces, &shapes, &filled.unpinned, &grid, opts).unwrap_or(plate);
            plates.push(plate);
        }
        for i in filled.unpinned {
            if !unpinned.contains(&i) {
                unpinned.push(i);
            }
        }
        floors.push(floor);
    }

    // A group that did not stay on one plate is said, never silent.
    let mut groups: Vec<&String> = Vec::new();
    for p in pieces {
        if let Some(g) = &p.pin.group {
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
    }
    for g in groups {
        let mut sets_seen: Vec<usize> = Vec::new();
        for plate in &plates {
            if sets_seen.contains(&plate.set) {
                continue;
            }
            let on: Vec<u32> = plates
                .iter()
                .filter(|p| p.set == plate.set && p.placed.iter().any(|(i, _)| pieces[*i].pin.group.as_ref() == Some(g)))
                .map(|p| p.number)
                .collect();
            if on.len() > 1 {
                notes.push(format!(
                    "group {g:?} does not fit one plate; its pieces are on plates {}",
                    on.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
            sets_seen.push(plate.set);
        }
    }

    let several_sets = opts.variants.len() > 1;
    let mut out_plates: Vec<Plate> = Vec::with_capacity(plates.len());
    let mut placements: Vec<Placement> = Vec::new();
    for (k, p) in plates.iter().enumerate() {
        let count = plates.iter().filter(|q| q.set == p.set).count();
        let name = match p.variant {
            None if count == 1 => "Test coupons".to_owned(),
            None => format!("Test coupons {}", p.number),
            Some(v) if several_sets => format!("{} {}", v.title(), p.number),
            Some(_) if count == 1 => "Parts".to_owned(),
            Some(_) => format!("Parts {}", p.number),
        };
        let mut mine = Vec::new();
        let mut extent = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for (i, spot) in &p.placed {
            let piece = &pieces[*i];
            let fp = footprint_of(&shapes[*i], spot);
            let px = opts.region[0] + (spot.cx as f64 - grid.pad as f64) * opts.cell;
            let py = opts.region[1] + (spot.cy as f64 - grid.pad as f64) * opts.cell;
            let shift = Vec3::new(px - fp.origin.0, py - fp.origin.1, 0.0);
            let spin = Transform::rotation(Vec3::ZERO, Vec3::Z, fp.spin.to_radians());
            let transform = piece.candidates[spot.cand].transform.then(spin).then(Transform::translation(shift));
            let footprint = [fp.extent[0] + shift.x, fp.extent[1] + shift.y, fp.extent[2] + shift.x, fp.extent[3] + shift.y];
            extent = [extent[0].min(footprint[0]), extent[1].min(footprint[1]), extent[2].max(footprint[2]), extent[3].max(footprint[3])];
            placements.push(Placement {
                piece: *i,
                plate: k,
                transform,
                candidate: spot.cand,
                spin: fp.spin,
                footprint,
                need: shapes[*i].need,
                area: fp.area,
                spans: shapes[*i].spans.clone(),
            });
            mine.push(placements.len() - 1);
        }
        out_plates.push(Plate {
            name,
            variant: p.variant,
            number: p.number,
            placements: mine,
            colours: p.colours.clone(),
            colour_changes: 0,
            layers: 0,
            tower: p.tower.map(|t| grid.cells_mm(t)),
            tower_height: 0.0,
            extent,
            closest: None,
            oversize: p.oversize,
        });
    }
    Packed { plates: out_plates, placements, unpinned, floors }
}
