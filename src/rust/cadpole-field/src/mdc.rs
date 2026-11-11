//! Manifold dual contouring: field to watertight triangle mesh.
//!
//! # What it guarantees
//!
//! * **Watertight and closed.** Every triangle edge is shared by exactly two
//!   triangles with opposite orientation, which is what `Mesh::is_watertight`
//!   and the manifold mesh kernel both demand. This is structural, not
//!   heuristic: a quad is emitted per cut grid edge, and each quad side is
//!   shared with exactly one other quad through the grid face it lies on.
//! * **Edge- and vertex-manifold.** A cell gets *one vertex per connected
//!   component of the surface inside it*, not one vertex outright. Components
//!   are found by union-find over the cell's twelve edges, joined through the
//!   six faces; an ambiguous face (diagonally opposite corners agreeing) is
//!   resolved by sampling its centre. Both cells sharing a face sample the
//!   same centre and the same four corners, so they always agree on how that
//!   face's cut edges pair up — which is exactly the condition that makes the
//!   dual surface manifold rather than pinched (Ju et al. 2002; Schaefer,
//!   Ju & Warren, TVCG 2007).
//! * **Sharp features.** Each vertex minimises the quadratic error function
//!   over the Hermite data (crossing point + gradient) of its own component,
//!   solved through a truncated eigen-decomposition so a planar patch, a
//!   crease and a corner each get the right number of constrained directions.
//!   A minimiser inside its own cell stays exactly where it is; one that
//!   falls **outside** is clamped into the cell inset by a twentieth of a
//!   cell, not onto the cell wall: the neighbour across that wall clamps the
//!   same way, two vertices clamped onto one wall land on the same point (a
//!   neck or a crease thinner than a cell does exactly that), and two
//!   distinct vertices at one point weld into a non-manifold seam downstream.
//! * **Deterministic.** Cells are visited in a fixed index order, the octree
//!   descent is sequential, and nothing is keyed on hash iteration order.
//!
//! # Measured, then repaired, then refused
//!
//! None of the above is a proof that the surface is embedded: Ju & Udeshi's
//! intersection-free construction needs more than a cell clamp, and the
//! adaptive collapse is a local test. So [`contour`] measures its own output
//! the way `cadpole_core::solid::validate` will — every edge used once in each
//! direction (a count, not a signed sum, which an edge with four triangles
//! would pass), every vertex a single fan, no two distinct vertices within
//! weld distance, no triangles crossing or overlapping — and repairs what it
//! finds: a collapsed node on a defect is split back into its fine cells, and
//! a fine vertex retreats to the mass point of its crossings and then towards
//! its cell centre. What is still wrong after that is an error naming the
//! defect and where it is, never a returned mesh. The pairwise crossing search
//! is skipped above 250 000 triangles, as `solid::validate` skips it; the
//! counts are in [`ContourStats`].
//!
//! # Adaptivity
//!
//! The octree is descended with interval arithmetic, so a cell whose range
//! does not straddle zero is never subdivided and never sampled — the surface
//! band is the only thing that costs anything. With `error > 0` a second,
//! bottom-up pass collapses a node into one vertex when a single plane
//! through that vertex reproduces every sign in its subtree and the QEF
//! residual is within the threshold; the fine cells underneath then share
//! that one vertex, which turns their quads into triangles or drops them
//! entirely without ever opening a crack.

use std::collections::{HashMap, HashSet};

use cadpole_core::time::Stopwatch;

use crate::interval::Box3;
use crate::tape::{Evaluator, Tape};
use cadpole_core::fmath;

/// Corner `i` of a cell sits at `+ (i&1, (i>>1)&1, (i>>2)&1) * size`.
const CORNER_OFFSET: [[u8; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];

/// The twelve cell edges as (corner, corner). 0-3 run along x, 4-7 along y,
/// 8-11 along z.
const EDGE_CORNERS: [[u8; 2]; 12] = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

/// The six cell faces as (axis, side, four corners in cyclic order, four
/// edges in the matching order). Used to join cut edges into components.
const FACE_CORNERS: [[u8; 4]; 6] = [
    [0, 2, 6, 4], // -x
    [1, 5, 7, 3], // +x
    [0, 4, 5, 1], // -y
    [2, 3, 7, 6], // +y
    [0, 1, 3, 2], // -z
    [4, 6, 7, 5], // +z
];

#[derive(Clone, Copy, Debug)]
pub struct ContourOptions {
    /// Target cell size in mm at the finest octree level.
    pub resolution: f64,
    /// QEF residual (mm) below which a node may be collapsed. 0 keeps the
    /// uniform finest grid.
    pub error: f64,
    /// Explicit bounds; defaults to the tape's own, grown a little.
    pub bounds: Option<Box3>,
    /// Refuse rather than allocate past this many finest-level cells.
    pub max_cells: usize,
}

impl Default for ContourOptions {
    fn default() -> Self {
        ContourOptions { resolution: 0.5, error: 0.0, bounds: None, max_cells: 3_000_000 }
    }
}

/// Deepest octree, so a runaway `resolution=` is a diagnostic rather than a
/// machine that stops responding. 2^13 cells across the longest side.
pub const MAX_DEPTH: u32 = 13;

/// Fraction of a cell the interval-culling box is grown by, so rounding can
/// never cull a cell the surface is tangent to. See the comment at the
/// descent.
const CULL_PAD: f64 = 1e-6;

/// Fraction of a cell below which a lattice sample counts as on the surface
/// (see `Grid::sample_half`). Far below anything the grid can resolve, far
/// above the rounding noise of a distance to triangles.
const SNAP: f64 = 1e-6;

/// Fraction of a cell an out-of-cell QEF minimiser is kept away from the
/// cell's walls (see `solve_qef`), so vertices of neighbouring cells stay at
/// least a tenth of a cell apart.
const CLAMP_INSET: f64 = 0.05;

#[derive(Clone, Debug, Default)]
pub struct ContourStats {
    pub depth: u32,
    pub cell_size: f64,
    pub grid: [u32; 3],
    /// Finest-level cells the octree actually visited (the surface band).
    pub surface_cells: usize,
    /// Octree nodes examined, at every level.
    pub nodes_visited: usize,
    /// Nodes rejected outright by interval arithmetic.
    pub nodes_culled: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Cells that had to be split into more than one vertex to stay manifold.
    pub split_cells: usize,
    /// Fine cells whose vertex came from a collapsed ancestor.
    pub simplified_cells: usize,
    pub point_evals: usize,
    pub interval_evals: usize,
    pub ms: f64,
    /// Edges not used by exactly two triangles, once in each direction.
    /// Zero, or the contour is refused.
    pub non_manifold_edges: usize,
    /// Vertices whose triangles do not form one closed fan. Zero, or refused.
    pub non_manifold_vertices: usize,
    /// Pairs of distinct vertices close enough that a weld would merge them
    /// (which would make a non-manifold seam). Zero, or refused.
    pub coincident_vertices: usize,
    /// Pairs of triangles that pass through each other. Zero, or refused.
    pub crossing_pairs: usize,
    /// Pairs of coplanar triangles that share area. Zero, or refused.
    pub overlapping_pairs: usize,
    /// False when the mesh was over the pairwise-search limit, so
    /// `crossing_pairs` and `overlapping_pairs` were not measured.
    pub pairs_checked: bool,
    /// Vertex moves the repair pass made to clear a defect.
    pub repaired_vertices: usize,
    /// Collapsed nodes the repair pass split back into their fine cells.
    pub restored_nodes: usize,
    /// Cells the octree descent culled although the surface crosses their
    /// edges (the field's interval bound was too tight there), built
    /// afterwards so the surface stays closed.
    pub filled_cells: usize,
    /// Cut grid edges whose quad could not be built because one of the four
    /// cells around it was clipped by the grid boundary. Must be zero: a
    /// dropped quad is a hole.
    pub dropped_quads: usize,
}

/// A contoured surface, in the form the rest of Cadpole converts from.
#[derive(Clone, Debug, Default)]
pub struct ContourMesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub stats: ContourStats,
}

impl ContourMesh {
    pub fn volume(&self) -> f64 {
        let mut v = 0.0;
        for t in &self.tris {
            let (a, b, c) =
                (self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]);
            v += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        v / 6.0
    }

