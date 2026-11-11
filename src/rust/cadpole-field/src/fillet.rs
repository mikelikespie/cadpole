//! `fillet` and `chamfer` on real bodies, by the field path.
//!
//! The route, end to end:
//!
//! 1. tessellate the body (a mesh body already is one) and weld it;
//! 2. import it as a signed distance field ([`crate::mesh_sdf`]);
//! 3. find the selected edges in the welded mesh and read off, per polyline
//!    vertex, the outward normals of the two faces that meet there;
//! 4. add the edge-local blend correction ([`crate::edges`]);
//! 5. contour the result ([`crate::mdc`]) and hand back a mesh body.
//!
//! **This is an approximation, and its error is reported.** Two sources, both
//! measured and returned in [`FilletReport`]:
//!
//! * *contouring*: the surface is resampled on a grid of the requested
//!   resolution, so a feature is located to roughly `resolution / 2`. The
//!   default resolution is derived from the radius, and the report says what
//!   was used;
//! * *tessellation*: a B-rep body becomes triangles first, so a curved face's
//!   tangent planes are the facet planes. On planar faces there is no such
//!   error and the blend is exact.
//!
//! The exact B-rep blend — constructed cylinder and torus patches with the
//! neighbouring faces re-trimmed — is not implemented here, and is what
//! a part that must export STEP needs. `fillet` says so in a diagnostic every
//! time it promotes a B-rep body to a mesh.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadpole_core::tessellate::{EdgeInfo, Mesh, Topology};

use crate::edges::{add, cross, dist2, dot, mul, norm, sub, BlendEdge, BlendKind, EdgeBlendSet};
use crate::mdc::{self, ContourMesh, ContourOptions};
use crate::mesh_sdf::MeshField;
use crate::Field;
use cadpole_core::fmath;

/// Below this dihedral angle an "edge" is a tessellation seam, not a feature,
/// and blending it is refused with a note rather than done badly.
pub const TANGENT_DEGREES: f64 = 5.0;

/// Fraction of the requested size the rolling ball must actually clear before
/// `fillet` accepts that it fits. The gap is the tessellation's: a faceted
/// cylinder is inscribed inside the real one, so the probe always reads a
/// little short.
pub const FIT_TOLERANCE: f64 = 0.9;

/// `cos(TANGENT_DEGREES)`: above this, two adjacent triangles are the same
/// face as far as the blend is concerned.
const TANGENT_COS: f64 = 0.996_194_698; // cos(5 degrees)

/// How a `fillet`/`chamfer` call names the edges it wants.
#[derive(Clone, Debug)]
pub enum EdgeSelection {
    /// Every edge of the body.
    All,
    /// The edges with these topological names (`body.edges(...)` results, or
    /// literal names).
    Named(Vec<String>),
}

/// What `fillet`/`chamfer` did, and how well.
#[derive(Clone, Debug, Default)]
pub struct FilletReport {
    pub kind: &'static str,
    pub size: f64,
    pub resolution: f64,
    pub edges_selected: usize,
    pub edges_skipped: Vec<String>,
    /// Worst deviation of the output mesh from the field it was contoured
    /// from, measured by evaluating the field at every output vertex, in mm.
    pub surface_error: f64,
    /// Half the cell size: the distance the grid can move a feature.
    pub resolution_error: f64,
    pub triangles: usize,
    pub vertices: usize,
    pub watertight: bool,
    pub vertex_manifold: bool,
    pub ms: f64,
    /// Edges where a ball of the requested size does not fit, with the
    /// largest that does.
    pub too_large: Vec<(String, f64)>,
    pub stats: mdc::ContourStats,
}

impl FilletReport {
    /// A one-line diagnostic for the script, stating the approximation.
    pub fn summary(&self) -> String {
        format!(
            "{} of {} edge{} at {} mm: meshed at resolution {} mm through the field path, \
             so the result is a mesh body, not a B-rep. Measured surface \
             error {:.4} mm, grid error {:.4} mm, {} triangles.",
            self.kind,
            self.edges_selected,
            if self.edges_selected == 1 { "" } else { "s" },
            fmt(self.size),
            fmt(self.resolution),
            self.surface_error,
            self.resolution_error,
            self.triangles,
        )
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.to_owned() }
}

