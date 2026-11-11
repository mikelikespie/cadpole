//! Is this mesh a solid?
//!
//! One question, asked of every body the kernel, the print checks, the
//! renderer and the exporters hand around: is this surface the boundary of a
//! region of space? For a triangle mesh that means five things, and
//! [`validate`] measures every one of them rather than answering yes or no:
//!
//! | property | measured as |
//! | --- | --- |
//! | **closed** | every edge belongs to exactly two triangles: a boundary edge (one) is a hole, and three or more is a non-manifold seam — an internal wall shared by two lumps gives an edge four |
//! | **consistently oriented** | the two triangles on an edge traverse it in opposite directions, and the signed volume is positive (outward normals) |
//! | **embedded** | no two triangles pass through each other, and no two coplanar triangles overlap in area — a doubled or internal face |
//! | **clean** | how many triangles have no area, before and after welding |
//! | **shape** | connected components, Euler characteristic and genus, so a report can say "two shells" or "a torus" |
//! | **folds** | adjacent triangle pairs whose normals are antiparallel: a fin of zero thickness, or two sheets pleated back on each other. Reported, never a verdict — see [`Fold`] |
//!
//! The counts come from the *welded* mesh (vertices merged within
//! [`SolidOptions::weld_eps`]): a tessellator emits every face with its own
//! vertices, so an unwelded mesh has a boundary edge along every face seam.
//!
//! Why this lives in `cadpole-core` and not in the print checks: the mesh kernel
//! needs it before it will accept an operand (`to_manifold`), the print check
//! reports it, `cadpole-bench --signature` pins it, and the example suite asks it
//! of every body on every kernel. It is the one definition of "a solid" they
//! all share. The print checks add *where* (hole rims, clustered positions)
//! on top of the raw positions this returns.

use std::collections::HashMap;

use crate::geom::Vec3;
use crate::render::bvh::Bvh;
use crate::tessellate::Mesh;

/// How the checks are run.
#[derive(Clone, Copy, Debug)]
pub struct SolidOptions {
    /// Vertices closer than this merge before the edge map is built, in mm.
    /// The default is the distance the mesh kernel welds at, so "closed" means
    /// the same thing here as it does to `to_manifold`.
    pub weld_eps: f32,
    /// Above this many triangles the pairwise search (crossings and coplanar
    /// overlaps) is skipped and [`SolidReport::pairs_skipped`] says so, rather
    /// than spending a minute on a check nobody asked for. `usize::MAX` never
    /// skips.
    pub pair_limit: usize,
}

impl Default for SolidOptions {
    fn default() -> Self {
        SolidOptions { weld_eps: crate::mesh_kernel::WELD_EPS, pair_limit: 250_000 }
    }
}

/// Two triangles that meet where they should not: the indices into the
/// welded mesh, a point between them, and how much they share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrianglePair {
    pub tris: (u32, u32),
    pub at: Vec3,
    /// For a crossing, the length of the segment the two triangles have in
    /// common, in mm; for an overlap, the area they share, in mm².
    pub extent: f64,
    /// For a crossing, how far the one that passes through the other reaches
    /// past its plane, in mm: a hair says the surfaces graze, millimetres say
    /// they run through each other. Zero for an overlap.
    pub depth: f64,
}

/// Two triangles on one edge facing opposite ways: a fold.
///
/// Topologically it is nothing — every edge still has its two triangles,
/// wound against each other — which is why the edge checks cannot see it and
/// why it does not decide [`SolidReport::is_solid`]. Geometrically it is a
/// sheet of zero (or near-zero) thickness: an exact fin when the two free
/// corners coincide, a pleat when they are a hair apart. The mesh kernel's
/// exact boolean engine tolerates one, but manifold's `Auto` dispatch reads
/// it as coincident surface and routes the next boolean through the robust
/// engine, which keeps no face provenance — the impeller's wheel came back
/// from a bore cut as one face per triangle that way, from a single fold
/// whose tips were 0.07 mm apart. That is why it is reported here with its
/// position and its tip gap: the next such defect should be readable off
/// `cadpole inspect`, not found by instrumenting the kernel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fold {
    /// Indices of the two triangles, in the mesh the fold was found in.
    pub tris: (u32, u32),
    /// Midpoint of the shared edge.
    pub at: Vec3,
    /// Dot product of the two unit normals: at most [`FOLD_DOT`], and -1 for
    /// an exact fin.
    pub dot: f64,
    /// Distance between the two free corners (the tips), in mm. Zero for an
    /// exact fin; a pleat's tips are further apart. The mesh cleanup merges
    /// tips within twice the tessellation tolerance and leaves the rest.
    pub tip_gap: f64,
    /// The face id (`Mesh::face_ids`, a provenance id in the mesh kernel)
    /// each triangle carries, so a caller can say which named faces fold.
    pub faces: (u32, u32),
}

/// Two adjacent triangles count as a fold when the dot product of their unit
/// normals is below this: within about 2.6 degrees of antiparallel. Tight on
/// purpose — a thin but real feature, a fin a few degrees open, is geometry
/// and stays out of the report — and the same threshold the cleanup's tip
/// merge uses, so what it leaves is what this counts.
pub const FOLD_DOT: f64 = -0.999;

/// The folds in an index mesh: every edge with exactly two triangles whose
/// normals are antiparallel (dot below [`FOLD_DOT`]). Degenerate triangles
/// have no normal and never fold.
///
/// This is the one implementation. [`validate_welded`] runs it on the welded
/// mesh, and the mesh kernel runs it on manifold's own triangles before and
/// after cleaning, where it is the trace that shows a fin.
/// `face_ids` may be shorter than `tris` (or empty); a missing id reads 0.
///
/// Ordered with the closest tips first — the pair most likely to be a fin
/// the cleanup should have merged rather than a knife edge that is geometry
/// (the impeller's vanes have three folds by angle whose tips are millimetres
/// apart: their sharp trailing edges) — then the most exactly antiparallel,
/// then triangle index, so the first entry is the same from run to run and
/// is the one a report calls "worst".
pub fn folds(pos: &[Vec3], tris: &[[u32; 3]], face_ids: &[u32]) -> Vec<Fold> {
    // (edge, triangle) for every edge use, sorted, so the pairs come out in
    // index order and never in a hash map's.
    let mut uses: Vec<((u32, u32), u32)> = Vec::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            uses.push((if u < v { (u, v) } else { (v, u) }, i as u32));
        }
    }
    uses.sort_unstable();
    let normal = |t: &[u32; 3]| {
        let n = (pos[t[1] as usize] - pos[t[0] as usize]).cross(pos[t[2] as usize] - pos[t[0] as usize]);
        let l = n.length();
        if l > 0.0 && l.is_finite() { Some(n * (1.0 / l)) } else { None }
    };
    let face = |t: u32| face_ids.get(t as usize).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;
    while i < uses.len() {
        let e = uses[i].0;
        let mut j = i;
        while j < uses.len() && uses[j].0 == e {
            j += 1;
        }
        if j - i == 2 {
            let (a, b) = (uses[i].1, uses[i + 1].1);
            if let (Some(na), Some(nb)) = (normal(&tris[a as usize]), normal(&tris[b as usize])) {
                let dot = na.dot(nb);
                if dot < FOLD_DOT {
                    let tip = |t: &[u32; 3]| t.iter().copied().find(|&v| v != e.0 && v != e.1);
                    let tip_gap = match (tip(&tris[a as usize]), tip(&tris[b as usize])) {
                        (Some(c), Some(d)) => (pos[c as usize] - pos[d as usize]).length(),
                        _ => 0.0,
                    };
                    out.push(Fold {
                        tris: (a, b),
                        at: (pos[e.0 as usize] + pos[e.1 as usize]) * 0.5,
                        dot,
                        tip_gap,
                        faces: (face(a), face(b)),
                    });
                }
            }
        }
        i = j;
    }
    let cmp = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
    out.sort_by(|x, y| cmp(x.tip_gap, y.tip_gap).then(cmp(x.dot, y.dot)).then(x.tris.cmp(&y.tris)));
    out
}