    pub fn area(&self) -> f64 {
        let mut s = 0.0;
        for t in &self.tris {
            let (a, b, c) =
                (self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            s += (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
        }
        s
    }

    /// True when every edge is used by exactly two triangles, once in each
    /// direction.
    pub fn is_watertight(&self) -> bool {
        self.non_manifold_edges() == 0
    }

    /// Edges whose directed uses do not cancel: holes and same-way pairs.
    /// Blind to an edge with four triangles, two each way; that is what
    /// [`non_manifold_edges`](Self::non_manifold_edges) is for.
    pub fn open_edges(&self) -> usize {
        let mut m: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = if a < b { (a, b) } else { (b, a) };
                *m.entry(key).or_insert(0) += if a < b { 1 } else { -1 };
            }
        }
        m.values().filter(|&&c| c != 0).count()
    }

    /// Edges not used exactly once in each direction: holes, same-way pairs,
    /// and seams with three or more triangles.
    pub fn non_manifold_edges(&self) -> usize {
        bad_edges(&self.tris).len()
    }

    /// Everything the mesher promises, measured: closed and 2-manifold at
    /// every edge and vertex, no two distinct vertices within weld distance,
    /// no triangles crossing or overlapping (when the mesh is small enough
    /// for the pairwise search). `None` when it holds, else what failed.
    pub fn defects(&self) -> Option<String> {
        let d = find_defects(&self.positions, &self.tris);
        let edges = self.non_manifold_edges();
        let verts = self.non_manifold_vertices().len();
        if d.vertices.is_empty() && edges == 0 && verts == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if edges > 0 {
            parts.push(format!("{edges} non-manifold or open edges"));
        }
        if verts > 0 {
            parts.push(format!("{verts} pinched vertices"));
        }
        if d.coincident > 0 {
            parts.push(format!("{} pairs of distinct vertices within {COINCIDENT_EPS} mm", d.coincident));
        }
        if d.crossings > 0 {
            parts.push(format!("{} pairs of triangles crossing", d.crossings));
        }
        if d.overlaps > 0 {
            parts.push(format!("{} pairs of coplanar triangles overlapping", d.overlaps));
        }
        let at = d.vertices.first().map(|&v| self.positions[v as usize]).unwrap_or([0.0; 3]);
        Some(format!("{}, the first near ({:.3}, {:.3}, {:.3})", parts.join(", "), at[0], at[1], at[2]))
    }

    /// True when the triangles around every vertex form a single closed fan.
    /// Watertightness alone does not imply this: two cones meeting at a point
    /// are closed but not a 2-manifold there.
    pub fn is_vertex_manifold(&self) -> bool {
        self.non_manifold_vertices().is_empty()
    }

    pub fn non_manifold_vertices(&self) -> Vec<u32> {
        non_manifold_vertices_of(&self.tris)
    }
}

fn non_manifold_vertices_of(tris: &[[u32; 3]]) -> Vec<u32> {
    // Per vertex, the directed link edges (opposite side of each incident
    // triangle). The link is a single cycle iff following `next` from any
    // start returns to it after using every edge.
    let mut link: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let v = t[k];
            let a = t[(k + 1) % 3];
            let b = t[(k + 2) % 3];
            link.entry(v).or_default().push((a, b));
        }
    }
    let mut bad: Vec<u32> = Vec::new();
    for (v, edges) in &link {
        let mut next: HashMap<u32, u32> = HashMap::with_capacity(edges.len());
        let mut ok = true;
        for &(a, b) in edges {
            if next.insert(a, b).is_some() {
                ok = false; // two triangles leave the same link vertex
                break;
            }
        }
        if ok {
            let start = edges[0].0;
            let mut cur = start;
            let mut n = 0usize;
            loop {
                let Some(&nx) = next.get(&cur) else {
                    ok = false;
                    break;
                };
                cur = nx;
                n += 1;
                if cur == start {
                    break;
                }
                if n > edges.len() {
                    ok = false;
                    break;
                }
            }
            if ok && n != edges.len() {
                ok = false; // more than one cycle: a pinch point
            }
        }
        if !ok {
            bad.push(*v);
        }
    }
    bad.sort_unstable();
    bad
}

/// Hermite data for one cut edge: where the surface crosses and which way it
/// faces there.
#[derive(Clone, Copy, Debug)]
struct Hermite {
    p: [f64; 3],
    n: [f64; 3],
}

#[derive(Clone, Debug)]
struct CellData {
    /// Bit `i` set when corner `i` is inside (field < 0).
    signs: u8,
    /// One vertex index per component, in component order.
    verts: Vec<u32>,
    /// Which component owns each of the twelve edges; 255 = not cut.
    owner: [u8; 12],
}

/// The sampling grid.
///
/// Samples are addressed on a **half**-integer lattice — a cell corner is at
/// an even index, a face or cell centre at an odd one. That is not a detail:
/// two cells sharing a face must get bit-identical values at that face's
/// centre, or they can disagree about how its cut edges pair up and the dual
/// surface tears. Averaging the four corner positions would compute the same
/// point in a different summation order on each side, and float addition is
/// not associative; indexing the lattice makes both sides ask for the same
/// integer triple and hit the same cache entry.
struct Grid<'a, 'b> {
    ev: &'a mut Evaluator<'b>,
    origin: [f64; 3],
    h: f64,
    samples: HashMap<(i32, i32, i32), f64>,
    point_evals: usize,
    /// Lattice samples closer to zero than this read as exactly zero.
    snap: f64,
}

impl<'a, 'b> Grid<'a, 'b> {
    /// World position of a cell corner.
    #[inline]
    fn pos(&self, c: (i32, i32, i32)) -> [f64; 3] {
        self.pos_half((c.0 * 2, c.1 * 2, c.2 * 2))
    }

    /// World position of a half-lattice point.
    #[inline]
    fn pos_half(&self, c: (i32, i32, i32)) -> [f64; 3] {
        [
            self.origin[0] + c.0 as f64 * self.h * 0.5,
            self.origin[1] + c.1 as f64 * self.h * 0.5,
            self.origin[2] + c.2 as f64 * self.h * 0.5,
        ]
    }

    fn corner(&mut self, c: (i32, i32, i32)) -> f64 {
        self.sample_half((c.0 * 2, c.1 * 2, c.2 * 2))
    }

    fn sample_half(&mut self, c: (i32, i32, i32)) -> f64 {
        if let Some(v) = self.samples.get(&c) {
            return *v;
        }
        let p = self.pos_half(c);
        let mut v = self.ev.eval(p);
        // A sample within `snap` of zero is *on* the surface, and on the
        // surface counts as outside, the same for every cell that reads it.
        // Without this a face that lies on a grid plane -- a box from a
        // body, at round coordinates -- samples as +1e-9 at one corner and
        // -1e-9 at the next, the "surface" zigzags through the plane, and
        // the cells on both sides of it put a vertex at the same point.
        if v.abs() <= self.snap {
            v = 0.0;
        }
        self.point_evals += 1;
        self.samples.insert(c, v);
        v
    }

    fn at(&mut self, p: [f64; 3]) -> f64 {
        self.point_evals += 1;
        self.ev.eval(p)
    }
}

