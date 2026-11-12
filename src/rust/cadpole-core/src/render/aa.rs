//! Anti-aliasing and the frame skeleton: sample patterns, the shared triangle
//! inner loop, the tiled and threaded opaque geometry pass, deterministic
//! band-parallel helpers, tonemap and the final resolve.
//!
//! Everything here is deterministic by construction: tiles and bands write
//! disjoint memory, ties are broken by submission order, and no result ever
//! depends on which thread did the work.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{COV_OPAQUE, Ctx, Frame, Image, NO_FACE, NO_ITEM, ToneMap, View, add, dot, normalize, scale, sub, to_srgb8};

// ---------------------------------------------------------------------------
// Sample patterns.

const PATTERN_1: [(f32, f32); 1] = [(0.5, 0.5)];
/// Rotated grid.
const PATTERN_4: [(f32, f32); 4] = [(0.375, 0.125), (0.875, 0.375), (0.625, 0.875), (0.125, 0.625)];
/// D3D 8-rooks: ((1,-3),(-1,3),(5,1),(-3,-5),(-5,5),(-7,-1),(3,7),(7,-7)) / 16 + 0.5.
const PATTERN_8: [(f32, f32); 8] = [
    (0.5625, 0.3125),
    (0.4375, 0.6875),
    (0.8125, 0.5625),
    (0.3125, 0.1875),
    (0.1875, 0.8125),
    (0.0625, 0.4375),
    (0.6875, 0.9375),
    (0.9375, 0.0625),
];

/// Sub-pixel sample offsets for `samples` per pixel. 1 = pixel centre, 4 =
/// rotated grid, 8 = 8-rooks; other values snap to the nearest of {1, 4, 8}
/// (ties go to the smaller count). Every offset is in (0, 1) and each
/// pattern's mean is (0.5, 0.5).
pub fn pattern(samples: u32) -> &'static [(f32, f32)] {
    let s = samples as i64;
    let best = [1i64, 4, 8].into_iter().min_by_key(|&c| ((s - c).abs(), c)).unwrap();
    match best {
        1 => &PATTERN_1,
        4 => &PATTERN_4,
        _ => &PATTERN_8,
    }
}

/// Worker thread count: `requested` if > 0, else every hardware thread the
/// machine has (up to [`MAX_THREADS`]). Always 1 on wasm32.
///
/// There is deliberately no lower cap (a cap of 16 leaves half of a 32-thread
/// desktop idle): there is nothing for one to protect — bands write disjoint slices of one
/// buffer, nothing is shared, and the per-thread state is a stack — and
/// [`par_bands`] hands bands out from a queue rather than dealing them out
/// in advance, so a pool of unequal workers (performance cores and
/// efficiency cores in the same machine) self-balances: a fast core simply
/// takes more bands. Output never depends on the count; see
/// `trace::tests::thread_count_never_changes_the_image`.
pub fn resolve_threads(requested: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = requested;
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if requested > 0 {
            requested.min(MAX_THREADS)
        } else {
            std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).clamp(1, MAX_THREADS)
        }
    }
}

/// Ceiling on worker threads, for the default and for an explicit
/// `--threads`. High enough for any desktop or workstation; it exists only
/// so a typo cannot ask for a million threads.
pub const MAX_THREADS: u32 = 256;

/// Rows per band for a parallel pass over `rows` scanlines on `threads`
/// workers, given the pass's natural band height.
///
/// Bands are handed out from a queue, so the only thing that can leave a
/// thread idle is running out of bands — the *tail* of the queue. With
/// `natural` rows per band and few bands per thread the tail is a large
/// fraction of the work: a 480-row image in 4-row bands is 120 bands, which
/// is under four apiece across 32 threads, and the slowest band left at the
/// end is 25% of a worker's total. Aiming for several bands each shrinks
/// that tail without making the queue itself the bottleneck.
///
/// Band height never changes the image: every band writes a disjoint slice
/// and each pixel depends only on its own coordinates.
pub(crate) fn band_rows(rows: usize, threads: u32, natural: usize) -> usize {
    let natural = natural.max(1);
    if threads <= 1 {
        return natural;
    }
    // Eight bands per worker: enough that the tail is an eighth of a
    // worker's share, few enough that the queue lock is untouched between
    // chunks of real work.
    let want = (threads as usize).saturating_mul(8).max(1);
    rows.max(1).div_ceil(want).clamp(1, natural)
}

/// Milliseconds since the first call (0.0 on wasm32), for `RenderStats`.
pub(crate) fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

// ---------------------------------------------------------------------------
// Triangle inner loop.

/// Half-open pixel rectangle `[x0, x1) x [y0, y1)`.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

/// One edge of a set-up triangle in canonical orientation: `e(p) = sign *
/// ((p.y - ay) * dx - (p.x - ax) * dy)` with the endpoints ordered
/// lexicographically, so the two triangles sharing an edge compute
/// bit-identical (negated) values and a sample exactly on the edge is
/// claimed by exactly one of them (the top-left rule below).
#[derive(Clone, Copy)]
struct Edge {
    ax: f32,
    ay: f32,
    dx: f32,
    dy: f32,
    sign: f32,
    /// The edge is a top or left edge of its triangle.
    top_left: bool,
}

impl Edge {
    /// Directed edge `a -> b` of a positive-area triangle.
    fn new(a: [f32; 3], b: [f32; 3]) -> Edge {
        let top_left = is_top_left(a, b);
        let (a, b, sign) = if (a[0], a[1]) <= (b[0], b[1]) { (a, b, 1.0) } else { (b, a, -1.0) };
        Edge { ax: a[0], ay: a[1], dx: b[0] - a[0], dy: b[1] - a[1], sign, top_left }
    }

    #[inline(always)]
    fn eval(&self, px: f32, py: f32) -> f32 {
        self.sign * (self.dx * (py - self.ay) - self.dy * (px - self.ax))
    }

    /// Inside test with the top-left fill rule.
    #[inline(always)]
    fn inside(&self, e: f32) -> bool {
        e > 0.0 || (e == 0.0 && self.top_left)
    }
}

/// A projected triangle, reordered to positive screen area.
struct Setup {
    /// Screen (x, y, z) in canonical (positive-area) order.
    s: [[f32; 3]; 3],
    /// `perm[i]` = original vertex index of canonical vertex `i`.
    perm: [usize; 3],
    inv_area: f32,
    /// Bounding box, unclamped.
    min: [f32; 2],
    max: [f32; 2],
    /// Edge `i` is opposite canonical vertex `i`.
    edges: [Edge; 3],
}