/// [`folds`] on a [`Mesh`]'s own arrays, welded or not.
pub fn mesh_folds(mesh: &Mesh) -> Vec<Fold> {
    let pos: Vec<Vec3> = (0..mesh.vertex_count())
        .map(|i| Vec3::new(mesh.positions[i * 3] as f64, mesh.positions[i * 3 + 1] as f64, mesh.positions[i * 3 + 2] as f64))
        .collect();
    let tris: Vec<[u32; 3]> = mesh.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    folds(&pos, &tris, &mesh.face_ids)
}

/// What [`validate`] measured. Every list holds one position per defect so a
/// caller can say *where*; the counts are the lengths.
#[derive(Clone, Debug, Default)]
pub struct SolidReport {
    /// Triangles after welding.
    pub triangles: usize,
    /// Vertices that at least one triangle uses, after welding.
    pub vertices: usize,
    /// Undirected edges, after welding.
    pub edges: usize,
    /// Triangles that collapsed to a line or a point when welded: two of
    /// their corners were within `weld_eps`.
    pub collapsed: usize,
    /// Centroids of the welded triangles that still have no area.
    pub degenerate: Vec<Vec3>,
    /// Edges used by exactly one triangle, directed the way that triangle
    /// walks them (so a caller can chain them into rims). Welded indices.
    pub boundary: Vec<(u32, u32)>,
    /// Midpoints of edges used by three or more triangles.
    pub non_manifold: Vec<Vec3>,
    /// Midpoints of edges whose two triangles traverse them the same way.
    pub flipped: Vec<Vec3>,
    /// A vertex of each shell wound the wrong way for how deeply it is
    /// nested: an outer shell (inside no other, or inside an even number)
    /// must face out, a cavity (inside an odd number) in. A shell inside
    /// another wound the same way covers its region twice; an inside-out
    /// shell beside the part bounds nothing. Only measured on a closed,
    /// consistently wound, embedded surface of two or more shells, and
    /// skipped with the pair search over [`SolidOptions::pair_limit`], or
    /// when the winding sums would cost more than [`NESTING_BUDGET`]
    /// triangle terms ([`SolidReport::nesting_skipped`]).
    pub misnested: Vec<Vec3>,
    /// The shell-nesting check was skipped as too costly (thousands of
    /// cavities in a large outer shell); `misnested` is then unknown, not
    /// empty.
    pub nesting_skipped: bool,
    /// Pairs of triangles that pass through each other, with the length of
    /// the crossing.
    pub crossings: Vec<TrianglePair>,
    /// Pairs of coplanar triangles that share area — a doubled face, or an
    /// internal wall left behind by a boolean — with the area shared.
    pub overlaps: Vec<TrianglePair>,
    /// The pairwise search was skipped because the mesh is over
    /// [`SolidOptions::pair_limit`]; `crossings` and `overlaps` are then
    /// unknown, not empty.
    pub pairs_skipped: bool,
    /// Signed volume by the divergence theorem: positive when the normals
    /// point out of the material.
    pub volume: f64,
    /// Connected components of the welded surface (a hollow part has two).
    pub components: usize,
    /// `V - E + F` of the welded surface.
    pub euler: i64,
    /// Total genus over the components, `(2·components − χ) / 2`. Only defined
    /// for a closed manifold surface, so `None` unless [`closed`](Self::closed).
    pub genus: Option<i64>,
    /// Adjacent triangle pairs facing opposite ways, worst first. A warning,
    /// not a verdict: see [`Fold`] and [`is_solid`](Self::is_solid).
    pub folds: Vec<Fold>,
}

impl SolidReport {
    /// Every edge is shared by exactly two triangles.
    pub fn closed(&self) -> bool {
        self.triangles > 0 && self.boundary.is_empty() && self.non_manifold.is_empty()
    }

    /// Closed, with the two triangles on every edge wound against each other
    /// and the normals pointing outward.
    pub fn oriented(&self) -> bool {
        self.closed() && self.flipped.is_empty() && self.volume > 0.0 && self.misnested.is_empty()
    }

    /// No two triangles pass through each other or lie on top of each other.
    /// True when the pairwise search was skipped: unknown is not a defect.
    pub fn embedded(&self) -> bool {
        self.crossings.is_empty() && self.overlaps.is_empty()
    }

    /// The verdict: closed, consistently oriented and embedded. Degenerate
    /// triangles and the component count do not decide it — a sliver with no
    /// area is ugly but bounds nothing wrong, and a hollow part is two shells.
    ///
    /// **Folds do not decide it either.** A fold is topologically clean —
    /// every edge has its two triangles, wound against each other — and
    /// bounds a region like any other closed surface; a slicer reads it as a
    /// zero-thickness wall it prints nothing for, and the mesh kernel's exact
    /// engine takes it as an operand. What a fold *does* is trip manifold's
    /// self-intersection verdict and so cost the next boolean its face
    /// provenance, which is a reason to say so loudly —
    /// [`warnings`](Self::warnings), [`summary`](Self::summary), `cadpole check`
    /// and `cadpole inspect` all do — and not a reason to refuse an export or
    /// fail `assert_solid` on a body that is, by every test of solidity, a
    /// solid.
    pub fn is_solid(&self) -> bool {
        self.oriented() && self.embedded()
    }

    /// The fold whose tips are closest, when there is one: the most likely
    /// to be a fin rather than a knife edge (see [`folds`]).
    pub fn worst_fold(&self) -> Option<&Fold> {
        self.folds.first()
    }