/// Contour a compiled field.
pub fn contour(tape: &Tape, opts: &ContourOptions) -> Result<ContourMesh, String> {
    let watch = Stopwatch::start();
    let bounds = opts
        .bounds
        .or_else(|| tape.bounds())
        .ok_or_else(|| "the field is unbounded, so it cannot be meshed; intersect it with a box or pass explicit bounds".to_owned())?;
    let h = opts.resolution;
    if !(h > 0.0) || !h.is_finite() {
        return Err(format!("resolution must be a positive number of millimetres, got {h}"));
    }
    // A margin of two cells so the surface never touches the grid boundary,
    // where a cell has no neighbour to close the mesh against.
    let bounds = bounds.expanded(2.0 * h);
    let extent = (bounds.x.width()).max(bounds.y.width()).max(bounds.z.width());
    if !(extent > 0.0) {
        return Err("the field is empty (its bounds have no extent)".to_owned());
    }
    let want_depth = fmath::log2(extent / h).ceil();
    if !(want_depth <= MAX_DEPTH as f64) {
        return Err(format!(
            "resolution {h} mm over a {extent:.1} mm body needs more than {}^3 cells; \
             use a coarser resolution or smaller bounds",
            1u32 << MAX_DEPTH
        ));
    }
    let depth = (want_depth as i64).clamp(1, MAX_DEPTH as i64) as u32;
    let n = 1i32 << depth;
    let side = h * n as f64;
    let c = bounds.center();
    let origin = [c[0] - side * 0.5, c[1] - side * 0.5, c[2] - side * 0.5];

    let mut ev = tape.evaluator();
    let mut interval_evals = 0usize;
    let mut nodes_visited = 0usize;
    let mut nodes_culled = 0usize;

    // ---- 1. octree descent with interval culling
    // (cell index, depth). A node covers cells [i*2^(depth-d) .. +2^(depth-d)).
    let mut leaves: Vec<(i32, i32, i32)> = Vec::new();
    let mut stack: Vec<((i32, i32, i32), u32)> = vec![((0, 0, 0), 0)];
    while let Some(((i, j, k), d)) = stack.pop() {
        nodes_visited += 1;
        let step = 1i32 << (depth - d);
        // The box has to be built from the *indices*, exactly as the corner
        // sampler builds corner positions, and then padded.
        //
        // Both matter. `origin + i*h + step*h` and `origin + (i+step)*h` are
        // not the same float, and a node whose upper face lands a few ulps
        // short of a surface that is tangent to it gets an interval like
        // `[-0.6, -1.3e-15]` and is culled — taking a whole plateau of the
        // model with it, because its children are never visited and the
        // quads dual to their edges then have nowhere to attach. The pad
        // covers whatever slack is left in the interval rules themselves; a
        // cell that is admitted without a sign change simply drops out at
        // the corner-sign test below, so padding costs work and never
        // correctness.
        let b = Box3::new(
            [
                origin[0] + i as f64 * h,
                origin[1] + j as f64 * h,
                origin[2] + k as f64 * h,
            ],
            [
                origin[0] + (i + step) as f64 * h,
                origin[1] + (j + step) as f64 * h,
                origin[2] + (k + step) as f64 * h,
            ],
        )
        .expanded(h * CULL_PAD);
        interval_evals += 1;
        if !ev.eval_box(b).contains_zero() {
            nodes_culled += 1;
            continue;
        }
        if d == depth {
            leaves.push((i, j, k));
            if leaves.len() > opts.max_cells {
                return Err(format!(
                    "resolution {h} mm puts more than {} cells on the surface of this body; \
                     use a coarser resolution",
                    opts.max_cells
                ));
            }
            continue;
        }
        let half = step / 2;
        // Push in reverse so children are popped in index order: the whole
        // traversal is then a pure function of the field.
        for c in (0..8).rev() {
            let o = CORNER_OFFSET[c];
            stack.push((
                (i + o[0] as i32 * half, j + o[1] as i32 * half, k + o[2] as i32 * half),
                d + 1,
            ));
        }
    }
    leaves.sort_unstable();

    let mut grid = Grid { ev: &mut ev, origin, h, samples: HashMap::new(), point_evals: 0, snap: h * SNAP };

    // ---- 2. per-cell components and QEF vertices
    let mut cells: HashMap<(i32, i32, i32), CellData> = HashMap::new();
    let mut verts = Verts::default();
    for &cell in &leaves {
        if let Some(data) = build_cell(&mut grid, cell, &mut verts) {
            cells.insert(cell, data);
        }
    }

    // ---- 2b. close the band
    //
    // Every cut grid edge needs all four cells around it, or its quad is
    // dropped and the surface has a hole. The descent finds them when the
    // field's interval bound is sound; a bound that is a little too tight
    // (a blend correction, a displacement) culls a cell the surface does
    // pass through. So walk the cut edges and build any neighbour the
    // descent missed, rather than trusting the bound with the topology.
    let mut queue: std::collections::VecDeque<(i32, i32, i32)> = {
        let mut k: Vec<(i32, i32, i32)> = cells.keys().copied().collect();
        k.sort_unstable();
        k.into()
    };
    let mut tried: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut filled_cells = 0usize;
    while let Some(cell) = queue.pop_front() {
        let owner = cells[&cell].owner;
        for e in 0..12 {
            if owner[e] == 255 {
                continue;
            }
            let axis = e / 4;
            let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
            let [a, _] = EDGE_CORNERS[e];
            let oa = CORNER_OFFSET[a as usize];
            let base = [cell.0 + oa[0] as i32, cell.1 + oa[1] as i32, cell.2 + oa[2] as i32];
            for off in QUAD_OFFSETS {
                let mut idx = base;
                idx[b] += off[0];
                idx[c] += off[1];
                let nb = (idx[0], idx[1], idx[2]);
                if idx.iter().any(|&i| i < 0 || i >= n) {
                    // The surface reaches the edge of the grid, which is the
                    // field's own bounds plus two cells: the bounds are
                    // wrong, and the mesh would be cut open here.
                    let p = grid.pos(cell);
                    return Err(format!(
                        "the field's surface runs past its declared bounds near ({:.3}, {:.3}, {:.3}), \
                         so meshing it would leave a hole there: the bounds the field \
                         claims are too tight for it",
                        p[0], p[1], p[2]
                    ));
                }
                if cells.contains_key(&nb) || !tried.insert(nb) {
                    continue;
                }
                if leaves.len() + filled_cells >= opts.max_cells {
                    return Err(format!(
                        "resolution {h} mm puts more than {} cells on the surface of this body; \
                         use a coarser resolution",
                        opts.max_cells
                    ));
                }
                if let Some(data) = build_cell(&mut grid, nb, &mut verts) {
                    cells.insert(nb, data);
                    filled_cells += 1;
                    queue.push_back(nb);
                }
            }
        }
    }
    let Verts { mut positions, mut normals, hermite: vertex_hermite, cell_lo: vertex_cell, split_cells } = verts;

    // ---- 3. adaptive simplification
    let fine_vertices = positions.len();
    let mut simplified_cells = 0usize;
    let mut remap: Vec<u32> = (0..positions.len() as u32).collect();
    if opts.error > 0.0 {
        simplified_cells = simplify(
            &cells,
            &mut positions,
            &mut normals,
            &vertex_hermite,
            &mut remap,
            origin,
            h,
            depth,
            opts.error,
        );
    }

    // ---- 4. quads over every cut grid edge, checked and repaired
    //
    // The cell clamp and the collapse test are local arguments, and a local
    // argument cannot see two vertices of different cells landing on the
    // same point, or a quad of one cell folding through a quad of the next.
    // So the result is *measured*, the same way `cadpole_core::solid` will
    // measure it downstream, and every vertex on a defect is moved back to a
    // safer place: a collapsed node is split back into its fine cells, and a
    // fine cell's QEF vertex retreats to the mass point of its crossings and
    // then towards the centre of its cell. Each step only ever trades
    // sharpness for safety, and the uniform, mass-point mesh is the fallback
    // the loop converges on. Whatever is still wrong after that is refused.
    let mut stage = vec![0u8; fine_vertices];
    let mut repaired_vertices = 0usize;
    let mut restored_nodes = 0usize;
    let (mut tris, mut dropped_quads, mut defects) = (Vec::new(), 0usize, Defects::default());
    for round in 0..=REPAIR_ROUNDS {
        let (t, d) = emit_all(&cells, &remap, &positions);
        tris = t;
        dropped_quads = d;
        defects = find_defects(&positions, &tris);
        if defects.vertices.is_empty() || round == REPAIR_ROUNDS {
            break;
        }
        let mut moved = false;
        for &v in &defects.vertices {
            let v = v as usize;
            if v >= fine_vertices {
                // A collapsed node: give its fine cells their own vertices back.
                let mut any = false;
                for (f, r) in remap.iter_mut().enumerate().take(fine_vertices) {
                    if *r as usize == v {
                        *r = f as u32;
                        any = true;
                    }
                }
                if any {
                    restored_nodes += 1;
                    moved = true;
                }
                continue;
            }
            let Some(p) = retreat(&vertex_hermite[v], &mut stage[v], positions[v], vertex_cell[v], h) else {
                continue;
            };
            positions[v] = p;
            repaired_vertices += 1;
            moved = true;
        }
        if !moved {
            break;
        }
    }

    // ---- 5. weld, drop vertices no triangle uses, and renumber
    //
    // The output is the welded mesh the defect check measured, so it holds
    // no two distinct vertices a later weld would merge.
    let mut tris = std::mem::take(&mut defects.welded);
    let mut used = vec![u32::MAX; positions.len()];
    let mut out_pos: Vec<[f64; 3]> = Vec::new();
    let mut out_nor: Vec<[f64; 3]> = Vec::new();
    for t in tris.iter_mut() {
        for v in t.iter_mut() {
            let i = *v as usize;
            if used[i] == u32::MAX {
                out_pos.push(positions[i]);
                out_nor.push(normals[i]);
                used[i] = out_pos.len() as u32 - 1;
            }
            *v = used[i];
        }
    }

    let mut mesh = ContourMesh {
        positions: out_pos,
        normals: out_nor,
        tris,
        stats: ContourStats {
            depth,
            cell_size: h,
            grid: [n as u32, n as u32, n as u32],
            surface_cells: leaves.len() + filled_cells,
            filled_cells,
            nodes_visited,
            nodes_culled,
            split_cells,
            simplified_cells,
            dropped_quads,
            point_evals: grid.point_evals,
            interval_evals,
            repaired_vertices,
            restored_nodes,
            coincident_vertices: 0,
            crossing_pairs: defects.crossings,
            overlapping_pairs: defects.overlaps,
            pairs_checked: defects.pairs_checked,
            ..Default::default()
        },
    };
    mesh.stats.vertices = mesh.positions.len();
    mesh.stats.triangles = mesh.tris.len();
    mesh.stats.non_manifold_edges = mesh.non_manifold_edges();
    mesh.stats.non_manifold_vertices = mesh.non_manifold_vertices().len();
    let s = &mesh.stats;
    if s.non_manifold_edges + s.non_manifold_vertices + s.coincident_vertices + s.crossing_pairs + s.overlapping_pairs > 0 {
        return Err(format!(
            "meshing at resolution {h} mm could not make a closed, non-self-intersecting \
             2-manifold surface: {}. The field has a feature (a gap, a neck or a wall) \
             about one cell thick here; a finer resolution resolves it",
            mesh.defects().unwrap_or_else(|| "the repair pass ran out of rounds".to_owned())
        ));
    }
    mesh.stats.ms = watch.elapsed_ms();
    Ok(mesh)
}

