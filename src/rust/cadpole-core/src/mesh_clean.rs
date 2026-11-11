//! Cleaning the mesh a boolean hands back: degenerate triangles, slivers and
//! folded sheets go, and the surface stays a closed 2-manifold throughout.
//!
//! **Why this exists.** Manifold's boolean is exact, and exactness is the
//! problem: two tessellated surfaces that cross at a grazing angle, or that
//! nearly coincide (a foot trimmed 0.25 mm by the body's own outline in
//! `examples/gridfinity_bin.star`), produce triangles that are geometrically
//! correct and a few hundredths of a millimetre wide. Manifold removes only
//! what is degenerate at *its* epsilon (a few nanometres at this scale), so
//! the result carried 28 zero-area triangles and a 0.03 mm sheet along the
//! base perimeter — which the path tracer renders as a bright patch when the
//! material is glass, and which a slicer reads as a wall. `Mesh::welded`
//! collapsed the zero-area ones, but only on a copy for `cadpole check`; the mesh
//! that was rendered and exported kept everything.
//!
//! **What it does.** Three operations, each one a topological edit that keeps
//! the mesh a closed, consistently oriented 2-manifold, repeated until nothing
//! is left to do:
//!
//! 1. **Short-edge collapse.** An edge shorter than `eps` is collapsed onto
//!    one of its ends. The end that survives is the one where more named
//!    faces meet, so a body's corners never move; the other end moves by less
//!    than `eps`. The collapse is refused when it would make the mesh
//!    non-manifold (the *link condition*: the two ends may share no
//!    neighbour beyond the apexes of the two triangles on the edge) or turn a
//!    well-shaped triangle inside out.
//! 2. **Cap flip.** A triangle whose smallest altitude is under `eps` but
//!    whose edges are all longer has its apex within `eps` of its longest
//!    edge. That edge is flipped: the triangle across it is re-cut through
//!    the apex, which is the same as splitting the neighbour there and
//!    dropping the cap. Nothing moves: the neighbour's surface passes through
//!    a point it was already within `eps` of, and the two pieces take the
//!    neighbour's face, because the cap's own face had no measurable area
//!    there. A flip is taken only when it strictly raises the smaller of the
//!    two altitudes, which is what makes the pass terminate.
//! 3. **Fold removal.** Two triangles on the same three vertices with
//!    opposite winding are a sheet of zero thickness, which is what a
//!    collapse leaves behind when it closes a sub-`eps` pocket. Both go, and
//!    the edges around them pair up with each other.
//!
//! Dropping a triangle on its own would open a hole; every removal here is a
//! collapse or a split, which is why the result can be checked afterwards
//! with [`closed_manifold`] and, should any pass have gone wrong, the
//! uncleaned mesh is returned instead of a damaged one.
//!
//! **And then the whole thing is measured.** Each edit is bounded by `eps`
//! and keeps the surface closed, but on a body whose *own* surfaces run
//! closer together than `eps` — `examples/gridfinity_baseplate.star` has a
//! cup wall and a slab wall 0.01 mm apart — moving one of them by less than
//! `eps` can still push it through the other. No local test sees that coming,
//! because the two surfaces are meant to be that close. So the cleaned mesh
//! is compared with the one that arrived using [`crate::solid::validate`],
//! the definition of a solid, and dropped whole if the body came out
//! *less* of a solid than it went in. A model like that keeps its slivers,
//! which is the conservative half of "never change the shape": the cleanup
//! may decline, but it may not do harm.
//!
//! **The epsilon.** `eps` is the tessellation tolerance — 0.05 mm by default.
//! It is the right size because it is the only size the mesh knows: a curved
//! face was flattened into chords that stray up to `eps` from the true
//! surface, so a feature thinner than `eps` is inside the noise and nothing
//! downstream (render, STL, the print checks) can tell it from an artefact.
//! Welding *by position* at that size would be wrong, though: two faces
//! 0.04 mm apart that were meant to be apart would fuse, and a deliberate
//! 0.04 mm gap is a thing a script can ask for. The operations above never do
//! that. They act only along existing edges, so their reach is topological,
//! not spatial: a vertex moves only towards a vertex it already shares an
//! edge with, and two sheets that do not touch cannot be brought together
//! however close they lie. What *can* disappear is a fin or a rim of the
//! solid itself that is thinner than `eps` — geometry the tessellation was
//! never accurate enough to promise.

use std::collections::{HashMap, HashSet};

use crate::geom::Vec3;

/// What a [`clean`] did, for a trace line or an assertion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanReport {
    /// Edges shorter than `eps` collapsed (two triangles each).
    pub collapsed_edges: usize,
    /// Slivers snapped onto their long edge.
    pub snapped_slivers: usize,
    /// Near-folds whose tips were merged (`Work::merge_fold_tips`).
    pub fold_tips: usize,
    /// Triangles removed as halves of a folded pair or for a repeated corner.
    pub dropped: usize,
    /// Passes run, including the one that found nothing to do.
    pub passes: usize,
    /// The pass whose edits broke the surface, when one did; its edits were
    /// thrown away and the cleanup stopped there.
    pub stopped_at_pass: Option<usize>,
    /// Set when the cleaned mesh failed the manifold check and the input was
    /// handed back unchanged. Never expected; guarded against anyway.
    pub abandoned: bool,
    /// Why the clean did not run to completion, when it did not: the reason
    /// behind `stopped_at_pass` or `abandoned`, for the note to the user.
    pub stop: Option<Stop>,
}

/// Why a [`clean`] kept less than every pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A pass's edits left the surface not closed; the passes before it were
    /// kept (or nothing, when it was the first).
    SurfaceBroke,
    /// The finished clean crossed itself more than the input did, and the
    /// last earlier pass that did not was kept — or none could be, and the
    /// input was handed back.
    CrossedMore,
    /// The input was not a closed manifold to begin with, so no state was
    /// safe to fall back to, and the finished clean was not closed either.
    NotClosed,
}

impl CleanReport {
    pub fn changed(&self) -> bool {
        !self.abandoned && (self.collapsed_edges + self.snapped_slivers + self.dropped) > 0 || self.fold_tips > 0
    }