    /// Findings that do not make the surface less of a solid but that the
    /// next operation will care about: today, folds. One line each, in the
    /// words `cadpole check` prints at warning level; empty when there are none.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(f) = self.worst_fold() {
            let n = self.folds.len();
            out.push(format!(
                "{n} fold{} (two adjacent triangles facing opposite ways), the closest at ({:.3}, \
                 {:.3}, {:.3}) with its tips {:.4} mm apart -- a hair apart is a zero-thickness fin \
                 the cleanup should have merged, millimetres apart is a knife edge and geometry; a \
                 fold is not a hole, but manifold's self-intersection test can read one as \
                 coincident surface, and the next boolean on this body \
                 would then run on the robust engine and lose its face names -- `cadpole inspect` \
                 prints that verdict",
                if n == 1 { "" } else { "s" },
                f.at.x,
                f.at.y,
                f.at.z,
                f.tip_gap
            ));
        }
        out
    }

    /// The failing properties by name, for an allowlist to key on:
    /// `holes`, `non-manifold`, `flipped`, `inside-out`, `misnested-shells`,
    /// `crossings`, `internal-faces`. Empty when it is a solid.
    pub fn failing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.triangles == 0 {
            out.push("empty");
            return out;
        }
        if !self.boundary.is_empty() {
            out.push("holes");
        }
        if !self.non_manifold.is_empty() {
            out.push("non-manifold");
        }
        if !self.flipped.is_empty() {
            out.push("flipped");
        }
        if self.closed() && self.flipped.is_empty() && self.volume <= 0.0 {
            out.push("inside-out");
        }
        if !self.misnested.is_empty() {
            out.push("misnested-shells");
        }
        if !self.crossings.is_empty() {
            out.push("crossings");
        }
        if !self.overlaps.is_empty() {
            out.push("internal-faces");
        }
        out
    }

    /// Why it is not a solid, one line per failing property; empty when it is.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.triangles == 0 {
            out.push("no triangles".to_owned());
            return out;
        }
        let n = |k: usize| if k == 1 { "" } else { "s" };
        if !self.boundary.is_empty() {
            out.push(format!("{} boundary edge{} (the surface has holes)", self.boundary.len(), n(self.boundary.len())));
        }
        if !self.non_manifold.is_empty() {
            out.push(format!(
                "{} non-manifold edge{} (three or more triangles meet there; an internal wall gives four)",
                self.non_manifold.len(),
                n(self.non_manifold.len())
            ));
        }
        if !self.flipped.is_empty() {
            out.push(format!(
                "{} edge{} whose two triangles are wound the same way (inconsistent orientation)",
                self.flipped.len(),
                n(self.flipped.len())
            ));
        }
        if self.boundary.is_empty() && self.non_manifold.is_empty() && self.flipped.is_empty() && self.volume <= 0.0 {
            out.push(format!("volume {:.4} mm³ is not positive: the surface is inside out", self.volume));
        }
        if !self.misnested.is_empty() {
            out.push(format!(
                "{} shell{} wound the wrong way for how {} nested (an outer shell must face out and \
                 a cavity in; a shell inside another facing the same way counts its volume twice)",
                self.misnested.len(),
                n(self.misnested.len()),
                if self.misnested.len() == 1 { "it is" } else { "they are" }
            ));
        }
        if !self.crossings.is_empty() {
            out.push(format!(
                "{} pair{} of triangles pass through each other (longest crossing {:.4} mm, deepest {:.4} mm)",
                self.crossings.len(),
                n(self.crossings.len()),
                self.crossings.iter().map(|c| c.extent).fold(0.0, f64::max),
                self.crossings.iter().map(|c| c.depth).fold(0.0, f64::max)
            ));
        }
        if !self.overlaps.is_empty() {
            out.push(format!(
                "{} pair{} of coplanar triangles overlap (doubled or internal faces, {:.4} mm² in all)",
                self.overlaps.len(),
                n(self.overlaps.len()),
                self.overlaps.iter().map(|c| c.extent).sum::<f64>()
            ));
        }
        out
    }

    /// One line: `solid: 1 component, genus 0, 74513.2 mm³` or
    /// `not a solid: 12 boundary edges (the surface has holes); ...`, with
    /// `; warning: ...` for each of [`warnings`](Self::warnings) after it.
    pub fn summary(&self) -> String {
        let mut out = if self.is_solid() {
            format!(
                "solid: {} component{}, genus {}, {:.1} mm³{}",
                self.components,
                if self.components == 1 { "" } else { "s" },
                self.genus.map(|g| g.to_string()).unwrap_or_else(|| "?".to_owned()),
                self.volume,
                match (self.pairs_skipped, self.nesting_skipped) {
                    (true, _) => " (crossings not checked)",
                    (false, true) => " (shell nesting not checked)",
                    _ => "",
                }
            )
        } else {
            format!("not a solid: {}", self.problems().join("; "))
        };
        for w in self.warnings() {
            out.push_str("; warning: ");
            out.push_str(&w);
        }
        out
    }
}

/// Validate a mesh as it comes from the tessellator or the mesh kernel:
/// welded at the default distance, every check run.
pub fn validate(mesh: &Mesh) -> SolidReport {
    validate_with(mesh, &SolidOptions::default())
}

/// [`validate`] with the weld distance and the pair limit chosen.
pub fn validate_with(mesh: &Mesh, opts: &SolidOptions) -> SolidReport {
    let welded = mesh.welded(opts.weld_eps);
    let collapsed = mesh.triangle_count().saturating_sub(welded.triangle_count());
    validate_welded(&welded, collapsed, opts)
}