/// The four cells around a grid edge along `axis`, as offsets in the two
/// other axes, wound so the quad's normal is +axis.
const QUAD_OFFSETS: [[i32; 2]; 4] = [[-1, -1], [0, -1], [0, 0], [-1, 0]];

/// The fine vertices as they are built, one per surface component per cell.
#[derive(Default)]
struct Verts {
    positions: Vec<[f64; 3]>,
    normals: Vec<[f64; 3]>,
    /// Hermite data per vertex, kept for the simplification and repair.
    hermite: Vec<Vec<Hermite>>,
    /// The low corner of each vertex's cell, for the repair pass.
    cell_lo: Vec<[f64; 3]>,
    /// Cells that had to be split into more than one vertex.
    split_cells: usize,
}

/// Sample one cell, find its surface components, and place one QEF vertex
/// per component. `None` when the surface does not cross the cell's edges.
fn build_cell(grid: &mut Grid<'_, '_>, cell: (i32, i32, i32), out: &mut Verts) -> Option<CellData> {
    let h = grid.h;
    let mut corner_val = [0.0f64; 8];
    let mut signs = 0u8;
    for c in 0..8 {
        let o = CORNER_OFFSET[c];
        let v = grid.corner((cell.0 + o[0] as i32, cell.1 + o[1] as i32, cell.2 + o[2] as i32));
        corner_val[c] = v;
        if v < 0.0 {
            signs |= 1 << c;
        }
    }
    if signs == 0 || signs == 0xff {
        return None; // the interval straddled zero but the corners do not
    }
    let comps = components(grid, cell, signs);
    let mut owner = [255u8; 12];
    let mut per_comp: Vec<Vec<Hermite>> = vec![Vec::new(); comps.count];
    for e in 0..12 {
        let Some(ci) = comps.of_edge[e] else { continue };
        owner[e] = ci as u8;
        let [a, b] = EDGE_CORNERS[e];
        let oa = CORNER_OFFSET[a as usize];
        let ob = CORNER_OFFSET[b as usize];
        let pa = grid.pos((cell.0 + oa[0] as i32, cell.1 + oa[1] as i32, cell.2 + oa[2] as i32));
        let pb = grid.pos((cell.0 + ob[0] as i32, cell.1 + ob[1] as i32, cell.2 + ob[2] as i32));
        let p = crossing(grid, pa, corner_val[a as usize], pb, corner_val[b as usize]);
        let n = grid.ev.gradient(p, h * 0.05);
        grid.point_evals += 6;
        per_comp[ci].push(Hermite { p, n });
    }
    let lo = grid.pos(cell);
    let mut verts = Vec::with_capacity(comps.count);
    for hs in per_comp {
        if hs.is_empty() {
            continue;
        }
        let (p, _err) = solve_qef(&hs, lo, h);
        let mut n = [0.0f64; 3];
        for hm in &hs {
            for t in 0..3 {
                n[t] += hm.n[t];
            }
        }
        out.positions.push(p);
        out.normals.push(unit(n));
        out.hermite.push(hs);
        out.cell_lo.push(lo);
        verts.push(out.positions.len() as u32 - 1);
    }
    if verts.is_empty() {
        return None;
    }
    if verts.len() > 1 {
        out.split_cells += 1;
    }
    // `owner` indexes the component list; components with no cut edge are
    // impossible, so the two lists line up.
    Some(CellData { signs, verts, owner })
}

/// Which component each cut edge of a cell belongs to.
struct Components {
    of_edge: [Option<usize>; 12],
    count: usize,
}