    /// What a user should be told about this clean, or nothing: a clean that
    /// ran to the end is not news, one that stopped early or gave up is,
    /// because the slivers and folds it would have removed are still in the
    /// body, and the next boolean on that body may pay for them (its operand
    /// then trips manifold's self-intersection test and is routed through
    /// the engine that keeps no face provenance).
    pub fn note(&self) -> Option<String> {
        let kept = format!(
            "{} short edge{} collapsed, {} sliver{} snapped, {} fold tip{} merged, {} triangle{} dropped",
            self.collapsed_edges,
            s(self.collapsed_edges),
            self.snapped_slivers,
            s(self.snapped_slivers),
            self.fold_tips,
            s(self.fold_tips),
            self.dropped,
            s(self.dropped)
        );
        let consequence = "what it did not remove stays in the body: triangles thinner than the \
                           tolerance, and any fold, which manifold's self-intersection test reads as \
                           coincident surface, so the next boolean on this body may run on the robust \
                           engine and lose its face names. `cadpole inspect` lists what is left";
        match (self.abandoned, self.stopped_at_pass, self.stop) {
            (true, _, why) => Some(format!(
                "mesh clean abandoned: {}, so the boolean's result is used exactly as the kernel \
                 returned it. {consequence}",
                match why {
                    Some(Stop::CrossedMore) =>
                        "the cleaned surface crossed itself more than the input did and no earlier \
                         pass was any better",
                    Some(Stop::NotClosed) | Some(Stop::SurfaceBroke) | None =>
                        "the cleaned surface was not a closed manifold and the input had no closed \
                         state to fall back to",
                }
            )),
            (false, Some(at), why) => Some(format!(
                "mesh clean stopped at pass {at} of {}: {}, so the passes before it were kept \
                 ({kept}). {consequence}",
                self.passes,
                match why {
                    Some(Stop::CrossedMore) =>
                        "the finished clean crossed itself more than the input did, and that pass \
                         is the last one that did not",
                    _ => "that pass's edits broke the surface",
                }
            )),
            (false, None, _) => None,
        }
    }
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Every collapse removes two triangles and every flip strictly raises the
/// sorted vector of altitudes ([`Work::snap`]), so the loop ends on its own;
/// a chain of `k` colinear slivers along one cut line unwinds from the
/// outside in, one or two per pass, and this is a bound well past any chain
/// a boolean has been seen to leave.
const MAX_PASSES: usize = 64;

/// Clean `tris`/`prov` in place. `pos` is read only: a surviving vertex never
/// moves, a removed one is simply no longer referenced. `prov[t]` is the face
/// (provenance id) of triangle `t` and is kept in step.
pub fn clean(pos: &[Vec3], tris: &mut Vec<[u32; 3]>, prov: &mut Vec<u32>, eps: f64) -> CleanReport {
    debug_assert_eq!(tris.len(), prov.len());
    if tris.is_empty() || !(eps > 0.0) {
        return CleanReport::default();
    }
    let mut w = Work::new(pos, tris, prov, eps);
    let mut report = CleanReport::default();
    // The state after every pass that left the surface closed, so that a pass
    // which breaks it -- or, at the end, a result that crosses itself more
    // than the input did -- costs only the passes from there on, not the
    // whole clean: on a 90,000-triangle boolean result, one bad edit late on
    // must not discard tens of thousands of good ones. Thinned to `HISTORY`
    // entries: the bisection
    // below then lands within a few passes of the best state.
    let input_closed = closed_manifold(tris.iter().copied());
    let mut history: Vec<Snapshot> = Vec::new();
    for pass in 0..MAX_PASSES {
        report.passes = pass + 1;
        let mut changed = 0;
        for t in 0..w.tris.len() {
            if !w.alive[t] {
                continue;
            }
            for k in 0..3 {
                let (u, v) = (w.tris[t][k], w.tris[t][(k + 1) % 3]);
                if (w.pos[u as usize] - w.pos[v as usize]).length() < eps {
                    let (gone, kept) = w.pick(u, v);
                    if w.collapse(gone, kept) {
                        changed += 1;
                        report.collapsed_edges += 1;
                        break; // `t` is dead now
                    }
                }
            }
        }
        for t in 0..w.tris.len() {
            if w.alive[t] && w.snap(t) {
                changed += 1;
                report.snapped_slivers += 1;
            }
        }
        let merged = w.merge_fold_tips();
        changed += merged;
        report.fold_tips += merged;
        let dropped = w.drop_folds();
        changed += dropped;
        report.dropped += dropped;
        if changed == 0 {
            break;
        }
        // An input that was not closed to begin with has no closed state to
        // fall back to; it takes the all-or-nothing path below.
        if !input_closed {
            continue;
        }
        if closed_manifold((0..w.tris.len()).filter(|&t| w.alive[t]).map(|t| w.tris[t])) {
            history.push(Snapshot { tris: w.tris.clone(), prov: w.prov.clone(), alive: w.alive.clone(), report });
            if history.len() > HISTORY {
                let kept: Vec<Snapshot> = history.drain(..).enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
                history = kept;
            }
        } else {
            report.stopped_at_pass = Some(pass + 1);
            report.stop = Some(Stop::SurfaceBroke);
            match history.pop() {
                Some(snap) => snap.restore(&mut w, &mut report),
                None => return report_stopped(report),
            }
            break;
        }
    }
    if !report.changed() {
        return report;
    }
    let out: Vec<(usize, [u32; 3])> = (0..w.tris.len()).filter(|&t| w.alive[t]).map(|t| (t, w.tris[t])).collect();
    if !closed_manifold(out.iter().map(|(_, t)| *t)) {
        // Only reachable for an input that was not closed to begin with: a
        // closed input's passes are each checked above. Said through the
        // report rather than swallowed, because the mesh that goes back is
        // the uncleaned one.
        report.abandoned = true;
        report.stop = Some(Stop::NotClosed);
        return report;
    }
    // Every edit above is bounded by `eps` and every one of them keeps the
    // surface closed — but on a model whose surfaces come *closer together*
    // than `eps`, moving one by less than that can still push it through the
    // one beside it. `examples/gridfinity_baseplate.star` has a cup wall and a
    // slab wall 0.01 mm apart and does exactly that: cleaning it produced
    // crossing triangles where there had been none. There is no local repair,
    // because the two surfaces are *meant* to be that close, so the whole
    // clean is measured against the mesh it started from and dropped if it
    // made the body less of a solid (by the definition every other
    // gate uses). A model like that keeps its slivers, which is the
    // conservative half of "never change the shape".
    //
    // The second validation is the expensive one and usually does not run: a
    // result with no crossings and no overlaps cannot have more of them than
    // its input did.
    let mut out = out;
    let after = crate::solid::validate(&as_mesh(pos, out.iter().map(|(_, t)| *t)));
    if !after.crossings.is_empty() || !after.overlaps.is_empty() {
        let before = crate::solid::validate(&as_mesh(pos, tris.iter().copied()));
        let no_worse = |v: &crate::solid::SolidReport| {
            v.crossings.len() <= before.crossings.len() && v.overlaps.len() <= before.overlaps.len()
        };
        if !no_worse(&after) {
            // The current state is the last snapshot (or later than any).
            // Find the last earlier snapshot that is no worse than the input,
            // by bisection over the history, assuming the damage, once done,
            // stays done -- which is what a push-through is.
            if !input_closed || history.len() < 2 {
                report.abandoned = true;
                report.stop = Some(Stop::CrossedMore);
                return report;
            }
            let (mut lo, mut hi) = (0usize, history.len() - 1);
            let mut best: Option<usize> = None;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let snap = &history[mid];
                let v = crate::solid::validate(&as_mesh(pos, (0..snap.tris.len()).filter(|&t| snap.alive[t]).map(|t| snap.tris[t])));
                if no_worse(&v) {
                    best = Some(mid);
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            let Some(i) = best else {
                report.abandoned = true;
                report.stop = Some(Stop::CrossedMore);
                return report;
            };
            let snap = history.swap_remove(i);
            let passes = report.passes;
            report = snap.report;
            report.passes = passes;
            report.stopped_at_pass = Some(snap.report.passes + 1);
            report.stop = Some(Stop::CrossedMore);
            w.tris = snap.tris;
            w.prov = snap.prov;
            w.alive = snap.alive;
            out = (0..w.tris.len()).filter(|&t| w.alive[t]).map(|t| (t, w.tris[t])).collect();
        }
    }
    let new_prov: Vec<u32> = out.iter().map(|(t, _)| w.prov[*t]).collect();
    *tris = out.into_iter().map(|(_, t)| t).collect();
    *prov = new_prov;
    report
}

/// How many per-pass states [`clean`] keeps for its bisection.
const HISTORY: usize = 32;

/// The state after one pass of [`clean`]: enough to resume the final checks
/// from, not enough to keep editing (the vertex stars are rebuilt on demand).
struct Snapshot {
    tris: Vec<[u32; 3]>,
    prov: Vec<u32>,
    alive: Vec<bool>,
    report: CleanReport,
}

impl Snapshot {
    fn restore(self, w: &mut Work<'_>, report: &mut CleanReport) {
        let (stopped, stop, passes) = (report.stopped_at_pass, report.stop, report.passes);
        w.tris = self.tris;
        w.prov = self.prov;
        w.alive = self.alive;
        *report = self.report;
        // The counts are the snapshot's; the stop, its reason and the number
        // of passes actually run are the clean's.
        report.stopped_at_pass = stopped;
        report.stop = stop;
        report.passes = passes;
    }
}

/// A clean whose very first pass broke the surface: nothing to keep.
fn report_stopped(mut report: CleanReport) -> CleanReport {
    report.collapsed_edges = 0;
    report.snapped_slivers = 0;
    report.fold_tips = 0;
    report.dropped = 0;
    report
}

/// Positions and triangles as a [`Mesh`](crate::tessellate::Mesh), for the
/// validator. Normals are not filled in: nothing the solid checks measure
/// reads them.
fn as_mesh(pos: &[Vec3], tris: impl Iterator<Item = [u32; 3]>) -> crate::tessellate::Mesh {
    let indices: Vec<u32> = tris.flatten().collect();
    crate::tessellate::Mesh {
        positions: pos.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect(),
        normals: vec![0.0; pos.len() * 3],
        face_ids: vec![0; indices.len() / 3],
        indices,
        colors: Vec::new(),
    }
}

/// Every undirected edge is used by exactly two triangles, one each way: a
/// closed, consistently oriented 2-manifold with no internal wall (an
/// internal double-sided sheet gives an edge four triangles, which
/// `Mesh::is_watertight`'s directed sum lets through).
pub fn closed_manifold(tris: impl IntoIterator<Item = [u32; 3]>) -> bool {
    let mut uses: HashMap<(u32, u32), (u8, u8)> = HashMap::new();
    let mut any = false;
    for t in tris {
        any = true;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a == b {
                return false;
            }
            let e = uses.entry(if a < b { (a, b) } else { (b, a) }).or_insert((0, 0));
            if a < b {
                e.0 = e.0.saturating_add(1);
            } else {
                e.1 = e.1.saturating_add(1);
            }
        }
    }
    any && uses.values().all(|&c| c == (1, 1))
}

/// Smallest altitude of a triangle: twice its area over its longest edge.
/// This is the width of a sliver, whichever way it is thin.
pub fn min_altitude(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let longest = (b - a).length().max((c - b).length()).max((a - c).length());
    if longest == 0.0 { 0.0 } else { (b - a).cross(c - a).length() / longest }
}

struct Work<'a> {
    pos: &'a [Vec3],
    tris: Vec<[u32; 3]>,
    prov: Vec<u32>,
    alive: Vec<bool>,
    /// Vertex -> triangles that use it. Entries go stale when a triangle
    /// changes; every reader checks `alive` and membership, so stale entries
    /// cost a lookup and nothing else.
    star: Vec<Vec<u32>>,
    eps: f64,
}