/// The checks on a mesh the caller has already welded. `collapsed` is how
/// many triangles the weld dropped, for the report.
pub fn validate_welded(welded: &Mesh, collapsed: usize, opts: &SolidOptions) -> SolidReport {
    let mut r = SolidReport { collapsed, triangles: welded.triangle_count(), ..Default::default() };
    if r.triangles == 0 {
        return r;
    }
    let scale = scale_of(welded);
    let area_eps = (scale * 1e-9).max(f64::MIN_POSITIVE);

    // ---- degenerate triangles ------------------------------------------
    for t in 0..welded.triangle_count() {
        let v = tri(welded, t);
        let (_, area) = tri_normal_area(&v);
        if area <= area_eps {
            r.degenerate.push(centroid(&v));
        }
    }

    // ---- the edge map ---------------------------------------------------
    // Key is the undirected edge (low, high); the value counts how many
    // triangles traverse it low->high and how many high->low.
    let mut edges: HashMap<(u32, u32), EdgeUse> = HashMap::with_capacity(welded.triangle_count() * 3 / 2);
    for f in welded.indices.chunks_exact(3) {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            let e = edges.entry(key).or_default();
            if a < b {
                e.forward += 1
            } else {
                e.backward += 1
            }
        }
    }
    r.edges = edges.len();

    let pos = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    // Sorted so the report is the same from run to run; a HashMap walks in
    // hash order.
    let mut keys: Vec<&(u32, u32)> = edges.keys().collect();
    keys.sort_unstable();
    for &&(a, b) in &keys {
        let e = &edges[&(a, b)];
        let n = e.forward + e.backward;
        if n == 1 {
            r.boundary.push(if e.forward == 1 { (a, b) } else { (b, a) });
        } else if n > 2 {
            r.non_manifold.push((pos(a) + pos(b)) * 0.5);
        } else if e.forward == 2 || e.backward == 2 {
            r.flipped.push((pos(a) + pos(b)) * 0.5);
        }
    }

    // ---- components, Euler characteristic, genus ------------------------
    let mut used = vec![false; welded.vertex_count()];
    for &i in &welded.indices {
        used[i as usize] = true;
    }
    r.vertices = used.iter().filter(|&&u| u).count();
    let mut uf = UnionFind::new(welded.vertex_count());
    for f in welded.indices.chunks_exact(3) {
        uf.union(f[0], f[1]);
        uf.union(f[1], f[2]);
    }
    r.components = (0..welded.vertex_count() as u32).filter(|&i| used[i as usize] && uf.find(i) == i).count();
    r.euler = r.vertices as i64 - r.edges as i64 + r.triangles as i64;
    if r.boundary.is_empty() && r.non_manifold.is_empty() {
        // χ = Σ (2 − 2 g_i) over the components, so the total genus is
        // (2c − χ) / 2. Odd only when the surface is not what it claims.
        let twice = 2 * r.components as i64 - r.euler;
        r.genus = if twice % 2 == 0 { Some(twice / 2) } else { None };
    }

    r.volume = welded.volume();

    // ---- pairs: crossings and coplanar overlaps -------------------------
    if welded.triangle_count() > opts.pair_limit {
        r.pairs_skipped = true;
    } else {
        let (crossings, overlaps) = pair_defects(welded);
        r.crossings = crossings;
        r.overlaps = overlaps;
    }

    // ---- shell nesting ----------------------------------------------------
    // Only on a surface that is closed, consistently wound and embedded:
    // nesting is undefined for shells that cross, and those already fail.
    if r.closed() && r.flipped.is_empty() && r.components > 1 && !r.pairs_skipped && r.embedded() {
        match misnested_shells(welded, &mut uf) {
            Some(m) => r.misnested = m,
            None => r.nesting_skipped = true,
        }
    }

    // ---- folds ------------------------------------------------------------
    // One pass over the edge uses, never skipped: it is linear, and it is the
    // check the pair search is blind to (the two triangles share an edge, so
    // they are never tested for crossing).
    r.folds = mesh_folds(welded);
    r
}

/// One vertex of every shell whose winding disagrees with its nesting depth
/// (see [`SolidReport::misnested`]). The surface must be closed and
/// consistently wound, so each shell's signed volume says which way it faces
/// and a point of one shell is inside another exactly when that shell's
/// winding number there is ±1. Shells are separate, so a vertex of one is
/// never on another; a bounding-box test keeps the winding sums to the shells
/// that could contain it. `None` when those sums would cost more than
/// [`NESTING_BUDGET`] triangle terms.
fn misnested_shells(welded: &Mesh, uf: &mut UnionFind) -> Option<Vec<Vec3>> {
    let pos = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut shells: Vec<Vec<[Vec3; 3]>> = Vec::new();
    for f in welded.indices.chunks_exact(3) {
        let root = uf.find(f[0]);
        let k = *index.entry(root).or_insert_with(|| {
            shells.push(Vec::new());
            shells.len() - 1
        });
        shells[k].push([pos(f[0]), pos(f[1]), pos(f[2])]);
    }
    let volume = |tris: &[[Vec3; 3]]| tris.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f64>() / 6.0;
    let bbox = |tris: &[[Vec3; 3]]| {
        let (mut lo, mut hi) = (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        for t in tris {
            for v in t {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        (lo, hi)
    };
    let vols: Vec<f64> = shells.iter().map(|t| volume(t)).collect();
    let boxes: Vec<(Vec3, Vec3)> = shells.iter().map(|t| bbox(t)).collect();
    let inside_box = |p: Vec3, (lo, hi): (Vec3, Vec3)| {
        p.x >= lo.x && p.y >= lo.y && p.z >= lo.z && p.x <= hi.x && p.y <= hi.y && p.z <= hi.z
    };
    // One winding number per (shell, other shell whose box holds it), each
    // a sum over the other's triangles. Counted before any is computed.
    let mut cost = 0usize;
    for (i, shell) in shells.iter().enumerate() {
        let p = shell[0][0];
        for (j, other) in shells.iter().enumerate() {
            if j != i && inside_box(p, boxes[j]) {
                cost += other.len();
            }
        }
        if cost > NESTING_BUDGET {
            return None;
        }
    }
    let mut out = Vec::new();
    for (i, shell) in shells.iter().enumerate() {
        let p = shell[0][0];
        let mut depth = 0usize;
        for (j, other) in shells.iter().enumerate() {
            let (lo, hi) = boxes[j];
            if j == i || p.x < lo.x || p.y < lo.y || p.z < lo.z || p.x > hi.x || p.y > hi.y || p.z > hi.z {
                continue;
            }
            if winding_number(other, p).abs() > 0.5 {
                depth += 1;
            }
        }
        let faces_out = vols[i] > 0.0;
        if faces_out != (depth % 2 == 0) {
            out.push(p);
        }
    }
    Some(out)
}

/// Triangle terms the shell-nesting check may sum (each an `atan2` and three
/// square roots): about a second, on every default validate.
pub const NESTING_BUDGET: usize = 20_000_000;

/// The generalised winding number of a closed triangle surface at `p`: the
/// solid angle it subtends over 4π (Van Oosterom and Strackee 1983). ±1
/// inside a closed shell, by its orientation, and 0 outside.
fn winding_number(tris: &[[Vec3; 3]], p: Vec3) -> f64 {
    let mut total = 0.0;
    for t in tris {
        let (a, b, c) = (t[0] - p, t[1] - p, t[2] - p);
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        let num = a.dot(b.cross(c));
        let den = la * lb * lc + a.dot(b) * lc + a.dot(c) * lb + b.dot(c) * la;
        total += 2.0 * crate::fmath::atan2(num, den);
    }
    total / (4.0 * std::f64::consts::PI)
}

#[derive(Default)]
struct EdgeUse {
    forward: u32,
    backward: u32,
}

struct UnionFind {
    parent: Vec<u32>,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        UnionFind { parent: (0..n as u32).collect() }
    }
    fn find(&mut self, mut i: u32) -> u32 {
        while self.parent[i as usize] != i {
            let p = self.parent[i as usize];
            self.parent[i as usize] = self.parent[p as usize];
            i = self.parent[i as usize];
        }
        i
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Lower root wins, so the representative is deterministic.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi as usize] = lo;
        }
    }
}