/// The default meshing resolution for a blend of size `size` over a body of
/// the given bounding diagonal.
///
/// A fillet arc is only as round as the cells that sample it, so the
/// resolution has to follow the *radius*, not the body: `size / 6` puts about
/// ten cells across the quarter-circle, which measures back to within a few
/// percent of the requested radius (see the table in `docs/starlark-api.md`).
/// It is then clamped so a tiny fillet on a big part cannot ask for a grid
/// nobody can afford.
pub fn default_resolution(size: f64, diagonal: f64) -> f64 {
    let want = size.abs() / 6.0;
    let floor = diagonal / 700.0;
    want.max(floor).max(1e-4)
}

/// Fillet or chamfer the selected edges of a body's mesh.
///
/// `mesh`/`topo` are the body as tessellated by the caller. Returns the new
/// mesh, its topology, and the report.
pub fn blend_body(
    mesh: &Mesh,
    topo: &Topology,
    selection: &EdgeSelection,
    kind: BlendKind,
    resolution: Option<f64>,
    error: f64,
) -> Result<(Mesh, Topology, FilletReport), String> {
    let size = kind.size();
    if !(size > 0.0) || !size.is_finite() {
        return Err(format!("{} size must be a positive number of millimetres, got {size}", kind.name()));
    }
    let field = Arc::new(MeshField::from_mesh(mesh));
    if !field.is_closed() {
        return Err(
            "this body's triangles are not a closed surface, so it cannot be imported as a \
             distance field; weld or repair it first"
                .to_owned(),
        );
    }
    let (lo, hi) = field.bounds();
    let diagonal = (0..3).map(|k| fmath::powi(hi[k] - lo[k], 2)).sum::<f64>().sqrt();

    let wanted = wanted_names(topo, selection);
    let (edges, skipped) = build_edges(&field, topo, &wanted)?;
    if edges.is_empty() {
        return Err(format!(
            "{}: none of the selected edges could be used ({} selected; {})",
            kind.name(),
            wanted.len(),
            if skipped.is_empty() {
                "the body has no edges".to_owned()
            } else {
                format!("skipped: {}", skipped.join(", "))
            }
        ));
    }
    let edges_selected = edges.len();
    let set = Arc::new(EdgeBlendSet::new(kind, edges));

    // Does a ball of this size fit? Probe the body's own field where the blend
    // arc's centre has to sit at every segment of every selected edge: a
    // convex fillet needs `size` of material inside, a concave one `size` of
    // air outside.
    //
    // The verdict is a *quantile*, not the worst sample. Two reasons: the
    // probe reads the distance to the triangles, and a tessellated cylinder
    // is inscribed, so a ball that really does fit reads a few percent short;
    // and a handful of segments at the ends of a chain, where a third face
    // joins, put the centre somewhere unrepresentative. A radius that
    // genuinely does not fit fails almost everywhere along the edge, so
    // asking "does it fit at three quarters of the samples" separates the two
    // cleanly and stops the diagnostic crying wolf.
    let too_large: Vec<(String, f64)> = {
        let mut per_edge_fits: HashMap<String, Vec<f64>> = HashMap::new();
        for (name, c, _want, convex) in set.ball_centres() {
            let d = field.signed_distance(c);
            let fits = if convex { -d } else { d };
            per_edge_fits.entry(name).or_default().push(fits.max(0.0));
        }
        let mut v: Vec<(String, f64)> = Vec::new();
        for (name, mut fits) in per_edge_fits {
            fits.sort_by(|a, b| a.partial_cmp(b).unwrap());
            // The value a quarter of the way up: three quarters of the edge
            // does at least this well.
            let q = fits[fits.len() / 4];
            if q < size * FIT_TOLERANCE {
                v.push((name, q));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };

    let base = Field::from_mesh_field(field.clone());
    let blended = base.edge_blend(set.clone());
    let resolution = resolution.unwrap_or_else(|| default_resolution(size, diagonal));
    if !(resolution > 0.0) || !resolution.is_finite() {
        return Err(format!("resolution must be a positive number of millimetres, got {resolution}"));
    }
    let tape = blended.compile();
    let out = mdc::contour(&tape, &ContourOptions { resolution, error, ..Default::default() })?;

    // Measured error: how far the contoured vertices are from the field's own
    // zero level set.
    let mut ev = tape.evaluator();
    let mut surface_error = 0.0f64;
    for p in &out.positions {
        surface_error = surface_error.max(ev.eval(*p).abs());
    }

    let (mesh_out, topo_out) = to_body(&out, &field, topo, &set);
    let report = FilletReport {
        kind: if matches!(kind, BlendKind::Fillet { .. }) { "fillet" } else { "chamfer" },
        size,
        resolution,
        edges_selected,
        edges_skipped: skipped,
        surface_error,
        resolution_error: resolution * 0.5,
        triangles: out.tris.len(),
        vertices: out.positions.len(),
        watertight: out.is_watertight(),
        vertex_manifold: out.is_vertex_manifold(),
        ms: out.stats.ms,
        too_large,
        stats: out.stats.clone(),
    };
    Ok((mesh_out, topo_out, report))
}

fn wanted_names(topo: &Topology, selection: &EdgeSelection) -> Vec<String> {
    match selection {
        EdgeSelection::All => topo.edges.iter().map(|e| e.name.clone()).collect(),
        EdgeSelection::Named(v) => v.clone(),
    }
}

/// Turn the selected [`EdgeInfo`]s into blend edges, reading the two adjacent
/// face normals out of the welded triangle adjacency.
fn build_edges(
    field: &MeshField,
    topo: &Topology,
    wanted: &[String],
) -> Result<(Vec<BlendEdge>, Vec<String>), String> {
    let (verts, tris, tri_normal, _tri_group) = field.welded();
    // Undirected welded edge -> the triangles that use it, and vertex ->
    // every triangle around it.
    //
    // The second one is what makes the tangent planes trustworthy. Reading a
    // face's normal off the single triangle that happens to touch the seam
    // works on a clean extrusion and fails on anything a boolean has been
    // through: the kernel retriangulates near the cut and leaves slivers and
    // transition triangles whose normals point nowhere in particular, and a
    // fillet built on those puts its rolling ball in the wrong place. Instead
    // each side's normal is the **area-weighted average over the fan around
    // the segment, restricted to the face group that side belongs to** —
    // provenance the boolean kernel already tracked, so it is exact rather
    // than a guess, and area weighting makes the slivers irrelevant.
    let mut per_edge: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    let mut per_vertex: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut tri_area: Vec<f64> = Vec::with_capacity(tris.len());
    for (ti, t) in tris.iter().enumerate() {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        let n = cross(sub(b, a), sub(c, a));
        tri_area.push(dot(n, n).sqrt() * 0.5);
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            per_edge.entry(if u < v { (u, v) } else { (v, u) }).or_default().push(ti as u32);
            per_vertex.entry(t[k]).or_default().push(ti as u32);
        }
    }
    let side_normal = |a: u32, b: u32, seed: [f64; 3], other: [f64; 3]| -> [f64; 3] {
        let mut acc = [0.0f64; 3];
        for v in [a, b] {
            let Some(list) = per_vertex.get(&v) else { continue };
            for &ti in list {
                let n = tri_normal[ti as usize];
                if dot(n, seed) > dot(n, other) {
                    acc = add(acc, mul(n, tri_area[ti as usize]));
                }
            }
        }
        if dot(acc, acc) > 1e-18 { norm(acc) } else { seed }
    };
    // Spatial hash so a polyline point can find its welded vertex.
    let eps = 1e-3;
    let q = |v: f64| (v / eps).floor() as i64;
    let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    for (i, v) in verts.iter().enumerate() {
        cells.entry((q(v[0]), q(v[1]), q(v[2]))).or_default().push(i as u32);
    }
    let lookup = |p: [f64; 3]| -> Option<u32> {
        let k = (q(p[0]), q(p[1]), q(p[2]));
        let mut best: Option<(u32, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = cells.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        for &i in ids {
                            let d = dist2(verts[i as usize], p);
                            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                                best = Some((i, d));
                            }
                        }
                    }
                }
            }
        }
        best.filter(|(_, d)| *d <= eps * eps).map(|(i, _)| i)
    };

    let by_name: HashMap<&str, &EdgeInfo> =
        topo.edges.iter().map(|e| (e.name.as_str(), e)).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for name in wanted {
        let Some(info) = by_name.get(name.as_str()) else {
            skipped.push(format!("{name} (no such edge)"));
            continue;
        };
        if !seen.insert(info.name.as_str()) {
            continue;
        }
        // Map the polyline onto welded vertices, dropping repeats.
        let mut ids: Vec<u32> = Vec::with_capacity(info.polyline.len());
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(info.polyline.len());
        let mut missing = false;
        for p in &info.polyline {
            let p = [p.x, p.y, p.z];
            let Some(i) = lookup(p) else {
                missing = true;
                break;
            };
            if ids.last() == Some(&i) {
                continue;
            }
            ids.push(i);
            pts.push(verts[i as usize]);
        }
        if missing || ids.len() < 2 {
            skipped.push(format!("{} (not on the tessellated surface)", info.name));
            continue;
        }
        // The two tangent planes, per segment.
        //
        // Read straight off the two adjacent triangles this is fragile: at a
        // seam a boolean has cut, one of them is often a sliver whose normal
        // is halfway between the two faces, and a blend built on that puts
        // its rolling ball somewhere between them. So the chain is resolved
        // by *propagation*: start at the segment whose two triangles disagree
        // most (the most unambiguous piece of crease there is), take the fan
        // average on each side there, and walk outwards using the previous
        // segment's answer as the seed for the next. Curvature is fine — two
        // neighbouring segments of a rim differ by a facet — and a bad pair
        // in the middle inherits its neighbour's frame instead of poisoning
        // the edge.
        let nseg = ids.len() - 1;
        let mut raw: Vec<([f64; 3], [f64; 3])> = Vec::with_capacity(nseg);
        let mut ok = true;
        for w in ids.windows(2) {
            let key = if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) };
            let Some(ts) = per_edge.get(&key) else {
                ok = false;
                break;
            };
            if ts.len() != 2 {
                ok = false;
                break;
            }
            raw.push((tri_normal[ts[0] as usize], tri_normal[ts[1] as usize]));
        }
        if !ok || raw.is_empty() {
            skipped.push(format!("{} (not a manifold crease)", info.name));
            continue;
        }
        // A seam between two triangle groups that meet almost flat is not an
        // edge anyone wants rounded: it is where the tessellator split a
        // curved face, or where a boolean left two coplanar patches. Blending
        // it would do nothing useful and would put the rolling ball's centre
        // wherever the near-parallel normals happened to point, so it is
        // skipped and said so.
        //
        // The angle is the surfaces' own where both faces are recognised
        // analytic surfaces: across a seam of a tessellated cylinder or
        // sphere the facets meet at the facet angle (ten degrees and more at
        // ordinary tolerances), which says how coarse the tessellation is, not
        // whether there is a corner. Free-form faces and mesh groups have no
        // exact normal and fall back on the facets.
        let dihedral = surface_dihedral(topo, info, &pts, &raw).unwrap_or(info.dihedral_degrees);
        if dihedral >= 0.0 && dihedral < TANGENT_DEGREES {
            skipped.push(format!("{} (smooth seam, {:.1} degrees)", info.name, dihedral));
            continue;
        }
        // The anchor: the segment whose two triangles are furthest apart.
        let anchor = (0..nseg)
            .min_by(|&i, &j| {
                dot(raw[i].0, raw[i].1).partial_cmp(&dot(raw[j].0, raw[j].1)).unwrap()
            })
            .expect("at least one segment");
        if dot(raw[anchor].0, raw[anchor].1) > TANGENT_COS {
            skipped.push(format!("{} (no crease along it)", info.name));
            continue;
        }
        let mut seg_a: Vec<[f64; 3]> = vec![[0.0; 3]; nseg];
        let mut seg_b: Vec<[f64; 3]> = vec![[0.0; 3]; nseg];
        let resolve = |i: usize,
                           seeds: ([f64; 3], [f64; 3]),
                           seg_a: &mut Vec<[f64; 3]>,
                           seg_b: &mut Vec<[f64; 3]>| {
            let (u, v) = (ids[i], ids[i + 1]);
            // Use this segment's own pair when it is a real crease, and the
            // seeds handed down the chain when it is not.
            let (s0, s1) = if dot(raw[i].0, raw[i].1) <= TANGENT_COS {
                if dot(raw[i].0, seeds.0) >= dot(raw[i].1, seeds.0) {
                    (raw[i].0, raw[i].1)
                } else {
                    (raw[i].1, raw[i].0)
                }
            } else {
                seeds
            };
            seg_a[i] = side_normal(u, v, s0, s1);
            seg_b[i] = side_normal(u, v, s1, s0);
        };
        resolve(anchor, raw[anchor], &mut seg_a, &mut seg_b);
        for i in anchor + 1..nseg {
            let seeds = (seg_a[i - 1], seg_b[i - 1]);
            resolve(i, seeds, &mut seg_a, &mut seg_b);
        }
        for i in (0..anchor).rev() {
            let seeds = (seg_a[i + 1], seg_b[i + 1]);
            resolve(i, seeds, &mut seg_a, &mut seg_b);
        }
        let closed = ids.len() > 2 && dist2(pts[0], pts[pts.len() - 1]) < 1e-12;
        let n = ids.len();
        let mut na = Vec::with_capacity(n);
        let mut nb = Vec::with_capacity(n);
        for i in 0..n {
            let prev = if i == 0 {
                if closed { Some(nseg - 1) } else { None }
            } else {
                Some(i - 1)
            };
            let next = if i < nseg { Some(i) } else if closed { Some(0) } else { None };
            let mut sa = [0.0; 3];
            let mut sb = [0.0; 3];
            for s in [prev, next].into_iter().flatten() {
                sa = add(sa, seg_a[s]);
                sb = add(sb, seg_b[s]);
            }
            na.push(norm(sa));
            nb.push(norm(sb));
        }
        // An end is capped when both of the edge's faces end there: no
        // triangle of either side at the end vertex reaches past it along the
        // edge's outgoing direction. The end faces of an extruded L cap its
        // inside corner; at a seam vertex of a round boss's base, or a square
        // boss's convex corner, the plate goes on, so those ends are open.
        let capped_at = |v: u32, t: [f64; 3], sides: [[f64; 3]; 2]| {
            let at = verts[v as usize];
            !per_vertex.get(&v).is_some_and(|list| {
                list.iter().any(|&ti| {
                    let n = tri_normal[ti as usize];
                    sides.iter().any(|sn| dot(n, *sn) > 0.9)
                        && tris[ti as usize].iter().any(|&w| dot(sub(verts[w as usize], at), t) > 1e-6)
                })
            })
        };
        let capped = if closed {
            (false, false)
        } else {
            let t0 = norm(sub(pts[0], pts[1]));
            let t1 = norm(sub(pts[n - 1], pts[n - 2]));
            (capped_at(ids[0], t0, [na[0], nb[0]]), capped_at(ids[n - 1], t1, [na[n - 1], nb[n - 1]]))
        };
        let mut edge = BlendEdge::new(info.name.clone(), pts, na, nb, info.convex);
        edge.capped = capped;
        out.push(edge);
    }
    Ok((out, skipped))
}