/// Union-find over the twelve edges, joined face by face. On a face with four
/// cut edges (diagonally opposite corners agreeing) the pairing is decided by
/// the sign at the face centre — a value both neighbouring cells compute
/// identically, which is what keeps the dual surface manifold across it.
fn components(grid: &mut Grid<'_, '_>, cell: (i32, i32, i32), signs: u8) -> Components {
    let cut = |e: usize| {
        let [a, b] = EDGE_CORNERS[e];
        ((signs >> a) & 1) != ((signs >> b) & 1)
    };
    let mut parent: [usize; 12] = [0; 12];
    for (i, p) in parent.iter_mut().enumerate() {
        *p = i;
    }
    fn find(parent: &mut [usize; 12], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    fn union(parent: &mut [usize; 12], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }

    for f in 0..6 {
        let c = FACE_CORNERS[f];
        // The four edges of the face, in the same cyclic order: edge k joins
        // corner c[k] to c[(k+1)%4].
        let mut fe = [0usize; 4];
        for k in 0..4 {
            let (u, v) = (c[k], c[(k + 1) % 4]);
            fe[k] = (0..12)
                .find(|&e| {
                    let [a, b] = EDGE_CORNERS[e];
                    (a == u && b == v) || (a == v && b == u)
                })
                .expect("every face side is a cell edge");
        }
        let cuts: Vec<usize> = (0..4).filter(|&k| cut(fe[k])).collect();
        match cuts.len() {
            2 => union(&mut parent, fe[cuts[0]], fe[cuts[1]]),
            4 => {
                // Ambiguous: corners c[0], c[2] share a sign and c[1], c[3]
                // share the other. The face centre says which pair the
                // surface separates -- and it must be *the same number* from
                // both sides of the face, so it is addressed on the half
                // lattice rather than averaged.
                let mut sum = [0i32; 3];
                for k in 0..4 {
                    let o = CORNER_OFFSET[c[k] as usize];
                    for t in 0..3 {
                        sum[t] += o[t] as i32;
                    }
                }
                let mid = grid.sample_half((
                    cell.0 * 2 + sum[0] / 2,
                    cell.1 * 2 + sum[1] / 2,
                    cell.2 * 2 + sum[2] / 2,
                ));
                let inside0 = ((signs >> c[0]) & 1) == 1;
                // If the centre agrees with c[0]/c[2], those two corners are
                // connected through the middle of the face, so the surface
                // separates c[1] and c[3]: the edges around c[1] pair up, and
                // the edges around c[3] pair up.
                if (mid < 0.0) == inside0 {
                    union(&mut parent, fe[0], fe[1]); // both touch c[1]
                    union(&mut parent, fe[2], fe[3]); // both touch c[3]
                } else {
                    union(&mut parent, fe[1], fe[2]); // both touch c[2]
                    union(&mut parent, fe[3], fe[0]); // both touch c[0]
                }
            }
            _ => {}
        }
    }

    let mut of_edge: [Option<usize>; 12] = [None; 12];
    let mut label: HashMap<usize, usize> = HashMap::new();
    let mut count = 0;
    for e in 0..12 {
        if !cut(e) {
            continue;
        }
        let r = find(&mut parent, e);
        let id = *label.entry(r).or_insert_with(|| {
            let i = count;
            count += 1;
            i
        });
        of_edge[e] = Some(id);
    }
    Components { of_edge, count }
}

/// The surface crossing on a cut edge, by the Illinois variant of regula
/// falsi (superlinear, and it never leaves the bracket).
fn crossing(grid: &mut Grid<'_, '_>, pa: [f64; 3], va: f64, pb: [f64; 3], vb: f64) -> [f64; 3] {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let (mut flo, mut fhi) = (va, vb);
    let lerp = |t: f64| {
        [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t, pa[2] + (pb[2] - pa[2]) * t]
    };
    if flo == 0.0 {
        return pa;
    }
    if fhi == 0.0 {
        return pb;
    }
    let mut t = 0.5;
    for _ in 0..24 {
        let denom = fhi - flo;
        t = if denom.abs() > 1e-300 { lo - flo * (hi - lo) / denom } else { 0.5 * (lo + hi) };
        // Keep the estimate strictly inside the bracket.
        let span = hi - lo;
        t = t.clamp(lo + span * 1e-4, hi - span * 1e-4);
        let v = grid.at(lerp(t));
        if v == 0.0 {
            return lerp(t);
        }
        if (v < 0.0) == (flo < 0.0) {
            lo = t;
            flo = v;
            fhi *= 0.5; // Illinois: stop one side from stalling
        } else {
            hi = t;
            fhi = v;
            flo *= 0.5;
        }
        if hi - lo < 1e-7 {
            break;
        }
    }
    lerp(t)
}

/// Minimise `sum (n_i . (x - p_i))^2`, starting from the mass point and
/// solving only in the directions the data actually constrains. Returns the
/// point clamped into the cell `[lo, lo + h]^3`, and the residual there.
fn solve_qef(hs: &[Hermite], lo: [f64; 3], h: f64) -> ([f64; 3], f64) {
    let mut mass = [0.0f64; 3];
    for hm in hs {
        for t in 0..3 {
            mass[t] += hm.p[t];
        }
    }
    let inv = 1.0 / hs.len() as f64;
    for t in 0..3 {
        mass[t] *= inv;
    }
    // Normal equations about the mass point: A (x - m) = b - A m.
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for hm in hs {
        let d = [hm.p[0] - mass[0], hm.p[1] - mass[1], hm.p[2] - mass[2]];
        let r = hm.n[0] * d[0] + hm.n[1] * d[1] + hm.n[2] * d[2];
        for i in 0..3 {
            atb[i] += hm.n[i] * r;
            for j in 0..3 {
                ata[i][j] += hm.n[i] * hm.n[j];
            }
        }
    }
    let (vals, vecs) = jacobi_eigen(ata);
    let max = vals.iter().fold(0.0f64, |a, &b| a.max(b.abs()));
    // Truncate the directions the data barely constrains: that is what makes
    // a planar patch slide to the mass point instead of shooting off, and a
    // corner stay put.
    let cutoff = max * 1e-3;
    let mut delta = [0.0f64; 3];
    for k in 0..3 {
        if vals[k].abs() <= cutoff {
            continue;
        }
        let v = [vecs[0][k], vecs[1][k], vecs[2][k]];
        let c = (v[0] * atb[0] + v[1] * atb[1] + v[2] * atb[2]) / vals[k];
        for t in 0..3 {
            delta[t] += v[t] * c;
        }
    }
    let mut x = [mass[0] + delta[0], mass[1] + delta[1], mass[2] + delta[2]];
    // Clamped into its own cell: the guard against a folded dual surface.
    // A minimiser that is already inside stays exactly where it is (that is
    // what keeps corners on grid planes exact). One that is outside is
    // clamped into the cell *inset* by `CLAMP_INSET`, not onto its wall:
    // the neighbour across that wall clamps the same way, and two vertices
    // clamped onto one wall from both sides land on the same point -- which
    // is what a crease or a neck thinner than a cell does -- and weld into a
    // non-manifold seam downstream.
    // Per axis: only an axis the minimiser is outside on, by more than the
    // lattice snap, is pulled into the inset; the others keep the plain
    // clamp, so a vertex on a feature lying on a cell wall stays on it.
    let slack = h * SNAP;
    for t in 0..3 {
        let outside = !(x[t] >= lo[t] - slack && x[t] <= lo[t] + h + slack);
        let m = if outside { h * CLAMP_INSET } else { 0.0 };
        x[t] = x[t].clamp(lo[t] + m, lo[t] + h - m);
        if !x[t].is_finite() {
            x[t] = lo[t] + h * 0.5;
        }
    }
    let mut err = 0.0;
    for hm in hs {
        let r = hm.n[0] * (x[0] - hm.p[0]) + hm.n[1] * (x[1] - hm.p[1]) + hm.n[2] * (x[2] - hm.p[2]);
        err += r * r;
    }
    (x, (err / hs.len() as f64).sqrt())
}

/// Cyclic Jacobi eigen-decomposition of a symmetric 3x3. Returns the
/// eigenvalues and a matrix whose *columns* are the eigenvectors.
fn jacobi_eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..24 {
        let mut off = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    off += a[i][j] * a[i][j];
                }
            }
        }
        if off < 1e-24 {
            break;
        }
        for p in 0..2 {
            for q in (p + 1)..3 {
                if a[p][q].abs() < 1e-30 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                let mut na = a;
                for k in 0..3 {
                    na[p][k] = c * a[p][k] - s * a[q][k];
                    na[q][k] = s * a[p][k] + c * a[q][k];
                }
                let mut nb = na;
                for k in 0..3 {
                    nb[k][p] = c * na[k][p] - s * na[k][q];
                    nb[k][q] = s * na[k][p] + c * na[k][q];
                }
                a = nb;
                let mut nv = v;
                for k in 0..3 {
                    nv[k][p] = c * v[k][p] - s * v[k][q];
                    nv[k][q] = s * v[k][p] + c * v[k][q];
                }
                v = nv;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

/// One octree node during the bottom-up collapse: the vertex that stands for
/// it, the Hermite data underneath it, and which finest cells it covers.
struct Agg {
    vert: u32,
    hermite: Vec<Hermite>,
    cells: Vec<(i32, i32, i32)>,
}

/// Bottom-up adaptive simplification.
///
/// A node collapses to one vertex only when *every* surface cell beneath it is
/// still represented by a single vertex (a cell that had to split to stay
/// manifold stops the collapse dead), the QEF residual over all its Hermite
/// data is within `error`, and the plane through the collapsed vertex
/// reproduces the sign of every sampled corner underneath it to within the
/// same `error`. That last test is the topological guard: it says the subtree
/// really is one flat disc, so replacing it with one vertex cannot change what
/// the surface is connected to.
///
/// Returns how many finest cells ended up pointing at an ancestor's vertex.
#[allow(clippy::too_many_arguments)]
fn simplify(
    cells: &HashMap<(i32, i32, i32), CellData>,
    positions: &mut Vec<[f64; 3]>,
    normals: &mut Vec<[f64; 3]>,
    hermite: &[Vec<Hermite>],
    remap: &mut Vec<u32>,
    origin: [f64; 3],
    h: f64,
    depth: u32,
    error: f64,
) -> usize {
    // counts[l] = how many finest-level surface cells sit under each node at
    // step 2^l, keyed by the node's lowest cell index.
    let mut counts: Vec<HashMap<(i32, i32, i32), usize>> = Vec::with_capacity(depth as usize + 1);
    {
        let mut cur: HashMap<(i32, i32, i32), usize> = HashMap::with_capacity(cells.len());
        for c in cells.keys() {
            *cur.entry(*c).or_insert(0) += 1;
        }
        counts.push(cur.clone());
        let mut step = 1i32;
        for _ in 0..depth {
            step = step.saturating_mul(2);
            let mut nxt: HashMap<(i32, i32, i32), usize> = HashMap::new();
            for (k, v) in &cur {
                *nxt.entry(node_key(*k, step)).or_insert(0) += v;
            }
            cur = nxt;
            counts.push(cur.clone());
        }
    }

    // Level 0: the cells that did not have to split.
    let mut live: HashMap<(i32, i32, i32), Agg> = HashMap::new();
    for (cell, data) in cells {
        if data.verts.len() != 1 {
            continue;
        }
        let v = data.verts[0];
        live.insert(*cell, Agg { vert: v, hermite: hermite[v as usize].clone(), cells: vec![*cell] });
    }

    let mut simplified = 0usize;
    let mut step = 1i32;
    for level in 1..=depth as usize {
        step = step.saturating_mul(2);
        let mut groups: HashMap<(i32, i32, i32), Vec<(i32, i32, i32)>> = HashMap::new();
        for cell in live.keys() {
            groups.entry(node_key(*cell, step)).or_default().push(*cell);
        }
        let mut keys: Vec<(i32, i32, i32)> = groups.keys().copied().collect();
        keys.sort_unstable();
        let mut next: HashMap<(i32, i32, i32), Agg> = HashMap::new();
        for key in keys {
            let members = groups.get_mut(&key).expect("key came from the map");
            members.sort_unstable();
            let covered: usize = members.iter().map(|m| live[m].cells.len()).sum();
            // Some surface under this node is represented by a split cell, or
            // by a node that already refused to collapse: leave it alone.
            if covered != counts[level].get(&key).copied().unwrap_or(0) {
                continue;
            }
            let mut hs: Vec<Hermite> = Vec::new();
            let mut member_cells: Vec<(i32, i32, i32)> = Vec::new();
            for m in members.iter() {
                let a = &live[m];
                hs.extend_from_slice(&a.hermite);
                member_cells.extend_from_slice(&a.cells);
            }
            if hs.is_empty() {
                continue;
            }
            let lo = [
                origin[0] + key.0 as f64 * h,
                origin[1] + key.1 as f64 * h,
                origin[2] + key.2 as f64 * h,
            ];
            let size = step as f64 * h;
            let (x, err) = solve_qef(&hs, lo, size);
            if err > error {
                continue;
            }
            let mut nsum = [0.0f64; 3];
            for hm in &hs {
                for t in 0..3 {
                    nsum[t] += hm.n[t];
                }
            }
            let n = unit(nsum);
            if !plane_explains_signs(cells, &member_cells, origin, h, x, n, error) {
                continue;
            }
            positions.push(x);
            normals.push(n);
            let nv = positions.len() as u32 - 1;
            remap.push(nv);
            for m in members.iter() {
                remap[live[m].vert as usize] = nv;
            }
            simplified += member_cells.len();
            next.insert(key, Agg { vert: nv, hermite: hs, cells: member_cells });
        }
        if next.is_empty() {
            break;
        }
        live = next;
    }

    // A vertex may have been merged more than once; follow the chain.
    for i in 0..remap.len() {
        let mut v = remap[i];
        let mut guard = 0;
        while remap[v as usize] != v && guard < 64 {
            v = remap[v as usize];
            guard += 1;
        }
        remap[i] = v;
    }
    simplified
}

#[inline]
fn node_key(c: (i32, i32, i32), step: i32) -> (i32, i32, i32) {
    (c.0.div_euclid(step) * step, c.1.div_euclid(step) * step, c.2.div_euclid(step) * step)
}

/// Does one plane through `x` with normal `n` predict the inside/outside of
/// every corner sampled under these cells? Corners closer to the plane than
/// `error` make no claim either way.
fn plane_explains_signs(
    cells: &HashMap<(i32, i32, i32), CellData>,
    member_cells: &[(i32, i32, i32)],
    origin: [f64; 3],
    h: f64,
    x: [f64; 3],
    n: [f64; 3],
    error: f64,
) -> bool {
    for m in member_cells {
        let Some(data) = cells.get(m) else { continue };
        for (c, o) in CORNER_OFFSET.iter().enumerate() {
            let gi = (m.0 + o[0] as i32, m.1 + o[1] as i32, m.2 + o[2] as i32);
            let p = [
                origin[0] + gi.0 as f64 * h,
                origin[1] + gi.1 as f64 * h,
                origin[2] + gi.2 as f64 * h,
            ];
            let s = n[0] * (p[0] - x[0]) + n[1] * (p[1] - x[1]) + n[2] * (p[2] - x[2]);
            if s.abs() <= error {
                continue;
            }
            let inside = (data.signs >> c) & 1 == 1;
            if (s < 0.0) != inside {
                return false;
            }
        }
    }
    true
}


/// Rounds of measure-and-repair before the mesher gives up. Each round only
/// moves vertices towards safer places (see [`retreat`]) or splits collapsed
/// nodes, so the loop is short in practice: one or two rounds.
const REPAIR_ROUNDS: usize = 8;

/// Two distinct vertices closer than this would be welded into one by
/// everything downstream (`cadpole_core::mesh_kernel::WELD_EPS` is 1e-4 mm,
/// and the mesh is stored as f32), turning two sheets into one non-manifold
/// seam. Twice the weld distance, so f32 rounding cannot sneak a pair under.
const COINCIDENT_EPS: f64 = 2e-4;

/// Above this many triangles the pairwise crossing search is skipped, the
/// same limit `cadpole_core::solid::SolidOptions` uses.
const PAIR_LIMIT: usize = 250_000;

/// One quad per cut grid edge, in a fixed order.
fn emit_all(
    cells: &HashMap<(i32, i32, i32), CellData>,
    remap: &[u32],
    positions: &[[f64; 3]],
) -> (Vec<[u32; 3]>, usize) {
    let mut dropped_quads = 0usize;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut seen: HashSet<(u8, i32, i32, i32)> = HashSet::new();
    let mut ordered: Vec<(i32, i32, i32)> = cells.keys().copied().collect();
    ordered.sort_unstable();
    for cell in ordered {
        let data = &cells[&cell];
        for e in 0..12 {
            if data.owner[e] == 255 {
                continue;
            }
            let axis = (e / 4) as u8;
            let [a, _] = EDGE_CORNERS[e];
            let oa = CORNER_OFFSET[a as usize];
            let key = (axis, cell.0 + oa[0] as i32, cell.1 + oa[1] as i32, cell.2 + oa[2] as i32);
            if !seen.insert(key) {
                continue;
            }
            if !emit_quad(cells, remap, positions, key, data.signs, e, &mut tris) {
                dropped_quads += 1;
            }
        }
    }
    (tris, dropped_quads)
}

/// What [`find_defects`] measured: the vertices touching any defect, and the
/// counts by kind.
#[derive(Default)]
struct Defects {
    vertices: Vec<u32>,
    coincident: usize,
    crossings: usize,
    overlaps: usize,
    /// Adjacent triangles facing opposite ways (`cadpole_core::solid::folds`):
    /// closed and manifold, but a zero-thickness fin a later mesh boolean can
    /// read as coincident surface. Repaired like the rest, never refused.
    folds: usize,
    pairs_checked: bool,
    /// The triangles with coincident vertices welded into one (each vertex
    /// replaced by the lowest index within weld distance) and the triangles
    /// that collapsed dropped: the mesh everything downstream will see.
    welded: Vec<[u32; 3]>,
}

/// Measure a contour the way `cadpole_core::solid::validate` will: weld
/// distinct vertices a weld would merge, then look for edges used other than
/// once each way, pinched vertices, and triangles that pass through or lie
/// on top of each other.
///
/// Coincident vertices are not a defect by themselves: the vertices of one
/// fan meeting at a sharp apex or along an edge on a grid plane weld into a
/// correct solid. Two sheets welding into one seam is the defect, and it
/// shows as a non-manifold edge or vertex of the welded mesh, so only those
/// are flagged (every original vertex of a flagged welded one).
fn find_defects(positions: &[[f64; 3]], tris: &[[u32; 3]]) -> Defects {
    let mut d = Defects::default();

    // Weld: union-find over a hash grid of cell COINCIDENT_EPS.
    let mut used = vec![false; positions.len()];
    for t in tris {
        for &v in t {
            used[v as usize] = true;
        }
    }
    let mut parent: Vec<u32> = (0..positions.len() as u32).collect();
    fn root(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    let q = |x: f64| (x / COINCIDENT_EPS).floor() as i64;
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    for (i, p) in positions.iter().enumerate() {
        if used[i] {
            grid.entry((q(p[0]), q(p[1]), q(p[2]))).or_default().push(i as u32);
        }
    }
    let mut keys: Vec<(i64, i64, i64)> = grid.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        for &i in &grid[&k] {
            let p = positions[i as usize];
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(other) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) else { continue };
                        for &j in other {
                            if j <= i {
                                continue;
                            }
                            let r = positions[j as usize];
                            let e = [p[0] - r[0], p[1] - r[1], p[2] - r[2]];
                            let dd = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
                            if dd <= COINCIDENT_EPS * COINCIDENT_EPS {
                                d.coincident += 1;
                                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                                if a != b {
                                    parent[a.max(b) as usize] = a.min(b);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let rep: Vec<u32> = (0..positions.len() as u32).map(|v| root(&mut parent, v)).collect();
    d.welded = tris
        .iter()
        .map(|t| [rep[t[0] as usize], rep[t[1] as usize], rep[t[2] as usize]])
        .filter(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0])
        .collect();

    let mut bad_reps: Vec<u32> = Vec::new();
    for (a, b) in bad_edges(&d.welded) {
        bad_reps.push(a);
        bad_reps.push(b);
    }
    bad_reps.extend(non_manifold_vertices_of(&d.welded));

    // Folds: an adaptive collapse can put a node's one vertex on the far side
    // of a neighbouring cell's, so the quad between them turns over. The
    // index structure stays manifold, so only the normals show it.
    let pos: Vec<cadpole_core::geom::Vec3> = positions.iter().map(|p| cadpole_core::geom::Vec3::new(p[0], p[1], p[2])).collect();
    let folds = cadpole_core::solid::folds(&pos, &d.welded, &[]);
    d.folds = folds.len();
    for f in &folds {
        bad_reps.extend_from_slice(&d.welded[f.tris.0 as usize]);
        bad_reps.extend_from_slice(&d.welded[f.tris.1 as usize]);
    }

    if d.welded.len() <= PAIR_LIMIT {
        d.pairs_checked = true;
        let mut mesh = cadpole_core::tessellate::Mesh::default();
        for p in positions {
            mesh.positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
            mesh.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        for t in &d.welded {
            mesh.indices.extend_from_slice(t);
            mesh.face_ids.push(0);
        }
        let (crossings, overlaps) = cadpole_core::solid::pair_defects(&mesh);
        d.crossings = crossings.len();
        d.overlaps = overlaps.len();
        for pair in crossings.iter().chain(&overlaps) {
            bad_reps.extend_from_slice(&d.welded[pair.tris.0 as usize]);
            bad_reps.extend_from_slice(&d.welded[pair.tris.1 as usize]);
        }
    }
    bad_reps.sort_unstable();
    bad_reps.dedup();
    let mut bad: Vec<u32> = (0..positions.len() as u32)
        .filter(|&v| used[v as usize] && bad_reps.binary_search(&rep[v as usize]).is_ok())
        .collect();
    bad.sort_unstable();
    d.vertices = bad;
    d
}

/// Undirected edges not used exactly once in each direction, as (low, high).
fn bad_edges(tris: &[[u32; 3]]) -> Vec<(u32, u32)> {
    let mut m: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let e = m.entry(if a < b { (a, b) } else { (b, a) }).or_insert((0, 0));
            if a < b {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    let mut out: Vec<(u32, u32)> = m.into_iter().filter(|(_, c)| *c != (1, 1)).map(|(k, _)| k).collect();
    out.sort_unstable();
    out
}

/// The next, safer position for a fine vertex on a defect: first the mass
/// point of its crossings (which a cell's own data puts strictly between its
/// cut edges, away from the neighbours'), then halfway from there to the
/// centre of its cell. `None` once both have been tried.
fn retreat(hs: &[Hermite], stage: &mut u8, current: [f64; 3], lo: [f64; 3], h: f64) -> Option<[f64; 3]> {
    let m = mass_point(hs);
    loop {
        let next = match *stage {
            0 => m,
            1 => [
                0.5 * (m[0] + lo[0] + 0.5 * h),
                0.5 * (m[1] + lo[1] + 0.5 * h),
                0.5 * (m[2] + lo[2] + 0.5 * h),
            ],
            _ => return None,
        };
        *stage += 1;
        if next != current {
            return Some(next);
        }
    }
}

fn mass_point(hs: &[Hermite]) -> [f64; 3] {
    let mut mass = [0.0f64; 3];
    for hm in hs {
        for t in 0..3 {
            mass[t] += hm.p[t];
        }
    }
    let inv = 1.0 / hs.len().max(1) as f64;
    [mass[0] * inv, mass[1] * inv, mass[2] * inv]
}

/// Emit the two triangles of the quad dual to one cut grid edge.
#[allow(clippy::too_many_arguments)]
fn emit_quad(
    cells: &HashMap<(i32, i32, i32), CellData>,
    remap: &[u32],
    positions: &[[f64; 3]],
    key: (u8, i32, i32, i32),
    signs: u8,
    edge: usize,
    tris: &mut Vec<[u32; 3]>,
) -> bool {
    let axis = key.0 as usize;
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    // The four cells sharing this edge, wound so the quad's normal is +axis.
    let mut quad = [0u32; 4];
    for (q, off) in QUAD_OFFSETS.iter().enumerate() {
        let mut idx = [key.1, key.2, key.3];
        idx[b] += off[0];
        idx[c] += off[1];
        let cell = (idx[0], idx[1], idx[2]);
        let Some(data) = cells.get(&cell) else {
            // A cut edge always has all four cells in the band; if one is
            // missing the grid boundary clipped it, and the quad is dropped
            // rather than half-emitted.
            return false;
        };
        // Which of this cell's twelve edges is the shared one, and which
        // component of it owns the edge.
        let local = local_edge(axis, off);
        let comp = data.owner[local];
        if comp == 255 {
            return false;
        }
        let v = data.verts[comp as usize];
        quad[q] = remap[v as usize];
    }
    // Orientation: the surface goes from inside to outside along +axis when
    // the low corner of the edge is inside.
    let [lo_corner, _] = EDGE_CORNERS[edge];
    let low_inside = (signs >> lo_corner) & 1 == 1;
    let (a, b2, c2, d) = if low_inside {
        (quad[0], quad[1], quad[2], quad[3])
    } else {
        (quad[3], quad[2], quad[1], quad[0])
    };
    // Split on whichever diagonal leaves the fatter pair of triangles. Both
    // choices have the same boundary, so either is watertight; the one that
    // avoids a sliver keeps the mesh's normals meaningful, which matters
    // because a pair of back-to-back slivers reads as a 180-degree crease to
    // anything measuring curvature.
    let ac = min_area(positions, a, b2, c2).min(min_area(positions, a, c2, d));
    let bd = min_area(positions, a, b2, d).min(min_area(positions, b2, c2, d));
    if ac >= bd {
        push_tri(tris, a, b2, c2);
        push_tri(tris, a, c2, d);
    } else {
        push_tri(tris, a, b2, d);
        push_tri(tris, b2, c2, d);
    }
    true
}

#[inline]
fn push_tri(tris: &mut Vec<[u32; 3]>, a: u32, b: u32, c: u32) {
    if a != b && b != c && a != c {
        tris.push([a, b, c]);
    }
}

/// Twice the area of a triangle, or 0 when two of its corners are the same
/// vertex (the quad collapsed).
#[inline]
fn min_area(positions: &[[f64; 3]], a: u32, b: u32, c: u32) -> f64 {
    if a == b || b == c || a == c {
        return 0.0;
    }
    let (p, q, r) = (positions[a as usize], positions[b as usize], positions[c as usize]);
    let u = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let v = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// Index of the cell edge that coincides with a grid edge along `axis`, for a
/// cell at the given `(b, c)` offset from the edge.
fn local_edge(axis: usize, off: &[i32; 2]) -> usize {
    // Within its own cell the edge starts at the corner whose `b` and `c`
    // bits are 1 when the cell sits on the negative side.
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut bits = [0u8; 3];
    bits[b] = if off[0] == 0 { 0 } else { 1 };
    bits[c] = if off[1] == 0 { 0 } else { 1 };
    let corner = bits[0] | (bits[1] << 1) | (bits[2] << 2);
    (0..12)
        .find(|&e| (e / 4) == axis && EDGE_CORNERS[e][0] == corner)
        .expect("a cell edge along the axis starting at that corner")
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-15 { [0.0, 0.0, 1.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Field;

    fn mesh_at(f: &Field, res: f64, error: f64) -> ContourMesh {
        contour(&f.compile(), &ContourOptions { resolution: res, error, ..Default::default() })
            .expect("contouring succeeds")
    }

    #[test]
    fn a_sphere_meshes_watertight_manifold_and_outward() {
        let m = mesh_at(&Field::sphere(5.0), 0.6, 0.0);
        assert!(m.is_watertight(), "{} open edges", m.open_edges());
        assert!(m.is_vertex_manifold(), "{:?}", m.non_manifold_vertices());
        let want = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        let v = m.volume();
        assert!(v > 0.0, "outward-facing: volume {v}");
        assert!((v - want).abs() / want < 0.01, "volume {v} vs {want}");
    }

    #[test]
    fn a_box_keeps_its_corners() {
        let m = mesh_at(&Field::boxed([4.0, 3.0, 2.0], 0.0), 0.5, 0.0);
        assert!(m.is_watertight());
        assert!(m.is_vertex_manifold());
        let v = m.volume();
        let want = 8.0 * 6.0 * 4.0;
        assert!((v - want).abs() / want < 1e-3, "volume {v} vs {want}");
        // Sharp features: the extreme vertices must sit on the true corners,
        // which only a QEF solve achieves (surface nets would round them off).
        let mut worst = 0.0f64;
        for p in &m.positions {
            let d = [p[0].abs() - 4.0, p[1].abs() - 3.0, p[2].abs() - 2.0];
            let outside =
                (fmath::powi(d[0].max(0.0), 2) + fmath::powi(d[1].max(0.0), 2) + fmath::powi(d[2].max(0.0), 2)).sqrt();
            let inside = d[0].max(d[1]).max(d[2]).min(0.0);
            worst = worst.max((outside + inside).abs());
        }
        assert!(worst < 1e-6, "every vertex is on the box surface; worst {worst}");
    }

    #[test]
    fn two_spheres_touching_at_a_point_stay_manifold() {
        // The classic pinch: without per-component vertices the shared cell
        // produces one vertex used by two sheets.
        let a = Field::sphere(2.0).translate([-2.0, 0.0, 0.0]);
        let b = Field::sphere(2.0).translate([2.0, 0.0, 0.0]);
        let m = mesh_at(&a.union(&b), 0.25, 0.0);
        assert!(m.is_watertight(), "{} open edges", m.open_edges());
        assert!(m.is_vertex_manifold(), "{:?}", m.non_manifold_vertices());
    }

    #[test]
    fn meshing_is_deterministic() {
        let f = Field::sphere(3.0).smooth_union(&Field::boxed([2.0, 2.0, 4.0], 0.3), 1.0);
        let a = mesh_at(&f, 0.4, 0.0);
        let b = mesh_at(&f, 0.4, 0.0);
        assert_eq!(a.tris, b.tris);
        assert_eq!(a.positions.len(), b.positions.len());
        for (p, q) in a.positions.iter().zip(&b.positions) {
            assert_eq!(p[0].to_bits(), q[0].to_bits());
            assert_eq!(p[1].to_bits(), q[1].to_bits());
            assert_eq!(p[2].to_bits(), q[2].to_bits());
        }
    }

    #[test]
    fn finer_resolution_converges_on_the_analytic_volume() {
        let want = 4.0 / 3.0 * std::f64::consts::PI * 64.0;
        let mut prev = f64::INFINITY;
        for res in [1.6, 0.8, 0.4] {
            let m = mesh_at(&Field::sphere(4.0), res, 0.0);
            assert!(m.is_watertight());
            let err = (m.volume() - want).abs() / want;
            assert!(err < prev, "resolution {res}: error {err} did not improve on {prev}");
            prev = err;
        }
        assert!(prev < 0.003, "the finest run is within 0.3%: {prev}");
    }

    #[test]
    fn interval_culling_visits_far_less_than_the_whole_grid() {
        let m = mesh_at(&Field::sphere(8.0), 0.25, 0.0);
        let total = m.stats.grid[0] as u64 * m.stats.grid[1] as u64 * m.stats.grid[2] as u64;
        assert!(
            (m.stats.surface_cells as u64) * 20 < total,
            "{} surface cells out of {total}",
            m.stats.surface_cells
        );
    }

    #[test]
    fn simplification_removes_triangles_from_flat_regions_without_opening_it() {
        let f = Field::boxed([6.0, 6.0, 6.0], 0.0);
        let fine = mesh_at(&f, 0.5, 0.0);
        let simple = mesh_at(&f, 0.5, 0.02);
        assert!(simple.is_watertight(), "{} open edges", simple.open_edges());
        assert!(simple.is_vertex_manifold(), "{:?}", simple.non_manifold_vertices());
        assert!(
            simple.tris.len() < fine.tris.len(),
            "adaptive: {} vs uniform {}",
            simple.tris.len(),
            fine.tris.len()
        );
        let want = fmath::powi(12.0f64, 3);
        assert!((simple.volume() - want).abs() / want < 0.01, "volume {}", simple.volume());
    }

    #[test]
    fn an_unbounded_field_is_a_diagnostic_not_a_hang() {
        let e = contour(&Field::half_space([0.0, 0.0, 1.0], 0.0).compile(), &ContourOptions::default())
            .unwrap_err();
        assert!(e.contains("unbounded"), "{e}");
    }

    #[test]
    fn the_qef_recovers_a_corner_a_crease_and_a_plane() {
        let h = 1.0;
        let lo = [0.0, 0.0, 0.0];
        // Three orthogonal planes meeting at (0.5, 0.5, 0.5): a corner.
        let corner = [
            Hermite { p: [0.5, 0.2, 0.3], n: [1.0, 0.0, 0.0] },
            Hermite { p: [0.1, 0.5, 0.9], n: [0.0, 1.0, 0.0] },
            Hermite { p: [0.7, 0.8, 0.5], n: [0.0, 0.0, 1.0] },
        ];
        let (x, e) = solve_qef(&corner, lo, h);
        assert!(e < 1e-9 && (x[0] - 0.5).abs() < 1e-9 && (x[1] - 0.5).abs() < 1e-9 && (x[2] - 0.5).abs() < 1e-9, "{x:?} {e}");
        // Two planes: a crease along z at x = 0.25, y = 0.75.
        let crease = [
            Hermite { p: [0.25, 0.1, 0.4], n: [1.0, 0.0, 0.0] },
            Hermite { p: [0.9, 0.75, 0.6], n: [0.0, 1.0, 0.0] },
        ];
        let (x, e) = solve_qef(&crease, lo, h);
        assert!(e < 1e-9 && (x[0] - 0.25).abs() < 1e-9 && (x[1] - 0.75).abs() < 1e-9, "{x:?} {e}");
        // One plane: the solution slides to the mass point on it.
        let plane = [
            Hermite { p: [0.2, 0.1, 0.5], n: [0.0, 0.0, 1.0] },
            Hermite { p: [0.8, 0.9, 0.5], n: [0.0, 0.0, 1.0] },
        ];
        let (x, e) = solve_qef(&plane, lo, h);
        assert!(e < 1e-9 && (x[2] - 0.5).abs() < 1e-9, "{x:?} {e}");
    }
}