// ---------------------------------------------------------------------------
// Pairs of triangles
// ---------------------------------------------------------------------------

/// Tolerance for the pair tests, in millimetres.
///
/// It has to cover f32 quantisation *at the coordinates the mesh actually
/// occupies*, not merely its size. `Mesh` stores positions as f32, so a point
/// 125 mm from the origin is located only to about 7.5e-6 mm, and two
/// triangles that are coplanar by construction differ by that much once
/// they are written down. Deriving the tolerance from the bounding-box
/// diagonal alone gave 1.5e-7 mm there, fifty times too small, and every
/// adjacent near-coplanar pair registered as a crossing.
///
/// That is where phantom self-intersections on the Gridfinity parts came
/// from: the same foot loft reported none
/// at the origin and one at the cell centre, and a baseplate's count went 29,
/// 33, 188 as the tessellation tolerance went 0.02, 0.05, 0.005. The count
/// tracked where the body sat and how many triangles it had. Neither is
/// geometry.
///
/// Eight ulps of f32 at the furthest coordinate, never below the extent
/// term. A genuine crossing is a gross overlap, millimetres deep, so a
/// tolerance of a few hundredths of a micron costs nothing in sensitivity.
pub fn pair_eps(mesh: &Mesh) -> f64 {
    let scale = scale_of(mesh);
    let far = match mesh.bbox() {
        Some((lo, hi)) => [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].into_iter().fold(0.0f64, |m, v| m.max(v.abs())),
        None => scale,
    };
    (far * f64::from(f32::EPSILON) * 8.0).max(scale * 1e-9)
}

/// Pairs of triangles that cross, and pairs that lie in one plane and share
/// area. Broad phase is the renderer's BVH (each triangle's box against the
/// tree); the narrow phase is the plane-interval test for a crossing, and a
/// polygon clip for an overlap.
///
/// Two triangles cross when each one's plane separates the other's vertices
/// *and* the two intervals they cut out of the line where the planes meet
/// overlap. Triangles that share a vertex are not tested for crossing — they
/// meet by design — but they *are* tested for overlap, because a face folded
/// back on itself shares an edge with the triangle it covers.
///
/// Returns the crossing pairs and the overlapping pairs.
pub fn pair_defects(mesh: &Mesh) -> (Vec<TrianglePair>, Vec<TrianglePair>) {
    let bvh = Bvh::from_mesh(mesh);
    let mut crossings = Vec::new();
    let mut overlaps = Vec::new();
    let mut near = Vec::new();
    let eps = pair_eps(mesh);
    for a in 0..mesh.triangle_count() {
        let va = tri(mesh, a);
        let (na, area_a) = tri_normal_area(&va);
        if area_a == 0.0 {
            continue;
        }
        let lo = va[0].min(va[1]).min(va[2]);
        let hi = va[0].max(va[1]).max(va[2]);
        bvh.overlapping(
            [(lo.x - eps) as f32, (lo.y - eps) as f32, (lo.z - eps) as f32],
            [(hi.x + eps) as f32, (hi.y + eps) as f32, (hi.z + eps) as f32],
            &mut near,
        );
        // Index order, so the report is the same from run to run.
        near.sort_unstable_by_key(|&i| bvh.tri(i).tri);
        for &i in &near {
            let b = bvh.tri(i).tri as usize;
            if b <= a {
                continue; // each pair once
            }
            let vb = tri(mesh, b);
            let (nb, area_b) = tri_normal_area(&vb);
            if area_b == 0.0 {
                continue;
            }
            let da: [f64; 3] = [na.dot(vb[0] - va[0]), na.dot(vb[1] - va[0]), na.dot(vb[2] - va[0])];
            let db: [f64; 3] = [nb.dot(va[0] - vb[0]), nb.dot(va[1] - vb[0]), nb.dot(va[2] - vb[0])];
            let coplanar = da.iter().chain(&db).all(|d| d.abs() <= eps);
            let at = (centroid(&va) + centroid(&vb)) * 0.5;
            if coplanar {
                if let Some(area) = coplanar_overlap(&va, na, &vb, eps) {
                    overlaps.push(TrianglePair { tris: (a as u32, b as u32), at, extent: area, depth: 0.0 });
                }
                continue;
            }
            if shares_vertex(mesh, a, b) {
                continue;
            }
            if let Some((len, depth)) = crosses(&va, na, &da, &vb, nb, &db, eps) {
                crossings.push(TrianglePair { tris: (a as u32, b as u32), at, extent: len, depth });
            }
        }
    }
    (crossings, overlaps)
}

fn shares_vertex(mesh: &Mesh, a: usize, b: usize) -> bool {
    let ia = &mesh.indices[a * 3..a * 3 + 3];
    let ib = &mesh.indices[b * 3..b * 3 + 3];
    ia.iter().any(|x| ib.contains(x))
}

/// Do two triangles pass through each other? The length of the segment they
/// have in common and how deep the crossing is, when they do. `da`/`db` are
/// the other triangle's vertices measured against each plane.
///
/// Passing *through* needs one of them to reach strictly past the other's
/// plane on both sides. Two triangles that only touch — a shell resting on a
/// face, a facet tangent to a plane, every vertex within `eps` of the other's
/// plane — share a segment but cross nothing, and a mesh stored as f32
/// cannot tell that apart from coincidence anyway.
pub(crate) fn crosses(
    a: &[Vec3; 3],
    na: Vec3,
    da: &[f64; 3],
    b: &[Vec3; 3],
    nb: Vec3,
    db: &[f64; 3],
    eps: f64,
) -> Option<(f64, f64)> {
    let (sa, sb) = (straddle(da, eps), straddle(db, eps));
    if sa.is_none() && sb.is_none() {
        return None;
    }
    if all_same_side(da, eps) || all_same_side(db, eps) {
        return None;
    }
    let dir = na.cross(nb);
    let len = dir.length();
    if len <= eps {
        return None; // parallel
    }
    let dir = dir * (1.0 / len);
    // `db` holds a's vertices measured against b's plane, and vice versa.
    let (ia, ib) = (interval(a, db, dir, eps)?, interval(b, da, dir, eps)?);
    let (lo, hi) = (ia.0.max(ib.0), ia.1.min(ib.1));
    if hi - lo > eps {
        Some((hi - lo, sa.unwrap_or(0.0).max(sb.unwrap_or(0.0))))
    } else {
        None
    }
}