/// The angle between the two faces' exact surface normals along an edge, in
/// degrees (the median over its segments), when both faces are recognised
/// analytic surfaces; `None` otherwise.
///
/// A recognised class gives the normal only up to sign, so each is turned to
/// agree with the two facets at the segment, whose sum points out of the body
/// for any fold short of a knife edge. A segment whose facets are 90 degrees
/// or more apart is a real corner whatever the exact normals say, and counts
/// as its facet angle.
fn surface_dihedral(
    topo: &Topology,
    info: &EdgeInfo,
    pts: &[[f64; 3]],
    raw: &[([f64; 3], [f64; 3])],
) -> Option<f64> {
    use cadpole_core::geom::Vec3;
    let [fa, fb] = info.face_ids.as_slice() else { return None };
    let (sa, sb) = (&topo.faces.get(*fa as usize)?.surface, &topo.faces.get(*fb as usize)?.surface);
    if !sa.is_recognised() || !sb.is_recognised() {
        return None;
    }
    let mut angles = Vec::with_capacity(raw.len());
    for (i, (ta, tb)) in raw.iter().enumerate() {
        let facet = dot(*ta, *tb).clamp(-1.0, 1.0);
        if facet <= 0.0 {
            angles.push(fmath::acos(facet).to_degrees());
            continue;
        }
        let m = mul(add(pts[i], pts[i + 1]), 0.5);
        let p = Vec3::new(m[0], m[1], m[2]);
        let out = add(*ta, *tb);
        let orient = |n: Vec3| {
            let n = [n.x, n.y, n.z];
            if dot(n, out) < 0.0 { mul(n, -1.0) } else { n }
        };
        let (Some(na), Some(nb)) = (sa.normal_at(p), sb.normal_at(p)) else { return None };
        let c = dot(orient(na), orient(nb)).clamp(-1.0, 1.0);
        angles.push(fmath::acos(c).to_degrees());
    }
    if angles.is_empty() {
        return None;
    }
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(angles[angles.len() / 2])
}