impl<'a> Work<'a> {
    fn new(pos: &'a [Vec3], tris: &[[u32; 3]], prov: &[u32], eps: f64) -> Work<'a> {
        let mut star: Vec<Vec<u32>> = vec![Vec::new(); pos.len()];
        for (t, tri) in tris.iter().enumerate() {
            for &v in tri {
                star[v as usize].push(t as u32);
            }
        }
        Work { pos, tris: tris.to_vec(), prov: prov.to_vec(), alive: vec![true; tris.len()], star, eps }
    }

    fn p(&self, v: u32) -> Vec3 {
        self.pos[v as usize]
    }

    fn normal_of(&self, t: [u32; 3]) -> Vec3 {
        (self.p(t[1]) - self.p(t[0])).cross(self.p(t[2]) - self.p(t[0]))
    }

    fn altitude_of(&self, t: [u32; 3]) -> f64 {
        min_altitude(self.p(t[0]), self.p(t[1]), self.p(t[2]))
    }

    /// Live triangles that use both `u` and `v`, in index order, no repeats.
    fn on_edge(&self, u: u32, v: u32) -> Vec<usize> {
        let mut out: Vec<usize> = self.star[u as usize]
            .iter()
            .map(|&t| t as usize)
            .filter(|&t| self.alive[t] && self.tris[t].contains(&u) && self.tris[t].contains(&v))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn neighbours(&self, u: u32) -> HashSet<u32> {
        let mut out = HashSet::new();
        for &t in &self.star[u as usize] {
            let t = t as usize;
            if self.alive[t] && self.tris[t].contains(&u) {
                for &v in &self.tris[t] {
                    if v != u {
                        out.insert(v);
                    }
                }
            }
        }
        out
    }

    /// The vertices at the ends of `u`'s link once the triangles in `except`
    /// are gone: those that appear in exactly one surviving triangle at `u`.
    /// Empty while the link is still a closed cycle.
    fn link_endpoints(&self, u: u32, except: &[usize]) -> HashSet<u32> {
        let mut count: HashMap<u32, u32> = HashMap::new();
        for &t in &self.star[u as usize] {
            let t = t as usize;
            if self.alive[t] && self.tris[t].contains(&u) && !except.contains(&t) {
                for &v in &self.tris[t] {
                    if v != u {
                        *count.entry(v).or_default() += 1;
                    }
                }
            }
        }
        count.into_iter().filter(|&(_, n)| n == 1).map(|(v, _)| v).collect()
    }

    /// [`Self::neighbours`], not counting the triangles in `except`.
    fn neighbours_without(&self, u: u32, except: &[usize]) -> HashSet<u32> {
        let mut out = HashSet::new();
        for &t in &self.star[u as usize] {
            let t = t as usize;
            if self.alive[t] && self.tris[t].contains(&u) && !except.contains(&t) {
                for &v in &self.tris[t] {
                    if v != u {
                        out.insert(v);
                    }
                }
            }
        }
        out
    }

    /// How many distinct faces meet at `u`: a corner counts three, a point
    /// on an edge two, a point inside a face one.
    fn faces_at(&self, u: u32) -> usize {
        let mut faces: Vec<u32> = self.star[u as usize]
            .iter()
            .map(|&t| t as usize)
            .filter(|&t| self.alive[t] && self.tris[t].contains(&u))
            .map(|t| self.prov[t])
            .collect();
        faces.sort_unstable();
        faces.dedup();
        faces.len()
    }

    /// Which end of a short edge goes: the one where fewer faces meet, so a
    /// corner outlives a point on a face. Ties keep the lower index.
    fn pick(&self, u: u32, v: u32) -> (u32, u32) {
        let (fu, fv) = (self.faces_at(u), self.faces_at(v));
        if fu < fv || (fu == fv && u > v) { (u, v) } else { (v, u) }
    }

    /// Merge `gone` into `kept`. False, and nothing changed, when the edge is
    /// not a manifold edge, the link condition fails, or a triangle would
    /// flip.
    ///
    /// The flip test does not spare slivers (altitude under `eps`): a
    /// sliver's normal is not noise at a twentieth of the tolerance — 2.5 µm
    /// of altitude on a millimetre of edge is a normal good to a milliradian
    /// — and a collapse that turns such a sliver over leaves a *fold* in the
    /// surface (the compressor wheel's two-micron trailing edge shows it). A
    /// fold is what the next boolean trips over, so no collapse may make
    /// one; the sliver it would have removed stays, which is the
    /// conservative half of "never change the shape".
    fn collapse(&mut self, gone: u32, kept: u32) -> bool {
        let shared = self.on_edge(gone, kept);
        if shared.len() != 2 {
            return false;
        }
        let apexes: Vec<u32> = shared
            .iter()
            .map(|&t| *self.tris[t].iter().find(|&&x| x != gone && x != kept).unwrap())
            .collect();
        let (ng, nk) = (self.neighbours(gone), self.neighbours(kept));
        if ng.intersection(&nk).any(|w| !apexes.contains(w)) {
            return false;
        }
        let fan: Vec<usize> = self.star[gone as usize]
            .iter()
            .map(|&t| t as usize)
            .filter(|&t| self.alive[t] && self.tris[t].contains(&gone) && !shared.contains(&t))
            .collect();
        for &t in &fan {
            let before = self.tris[t];
            let after = before.map(|x| if x == gone { kept } else { x });
            if self.altitude_of(before) >= 0.05 * self.eps && self.normal_of(before).dot(self.normal_of(after)) <= 0.0 {
                return false;
            }
        }
        for &t in &shared {
            self.alive[t] = false;
        }
        let mut fan = fan;
        fan.sort_unstable();
        fan.dedup();
        for t in fan {
            for x in self.tris[t].iter_mut() {
                if *x == gone {
                    *x = kept;
                }
            }
            self.star[kept as usize].push(t as u32);
        }
        self.star[gone as usize].clear();
        true
    }

    /// Flip the longest edge of sliver `t` — the classic cap removal: the
    /// apex `c` lies within `eps` of edge `ab`, so the triangle across `ab`
    /// is re-cut through `c` and the sliver's area (none, to within `eps`)
    /// goes to that neighbour's face. Nothing moves; the surface changes by
    /// less than the sliver's altitude.
    ///
    /// False when `t` is not a sliver, has a short edge (the collapse's
    /// job), the new diagonal already exists (the flip would pinch the
    /// mesh), a well-shaped neighbour would be turned over, or — the rule
    /// that makes the whole pass terminate — the flip does not *strictly*
    /// raise the smaller of the two altitudes. A flip that would be undone
    /// by the reverse flip fails that test one way or the other, and since
    /// every accepted flip raises the sorted vector of all altitudes
    /// lexicographically, no sequence of them can cycle.
    fn snap(&mut self, t: usize) -> bool {
        let tri = self.tris[t];
        let lens = [0, 1, 2].map(|k| (self.p(tri[(k + 1) % 3]) - self.p(tri[k])).length());
        if lens.iter().any(|&l| l < self.eps) {
            return false;
        }
        let k = (0..3).max_by(|&i, &j| lens[i].partial_cmp(&lens[j]).unwrap()).unwrap();
        let alt = self.normal_of(tri).length() / lens[k];
        if alt >= self.eps {
            return false;
        }
        let (a, b, c) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
        let shared = self.on_edge(a, b);
        if shared.len() != 2 {
            return false;
        }
        let t2 = if shared[0] == t { shared[1] } else { shared[0] };
        let tri2 = self.tris[t2];
        let Some(j) = (0..3).find(|&j| tri2[j] == b && tri2[(j + 1) % 3] == a) else {
            return false; // same winding on a shared edge: not ours to fix
        };
        let d = tri2[(j + 2) % 3];
        if d == c || !self.on_edge(c, d).is_empty() {
            return false;
        }
        let (n1, n2) = ([a, d, c], [d, b, c]);
        let before = alt.min(self.altitude_of(tri2));
        let after = self.altitude_of(n1).min(self.altitude_of(n2));
        if after <= before {
            return false;
        }
        // The quad `a d b c` must be **convex** seen along its own normal, or
        // the two new triangles do not cover the two old ones: they fold back
        // over each other, and what looks like a tidier pair of triangles is
        // a pair that crosses, or lies on top of, the surface next door.
        // That is not hypothetical — without this test the Gridfinity
        // baseplate and baselid came out of the cleanup with crossings and
        // doubled faces that `cadpole_core::solid::validate` rejects, because a
        // cap sitting against a *concave* neighbour (its far corner `d`
        // projecting way outside the shared edge) was flipped anyway.
        //
        // With it, the flip is a re-cut of one flat quadrilateral along its
        // other diagonal, and the surface moves by at most `eps`: `c` is
        // within `eps` of the line `ab`, `ab` lies in the neighbour's plane,
        // so `c` is within `eps` of that plane and the quad is that flat.
        let quad = self.normal_of(tri2) + self.normal_of(tri);
        if self.normal_of(n1).dot(quad) <= 0.0 || self.normal_of(n2).dot(quad) <= 0.0 {
            return false;
        }
        self.tris[t2] = n1;
        self.tris[t] = n2;
        self.prov[t] = self.prov[t2];
        self.star[c as usize].push(t2 as u32);
        self.star[d as usize].push(t as u32);
        true
    }

    /// Merge the tips of near-folds: two triangles on one edge facing
    /// opposite ways whose free corners lie within `eps` of each other.
    ///
    /// That is the fin `drop_folds` removes, except that the boolean left the
    /// two tips a hair apart, so by index it is two triangles and by altitude
    /// (the fin can be millimetres tall) neither is a sliver. It is still a
    /// coincident sheet, and manifold's exact engine says so on the way back
    /// in: `has_self_intersections` is true, the next boolean is routed to
    /// the robust engine, and that engine keeps no face provenance -- an
    /// impeller's wheel came back from a bore cut as 120,000 faces for
    /// 125,000 triangles that way, one face per triangle. Merging the tips
    /// turns the pair into an exact fin, which goes in the same pass. The
    /// tips may be up to `2 * eps` apart -- the one bounded edit here that
    /// exceeds `eps`, and deliberately: a fold is a defect, not geometry, the
    /// impeller's fin had its tips 0.07 mm apart at a 0.05 mm tolerance, and a
    /// pleat that thin is indistinguishable from a fin at that tolerance
    /// anyway. A fold whose tips are further apart is left alone as a real
    /// pleat. Returns how many pairs were merged.
    fn merge_fold_tips(&mut self) -> usize {
        let mut merged = 0;
        for t in 0..self.tris.len() {
            if !self.alive[t] {
                continue;
            }
            let tri = self.tris[t];
            let n1 = self.normal_of(tri);
            let l1 = n1.length();
            if l1 <= 0.0 {
                continue;
            }
            for k in 0..3 {
                let (u, v) = (tri[k], tri[(k + 1) % 3]);
                let shared = self.on_edge(u, v);
                if shared.len() != 2 {
                    continue;
                }
                let other = if shared[0] == t { shared[1] } else { shared[0] };
                let n2 = self.normal_of(self.tris[other]);
                let l2 = n2.length();
                if l2 <= 0.0 || n1.dot(n2) > -0.999 * l1 * l2 {
                    continue;
                }
                let c = tri[(k + 2) % 3];
                let d = *self.tris[other].iter().find(|&&x| x != u && x != v).unwrap();
                let explain = crate::kernel::trace::enabled();
                if c == d || (self.p(c) - self.p(d)).length() >= 2.0 * self.eps {
                    if explain {
                        eprintln!("fold at {:?}/{:?}: tips {} apart, eps {}", tri, self.tris[other], (self.p(c) - self.p(d)).length(), self.eps);
                    }
                    continue;
                }
                let (gone, kept) = self.pick(c, d);
                // Everything that goes: the fold pair, and any triangles on
                // the tip-to-tip edge (they become needles). The tips may
                // share no other neighbour than the base and those
                // triangles' apexes, or the merge pinches the surface -- the
                // edge-collapse link condition, widened by the base, which is
                // a neighbour of both tips only through the pair itself.
                let joined = self.on_edge(gone, kept);
                let mut dead = vec![t, other];
                dead.extend(joined.iter().copied());
                // The link condition. Without the triangles that go, each
                // tip's link is an open path; gluing the two makes one cycle
                // -- a manifold vertex -- exactly when both paths have the
                // same two endpoints and share nothing else. Anything else
                // the merge would pinch.
                let (eg, ek) = (self.link_endpoints(gone, &dead), self.link_endpoints(kept, &dead));
                let (ng, nk) = (self.neighbours_without(gone, &dead), self.neighbours_without(kept, &dead));
                let common: HashSet<u32> = ng.intersection(&nk).copied().collect();
                if eg.len() != 2 || eg != ek || common != eg {
                    if explain {
                        eprintln!("fold at {:?}/{:?}: link endpoints {:?}/{:?}, shared neighbours {:?}", tri, self.tris[other], eg, ek, common);
                    }
                    continue;
                }
                let fan: Vec<usize> = self.star[gone as usize]
                    .iter()
                    .map(|&x| x as usize)
                    .filter(|&x| self.alive[x] && self.tris[x].contains(&gone) && !dead.contains(&x))
                    .collect();
                let flips_a_good_triangle = fan.iter().any(|&x| {
                    let before = self.tris[x];
                    let after = before.map(|y| if y == gone { kept } else { y });
                    self.altitude_of(before) >= self.eps && self.normal_of(before).dot(self.normal_of(after)) <= 0.0
                });
                if flips_a_good_triangle {
                    if explain {
                        eprintln!("fold at {:?}/{:?}: merging would flip a good triangle", tri, self.tris[other]);
                    }
                    continue;
                }
                for x in dead {
                    self.alive[x] = false;
                }
                let mut fan = fan;
                fan.sort_unstable();
                fan.dedup();
                for x in fan {
                    for y in self.tris[x].iter_mut() {
                        if *y == gone {
                            *y = kept;
                        }
                    }
                    self.star[kept as usize].push(x as u32);
                }
                self.star[gone as usize].clear();
                merged += 1;
                break; // `t` is dead now
            }
        }
        merged
    }

    /// Remove triangles with a repeated corner and pairs of triangles on the
    /// same three vertices with opposite winding. Returns how many went.
    fn drop_folds(&mut self) -> usize {
        let mut dropped = 0;
        let mut groups: HashMap<[u32; 3], Vec<usize>> = HashMap::new();
        for t in 0..self.tris.len() {
            if !self.alive[t] {
                continue;
            }
            let mut key = self.tris[t];
            key.sort_unstable();
            if key[0] == key[1] || key[1] == key[2] {
                self.alive[t] = false;
                dropped += 1;
                continue;
            }
            groups.entry(key).or_default().push(t);
        }
        for (key, ts) in groups {
            if ts.len() != 2 {
                continue;
            }
            let parity = |t: usize| {
                let tri = self.tris[t];
                let i = tri.iter().position(|&x| x == key[0]).unwrap();
                tri[(i + 1) % 3] == key[1]
            };
            if parity(ts[0]) != parity(ts[1]) {
                self.alive[ts[0]] = false;
                self.alive[ts[1]] = false;
                dropped += 2;
            }
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fin a boolean leaves when a cut grazes a surface, reduced to what
    /// matters: two tall triangles on a short base facing opposite ways, tips
    /// a hair apart, each tip closed up to its own far vertex so the tips
    /// share neither an edge (an ordinary short-edge collapse would take them
    /// first) nor a neighbour beyond the base (which would make the merge a
    /// pinch). Base 0.14 -- wider than eps -- tips 0.05 apart, 9 tall.
    fn fin() -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>) {
        let pos = vec![
            Vec3::new(0.0, 0.0, 0.0),    // 0 u
            Vec3::new(0.14, 0.0, 0.0),   // 1 v
            Vec3::new(0.07, 0.0, 9.0),   // 2 c, tip
            Vec3::new(0.07, 0.05, 9.0),  // 3 d, the other tip
            Vec3::new(-4.0, 1.0, 4.0),   // 4 e, c's side of the surface
            Vec3::new(4.0, 1.0, 4.0),    // 5 f, d's side
        ];
        let tris = vec![[0, 1, 2], [1, 0, 3], [2, 1, 4], [0, 2, 4], [3, 0, 5], [1, 3, 5], [4, 1, 5], [0, 4, 5]];
        let prov = vec![7; 8];
        (pos, tris, prov)
    }

    #[test]
    fn near_fold_tips_are_merged_and_the_fin_goes() {
        let (pos, tris, prov) = fin();
        assert!(closed_manifold(tris.iter().copied()));
        let n = |t: &[u32; 3]| (pos[t[1] as usize] - pos[t[0] as usize]).cross(pos[t[2] as usize] - pos[t[0] as usize]).normalized();
        assert!(n(&tris[0]).dot(n(&tris[1])) < -0.999, "the fixture is a fold");
        assert!(min_altitude(pos[0], pos[1], pos[2]) > 0.1, "and neither half is a sliver");

        let mut w = Work::new(&pos, &tris, &prov, 0.1);
        assert_eq!(w.merge_fold_tips(), 1);
        let alive: Vec<[u32; 3]> = (0..w.tris.len()).filter(|&t| w.alive[t]).map(|t| w.tris[t]).collect();
        assert_eq!(alive.len(), 6, "the fold pair is gone: {alive:?}");
        assert!(alive.iter().all(|t| !t.contains(&3)), "tip d was merged into c: {alive:?}");
        assert!(closed_manifold(alive.iter().copied()), "still closed: {alive:?}");
        assert_eq!(w.drop_folds(), 0, "and nothing else was folded");

        // The whole pass agrees, and leaves a closed surface.
        let (pos, mut tris, mut prov) = fin();
        let report = clean(&pos, &mut tris, &mut prov, 0.1);
        assert_eq!(report.fold_tips, 1, "{report:?}");
        assert!(!report.abandoned, "{report:?}");
        assert_eq!(tris.len(), 6);
        assert!(closed_manifold(tris.iter().copied()));
    }

    /// The fin exactly as the impeller's union left it: tips joined by an
    /// edge whose two triangles have as apexes one base vertex (so that
    /// vertex has degree three) and a fifth vertex. The plain edge collapse
    /// refuses this: the tips share the other base vertex as a neighbour, and
    /// only through the fold pair itself. Tips 0.07 apart at eps 0.05.
    #[test]
    fn a_fold_whose_tips_share_an_edge_is_removed_whole() {
        let pos = vec![
            Vec3::new(0.0, 0.0, 0.0),    // 0 u
            Vec3::new(0.14, 0.0, 0.0),   // 1 v
            Vec3::new(0.07, 0.0, 9.0),   // 2 c
            Vec3::new(0.07, 0.07, 9.0),  // 3 d
            Vec3::new(0.07, 3.0, 4.5),   // 4 x
        ];
        let tris = vec![[2, 0, 1], [3, 1, 0], [3, 2, 1], [2, 3, 4], [0, 2, 4], [3, 0, 4]];
        let prov = vec![7; 6];
        assert!(closed_manifold(tris.iter().copied()));
        let mut w = Work::new(&pos, &tris, &prov, 0.05);
        assert!(!w.collapse(3, 2) && !w.collapse(2, 3), "the plain collapse refuses this needle");
        assert_eq!(w.merge_fold_tips(), 1);
        let alive: Vec<[u32; 3]> = (0..w.tris.len()).filter(|&t| w.alive[t]).map(|t| w.tris[t]).collect();
        assert_eq!(alive.len(), 2, "pair and tip-edge triangles went: {alive:?}");
        assert!(closed_manifold(alive.iter().copied()));
        // The two that remain are an exact fin, for drop_folds.
        assert_eq!(w.drop_folds(), 2);
    }

    #[test]
    fn a_real_pleat_is_left_alone() {
        // The same shape on a 3 mm base with the tips 0.3 apart: still a
        // fold by angle, but further than 2 * eps, so it is geometry.
        let pos = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(1.5, 0.0, 9.0),
            Vec3::new(1.5, 0.3, 9.0),
            Vec3::new(-4.0, 1.0, 4.0),
            Vec3::new(7.0, 1.0, 4.0),
        ];
        let tris = vec![[0, 1, 2], [1, 0, 3], [2, 1, 4], [0, 2, 4], [3, 0, 5], [1, 3, 5], [4, 1, 5], [0, 4, 5]];
        let prov = vec![7; 8];
        let n = |t: &[u32; 3]| (pos[t[1] as usize] - pos[t[0] as usize]).cross(pos[t[2] as usize] - pos[t[0] as usize]).normalized();
        assert!(n(&tris[0]).dot(n(&tris[1])) < -0.999, "still a fold by angle");
        let mut w = Work::new(&pos, &tris, &prov, 0.1);
        assert_eq!(w.merge_fold_tips(), 0);
        assert!(w.alive.iter().all(|&a| a));
    }

    /// A closed box as an index mesh with one provenance id per face, the way
    /// manifold hands one back.
    fn cube(o: Vec3, s: f64) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>) {
        let pos: Vec<Vec3> = (0..8)
            .map(|i| {
                Vec3::new(
                    o.x + if i & 1 != 0 { s } else { 0.0 },
                    o.y + if i & 2 != 0 { s } else { 0.0 },
                    o.z + if i & 4 != 0 { s } else { 0.0 },
                )
            })
            .collect();
        // Outward-facing quads, split into two triangles each.
        let quads: [[u32; 4]; 6] = [
            [0, 2, 3, 1], // -z
            [4, 5, 7, 6], // +z
            [0, 1, 5, 4], // -y
            [2, 6, 7, 3], // +y
            [0, 4, 6, 2], // -x
            [1, 3, 7, 5], // +x
        ];
        let mut tris = Vec::new();
        let mut prov = Vec::new();
        for (f, q) in quads.iter().enumerate() {
            tris.push([q[0], q[1], q[2]]);
            tris.push([q[0], q[2], q[3]]);
            prov.push(f as u32);
            prov.push(f as u32);
        }
        (pos, tris, prov)
    }

    fn volume(pos: &[Vec3], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| pos[t[0] as usize].dot(pos[t[1] as usize].cross(pos[t[2] as usize])) / 6.0)
            .sum()
    }