/// How far a triangle reaches past a plane on both sides: the smaller of its
/// furthest vertex above and below, or `None` when it does not reach past
/// `eps` on both.
fn straddle(d: &[f64; 3], eps: f64) -> Option<f64> {
    let above = d.iter().cloned().fold(0.0f64, f64::max);
    let below = d.iter().cloned().fold(0.0f64, |m, x| m.max(-x));
    if above > eps && below > eps {
        Some(above.min(below))
    } else {
        None
    }
}

fn all_same_side(d: &[f64; 3], eps: f64) -> bool {
    d.iter().all(|&x| x > eps) || d.iter().all(|&x| x < -eps)
}

/// Where the triangle crosses the line of intersection, as a range of the
/// parameter along `dir`. The two vertices on one side of the other plane and
/// the one on the other side give two crossing points.
fn interval(v: &[Vec3; 3], d: &[f64; 3], dir: Vec3, eps: f64) -> Option<(f64, f64)> {
    let p: [f64; 3] = [dir.dot(v[0]), dir.dot(v[1]), dir.dot(v[2])];
    let mut ts = Vec::with_capacity(2);
    for k in 0..3 {
        let j = (k + 1) % 3;
        let (dk, dj) = (d[k], d[j]);
        if dk.abs() <= eps {
            ts.push(p[k]);
        }
        if (dk > eps && dj < -eps) || (dk < -eps && dj > eps) {
            ts.push(p[k] + (p[j] - p[k]) * (dk / (dk - dj)));
        }
    }
    if ts.len() < 2 {
        return None;
    }
    let lo = ts.iter().cloned().fold(f64::MAX, f64::min);
    let hi = ts.iter().cloned().fold(f64::MIN, f64::max);
    Some((lo, hi))
}