/// `(b - a) x (p - a)`: positive when `p` is on the interior side of the
/// directed edge `a -> b` for a positive-area triangle (y down).
#[inline]
fn edge_fn(a: [f32; 3], b: [f32; 3], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Top-left rule for the directed edge `a -> b` of a positive-area triangle
/// in y-down screen space: the interior lies on the `edge_fn > 0` side, so a
/// horizontal edge running right is a top edge and an edge running up is a
/// left edge.
#[inline]
fn is_top_left(a: [f32; 3], b: [f32; 3]) -> bool {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    (dy == 0.0 && dx > 0.0) || dy < 0.0
}

fn setup_screen(s: [[f32; 3]; 3]) -> Option<Setup> {
    if s.iter().any(|v| !(v[0].is_finite() && v[1].is_finite() && v[2].is_finite())) {
        return None;
    }
    let area = edge_fn(s[0], s[1], [s[2][0], s[2][1]]);
    if !(area.abs() >= 1e-12) {
        return None;
    }
    let (s, perm) = if area < 0.0 { ([s[0], s[2], s[1]], [0usize, 2, 1]) } else { (s, [0usize, 1, 2]) };
    let min = [s[0][0].min(s[1][0]).min(s[2][0]), s[0][1].min(s[1][1]).min(s[2][1])];
    let max = [s[0][0].max(s[1][0]).max(s[2][0]), s[0][1].max(s[1][1]).max(s[2][1])];
    Some(Setup {
        s,
        perm,
        inv_area: 1.0 / area.abs(),
        min,
        max,
        edges: [Edge::new(s[1], s[2]), Edge::new(s[2], s[0]), Edge::new(s[0], s[1])],
    })
}

/// Bounding-box scan of `t` restricted to `rect`. The callback gets pixel x,
/// y, the view-space depth at the sample and barycentric weights in the
/// triangle's *original* vertex order.
fn scan(t: &Setup, rect: Rect, orthographic: bool, mut f: impl FnMut(u32, u32, f32, [f32; 3])) {
    if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
        return;
    }
    // Clamp in i64 so wildly off-screen coordinates cannot overflow.
    let x0 = (t.min[0].floor() as i64).max(rect.x0 as i64);
    let x1 = (t.max[0].ceil() as i64).min(rect.x1 as i64 - 1);
    let y0 = (t.min[1].floor() as i64).max(rect.y0 as i64);
    let y1 = (t.max[1].ceil() as i64).min(rect.y1 as i64 - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }
    let s = &t.s;
    let inv_z = [1.0 / s[0][2], 1.0 / s[1][2], 1.0 / s[2][2]];
    let [ed0, ed1, ed2] = t.edges;
    for y in y0..=y1 {
        let py = y as f32 + 0.5;
        for x in x0..=x1 {
            let px = x as f32 + 0.5;
            let e0 = ed0.eval(px, py);
            let e1 = ed1.eval(px, py);
            let e2 = ed2.eval(px, py);
            if !(ed0.inside(e0) && ed1.inside(e1) && ed2.inside(e2)) {
                continue;
            }
            let l = [e0 * t.inv_area, e1 * t.inv_area, e2 * t.inv_area];
            let (z, b) = if orthographic {
                (l[0] * s[0][2] + l[1] * s[1][2] + l[2] * s[2][2], l)
            } else {
                let q = [l[0] * inv_z[0], l[1] * inv_z[1], l[2] * inv_z[2]];
                let sum = q[0] + q[1] + q[2];
                if !(sum > 0.0) {
                    continue;
                }
                let z = 1.0 / sum;
                (z, [q[0] * z, q[1] * z, q[2] * z])
            };
            let mut bary = [0.0f32; 3];
            bary[t.perm[0]] = b[0];
            bary[t.perm[1]] = b[1];
            bary[t.perm[2]] = b[2];
            f(x as u32, y as u32, z, bary);
        }
    }
}

/// Shared triangle inner loop: bounding-box scan with f32 edge functions, a
/// top-left fill rule (shared edges hit exactly once), perspective-correct
/// barycentrics (affine when orthographic), bbox clamped to `w x h`.
/// Degenerate triangles (|area| < 1e-12) and triangles with any vertex at
/// z <= 1e-4 in perspective are skipped (near-plane clipping is a follow-up).
/// `tri` is world space; the callback gets pixel x, y, view-space z at the
/// sample and barycentric weights for (v0, v1, v2).
pub(crate) fn for_each_fragment(view: &View, w: u32, h: u32, tri: [[f32; 3]; 3], f: impl FnMut(u32, u32, f32, [f32; 3])) {
    let (Some(s0), Some(s1), Some(s2)) = (view.project(tri[0]), view.project(tri[1]), view.project(tri[2])) else {
        return;
    };
    if let Some(t) = setup_screen([s0, s1, s2]) {
        scan(&t, Rect { x0: 0, y0: 0, x1: w, y1: h }, view.orthographic, f);
    }
}

// ---------------------------------------------------------------------------
// Opaque geometry pass.

const TILE: u32 = 32;

/// Thread-local output of one 32x32 tile. Visibility is resolved with only
/// depth, a triangle reference and barycentrics per pixel; positions and
/// normals are interpolated once per surviving pixel afterwards.
struct TileOut {
    depth: Vec<f32>,
    /// Index into the tile's bin; `u32::MAX` = none.
    tri: Vec<u32>,
    bary: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    pos: Vec<[f32; 3]>,
    item: Vec<u16>,
    face: Vec<u32>,
    /// Triangle index within the item's mesh, for per-triangle fields.
    mesh_tri: Vec<u32>,
}

impl TileOut {
    fn new() -> TileOut {
        let n = (TILE * TILE) as usize;
        TileOut {
            depth: vec![f32::INFINITY; n],
            tri: vec![u32::MAX; n],
            bary: vec![[0.0; 3]; n],
            normal: vec![[0.0; 3]; n],
            pos: vec![[0.0; 3]; n],
            item: vec![NO_ITEM; n],
            face: vec![NO_FACE; n],
            mesh_tri: vec![super::NO_TRI; n],
        }
    }
}

/// Screen-space vertices of one item, `NaN` where projection failed.
struct Projected {
    screen: Vec<[f32; 3]>,
}

fn project_item(view: &View, mesh: &super::Mesh) -> Projected {
    let screen = mesh
        .positions
        .chunks_exact(3)
        .map(|p| view.project([p[0], p[1], p[2]]).unwrap_or([f32::NAN; 3]))
        .collect();
    Projected { screen }
}