    fn worst_altitude(pos: &[Vec3], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| min_altitude(pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]))
            .fold(f64::INFINITY, f64::min)
    }

    /// Fan triangle `t` around a new vertex `p` inside it, keeping the mesh
    /// closed. With `p` a hair from one of `t`'s edges this is exactly what a
    /// boolean leaves behind: a sliver along that edge, still manifold.
    fn fan_in(pos: &mut Vec<Vec3>, tris: &mut Vec<[u32; 3]>, prov: &mut Vec<u32>, t: usize, p: Vec3) -> u32 {
        let m = pos.len() as u32;
        pos.push(p);
        let [a, b, c] = tris[t];
        tris[t] = [a, b, m];
        tris.push([b, c, m]);
        tris.push([c, a, m]);
        prov.push(prov[t]);
        prov.push(prov[t]);
        m
    }

    /// `cube()`'s +z face triangle `[4, 5, 7]`, whose edge 5-7 is the top of
    /// the +x face.
    const TOP_TRI: usize = 2;

    #[test]
    fn a_clean_box_is_left_alone() {
        let (pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        let before = (tris.clone(), prov.clone());
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert!(!r.changed(), "{r:?}");
        assert_eq!((tris, prov), before);
    }

    #[test]
    fn a_vertex_a_hair_off_an_edge_is_snapped_and_the_box_stays_closed() {
        let (mut pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        // A vertex on the +z face 0.01 mm in from its +x edge: the triangle
        // it makes with that edge is a sliver 0.01 mm wide and 10 mm long.
        let m = fan_in(&mut pos, &mut tris, &mut prov, TOP_TRI, Vec3::new(9.99, 5.0, 10.0));
        assert!(closed_manifold(tris.iter().copied()));
        assert!(worst_altitude(&pos, &tris) < 0.05, "the fixture has a sliver");
        let v0 = volume(&pos, &tris);
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert_eq!(r.snapped_slivers, 1, "{r:?}");
        assert!(r.changed() && !r.abandoned, "{r:?}");
        assert!(closed_manifold(tris.iter().copied()));
        assert!(worst_altitude(&pos, &tris) >= 0.05, "{}", worst_altitude(&pos, &tris));
        assert!((volume(&pos, &tris) - v0).abs() < 0.05 * 100.0, "volume moved by more than eps × a face");
        // The sliver's area went to the +x face it lay along, which now
        // passes through the vertex; nothing was invented, and every face id
        // is one the box had.
        assert!(tris.iter().zip(&prov).filter(|(t, _)| t.contains(&m)).all(|(_, &f)| f == 5 || f == 1));
        assert!(tris.iter().zip(&prov).any(|(t, &f)| t.contains(&m) && f == 5));
        assert!(prov.iter().all(|&f| f < 6));
        assert_eq!(tris.len(), prov.len());
    }

    #[test]
    fn a_short_edge_collapses_onto_the_corner_not_the_other_way() {
        let (mut pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        // A needle: a vertex on the top +x edge 0.02 mm from corner 7, so the
        // triangle it makes with that edge has no area at all.
        let m = fan_in(&mut pos, &mut tris, &mut prov, TOP_TRI, Vec3::new(10.0, 9.98, 10.0));
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert_eq!(r.collapsed_edges, 1, "{r:?}");
        assert!(closed_manifold(tris.iter().copied()));
        // Corner 7 (three faces meet) survived; the needle vertex (one) is gone.
        assert!(tris.iter().any(|t| t.contains(&7)));
        assert!(!tris.iter().any(|t| t.contains(&m)));
        assert_eq!(tris.len(), 12);
        assert!((volume(&pos, &tris) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn a_deliberate_gap_narrower_than_eps_is_not_bridged() {
        let (mut pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        let (p2, t2, v2) = cube(Vec3::new(10.04, 0.0, 0.0), 10.0);
        let off = pos.len() as u32;
        pos.extend(p2);
        tris.extend(t2.iter().map(|t| t.map(|i| i + off)));
        prov.extend(v2.iter().map(|f| f + 6));
        let before = (tris.clone(), prov.clone());
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert!(!r.changed(), "two boxes 0.04 apart share no edge, so nothing may move: {r:?}");
        assert_eq!((tris, prov), before);
    }

    #[test]
    fn a_folded_pair_is_removed_as_a_pair() {
        let (pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        // A zero-thickness flap standing inside the solid on three of its
        // vertices: both sides go, and the box is exactly what is left.
        tris.push([0, 3, 5]);
        tris.push([5, 3, 0]);
        prov.push(0);
        prov.push(0);
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert_eq!(r.dropped, 2, "{r:?}");
        assert_eq!(tris.len(), 12);
        assert!(closed_manifold(tris.iter().copied()));
    }

    #[test]
    fn an_input_that_is_not_a_manifold_is_handed_back_untouched() {
        let (mut pos, mut tris, mut prov) = cube(Vec3::ZERO, 10.0);
        // A sliver to act on, plus a third triangle on one edge so the result
        // can never pass the manifold check.
        fan_in(&mut pos, &mut tris, &mut prov, TOP_TRI, Vec3::new(9.99, 5.0, 10.0));
        tris.push([0, 1, 2]);
        prov.push(0);
        let before = (tris.clone(), prov.clone());
        let r = clean(&pos, &mut tris, &mut prov, 0.05);
        assert!(r.abandoned, "{r:?}");
        assert_eq!(r.stop, Some(Stop::NotClosed), "{r:?}");
        assert_eq!((tris, prov), before);
        // And the user is told, in words that say what it means for them.
        let note = r.note().expect("an abandoned clean is news");
        assert!(note.starts_with("mesh clean abandoned: "), "{note}");
        assert!(note.contains("not a closed manifold"), "{note}");
        assert!(note.contains("robust engine"), "the consequence is in the note: {note}");
        assert!(note.contains("cadpole inspect"), "{note}");
    }

    /// A clean that ran to the end is not news; one that stopped is, and the
    /// note says at which pass, why, what was kept and what it costs.
    #[test]
    fn the_note_says_where_a_clean_stopped_and_why() {
        // The fold fixture cleans completely: no note.
        let (pos, mut tris, mut prov) = fin();
        let r = clean(&pos, &mut tris, &mut prov, 0.1);
        assert!(!r.abandoned && r.stopped_at_pass.is_none(), "{r:?}");
        assert_eq!(r.note(), None, "{r:?}");

        // A pass broke the surface: the passes before it were kept.
        let r = CleanReport {
            collapsed_edges: 30_000,
            snapped_slivers: 12,
            fold_tips: 1,
            dropped: 2,
            passes: 49,
            stopped_at_pass: Some(49),
            stop: Some(Stop::SurfaceBroke),
            abandoned: false,
        };
        let note = r.note().unwrap();
        assert!(note.starts_with("mesh clean stopped at pass 49 of 49: that pass's edits broke the surface"), "{note}");
        assert!(note.contains("30000 short edges collapsed, 12 slivers snapped, 1 fold tip merged, 2 triangles dropped"), "{note}");
        assert!(note.contains("lose its face names"), "{note}");

        // The bisection kept an earlier pass.
        let r = CleanReport { passes: 20, stopped_at_pass: Some(13), stop: Some(Stop::CrossedMore), ..r };
        let note = r.note().unwrap();
        assert!(note.starts_with("mesh clean stopped at pass 13 of 20: the finished clean crossed itself"), "{note}");

        // Nothing to fall back to.
        let r = CleanReport { abandoned: true, stop: Some(Stop::CrossedMore), ..r };
        let note = r.note().unwrap();
        assert!(note.starts_with("mesh clean abandoned: the cleaned surface crossed itself more than the input"), "{note}");
        assert!(note.contains("exactly as the kernel returned it"), "{note}");
    }

    #[test]
    fn closed_manifold_rejects_an_internal_wall_that_watertight_lets_through() {
        let (pos, mut tris, _) = cube(Vec3::ZERO, 10.0);
        assert!(closed_manifold(tris.iter().copied()));
        // A double-sided sheet across the inside: every one of its edges now
        // has four triangles, two each way.
        tris.push([0, 3, 5]);
        tris.push([5, 3, 0]);
        assert!(!closed_manifold(tris.iter().copied()));
        let mesh = crate::tessellate::Mesh {
            positions: pos.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect(),
            normals: vec![0.0; pos.len() * 3],
            indices: tris.iter().flatten().copied().collect(),
            face_ids: vec![0; tris.len()],
            colors: Vec::new(),
        };
        assert!(mesh.is_watertight(), "the directed sum cannot see it");
        assert!(!mesh.is_closed_manifold());
    }
}