/// Do two coplanar triangles share area, and how much? `b` is clipped against
/// `a`'s three edges in `a`'s own 2D frame and what is left is measured.
/// Touching along an edge or at a corner leaves nothing; a doubled face
/// leaves all of it.
///
/// The noise floor is a band `eps` wide around the smaller triangle. Two
/// triangles adjacent in one plane can fold over each other by that much
/// once their corners are written down as f32, and a sliver whose whole
/// width is below the rounding can fold completely — the mesh kernel emits
/// such slivers where a cut grazes a face. A doubled face or an internal
/// wall shares whole triangles, orders of magnitude above the band.
pub(crate) fn coplanar_overlap(a: &[Vec3; 3], na: Vec3, b: &[Vec3; 3], eps: f64) -> Option<f64> {
    // A frame in a's plane in which a is counter-clockwise: u along its first
    // edge, v = n × u, which puts a's third corner on the positive side.
    let u = (a[1] - a[0]).normalized();
    let v = na.cross(u);
    let to2 = |p: Vec3| {
        let d = p - a[0];
        (d.dot(u), d.dot(v))
    };
    let pa = [to2(a[0]), to2(a[1]), to2(a[2])];
    let mut poly: Vec<(f64, f64)> = vec![to2(b[0]), to2(b[1]), to2(b[2])];
    for k in 0..3 {
        let (p, q) = (pa[k], pa[(k + 1) % 3]);
        let inside = |x: (f64, f64)| (q.0 - p.0) * (x.1 - p.1) - (q.1 - p.1) * (x.0 - p.0) >= 0.0;
        let mut next = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let cur = poly[i];
            let prev = poly[(i + poly.len() - 1) % poly.len()];
            let (ci, pi) = (inside(cur), inside(prev));
            if ci != pi {
                // The edge prev->cur crosses the clip line; keep the crossing.
                let dp = (q.0 - p.0) * (prev.1 - p.1) - (q.1 - p.1) * (prev.0 - p.0);
                let dc = (q.0 - p.0) * (cur.1 - p.1) - (q.1 - p.1) * (cur.0 - p.0);
                let t = dp / (dp - dc);
                next.push((prev.0 + (cur.0 - prev.0) * t, prev.1 + (cur.1 - prev.1) * t));
            }
            if ci {
                next.push(cur);
            }
        }
        poly = next;
        if poly.len() < 3 {
            return None;
        }
    }
    let mut twice = 0.0;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        twice += p.0 * q.1 - q.0 * p.1;
    }
    let area = twice.abs() / 2.0;
    let perimeter = |t: &[Vec3; 3]| (t[1] - t[0]).length() + (t[2] - t[1]).length() + (t[0] - t[2]).length();
    let floor = eps * perimeter(a).min(perimeter(b));
    if area > floor.max(eps * eps) {
        Some(area)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The three corners of triangle `t`.
pub fn tri(mesh: &Mesh, t: usize) -> [Vec3; 3] {
    let i = t * 3;
    let p = |k: usize| {
        let v = mesh.indices[i + k] as usize * 3;
        Vec3::new(mesh.positions[v] as f64, mesh.positions[v + 1] as f64, mesh.positions[v + 2] as f64)
    };
    [p(0), p(1), p(2)]
}

/// Unit normal of a triangle from its winding, and its area. A degenerate
/// triangle gives `(Vec3::ZERO, 0.0)`.
pub fn tri_normal_area(v: &[Vec3; 3]) -> (Vec3, f64) {
    let n = (v[1] - v[0]).cross(v[2] - v[0]);
    let l = n.length();
    if l <= 0.0 || !l.is_finite() {
        (Vec3::ZERO, 0.0)
    } else {
        (n * (1.0 / l), l / 2.0)
    }
}

pub fn centroid(v: &[Vec3; 3]) -> Vec3 {
    (v[0] + v[1] + v[2]) * (1.0 / 3.0)
}

/// Longest diagonal of the mesh's bounding box, or 1 mm for an empty mesh.
pub fn scale_of(mesh: &Mesh) -> f64 {
    match mesh.bbox() {
        Some((lo, hi)) => (hi - lo).length().max(1e-6),
        None => 1.0,
    }
}

pub mod fixtures;

#[cfg(test)]
mod props;

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::fmath;

    #[test]
    fn a_box_is_a_solid_with_one_component_and_genus_zero() {
        let r = validate(&box_mesh(10.0, 10.0, 10.0));
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(r.components, 1);
        assert_eq!(r.genus, Some(0));
        assert_eq!(r.euler, 2, "V − E + F of a sphere is 2");
        assert_eq!((r.vertices, r.edges, r.triangles), (8, 18, 12));
        assert!((r.volume - 1000.0).abs() < 1e-6, "volume {}", r.volume);
        assert!(r.problems().is_empty());
        assert!(r.summary().starts_with("solid: 1 component, genus 0"), "{}", r.summary());
    }

    #[test]
    fn a_hollow_box_is_two_components_and_still_a_solid() {
        let r = validate(&hollow_box(20.0, 2.0));
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(r.components, 2, "outer shell and the cavity");
        assert_eq!(r.genus, Some(0));
        assert_eq!(r.euler, 4, "two spheres");
        let want = 8000.0 - fmath::powi(16.0f64, 3);
        assert!((r.volume - want).abs() < 1e-6, "volume {} want {want}", r.volume);
    }

    #[test]
    fn a_missing_top_is_four_boundary_edges() {
        let r = validate(&holed_box(20.0, 10.0, 6.0));
        assert!(!r.closed());
        assert!(!r.is_solid());
        assert_eq!(r.boundary.len(), 4);
        assert_eq!(r.genus, None, "genus is undefined with a hole");
        assert!(r.problems()[0].contains("4 boundary edges"), "{:?}", r.problems());
    }

    #[test]
    fn an_internal_wall_is_four_triangles_on_an_edge() {
        // Two boxes side by side sharing the x = 10 face, both closed: the
        // shared face is emitted twice, back to back, and every edge of it is
        // used by four triangles. That is what a boolean that left the seam
        // in looks like.
        let mut m = box_mesh(10.0, 10.0, 10.0);
        let b = box_between(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0));
        append(&mut m, &b);
        let r = validate(&m);
        assert!(!r.closed(), "{}", r.summary());
        assert!(!r.non_manifold.is_empty(), "the seam's edges");
        assert_eq!(
            r.non_manifold.len(),
            5,
            "the four rim edges of the shared face and its diagonal: {:?}",
            r.non_manifold
        );
        // The two copies of the face also lie on top of each other.
        assert!(!r.overlaps.is_empty(), "the doubled face overlaps itself");
        assert!(r.problems().iter().any(|p| p.contains("non-manifold")), "{:?}", r.problems());
        assert_eq!(r.failing(), vec!["non-manifold", "internal-faces"]);
    }

    #[test]
    fn one_backwards_face_is_flipped_edges() {
        let r = validate(&flipped_face_box(8.0, 8.0, 8.0));
        assert!(r.closed(), "still every edge on two triangles");
        assert!(!r.oriented());
        assert_eq!(r.flipped.len(), 4, "{:?}", r.flipped);
        assert!(!r.is_solid());
    }

    #[test]
    fn an_inside_out_box_has_negative_volume() {
        let mut m = Mesh::default();
        add_box(&mut m, Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0), true, 0);
        let r = validate(&m);
        assert!(r.closed());
        assert!(r.flipped.is_empty(), "consistently wound, just inward");
        assert!(r.volume < 0.0);
        assert!(!r.oriented());
        assert!(r.problems().iter().any(|p| p.contains("inside out")), "{:?}", r.problems());
    }

    #[test]
    fn a_zero_area_triangle_is_counted_where_it_is() {
        let r = validate(&box_with_degenerate(10.0, 10.0, 10.0));
        assert_eq!(r.degenerate.len(), 1);
        assert!((r.degenerate[0] - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-9, "{:?}", r.degenerate[0]);
        // It hangs off an edge, so the box is no longer closed either.
        assert!(!r.closed());
    }

    #[test]
    fn two_boxes_through_each_other_cross() {
        let r = validate(&overlapping_boxes());
        assert!(!r.crossings.is_empty(), "the skins cross");
        assert!(r.overlaps.is_empty(), "nothing coplanar here: {:?}", r.overlaps);
        assert!(!r.is_solid());
        assert!(r.problems().iter().any(|p| p.contains("pass through")), "{:?}", r.problems());
    }

    #[test]
    fn a_face_doubled_in_place_is_an_overlap_not_a_crossing() {
        // The +Z face of a box emitted a second time with a *different*
        // triangulation, so no edge is shared and the edge map is clean —
        // which is exactly the case the edge checks cannot see.
        let mut m = box_mesh(10.0, 10.0, 10.0);
        let (a, b, c, d) = (
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::new(10.0, 10.0, 10.0),
            Vec3::new(0.0, 10.0, 10.0),
        );
        // The other diagonal, and pulled in from the rim so no vertex welds
        // and no edge reaches the side faces.
        let pull = |p: Vec3| Vec3::new(1.0 + p.x * 0.8, 1.0 + p.y * 0.8, p.z);
        push_tri(&mut m, pull(a), pull(b), pull(d), 7);
        push_tri(&mut m, pull(b), pull(c), pull(d), 7);
        let r = validate(&m);
        assert!(r.crossings.is_empty(), "coplanar is not crossing: {:?}", r.crossings);
        assert!(!r.overlaps.is_empty(), "the doubled face");
        assert!(!r.is_solid());
        assert!(r.problems().iter().any(|p| p.contains("coplanar")), "{:?}", r.problems());
        // The patch is open, so it is also four boundary edges.
        assert_eq!(r.failing(), vec!["holes", "internal-faces"]);
    }

    #[test]
    fn coplanar_neighbours_that_only_touch_do_not_overlap() {
        // Two triangles of one flat face share an edge; a third touches at a
        // corner. Neither shares area.
        let mut m = Mesh::default();
        let z = 3.0;
        push_tri(&mut m, Vec3::new(0.0, 0.0, z), Vec3::new(10.0, 0.0, z), Vec3::new(0.0, 10.0, z), 0);
        push_tri(&mut m, Vec3::new(10.0, 0.0, z), Vec3::new(10.0, 10.0, z), Vec3::new(0.0, 10.0, z), 0);
        push_tri(&mut m, Vec3::new(10.0, 10.0, z), Vec3::new(20.0, 10.0, z), Vec3::new(10.0, 20.0, z), 0);
        let (crossings, overlaps) = pair_defects(&m.welded(1e-4));
        assert!(crossings.is_empty());
        assert!(overlaps.is_empty(), "{overlaps:?}");
    }

    #[test]
    fn a_sliver_folded_by_rounding_is_not_an_internal_face() {
        // Two coplanar triangles sharing an edge, the second a sliver whose
        // far corner is folded back over the first by less than the pair
        // tolerance: what f32 rounding does to a grazing cut. Not a defect.
        let mut m = Mesh::default();
        let (a, b, c) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0));
        push_tri(&mut m, a, b, c, 0);
        // Folded over `a b c` by 1e-6 mm along the shared edge `a b`.
        push_tri(&mut m, b, a, Vec3::new(5.0, 1e-6, 0.0), 0);
        let (_, overlaps) = pair_defects(&m.welded(1e-4));
        assert!(overlaps.is_empty(), "{overlaps:?}");
        // ...but a fold a tenth of a millimetre deep is one.
        let mut m = Mesh::default();
        push_tri(&mut m, a, b, c, 0);
        push_tri(&mut m, b, a, Vec3::new(5.0, 0.1, 0.0), 0);
        let (_, overlaps) = pair_defects(&m.welded(1e-4));
        assert_eq!(overlaps.len(), 1, "{overlaps:?}");
        assert!((overlaps[0].extent - 0.5).abs() < 1e-6, "half of 10 × 0.1: {}", overlaps[0].extent);
    }

    #[test]
    fn a_torus_has_genus_one() {
        let r = validate(&torus_mesh(10.0, 3.0, 24, 12));
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(r.components, 1);
        assert_eq!(r.euler, 0);
        assert_eq!(r.genus, Some(1));
    }

    #[test]
    fn a_single_box_does_not_intersect_itself() {
        let (c, o) = pair_defects(&box_mesh(10.0, 10.0, 10.0).welded(1e-4));
        assert!(c.is_empty() && o.is_empty());
    }

    #[test]
    fn a_hollow_box_does_not_intersect_itself() {
        let (c, o) = pair_defects(&hollow_box(20.0, 1.6).welded(1e-4));
        assert!(c.is_empty() && o.is_empty(), "the shells are 1.6 mm apart");
    }

    /// Whether a body crosses itself cannot depend on where it is standing.
    ///
    /// It did. `Mesh` stores positions as f32, so the same part modelled 200 mm
    /// from the origin is written down less precisely than one at the origin,
    /// and a tolerance derived from the body's *size* did not know that. Every
    /// Gridfinity part reported phantom self-intersections whose count tracked
    /// the cell position and the triangle count.
    #[test]
    fn moving_a_body_does_not_make_it_cross_itself() {
        use crate::geom::Transform;
        let at_origin = hollow_box(20.0, 1.6).welded(1e-4);
        for offset in [20.75, 200.0, 1000.0] {
            let moved = at_origin.transformed(&Transform::translation(Vec3::new(offset, offset, 0.0)));
            let r = validate(&moved);
            assert!(r.is_solid(), "moved {offset} mm: {}", r.summary());
        }
    }

    /// ...and the tolerance must not have grown so far that a real overlap is
    /// missed, wherever it sits.
    #[test]
    fn a_real_crossing_is_still_found_far_from_the_origin() {
        use crate::geom::Transform;
        let m = overlapping_boxes();
        for offset in [200.0, 1000.0] {
            let moved = m.transformed(&Transform::translation(Vec3::new(offset, offset, 0.0)));
            assert!(!validate(&moved).crossings.is_empty(), "missed a real crossing at {offset} mm out");
        }
    }

    #[test]
    fn the_pair_search_can_be_skipped_and_says_so() {
        let r = validate_with(&overlapping_boxes(), &SolidOptions { pair_limit: 1, ..Default::default() });
        assert!(r.pairs_skipped);
        assert!(r.crossings.is_empty(), "unknown, not found");
        assert!(r.summary().contains("not checked"), "{}", r.summary());
    }

    #[test]
    fn an_empty_mesh_is_not_a_solid() {
        let r = validate(&Mesh::default());
        assert!(!r.is_solid());
        assert_eq!(r.problems(), vec!["no triangles".to_owned()]);
    }

    /// The impeller's defect, on its fixture: a fold is found, located and
    /// measured, and the surface it sits on is still a solid — the edge
    /// checks are clean and the pair search never tests two triangles that
    /// share an edge. What the report adds is the warning, in the summary
    /// and on its own.
    #[test]
    fn a_fold_is_reported_and_the_surface_is_still_a_solid() {
        let m = slit_box(0.05);
        let r = validate(&m);
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(r.folds.len(), 1, "{:?}", r.folds);
        let f = r.worst_fold().unwrap();
        assert!(f.dot < FOLD_DOT, "{f:?}");
        assert!((f.tip_gap - 0.05).abs() < 1e-6, "tips 0.05 apart: {f:?}");
        assert!((f.at - Vec3::new(5.0, 5.0, 1.0)).length() < 1e-6, "the shared edge's midpoint: {f:?}");
        assert_eq!(f.faces, (7, 8), "the two slit walls, in triangle order");
        assert!(r.failing().is_empty(), "a fold is not a failing check");
        assert!(r.problems().is_empty());
        assert_eq!(r.warnings().len(), 1);
        assert!(r.warnings()[0].starts_with("1 fold ("), "{:?}", r.warnings());
        assert!(r.warnings()[0].contains("0.0500 mm apart"), "{:?}", r.warnings());
        assert!(r.summary().starts_with("solid: "), "{}", r.summary());
        assert!(r.summary().contains("; warning: 1 fold"), "{}", r.summary());
    }

    /// Tips that coincide weld into one vertex, and the slit's walls are then
    /// two triangles on the same three vertices with opposite winding: the
    /// exact fin, dot -1, no gap. Found by the same test — and, unlike the
    /// near-fin, by the pair search too, as a doubled face, with the walls'
    /// other edges now four triangles deep. That is the gap the fold check
    /// fills: a hair of tip gap and every other check goes quiet.
    #[test]
    fn an_exact_fin_is_a_fold_with_no_tip_gap() {
        let r = validate(&slit_box(0.0));
        assert_eq!(r.folds.len(), 1, "{:?}", r.folds);
        let f = r.folds[0];
        assert!(f.dot < -0.9999, "{f:?}");
        assert_eq!(f.tip_gap, 0.0);
        assert!(!r.is_solid(), "{}", r.summary());
        assert!(r.failing().contains(&"internal-faces"), "{:?}", r.failing());
    }

    /// A slit open by a few degrees is a thin feature, not a fold: the
    /// threshold is tight so a real pleat stays out of the report.
    #[test]
    fn a_fin_open_by_a_few_degrees_is_geometry_not_a_fold() {
        // Tips 1.0 mm apart over a 9 mm slit: about 6 degrees open.
        let r = validate(&slit_box(1.0));
        assert!(r.is_solid(), "{}", r.summary());
        assert!(r.folds.is_empty(), "{:?}", r.folds);
        assert!(r.warnings().is_empty());
        assert!(!r.summary().contains("warning"), "{}", r.summary());
        // And the plain shapes have none either.
        assert!(validate(&box_mesh(10.0, 10.0, 10.0)).folds.is_empty());
        assert!(validate(&hollow_box(20.0, 2.0)).folds.is_empty());
        assert!(validate(&torus_mesh(10.0, 3.0, 24, 12)).folds.is_empty());
    }

    /// The order is by geometry and index, never by a hash map's walk, so
    /// "the worst fold" is the same fold every run.
    #[test]
    fn folds_are_reported_in_a_fixed_order() {
        let mut m = slit_box(0.05);
        let second = slit_box(0.02).transformed(&crate::geom::Transform::translation(Vec3::new(30.0, 0.0, 0.0)));
        append(&mut m, &second);
        let a = validate(&m).folds;
        let b = validate(&m).folds;
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
        // The most exactly antiparallel pair first: the tighter fin. (Its
        // positions are f32, and 35 mm out that is good to a few microns.)
        assert!(a[0].dot <= a[1].dot, "{a:?}");
        assert!((a[0].tip_gap - 0.02).abs() < 1e-4, "{a:?}");
    }
}