/// Rasterizes every opaque item into the G-buffer: depth, normal, pos, item,
/// face and `COV_OPAQUE` coverage. Triangles are binned into 32x32 tiles in
/// (item, triangle) order; workers pop tiles from a shared counter and
/// rasterize into thread-local tiles with a strict `z < depth` test (first
/// submitted wins ties), which are then copied into the frame in tile order.
/// Output is independent of the thread count by construction.
pub(crate) fn geometry_pass(frame: &mut Frame, ctx: &Ctx<'_>) {
    let (w, h) = (frame.w, frame.h);
    let tiles_x = w.div_ceil(TILE);
    let tiles_y = h.div_ceil(TILE);
    let n_tiles = (tiles_x * tiles_y) as usize;
    let view = ctx.view;

    // Project once per vertex and bin triangles by screen bbox.
    let projected: Vec<Projected> = ctx.items.iter().map(|it| project_item(view, it.mesh)).collect();
    let mut bins: Vec<Vec<(u16, u32)>> = vec![Vec::new(); n_tiles];
    for (ii, item) in ctx.items.iter().enumerate() {
        if item.is_translucent() || ii >= NO_ITEM as usize {
            continue;
        }
        let scr = &projected[ii].screen;
        for (t, tri) in item.mesh.indices.chunks_exact(3).enumerate() {
            let (Some(a), Some(b), Some(c)) =
                (scr.get(tri[0] as usize), scr.get(tri[1] as usize), scr.get(tri[2] as usize))
            else {
                continue;
            };
            if a[0].is_nan() || b[0].is_nan() || c[0].is_nan() {
                continue;
            }
            let min_x = a[0].min(b[0]).min(c[0]).floor();
            let max_x = a[0].max(b[0]).max(c[0]).ceil();
            let min_y = a[1].min(b[1]).min(c[1]).floor();
            let max_y = a[1].max(b[1]).max(c[1]).ceil();
            if !(max_x >= 0.0 && max_y >= 0.0 && min_x < w as f32 && min_y < h as f32) {
                continue;
            }
            let tx0 = (min_x.max(0.0) as u32) / TILE;
            let tx1 = ((max_x.min(w as f32 - 1.0)) as u32) / TILE;
            let ty0 = (min_y.max(0.0) as u32) / TILE;
            let ty1 = ((max_y.min(h as f32 - 1.0)) as u32) / TILE;
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    bins[(ty * tiles_x + tx) as usize].push((ii as u16, t as u32));
                }
            }
        }
    }

    let ortho = view.orthographic;
    let vertex = |m: &super::Mesh, i: u32| {
        let i = i as usize * 3;
        [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
    };
    let vertex_normal = |m: &super::Mesh, i: u32| {
        let i = i as usize * 3;
        if i + 2 < m.normals.len() { [m.normals[i], m.normals[i + 1], m.normals[i + 2]] } else { [0.0, 0.0, 1.0] }
    };
    let raster_tile = |ti: usize| -> TileOut {
        let tx = (ti as u32) % tiles_x;
        let ty = (ti as u32) / tiles_x;
        let rect = Rect { x0: tx * TILE, y0: ty * TILE, x1: ((tx + 1) * TILE).min(w), y1: ((ty + 1) * TILE).min(h) };
        let mut out = TileOut::new();
        let bin = &bins[ti];
        // Visibility: strict `z < depth`, first submitted wins ties.
        for (bi, &(ii, t)) in bin.iter().enumerate() {
            let m = ctx.items[ii as usize].mesh;
            let idx = [m.indices[t as usize * 3], m.indices[t as usize * 3 + 1], m.indices[t as usize * 3 + 2]];
            let scr = &projected[ii as usize].screen;
            let Some(setup) = setup_screen([scr[idx[0] as usize], scr[idx[1] as usize], scr[idx[2] as usize]]) else {
                continue;
            };
            scan(&setup, rect, ortho, |x, y, z, b| {
                let li = ((y - rect.y0) * TILE + (x - rect.x0)) as usize;
                if z < out.depth[li] {
                    out.depth[li] = z;
                    out.tri[li] = bi as u32;
                    out.bary[li] = b;
                }
            });
        }
        // Attributes for the surviving pixels.
        for li in 0..(TILE * TILE) as usize {
            let bi = out.tri[li];
            if bi == u32::MAX {
                continue;
            }
            let (ii, t) = bin[bi as usize];
            let m = ctx.items[ii as usize].mesh;
            let idx = [m.indices[t as usize * 3], m.indices[t as usize * 3 + 1], m.indices[t as usize * 3 + 2]];
            let b = out.bary[li];
            let (p0, p1, p2) = (vertex(m, idx[0]), vertex(m, idx[1]), vertex(m, idx[2]));
            let (n0, n1, n2) = (vertex_normal(m, idx[0]), vertex_normal(m, idx[1]), vertex_normal(m, idx[2]));
            let pos = add(add(scale(p0, b[0]), scale(p1, b[1])), scale(p2, b[2]));
            let mut n = normalize(add(add(scale(n0, b[0]), scale(n1, b[1])), scale(n2, b[2])));
            // Flip toward the viewer; only the sign of the dot product
            // matters, so the eye vector is not normalized.
            let facing = if ortho { -dot(n, view.fwd) } else { dot(n, sub(view.eye, pos)) };
            if facing < 0.0 {
                n = scale(n, -1.0);
            }
            out.pos[li] = pos;
            out.normal[li] = n;
            out.item[li] = ii;
            out.face[li] = m.face_ids.get(t as usize).copied().unwrap_or(NO_FACE);
            out.mesh_tri[li] = t;
        }
        out
    };

    let mut results: Vec<(usize, TileOut)> = Vec::new();
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { ctx.threads.max(1) };
    if threads == 1 {
        for ti in 0..n_tiles {
            if !bins[ti].is_empty() {
                results.push((ti, raster_tile(ti)));
            }
        }
    } else {
        let next = AtomicUsize::new(0);
        let worker = || {
            let mut local = Vec::new();
            loop {
                let ti = next.fetch_add(1, Ordering::Relaxed);
                if ti >= n_tiles {
                    break;
                }
                if !bins[ti].is_empty() {
                    local.push((ti, raster_tile(ti)));
                }
            }
            local
        };
        let n = threads.min(n_tiles.max(1) as u32);
        results = std::thread::scope(|s| {
            let handles: Vec<_> = (0..n).map(|_| s.spawn(worker)).collect();
            handles.into_iter().flat_map(|h| h.join().expect("geometry worker panicked")).collect()
        });
        results.sort_by_key(|(ti, _)| *ti);
    }

    for (ti, tile) in &results {
        let tx = (*ti as u32) % tiles_x;
        let ty = (*ti as u32) / tiles_x;
        let x0 = tx * TILE;
        let y0 = ty * TILE;
        let x1 = ((tx + 1) * TILE).min(w);
        let y1 = ((ty + 1) * TILE).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                let li = ((y - y0) * TILE + (x - x0)) as usize;
                if tile.item[li] == NO_ITEM {
                    continue;
                }
                let fi = frame.idx(x, y);
                frame.depth[fi] = tile.depth[li];
                frame.normal[fi] = tile.normal[li];
                frame.pos[fi] = tile.pos[li];
                frame.item[fi] = tile.item[li];
                frame.face[fi] = tile.face[li];
                frame.tri[fi] = tile.mesh_tri[li];
                frame.coverage[fi] |= COV_OPAQUE;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic band parallelism.

/// Splits `buf` into bands of `band_rows * row_len` elements and calls
/// `f(y0, band)` for each on up to `threads` scoped threads (sequentially
/// when `threads == 1` or on wasm32). Bands write disjoint memory; `f` must
/// depend only on its inputs, so the result never depends on scheduling.
pub(crate) fn par_bands<T: Send, F: Fn(usize, &mut [T]) + Sync>(buf: &mut [T], row_len: usize, band_rows: usize, threads: u32, f: F) {
    let chunk = (row_len.max(1) * band_rows.max(1)).max(1);
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { threads.max(1) };
    if threads == 1 {
        for (i, band) in buf.chunks_mut(chunk).enumerate() {
            f(i * band_rows, band);
        }
        return;
    }
    let bands: Vec<(usize, &mut [T])> = buf.chunks_mut(chunk).enumerate().map(|(i, b)| (i * band_rows, b)).collect();
    let n = threads.min(bands.len().max(1) as u32);
    let queue = Mutex::new(bands.into_iter());
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                loop {
                    let next = queue.lock().expect("band queue").next();
                    match next {
                        Some((y0, band)) => f(y0, band),
                        None => break,
                    }
                }
            });
        }
    });
}