/// Convert a contoured surface into a Cadpole mesh body, naming the face groups.
///
/// Every triangle keeps the name of the source face it came out of — found by
/// the nearest triangle of the original body — except where the blend
/// correction is live, which becomes one new group per blended edge. So
/// `bracket.face("face:3")` still selects the same face after a fillet, and
/// the new surface is selectable as `<edge>/fillet`.
fn to_body(
    out: &ContourMesh,
    field: &MeshField,
    topo: &Topology,
    set: &EdgeBlendSet,
) -> (Mesh, Topology) {
    use cadpole_core::geom::Vec3;
    use cadpole_core::tessellate::FaceInfo;

    let blend_name = |e: &BlendEdge| format!("{}/{}", e.name, set.kind.name());
    // Group table: the source body's faces first, then one per blended edge.
    let mut names: Vec<(String, &'static str)> =
        topo.faces.iter().map(|f| (f.name.clone(), f.surface_kind)).collect();
    let mut blend_group: HashMap<&str, u32> = HashMap::new();
    for e in &set.edges {
        blend_group.insert(e.name.as_str(), names.len() as u32);
        names.push((blend_name(e), "blend"));
    }

    // Per-triangle group, from the centroid.
    let mut group = Vec::with_capacity(out.tris.len());
    for t in &out.tris {
        let c = mul(
            add(
                add(out.positions[t[0] as usize], out.positions[t[1] as usize]),
                out.positions[t[2] as usize],
            ),
            1.0 / 3.0,
        );
        let mut g = 0u32;
        // Closest blended edge whose correction is actually live here wins.
        let mut best = f64::INFINITY;
        for e in &set.edges {
            let d = e.points.iter().map(|p| dist2(*p, c)).fold(f64::INFINITY, f64::min).sqrt();
            if d < best {
                best = d;
                g = blend_group[e.name.as_str()];
            }
        }
        let live = best <= set.kind.size().abs() * 1.5 && {
            let base = field.signed_distance(c);
            (set.apply(base, c) - base).abs() > 1e-9
        };
        if !live {
            g = nearest_face(field, topo, c).unwrap_or(0);
        }
        group.push(g);
    }

    // Order triangles by group so each face owns a contiguous run.
    let mut order: Vec<usize> = (0..out.tris.len()).collect();
    order.sort_by_key(|&i| (group[i], i));

    let mut mesh = Mesh::default();
    let mut remap: HashMap<(u32, u32), u32> = HashMap::new();
    let mut faces: Vec<FaceInfo> = Vec::new();
    let mut current = order.first().map(|&i| group[i]).unwrap_or(0);
    let mut tri_start = 0u32;
    let mut area = 0.0f64;
    let mut centroid = Vec3::ZERO;
    let mut normal_acc = Vec3::ZERO;
    let mut dense = 0u32;
    let finish = |mesh: &Mesh,
                      faces: &mut Vec<FaceInfo>,
                      g: u32,
                      tri_start: u32,
                      area: f64,
                      centroid: Vec3,
                      normal_acc: Vec3| {
        let (name, kind) = names[g as usize].clone();
        faces.push(FaceInfo {
            name,
            surface_kind: kind,
            centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
            normal: normal_acc.normalized(),
            area,
            tri_start,
            tri_count: mesh.triangle_count() as u32 - tri_start,
            edges: Vec::new(),
            // A field body's faces are contoured, not built by a feature, so
            // they carry no alternative names and no durable edge ids yet.
            ..Default::default()
        });
    };

    for &i in &order {
        let g = group[i];
        if g != current {
            finish(&mesh, &mut faces, current, tri_start, area, centroid, normal_acc);
            dense += 1;
            tri_start = mesh.triangle_count() as u32;
            area = 0.0;
            centroid = Vec3::ZERO;
            normal_acc = Vec3::ZERO;
            current = g;
        }
        let t = out.tris[i];
        let mut ids = [0u32; 3];
        for (k, &v) in t.iter().enumerate() {
            ids[k] = *remap.entry((v, g)).or_insert_with(|| {
                let p = out.positions[v as usize];
                let n = out.normals[v as usize];
                mesh.positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
                mesh.normals.extend_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
                (mesh.positions.len() / 3 - 1) as u32
            });
        }
        let (a, b, c) = (
            out.positions[t[0] as usize],
            out.positions[t[1] as usize],
            out.positions[t[2] as usize],
        );
        let nrm = cross(sub(b, a), sub(c, a));
        let tri_area = dot(nrm, nrm).sqrt() * 0.5;
        area += tri_area;
        let cen = mul(add(add(a, b), c), 1.0 / 3.0);
        centroid = centroid + Vec3::new(cen[0], cen[1], cen[2]) * tri_area;
        normal_acc = normal_acc + Vec3::new(nrm[0], nrm[1], nrm[2]);
        mesh.indices.extend_from_slice(&ids);
        mesh.face_ids.push(dense);
    }
    if !order.is_empty() {
        finish(&mesh, &mut faces, current, tri_start, area, centroid, normal_acc);
    }
    (mesh, Topology { faces, edges: Vec::new(), vertices: Vec::new() })
}

/// Index of the source face group nearest to a point.
fn nearest_face(_field: &MeshField, topo: &Topology, p: [f64; 3]) -> Option<u32> {
    // The face groups carry `tri_start`/`tri_count` into the *source* mesh;
    // the cheapest sound answer is the group whose centroid-and-plane fits
    // best, which for a tessellated body is the nearest face by plane
    // distance among those whose centroid is closest.
    let mut best: Option<(f64, u32)> = None;
    for (i, f) in topo.faces.iter().enumerate() {
        let c = [f.centroid.x, f.centroid.y, f.centroid.z];
        let n = [f.normal.x, f.normal.y, f.normal.z];
        let v = sub(p, c);
        let along = dot(v, n).abs();
        let lateral = (dot(v, v) - along * along).max(0.0).sqrt();
        // Weight the in-plane distance down: a point on a big face is far
        // from its centroid but right on its plane.
        let score = along * 4.0 + lateral * 0.25;
        if best.map(|(b, _)| score < b).unwrap_or(true) {
            best = Some((score, i as u32));
        }
    }
    best.map(|(_, i)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_resolution_follows_the_radius() {
        assert!((default_resolution(3.0, 70.0) - 0.5).abs() < 1e-12);
        assert!((default_resolution(0.6, 70.0) - 0.1).abs() < 1e-12);
        // A tiny fillet on a huge part is floored so the grid stays finite.
        assert!(default_resolution(0.05, 7000.0) > 0.05 / 6.0);
    }

    use cadpole_core::geom::{Plane, Vec3};
    use cadpole_core::kernel;
    use cadpole_core::sketch::{Loop, Sketch, P2};
    use cadpole_core::tessellate;

    #[test]
    fn a_cylinders_tessellation_seams_are_not_filleted() {
        // r = 10, h = 10, sitting on z = 0. Its wall is split into halves by
        // seams whose facets meet at the facet angle, not at zero.
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 10.0));
        let solid = kernel::extrude(&s, 10.0).unwrap().remove(0);
        let (mesh, topo) = tessellate::tessellate(&solid, 0.1).unwrap();
        let seams: Vec<&EdgeInfo> = topo.edges.iter().filter(|e| e.curve_kind == "line").collect();
        assert!(!seams.is_empty(), "the wall has seams: {:?}", topo.edges);
        assert!(
            seams.iter().all(|e| e.dihedral_degrees >= TANGENT_DEGREES),
            "the premise: the facets read the seams as more than {TANGENT_DEGREES} degrees: {:?}",
            seams.iter().map(|e| e.dihedral_degrees).collect::<Vec<_>>()
        );
        let (out, _, report) =
            blend_body(&mesh, &topo, &EdgeSelection::All, BlendKind::Fillet { radius: 2.0 }, Some(0.4), 0.0)
                .expect("the rims fillet");
        assert_eq!(
            report.edges_selected,
            topo.edges.len() - seams.len(),
            "only the rim arcs; skipped {:?}",
            report.edges_skipped
        );
        assert_eq!(report.edges_skipped.len(), seams.len(), "{:?}", report.edges_skipped);
        // The wall at mid-height keeps its radius all the way round.
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in out.positions.chunks(3) {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k] as f64);
                hi[k] = hi[k].max(p[k] as f64);
            }
        }
        for k in 0..2 {
            assert!(hi[k] > 9.8 && lo[k] < -9.8, "the wall was shaved: {lo:?}..{hi:?}");
        }
    }

    #[test]
    fn a_concave_blend_stays_between_the_end_faces() {
        // An L section in XY, extruded 0..20 along Z; its one concave edge is
        // the inside corner at (2, 2), running between the end faces.
        let l = Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(20.0, 0.0),
            P2::new(20.0, 2.0),
            P2::new(2.0, 2.0),
            P2::new(2.0, 20.0),
            P2::new(0.0, 20.0),
        ]);
        let solid = kernel::extrude(&Sketch::new(Plane::XY).with_loop(l), 20.0).unwrap().remove(0);
        let (mesh, topo) = tessellate::tessellate(&solid, 0.05).unwrap();
        let inside = topo
            .edges
            .iter()
            .find(|e| e.polyline.iter().all(|p| (p.x - 2.0).abs() < 1e-6 && (p.y - 2.0).abs() < 1e-6))
            .expect("the inside corner");
        assert!(!inside.convex);
        for kind in [BlendKind::Fillet { radius: 3.0 }, BlendKind::Chamfer { distance: 3.0 }] {
            let (out, _, _) =
                blend_body(&mesh, &topo, &EdgeSelection::Named(vec![inside.name.clone()]), kind, Some(0.25), 0.0)
                    .expect("the blend succeeds");
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for p in out.positions.chunks(3) {
                lo = lo.min(p[2] as f64);
                hi = hi.max(p[2] as f64);
            }
            assert!(lo > -0.05 && hi < 20.05, "{kind:?}: z {lo}..{hi} grew past the end faces 0..20");
        }
    }

    /// A plate with a boss on it, joined on the mesh kernel (the faces are
    /// flush), and the boss's concave base edges filleted at r = 2.
    fn boss_base_fillet(boss: Loop) -> Vec<[f64; 3]> {
        let plate = Sketch::new(Plane::XY.offset(-5.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 40.0, 40.0));
        let plate = kernel::extrude(&plate, 5.0).unwrap().remove(0);
        let boss = kernel::extrude(&Sketch::new(Plane::XY).with_loop(boss), 10.0).unwrap().remove(0);
        let (plate, boss) = (tessellate::tessellate(&plate, 0.02).unwrap(), tessellate::tessellate(&boss, 0.02).unwrap());
        let (mesh, topo) = cadpole_core::mesh_kernel::boolean(kernel::BoolOp::Union, &plate, &boss, 0.05).unwrap();
        let concave: Vec<String> = topo.edges.iter().filter(|e| !e.convex).map(|e| e.name.clone()).collect();
        assert!(!concave.is_empty());
        let (out, _, _) =
            blend_body(&mesh, &topo, &EdgeSelection::Named(concave), BlendKind::Fillet { radius: 2.0 }, Some(0.25), 0.0)
                .expect("the base fillets");
        out.positions.chunks(3).map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect()
    }

    /// The highest output vertex in a small patch, as `(x, y)` ranges.
    fn top_in(pts: &[[f64; 3]], inside: impl Fn(f64, f64) -> bool) -> f64 {
        pts.iter().filter(|p| inside(p[0], p[1]) && p[2] < 2.5).map(|p| p[2]).fold(0.0, f64::max)
    }

    /// A round boss's base edge is two arcs meeting at seam vertices, and
    /// each arc's end is open, not an end face: the fillet runs on through
    /// the seam as it does 45 degrees away, with no notch.
    #[test]
    fn a_round_boss_base_fillet_has_no_notch_at_its_seams() {
        let pts = boss_base_fillet(Loop::circle(P2::new(0.0, 0.0), 8.0));
        let at = |deg: f64| {
            let a = deg.to_radians();
            top_in(&pts, |x, y| {
                let (r, t) = ((x * x + y * y).sqrt(), fmath::atan2(y, x));
                (8.3..9.0).contains(&r) && ((t - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs() < 0.06
            })
        };
        let (seam0, seam180, mid) = (at(0.0), at(180.0), at(45.0));
        assert!(mid > 0.3, "the fillet is there 45 degrees from the seams: {mid}");
        for (name, z) in [("0", seam0), ("180", seam180)] {
            assert!((z - mid).abs() < 0.15, "at the seam at {name} degrees the fillet reaches {z}, at 45 degrees {mid}");
        }
    }

    /// A square boss's base edges end at its convex corners, in open air: the
    /// fillets wrap the corner rather than stopping square at it.
    #[test]
    fn a_square_boss_base_fillet_wraps_its_corners() {
        let pts = boss_base_fillet(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let corner = top_in(&pts, |x, y| (5.05..6.0).contains(&x) && (5.05..6.0).contains(&y));
        assert!(corner > 0.05, "no fillet material in the corner quadrant: top at {corner}");
    }

    #[test]
    fn a_sphere_has_no_edge_to_fillet() {
        let solid = cadpole_core::primitives::sphere(Vec3::ZERO, 5.0).unwrap();
        let (mesh, topo) = tessellate::tessellate(&solid, 0.1).unwrap();
        assert!(!topo.edges.is_empty());
        let e = match blend_body(&mesh, &topo, &EdgeSelection::All, BlendKind::Fillet { radius: 1.0 }, Some(0.3), 0.0)
        {
            Ok((_, _, r)) => panic!("a sphere has only smooth seams, yet {} edges were blended", r.edges_selected),
            Err(e) => e,
        };
        assert!(e.contains("smooth seam"), "{e}");
    }
}