/// [`par_bands`] for two output buffers of equal length that must be written
/// together (e.g. normal + pos, or hdr + coverage).
pub(crate) fn par_bands2<A: Send, B: Send, F: Fn(usize, &mut [A], &mut [B]) + Sync>(
    a: &mut [A],
    b: &mut [B],
    row_len: usize,
    band_rows: usize,
    threads: u32,
    f: F,
) {
    debug_assert_eq!(a.len(), b.len());
    let chunk = (row_len.max(1) * band_rows.max(1)).max(1);
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { threads.max(1) };
    if threads == 1 {
        for (i, (ba, bb)) in a.chunks_mut(chunk).zip(b.chunks_mut(chunk)).enumerate() {
            f(i * band_rows, ba, bb);
        }
        return;
    }
    let bands: Vec<(usize, &mut [A], &mut [B])> =
        a.chunks_mut(chunk).zip(b.chunks_mut(chunk)).enumerate().map(|(i, (x, y))| (i * band_rows, x, y)).collect();
    let n = threads.min(bands.len().max(1) as u32);
    let queue = Mutex::new(bands.into_iter());
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                loop {
                    let next = queue.lock().expect("band queue").next();
                    match next {
                        Some((y0, ba, bb)) => f(y0, ba, bb),
                        None => break,
                    }
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Tonemap.

/// ACES fitted curve (Narkowicz).
#[inline]
fn aces(x: f32) -> f32 {
    let x = x.max(0.0);
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

/// Solves `(2.51 - 2.43y) x^2 + (0.03 - 0.59y) x - 0.14y = 0` for x >= 0.
#[inline]
fn inv_aces(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    if y <= 0.0 {
        return 0.0;
    }
    let a = 2.51 - 2.43 * y;
    let b = 0.03 - 0.59 * y;
    let c = -0.14 * y;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    ((-b + disc.sqrt()) / (2.0 * a)).max(0.0)
}

/// Where Khronos PBR Neutral starts compressing: below this peak the curve
/// is the identity (less the black-point lift), which is the whole point of
/// it — a base colour lit by unit grey light comes out *as* the base colour.
const NEUTRAL_START: f32 = 0.8 - 0.04;
/// How much the compressed highlights are desaturated toward white.
const NEUTRAL_DESATURATION: f32 = 0.15;

/// Khronos PBR Neutral (Apache-2.0; the three.js `NeutralToneMapping` form).
///
/// Unlike ACES it works on the colour as a whole: a black-point lift from the
/// *minimum* channel, then a rational compression of the *maximum* channel
/// with the other two scaled along, then a controlled mix toward white. No
/// channel is curved on its own, so there is no hue rotation — a filament red
/// stays red instead of going orange — and everything with all channels in
/// roughly 0.08..0.8 is reproduced exactly.
#[inline]
fn neutral(c: [f32; 3]) -> [f32; 3] {
    let c = c.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let x = c[0].min(c[1]).min(c[2]);
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    let c = c.map(|v| v - offset);
    let peak = c[0].max(c[1]).max(c[2]);
    if peak < NEUTRAL_START {
        return c.map(|v| v.clamp(0.0, 1.0));
    }
    let d = 1.0 - NEUTRAL_START;
    let new_peak = 1.0 - d * d / (peak + d - NEUTRAL_START);
    let c = c.map(|v| v * new_peak / peak);
    let g = 1.0 - 1.0 / (NEUTRAL_DESATURATION * (peak - new_peak) + 1.0);
    c.map(|v| (v * (1.0 - g) + new_peak * g).clamp(0.0, 1.0))
}

/// Closed-form inverse of [`neutral`]. Both branches of the final mix have
/// `new_peak` as their maximum, so the compressed peak is read straight off
/// the output, the original peak follows from the rational curve, the
/// desaturation mix is undone with the same `g`, and the black-point offset
/// is recovered from the minimum channel (the quadratic branch when it was
/// lifted, the constant otherwise).
fn inv_neutral(d: [f32; 3]) -> [f32; 3] {
    let o = d.map(|v| (v as f64).clamp(0.0, 1.0));
    let np = o[0].max(o[1]).max(o[2]);
    let start = NEUTRAL_START as f64;
    let mut c = o;
    if np >= start {
        let dd = 1.0 - start;
        // A display white has no finite pre-image; stop just short of it.
        let np_c = np.min(1.0 - 1e-9);
        let p = dd * dd / (1.0 - np_c) - dd + start;
        let g = 1.0 - 1.0 / (NEUTRAL_DESATURATION as f64 * (p - np_c) + 1.0);
        c = o.map(|v| ((v - np_c * g) / (1.0 - g)).max(0.0) * p / np_c);
    }
    let mc = c[0].min(c[1]).min(c[2]);
    let offset = if mc < 0.04 {
        let x = (mc / 6.25).sqrt();
        x - mc
    } else {
        0.04
    };
    c.map(|v| (v + offset) as f32)
}

/// Exposure then the tone curve `tm`, clamped to 0..1. Monotone;
/// `tonemap_rgb(0) == 0`. [`ToneMap::None`] is the plain clamp the tests and
/// the debug views use, and applies no exposure, as it never did.
pub fn tonemap_rgb(c: [f32; 3], exposure: f32, tm: ToneMap) -> [f32; 3] {
    match tm {
        ToneMap::Aces => c.map(|v| aces(v * exposure)),
        ToneMap::Neutral => neutral(c.map(|v| v * exposure)),
        ToneMap::None => c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }),
    }
}

/// Closed-form inverse of [`tonemap_rgb`]: `tonemap_rgb(inverse_tonemap_rgb(d)) == d`
/// within 1e-5 for `d` in 0..1 (greys and colours alike). The identity under
/// [`ToneMap::None`].
pub fn inverse_tonemap_rgb(d: [f32; 3], exposure: f32, tm: ToneMap) -> [f32; 3] {
    let e = (exposure as f64).max(1e-6);
    match tm {
        ToneMap::Aces => d.map(|v| (inv_aces(v as f64) / e) as f32),
        ToneMap::Neutral => inv_neutral(d).map(|v| (v as f64 / e) as f32),
        ToneMap::None => d,
    }
}

/// Bloom: what is brighter than display white spills a little into its
/// neighbours, the way a lens does. `b = max(hdr - 1, 0)` is blurred with a
/// Gaussian of `sigma` = 1 % of the width and added back at 5 %. Done on a
/// buffer a quarter the size in each direction, because a 16-pixel Gaussian
/// on a 2 Mpx frame is a third of a second per pass and its result is
/// smooth by construction; the bilinear upsample is invisible under it.
///
/// Opt-in (`RenderOptions::bloom`): the halo crosses the silhouette, so a
/// backdrop pixel next to a highlight is no longer bit-exactly the
/// background, which every contact sheet and golden relies on.
pub(crate) fn bloom(hdr: &mut [[f32; 3]], w: u32, h: u32) {
    const DOWN: usize = 4;
    const STRENGTH: f32 = 0.05;
    let (w, h) = (w as usize, h as usize);
    if w < DOWN * 2 || h < DOWN * 2 || hdr.len() < w * h {
        return;
    }
    let (sw, sh) = (w.div_ceil(DOWN), h.div_ceil(DOWN));
    // Down: box-average the over-white part of each 4x4 block.
    let mut small = vec![[0.0f32; 3]; sw * sh];
    for sy in 0..sh {
        for sx in 0..sw {
            let mut acc = [0.0f32; 3];
            let mut n = 0.0f32;
            for y in sy * DOWN..((sy + 1) * DOWN).min(h) {
                for x in sx * DOWN..((sx + 1) * DOWN).min(w) {
                    let p = hdr[y * w + x];
                    for c in 0..3 {
                        acc[c] += (p[c] - 1.0).max(0.0);
                    }
                    n += 1.0;
                }
            }
            small[sy * sw + sx] = acc.map(|v| v / n.max(1.0));
        }
    }
    // Separable Gaussian, sigma = 1 % of the full width, in small pixels.
    let sigma = (w as f32 * 0.01 / DOWN as f32).max(0.5);
    let radius = (sigma * 3.0).ceil() as i64;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let ksum: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / ksum).collect();
    let mut tmp = vec![[0.0f32; 3]; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0.0f32; 3];
            for (i, k) in kernel.iter().enumerate() {
                let xx = (x as i64 + i as i64 - radius).clamp(0, sw as i64 - 1) as usize;
                let p = small[y * sw + xx];
                for c in 0..3 {
                    acc[c] += p[c] * k;
                }
            }
            tmp[y * sw + x] = acc;
        }
    }
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0.0f32; 3];
            for (i, k) in kernel.iter().enumerate() {
                let yy = (y as i64 + i as i64 - radius).clamp(0, sh as i64 - 1) as usize;
                let p = tmp[yy * sw + x];
                for c in 0..3 {
                    acc[c] += p[c] * k;
                }
            }
            small[y * sw + x] = acc;
        }
    }
    // Up: bilinear, added back.
    let at = |x: f32, y: f32| -> [f32; 3] {
        let fx = (x / DOWN as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
        let fy = (y / DOWN as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let mut out = [0.0f32; 3];
        for c in 0..3 {
            let a = small[y0 * sw + x0][c] * (1.0 - tx) + small[y0 * sw + x1][c] * tx;
            let b = small[y1 * sw + x0][c] * (1.0 - tx) + small[y1 * sw + x1][c] * tx;
            out[c] = a * (1.0 - ty) + b * ty;
        }
        out
    };
    for y in 0..h {
        for x in 0..w {
            let b = at(x as f32 + 0.5, y as f32 + 0.5);
            let p = &mut hdr[y * w + x];
            for c in 0..3 {
                p[c] += STRENGTH * b[c];
            }
        }
    }
}

/// `hdr` -> `display`. Uncovered pixels get `background * ground` exactly
/// (no tonemap) so user backgrounds and contact sheets stay bit-exact;
/// covered pixels get `tonemap_rgb(hdr, exposure, opts.tonemap)`, which is
/// a plain clamp under [`ToneMap::None`].
pub(crate) fn tonemap(frame: &mut Frame, ctx: &Ctx<'_>, oit: &super::oit::Buffers) {
    let bg = ctx.opts.background;
    let exposure = ctx.opts.exposure;
    let tm = ctx.opts.tonemap;
    let w = frame.w as usize;
    let (hdr, cov, ground) = (&frame.hdr, &frame.coverage, &frame.ground);
    par_bands(&mut frame.display, w, 32, ctx.threads, |y0, band| {
        let base = y0 * w;
        for (i, d) in band.iter_mut().enumerate() {
            let idx = base + i;
            *d = if cov[idx] == 0 {
                let g = ground[idx];
                [bg[0] * g, bg[1] * g, bg[2] * g]
            } else {
                tonemap_rgb(hdr[idx], exposure, tm)
            };
        }
    });
    if ctx.opts.transparent {
        coverage_alpha(frame, ctx, oit);
    }
}

/// The alpha of a frame drawn without a backdrop (over black, so `display`
/// is already premultiplied): 1 on an opaque surface; what a translucent
/// stack does not let through where nothing opaque is behind it; and on
/// bare ground the contact shadow's darkening, which is zero unless the
/// shadow catcher kept the shadow.
fn coverage_alpha(frame: &mut Frame, ctx: &Ctx<'_>, oit: &super::oit::Buffers) {
    let w = frame.w as usize;
    let (cov, ground) = (&frame.coverage, &frame.ground);
    par_bands(&mut frame.alpha, w, 32, ctx.threads, |y0, band| {
        let base = y0 * w;
        for (i, a) in band.iter_mut().enumerate() {
            let idx = base + i;
            *a = if cov[idx] == 0 {
                (1.0 - ground[idx]).clamp(0.0, 1.0)
            } else if cov[idx] & super::COV_OPAQUE != 0 {
                1.0
            } else {
                let t = oit.transmittance.get(idx).copied().unwrap_or([1.0; 3]);
                (1.0 - (t[0] + t[1] + t[2]) / 3.0).clamp(0.0, 1.0)
            };
        }
    });
}

// ---------------------------------------------------------------------------
// Accumulate and resolve.

pub(crate) fn accumulate(acc: &mut [[f32; 3]], display: &[[f32; 3]]) {
    for (a, d) in acc.iter_mut().zip(display) {
        a[0] += d[0];
        a[1] += d[1];
        a[2] += d[2];
    }
}

/// Averages the accumulated display-linear samples (never in sRGB), applies
/// a 2x2 ordered dither of +-0.5 LSB at quantization and encodes to sRGB8.
/// Pixels whose accumulated value is exactly `n` background samples (same
/// f32 summation sequence as [`accumulate`]) are written as
/// `to_srgb8(background)` without dither, so the background stays bit-exact.
pub(crate) fn resolve(acc: &[[f32; 3]], n: u32, w: u32, h: u32, background: Option<[f32; 3]>) -> Image {
    let n = n.max(1);
    let inv = 1.0 / n as f32;
    let bg_acc = background.map(|bg| {
        let mut a = [0.0f32; 3];
        for _ in 0..n {
            a[0] += bg[0];
            a[1] += bg[1];
            a[2] += bg[2];
        }
        a
    });
    let bg8 = background.map(|bg| bg.map(to_srgb8));
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    // Bayer 2x2 thresholds, centred: offsets in [-0.375, 0.375] LSB.
    const BAYER: [[f32; 2]; 2] = [[-0.375, 0.125], [0.375, -0.125]];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let a = acc[y * w as usize + x];
            let o = (y * w as usize + x) * 4;
            if let (Some(ba), Some(b8)) = (bg_acc, bg8) {
                if a == ba {
                    rgba[o..o + 3].copy_from_slice(&b8);
                    rgba[o + 3] = 255;
                    continue;
                }
            }
            let v = [a[0] * inv, a[1] * inv, a[2] * inv];
            for c in 0..3 {
                rgba[o + c] = dither_srgb8(v[c], BAYER[y & 1][x & 1]);
            }
            rgba[o + 3] = 255;
        }
    }
    Image { width: w, height: h, rgba }
}

/// sRGB-encode `v` (display-linear) with a sub-LSB `offset` added before
/// rounding.
#[inline]
pub(crate) fn dither_srgb8(v: f32, offset: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5 + offset).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Material;
    use crate::render::testing::*;
    use crate::render::{Camera, Item, RenderOptions, Scene, render};

    #[test]
    fn patterns_are_centred_and_inside_the_pixel() {
        for s in [1u32, 4, 8] {
            let p = pattern(s);
            assert_eq!(p.len(), s as usize);
            let (mx, my) = p.iter().fold((0.0f64, 0.0f64), |(a, b), &(x, y)| (a + x as f64, b + y as f64));
            assert!((mx / p.len() as f64 - 0.5).abs() < 1e-6, "samples={s}");
            assert!((my / p.len() as f64 - 0.5).abs() < 1e-6, "samples={s}");
            for &(x, y) in p {
                assert!(x > 0.0 && x < 1.0 && y > 0.0 && y < 1.0);
            }
        }
        assert_eq!(pattern(0).len(), 1);
        assert_eq!(pattern(2).len(), 1);
        assert_eq!(pattern(3).len(), 4);
        assert_eq!(pattern(6).len(), 4);
        assert_eq!(pattern(7).len(), 8);
        assert_eq!(pattern(100).len(), 8);
    }

    #[test]
    fn resolve_averages_in_display_linear() {
        // One pixel: white + black over two samples is display-linear 0.5,
        // which is sRGB 188, not 128.
        let acc = [[1.0f32, 1.0, 1.0]];
        let img = resolve(&acc, 2, 1, 1, None);
        assert!((187..=189).contains(&img.rgba[0]), "got {}", img.rgba[0]);
        assert_eq!(img.rgba[3], 255);
    }

    #[test]
    fn resolve_keeps_background_bit_exact() {
        let bg = [0.13f32, 0.14, 0.16];
        for n in [1u32, 4, 8] {
            // Accumulate the way the driver does.
            let mut acc = vec![[0.0f32; 3]; 4];
            for _ in 0..n {
                accumulate(&mut acc, &[bg; 4]);
            }
            let img = resolve(&acc, n, 2, 2, Some(bg));
            for p in img.rgba.chunks_exact(4) {
                assert_eq!(&p[..3], &bg.map(to_srgb8), "n={n}");
            }
        }
    }

    #[test]
    fn tonemap_is_monotone_and_invertible() {
        for tm in [ToneMap::Aces, ToneMap::Neutral] {
            let mut prev = -1.0f32;
            let mut x = 0.0f32;
            while x <= 16.0 {
                let y = tonemap_rgb([x; 3], 1.0, tm)[0];
                assert!(y >= prev, "{tm:?} not monotone at {x}");
                assert!((0.0..=1.0).contains(&y));
                prev = y;
                x += 0.01;
            }
            assert_eq!(tonemap_rgb([0.0; 3], 1.0, tm), [0.0; 3]);
            assert!(tonemap_rgb([1e4; 3], 1.0, tm)[0] <= 1.0);
            for exposure in [0.5f32, 1.0, 2.0] {
                for i in 0..=1000 {
                    let d = i as f32 / 1000.0;
                    let back = tonemap_rgb(inverse_tonemap_rgb([d; 3], exposure, tm), exposure, tm)[0];
                    assert!((back - d).abs() < 1e-5, "{tm:?} d={d} exposure={exposure} back={back}");
                }
            }
        }
        // The plain clamp is its own inverse and applies no exposure.
        assert_eq!(tonemap_rgb([0.3, 2.0, -1.0], 2.0, ToneMap::None), [0.3, 1.0, 0.0]);
        assert_eq!(inverse_tonemap_rgb([0.3, 0.5, 0.7], 2.0, ToneMap::None), [0.3, 0.5, 0.7]);
    }

    /// What Neutral is for: a base colour under unit grey light comes out as
    /// itself, and a saturated colour stays its own hue where ACES drifts it.
    /// The numbers are the Khronos specification's: exact reproduction with
    /// every channel in 0.08..0.8 (less the 0.04 black-point lift, which is
    /// the one deliberate departure), no hue rotation above it.
    #[test]
    fn neutral_reproduces_base_colours_and_keeps_hue() {
        for c in [[0.5f32, 0.5, 0.5], [0.72, 0.74, 0.78], [0.584, 0.089, 0.047], [0.1, 0.6, 0.3]] {
            let out = tonemap_rgb(c, 1.0, ToneMap::Neutral);
            // The lift is the constant 0.04 once the darkest channel is
            // past 0.08, and `x - 6.25 x²` (which meets it there) below.
            let x = c[0].min(c[1]).min(c[2]);
            let lift = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
            let want = c.map(|v| v - lift);
            for k in 0..3 {
                assert!((out[k] - want[k]).abs() < 1e-5, "{c:?} -> {out:?}, wanted {want:?}");
            }
            // A colour is recovered exactly, not only a grey.
            let back = tonemap_rgb(inverse_tonemap_rgb(out, 1.0, ToneMap::Neutral), 1.0, ToneMap::Neutral);
            for k in 0..3 {
                assert!((back[k] - out[k]).abs() < 1e-5, "{c:?}: round trip {back:?} vs {out:?}");
            }
        }
        // A filament red at 1.35x exposure: ACES pulls the hue toward
        // orange (the blue channel rises relative to red); Neutral keeps the
        // channel ratios and moves the whole colour toward white instead.
        let red = [0.584f32, 0.089, 0.047];
        let aces = tonemap_rgb(red, 1.35, ToneMap::Aces);
        let neutral = tonemap_rgb(red, 1.35, ToneMap::Neutral);
        let hue = |c: [f32; 3]| (c[1] - c[2]) / (c[0] - c[2]).max(1e-6);
        assert!((hue(neutral) - hue(red)).abs() < 0.02, "neutral rotated the hue: {neutral:?}");
        assert!((hue(aces) - hue(red)).abs() > 0.05, "aces was expected to drift: {aces:?}");
        let sat = |c: [f32; 3]| 1.0 - c[2] / c[0].max(1e-6);
        assert!(sat(neutral) > sat(aces), "neutral {neutral:?} should keep more saturation than aces {aces:?}");
        // Above the knee the compression is monotone in the peak and never
        // reaches white.
        let bright = tonemap_rgb([4.0, 2.0, 1.0], 1.0, ToneMap::Neutral);
        assert!(bright[0] < 1.0 && bright[0] > bright[1] && bright[1] > bright[2], "{bright:?}");
    }

    /// Bloom moves energy out of what is over white and nowhere else: a
    /// frame with nothing over 1 is untouched, and a single hot pixel lifts
    /// its neighbours by a little and far pixels by nothing.
    #[test]
    fn bloom_spreads_only_the_overbright() {
        let (w, h) = (64u32, 48u32);
        let mut flat = vec![[0.5f32; 3]; (w * h) as usize];
        let before = flat.clone();
        bloom(&mut flat, w, h);
        assert_eq!(flat, before, "nothing over white, so nothing to spread");
        let mut hot = vec![[0.2f32; 3]; (w * h) as usize];
        hot[(24 * w + 32) as usize] = [20.0; 3];
        bloom(&mut hot, w, h);
        let near = hot[(24 * w + 34) as usize][0];
        let far = hot[(2 * w + 2) as usize][0];
        assert!(near > 0.2 + 1e-4, "the pixel next to the highlight did not brighten: {near}");
        assert!((far - 0.2).abs() < 1e-4, "a far pixel moved: {far}");
        assert!(hot[(24 * w + 32) as usize][0] >= 20.0, "the highlight itself lost energy");
    }

    fn screen_view(w: u32, h: u32, orthographic: bool) -> View {
        // Eye at the origin looking down +y with z up; 1 unit = 1 px at
        // distance `focal` in perspective.
        let cam = Camera {
            eye: crate::geom::Vec3::ZERO,
            target: crate::geom::Vec3::new(0.0, 10.0, 0.0),
            up: crate::geom::Vec3::Z,
            fov_degrees: 90.0,
            orthographic,
            shift: 0.0,
        };
        View::new(&cam, w, h)
    }

    #[test]
    fn shared_diagonal_covers_each_pixel_exactly_once() {
        // Two triangles across a 64x64 quad whose diagonal passes through
        // pixel centres: every pixel must be hit exactly once.
        let view = screen_view(64, 64, true);
        let s = view.ortho_scale;
        // World coordinates that project to screen 0..64.
        let px = |sx: f32, sy: f32| view.unproject(sx, sy, 5.0);
        let (a, b, c, d) = (px(0.0, 0.0), px(64.0, 0.0), px(64.0, 64.0), px(0.0, 64.0));
        let _ = s;
        let mut hits = vec![0u32; 64 * 64];
        let mut count = 0;
        for tri in [[a, b, c], [a, c, d]] {
            for_each_fragment(&view, 64, 64, tri, |x, y, _, _| {
                hits[(y * 64 + x) as usize] += 1;
                count += 1;
            });
        }
        assert_eq!(count, 4096);
        assert!(hits.iter().all(|&h| h == 1), "gaps or double hits on the shared edge");
        // The same quad through the geometry pass: 4096 covered pixels.
        let mesh = quad_mesh([a, b, c, d]);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        let cam = Camera { eye: crate::geom::Vec3::ZERO, target: crate::geom::Vec3::new(0.0, 10.0, 0.0), up: crate::geom::Vec3::Z, fov_degrees: 90.0, orthographic: true, shift: 0.0 };
        let frame = render_frame(&items, &cam, &RenderOptions { width: 64, height: 64, ..Default::default() });
        assert_eq!(frame.item.iter().filter(|&&i| i != NO_ITEM).count(), 4096);
    }

    fn quad_mesh(p: [[f32; 3]; 4]) -> crate::tessellate::Mesh {
        crate::tessellate::Mesh {
            positions: p.iter().flat_map(|v| v.iter().copied()).collect(),
            normals: [[0.0f32, -1.0, 0.0]; 4].iter().flat_map(|v| v.iter().copied()).collect(),
            indices: vec![0, 1, 2, 0, 2, 3],
            face_ids: vec![0, 0],
            colors: Vec::new(),
        }
    }

    #[test]
    fn barycentrics_are_perspective_correct() {
        // A quad from depth 10 (z = -1) to depth 100 (z = +1). The attribute
        // t = 0 at the near edge and 1 at the far edge; the t = 0.5 line is
        // the world midpoint (depth 55, z = 0), which projects to the screen
        // centre row. Affine interpolation would put it at h/2 + 0.045*focal.
        let (w, h) = (64u32, 480u32);
        for ortho in [false, true] {
            let view = screen_view(w, h, ortho);
            let (a, b, c, d) = ([-1.0, 10.0, -1.0], [1.0, 10.0, -1.0], [1.0, 100.0, 1.0], [-1.0, 100.0, 1.0]);
            let t_of = [0.0f32, 0.0, 1.0, 1.0];
            let mut rows: Vec<Vec<(u32, f32)>> = vec![Vec::new(); h as usize];
            for (tri, ids) in [([a, b, c], [0usize, 1, 2]), ([a, c, d], [0, 2, 3])] {
                for_each_fragment(&view, w, h, tri, |x, y, _, bary| {
                    let t = bary[0] * t_of[ids[0]] + bary[1] * t_of[ids[1]] + bary[2] * t_of[ids[2]];
                    rows[y as usize].push((x, t));
                });
            }
            // Walk the centre column down: find where t crosses 0.5.
            let col = w / 2;
            let mut prev: Option<(u32, f32)> = None;
            let mut crossing = None;
            for (y, row) in rows.iter().enumerate() {
                let Some(&(_, t)) = row.iter().find(|(x, _)| *x == col) else { continue };
                if let Some((py, pt)) = prev {
                    if (pt < 0.5) != (t < 0.5) {
                        let f = (0.5 - pt) / (t - pt);
                        crossing = Some(py as f32 + 0.5 + f * (y as f32 - py as f32));
                        break;
                    }
                }
                prev = Some((y as u32, t));
            }
            let crossing = crossing.expect("midline not found");
            let expected = h as f32 * 0.5;
            assert!((crossing - expected).abs() < 0.5, "ortho={ortho}: crossing {crossing} vs {expected}");
            if !ortho {
                let affine = expected + 0.045 * view.focal;
                assert!((crossing - affine).abs() > 5.0, "perspective result should not sit at the affine position");
            }
        }
    }

    #[test]
    fn large_quad_has_no_holes_at_tile_boundaries() {
        // A single quad covering the whole 200x130 frame (spans several tile
        // rows and columns, including partial tiles): every pixel is covered.
        let (w, h) = (200u32, 130u32);
        let cam = Camera { eye: crate::geom::Vec3::ZERO, target: crate::geom::Vec3::new(0.0, 10.0, 0.0), up: crate::geom::Vec3::Z, fov_degrees: 60.0, orthographic: false, shift: 0.0 };
        let view = View::new(&cam, w, h);
        let px = |sx: f32, sy: f32| view.unproject(sx, sy, 20.0);
        let mesh = quad_mesh([px(-2.0, -2.0), px(w as f32 + 2.0, -2.0), px(w as f32 + 2.0, h as f32 + 2.0), px(-2.0, h as f32 + 2.0)]);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        for threads in [1u32, 3] {
            let frame = render_frame(&items, &cam, &RenderOptions { width: w, height: h, threads, ..Default::default() });
            let holes = frame.item.iter().filter(|&&i| i == NO_ITEM).count();
            assert_eq!(holes, 0, "threads={threads}");
        }
    }

    #[test]
    fn output_is_independent_of_thread_count() {
        let (mesh, edges) = cylinder();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        let hash = |threads: u32| {
            let img = render(&items, &cam, &RenderOptions { width: 256, height: 192, threads, samples: 4, ..Default::default() });
            fnv1a64(&img.rgba)
        };
        let h1 = hash(1);
        assert_eq!(h1, hash(2));
        assert_eq!(h1, hash(8));
        assert_eq!(h1, hash(0));
        assert_eq!(h1, hash(1));
    }

    #[test]
    fn supersampling_smooths_the_silhouette() {
        // Unit box in iso view. Along rows crossing the right silhouette,
        // count distinct 8-bit values strictly between the surface and the
        // background within 3 px of the depth discontinuity. A row can only
        // hold as many partially covered pixels as the edge advances per row,
        // so the ">= 2 values" check is applied to shallow segments (>= 2 px
        // per row) and the ">= 1 value" check to every row.
        let (mesh, edges) = unit_box();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        let base = RenderOptions { width: 320, height: 240, draw_grid: false, draw_edges: false, ..Default::default() };
        let frame = render_frame(&items, &cam, &RenderOptions { samples: 1, ..base.clone() });
        let one = render(&items, &cam, &RenderOptions { samples: 1, ..base.clone() });
        let four = render(&items, &cam, &RenderOptions { samples: 4, ..base.clone() });
        // Rightmost covered pixel per row (the silhouette) and its slope.
        let right_of = |y: u32| (0..320).rev().find(|&x| frame.item[frame.idx(x, y)] != NO_ITEM);
        let mut rows = Vec::new();
        for y in (10..230).step_by(2) {
            let (Some(x), Some(xa), Some(xb)) = (right_of(y), right_of(y - 1), right_of(y + 1)) else { continue };
            if x > 3 && x < 316 {
                rows.push((x, y, (xa as i32 - xb as i32).unsigned_abs() >= 4));
            }
        }
        let shallow: Vec<(u32, u32)> = rows.iter().filter(|r| r.2).map(|r| (r.0, r.1)).collect();
        assert!(rows.len() >= 40, "only {} rows cross the silhouette", rows.len());
        assert!(shallow.len() >= 8, "only {} shallow rows", shallow.len());
        // A 2-level margin keeps the +-0.5 LSB dither from counting.
        let intermediate = |img: &Image, x: u32, y: u32| -> usize {
            let vals: Vec<u8> = (x - 3..=x + 3).map(|xx| luma(img, xx, y).round() as u8).collect();
            let (lo, hi) = (*vals.iter().min().unwrap(), *vals.iter().max().unwrap());
            if hi - lo < 6 {
                return 0;
            }
            let mut mids: Vec<u8> = vals.iter().copied().filter(|&v| v > lo + 2 && v < hi - 2).collect();
            mids.sort_unstable();
            mids.dedup();
            mids.len()
        };
        let n = shallow.len();
        let smooth4 = shallow.iter().filter(|&&(x, y)| intermediate(&four, x, y) >= 2).count();
        let smooth1 = shallow.iter().filter(|&&(x, y)| intermediate(&one, x, y) >= 2).count();
        // Half the rows, not four fifths: whether a row shows one or two
        // distinct intermediate values also depends on where the flat-shaded
        // face's colour sits inside its quantization step (the resolve's
        // +-0.5 LSB dither splits a face that lands on a boundary into two
        // values and leaves one that lands mid-step alone), which is a
        // property of the shading, not of the anti-aliasing. The samples=1
        // bound below is what makes this a supersampling test.
        assert!(smooth4 * 2 >= n, "samples=4: {smooth4}/{n} shallow rows show >= 2 intermediate values");
        assert!(smooth1 * 5 <= n, "samples=1: {smooth1}/{n} shallow rows show >= 2 intermediate values");
        let n = rows.len();
        let any4 = rows.iter().filter(|&&(x, y, _)| intermediate(&four, x, y) >= 1).count();
        let any1 = rows.iter().filter(|&&(x, y, _)| intermediate(&one, x, y) >= 1).count();
        assert!(any4 * 10 >= n * 7, "samples=4: {any4}/{n} rows show an intermediate value");
        assert!(any1 * 5 <= n, "samples=1: {any1}/{n} rows show an intermediate value");
    }

    #[test]
    fn par_bands_matches_sequential() {
        let (w, rows) = (37usize, 101usize);
        let f = |y0: usize, band: &mut [u32]| {
            for (i, v) in band.iter_mut().enumerate() {
                *v = (y0 * w + i) as u32 * 3 + 1;
            }
        };
        let mut seq = vec![0u32; w * rows];
        par_bands(&mut seq, w, 32, 1, f);
        let mut par = vec![0u32; w * rows];
        par_bands(&mut par, w, 32, 5, f);
        assert_eq!(seq, par);
        let expect: Vec<u32> = (0..w * rows).map(|i| i as u32 * 3 + 1).collect();
        assert_eq!(seq, expect);
        let mut a = vec![0u32; w * rows];
        let mut b = vec![0u8; w * rows];
        par_bands2(&mut a, &mut b, w, 16, 4, |y0, ba, bb| {
            for (i, (x, y)) in ba.iter_mut().zip(bb.iter_mut()).enumerate() {
                *x = (y0 * w + i) as u32;
                *y = ((y0 * w + i) % 251) as u8;
            }
        });
        assert!(a.iter().enumerate().all(|(i, &v)| v as usize == i));
        assert!(b.iter().enumerate().all(|(i, &v)| v as usize == i % 251));
    }

    #[test]
    fn scene_extents_cover_all_items() {
        let (mesh, _) = unit_box();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        let scene = Scene::from_items(&items);
        assert_eq!(scene.lo, [0.0, 0.0, 0.0]);
        assert_eq!(scene.hi, [1.0, 1.0, 1.0]);
        assert_eq!(scene.ground_z, 0.0);
        assert!((scene.diag - 3f32.sqrt()).abs() < 1e-6);
        assert!(Scene::from_items(&[]).diag >= 1e-3);
    }

    #[test]
    fn planter_like_geometry_renders_smooth_rim() {
        // Real kernel geometry through the whole pipeline, samples=4: the
        // image is deterministic, has an object in the middle and its rim
        // shows intermediate values (anti-aliased) rather than a hard step.
        let (mesh, edges) = planter_shell();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mat = Material { base_color: [0.82, 0.55, 0.42], roughness: 0.85, ..Material::default() };
        let items = [Item::new(&mesh, &mat, &edges)];
        let opts = RenderOptions { width: 320, height: 240, samples: 4, threads: 2, ..Default::default() };
        let img = render(&items, &cam, &opts);
        let bg = mean_luma(&img, (0, 0, 8, 8));
        let mid = mean_luma(&img, (150, 110, 170, 130));
        assert!((mid - bg).abs() > 20.0, "object not visible: mid={mid} bg={bg}");
        assert!(distinct_levels(&img, (60, 40, 260, 200)) > 40);
        assert_eq!(fnv1a64(&img.rgba), fnv1a64(&render(&items, &cam, &opts).rgba));
    }
}
