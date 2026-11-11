//! Patterns along a curve and on a surface.
//!
//! `pattern_linear` and `pattern_circular` place copies by a rule that is
//! its own frame: a step vector, a turn about an axis. The two patterns here
//! place copies where something *else* already says where to go — a
//! [`Curve`] (a rivet line along a wing rib, posts round a ring, beads on a
//! helix) or the surface of a face (effusion holes on a combustor's conical
//! liner, knobs on a tyre's toroidal crown, a grid of studs on a plate) —
//! and read the frame off it: the sweep's moving frame along a curve
//! ([`crate::sweep::FrameField`], the rotation-minimising frame by double
//! reflection), the surface point and its outward normal on a face.
//!
//! **What this module is.** Pure layout: [`along`] and [`on_surface`] turn a
//! specification into a list of rigid [`Transform`]s (with the arc-length
//! station or the row and column each copy sits at), and nothing here knows
//! a body. [`Context::pattern_along`] and [`Context::pattern_on_surface`]
//! then place the copies exactly as `pattern_linear` does — instances by
//! transform (the source is tessellated once and every copy shares
//! that mesh, moved), or one fused body through [`Context::pattern`]'s
//! single `compose` — so the kernel-op story is the existing one and
//! the new code is the geometry.
//!
//! **The copy's frame.** The body is placed as drawn at the origin: its
//! origin lands on the station point, and with `align` its **+z** turns
//! along the curve's tangent (or the face's outward normal), its **+x**
//! along the frame's normal (the seed direction, `up=` when given) on a
//! curve, or along the row on a surface, and its +y completes the
//! right-handed frame. So a `cylinder(d=0.6, height=4)` on a face stands on the
//! face pointing outward; a hole cutter is drawn straddling `z = 0`.
//! Without `align` (or with `normal=False`) the body keeps its orientation
//! and is only moved.
//!
//! **Equal arc length, in millimetres.** Stations along a curve are at equal
//! arc length (or a given spacing) through the curve's own length tables
//! (exact for lines, arcs and cylindrical helices, quadrature for
//! the rest). On a surface, every analytic class ([`SurfaceClass`]) is
//! charted as rows along its natural circles or lines and rows stacked
//! across them, and both directions are measured in millimetres on the
//! surface, never in the parameter: a cone's rows are circles of different
//! radii, so `count=` is shared out among the rows in proportion to their
//! length (the pitch is then the same in millimetres on every row, as
//! nearly as whole numbers allow), while `cols=` puts the same number on
//! every row (columns line up on the rulings, the pitch grows with the
//! radius) and `spacing=(du, dv)` puts as many as fit at `du` along and
//! `dv` across. A free-form face is its NURBS domain sampled by chord
//! length along its isolines.
//!
//! **Trimming.** A face is a trimmed patch: a plate with a hole, a
//! cylinder cut by a slot. The chart covers the face's parametric extent,
//! and every candidate is then tested against the face itself — inside when
//! it lies within the chord tolerance of one of the face's triangles, and
//! at least `margin` from every boundary edge of the face — so copies land
//! only on material the face has.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

use truck_modeling::{ParametricSurface, ParametricSurface3D, SPHint2D, Surface};

use crate::assembly::{stable_x, Frame};
use crate::context::{fmt_num, Context, Span};
use crate::curve::{perpendicular_frame, Curve};
use crate::fmath;
use crate::geom::{Transform, Vec3};
use crate::ids::BodyId;
use crate::kernel::{point_to_vec3, vector_to_vec3, KernelChoice};
use crate::memo::KeyBuilder;
use crate::naming::point_triangle_distance;
use crate::sketch::Coord;
use crate::surface_class::SurfaceClass;
use crate::sweep::{Frame as SweepFrame, FrameField};
use crate::tessellate::{FaceInfo, Mesh, Topology};
use crate::{CadpoleError, Result};

/// Two stations closer than this, in mm, are the same station.
const STATION_EPS: f64 = 1e-9;

/// How many bins the periodic direction of a face is rasterised into to
/// decide whether the face goes all the way round (half a degree each).
const PERIOD_BINS: usize = 720;

/// Samples along an isoline of a free-form face for its chord-length table.
const FREEFORM_SAMPLES: usize = 96;

// ------------------------------------------------------------------ along

/// What `pattern_along` is asked for.
#[derive(Clone, Copy, Debug)]
pub struct AlongSpec<'a> {
    pub curve: &'a Curve,
    /// Copies at equal arc length over the curve (or the window).
    pub count: Option<usize>,
    /// Copies this many mm apart from the start; with `count` too, that
    /// many of them.
    pub spacing: Option<f64>,
    pub frame: SweepFrame,
    /// The seed of the frame's normal, and `sweep_frame.fixed`'s up.
    pub up: Option<Vec3>,
    /// The window of arc length the copies lie in, in mm from the curve's
    /// start; the whole curve when absent.
    pub start: Option<f64>,
    pub end: Option<f64>,
    /// Turn each copy's +z along the tangent (else translate only).
    pub align: bool,
    /// The most copies a pattern may make (the script layer's limit).
    pub max_count: usize,
}

/// The copies `pattern_along` places.
#[derive(Clone, Debug)]
pub struct AlongLayout {
    pub transforms: Vec<Transform>,
    /// The arc length from the curve's start each copy sits at, in mm.
    pub stations: Vec<f64>,
    pub length: f64,
    /// True when the copies go round a closed curve (the last is a step
    /// short of the seam, as `pattern_circular` divides a full turn).
    pub closed: bool,
}

/// Where the copies of `pattern_along` go and how each is turned.
pub fn along(spec: &AlongSpec<'_>) -> Result<AlongLayout> {
    let curve = spec.curve;
    if curve.segments.is_empty() {
        return Err(CadpoleError::invalid("pattern_along: the curve has no segments"));
    }
    let length = curve.length();
    if !(length > 0.0) || !length.is_finite() {
        return Err(CadpoleError::invalid("pattern_along: the curve has no length"));
    }
    let windowed = spec.start.is_some() || spec.end.is_some();
    let s0 = spec.start.unwrap_or(0.0);
    let s1 = spec.end.unwrap_or(length);
    for (what, v) in [("start", s0), ("end", s1)] {
        if !v.is_finite() || v < -STATION_EPS || v > length + STATION_EPS {
            return Err(CadpoleError::invalid(format!(
                "pattern_along: {what}={} is off the curve, which runs from 0 to {} mm of arc length",
                fmt_num(v),
                fmt_num(length)
            )));
        }
    }
    if s1 <= s0 + STATION_EPS {
        return Err(CadpoleError::invalid(format!(
            "pattern_along: start={} must come before end={} along the curve",
            fmt_num(s0),
            fmt_num(s1)
        )));
    }
    let (s0, s1) = (s0.max(0.0), s1.min(length));
    let closed = curve.closed && !windowed;
    let span = s1 - s0;
    let window = if windowed { format!(" (the window {} mm)", fmt_num(span)) } else { String::new() };
    let too_many = |n: usize| {
        CadpoleError::invalid(format!(
            "pattern_along: {n} copies is more than the limit of {}; use a larger spacing",
            spec.max_count
        ))
    };
    let stations: Vec<f64> = match (spec.count, spec.spacing) {
        (None, None) => return Err(CadpoleError::invalid("pattern_along: give count= (copies at equal arc length) or spacing= (mm between copies)")),
        (Some(n), None) => {
            if n > spec.max_count {
                return Err(too_many(n));
            }
            if closed {
                (0..n).map(|k| s0 + span * k as f64 / n as f64).collect()
            } else if n == 1 {
                vec![s0]
            } else {
                (0..n).map(|k| s0 + span * k as f64 / (n - 1) as f64).collect()
            }
        }
        (None, Some(d)) => {
            if !(d > 0.0) || !d.is_finite() {
                return Err(CadpoleError::invalid(format!("pattern_along: spacing must be a positive number of mm, got {}", fmt_num(d))));
            }
            let fit = (span / d + STATION_EPS).floor();
            let n = if closed { fit.max(1.0) as usize } else { fit as usize + 1 };
            if n > spec.max_count {
                return Err(too_many(n));
            }
            (0..n).map(|k| s0 + d * k as f64).collect()
        }
        (Some(n), Some(d)) => {
            if !(d > 0.0) || !d.is_finite() {
                return Err(CadpoleError::invalid(format!("pattern_along: spacing must be a positive number of mm, got {}", fmt_num(d))));
            }
            if n > spec.max_count {
                return Err(too_many(n));
            }
            let need = d * (n - 1) as f64;
            if need > span + STATION_EPS {
                return Err(CadpoleError::invalid(format!(
                    "pattern_along: {n} copies {} mm apart span {} mm, and the curve is {} mm long{window}; lower \
                     the count, close up the spacing, or lengthen the curve",
                    fmt_num(d),
                    fmt_num(need),
                    fmt_num(length)
                )));
            }
            (0..n).map(|k| s0 + d * k as f64).collect()
        }
    };
    // The frames: the sweep's field, lenient about corners and a closed
    // path, seeded like `Curve.start_plane`.
    let t0 = curve.segments[0].start_direction().unwrap_or(Vec3::Z);
    let seed = perpendicular_frame(t0).0;
    let field = FrameField::lenient(curve, spec.frame, spec.up, seed).map_err(rename_sweep)?;
    let cum = curve.cumulative_lengths();
    let mut transforms = Vec::with_capacity(stations.len());
    for &s in &stations {
        let (i, local) = locate(&cum, s);
        let t = curve.segments[i].param_at_length(local);
        let f = field.at(curve, i, t);
        transforms.push(if spec.align {
            Frame { origin: f.point, z: f.t, x: f.n }.to_transform()
        } else {
            Transform::translation(f.point)
        });
    }
    Ok(AlongLayout { transforms, stations, length, closed })
}

/// The segment an arc length `s` falls in, and the length into it. A
/// station at a joint belongs to the segment leaving it, so a copy at a
/// corner faces the way the curve goes on.
fn locate(cum: &[f64], s: f64) -> (usize, f64) {
    let n = cum.len() - 1;
    for i in 0..n {
        if s < cum[i + 1] - STATION_EPS {
            return (i, (s - cum[i]).max(0.0));
        }
    }
    (n - 1, (s - cum[n - 1]).max(0.0))
}

/// A frame-field refusal, which speaks of `sweep_along`, in this op's name.
fn rename_sweep(e: CadpoleError) -> CadpoleError {
    CadpoleError::invalid(e.to_string().replacen("sweep_along:", "pattern_along:", 1))
}

// ------------------------------------------------------------- on surface

/// How the copies of `pattern_on_surface` are counted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grid {
    /// `rows` rows of `cols` copies: the same number on every row.
    RowsCols { rows: usize, cols: usize },
    /// `count` copies in all, shared out among `rows` rows (chosen for
    /// near-square cells when `None`) in proportion to each row's length.
    Count { rows: Option<usize>, count: usize },
    /// As many as fit `du` mm apart along a row and `dv` mm between rows.
    Spacing { du: f64, dv: f64 },
}

/// What `pattern_on_surface` is asked for.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceSpec {
    pub grid: Grid,
    /// Turn each copy's +z along the outward normal (else translate only).
    pub normal: bool,
    /// Shift every other row by half a pitch along the row.
    pub stagger: bool,
    /// The least distance from a copy's point to the face's boundary, mm.
    pub margin: f64,
    pub max_count: usize,
}

/// The copies `pattern_on_surface` places.
#[derive(Clone, Debug)]
pub struct SurfaceLayout {
    pub transforms: Vec<Transform>,
    /// `(row, column)` of each copy, the column counted within its row
    /// among the copies that were kept.
    pub cells: Vec<(usize, usize)>,
    /// The surface point of each copy.
    pub points: Vec<Vec3>,
    /// How many rows were laid out, and how many copies each was given
    /// before trimming.
    pub rows: usize,
    pub per_row: Vec<usize>,
    /// The pitch along each row, in mm (0 for a row of one).
    pub row_pitch: Vec<f64>,
    /// The pitch between rows, in mm (0 for a single row).
    pub across_pitch: f64,
    /// Candidates the face's trimmed region or the margin excluded.
    pub skipped: usize,
    /// The chart the face was laid out on: its class name, or `nurbs`.
    pub chart: &'static str,
    /// Which direction goes all the way round, for the report.
    pub closed_rows: bool,
    pub closed_across: bool,
}

/// Where the copies of `pattern_on_surface` go on `faces` of the body whose
/// display mesh is `(mesh, topo)`, tessellated within `chord` mm of the
/// exact surface. Several faces are one surface seen in pieces — truck
/// makes a cylinder's side, a cone's and a sphere's as two halves — and
/// must lie on the same analytic surface; the seams between them are not
/// boundaries.
pub fn on_surface(mesh: &Mesh, topo: &Topology, faces: &[&FaceInfo], chord: f64, spec: &SurfaceSpec) -> Result<SurfaceLayout> {
    if !(spec.margin >= 0.0) || !spec.margin.is_finite() {
        return Err(CadpoleError::invalid(format!(
            "pattern_on_surface: margin must be a distance in mm, at least 0, got {}",
            fmt_num(spec.margin)
        )));
    }
    let Some(face) = faces.first() else {
        return Err(CadpoleError::invalid("pattern_on_surface: no face to lay copies on"));
    };
    for other in &faces[1..] {
        if !same_surface(&face.surface, &other.surface, face.geometry.as_ref(), other.geometry.as_ref()) {
            return Err(CadpoleError::invalid(format!(
                "pattern_on_surface: faces {} ({}) and {} ({}) lie on different surfaces; a pattern lies on one \
                 surface, so give the faces of one (a cylinder's two halves are one surface)",
                face.name,
                face.kind(),
                other.name,
                other.kind()
            )));
        }
    }
    let tris: Vec<[Vec3; 3]> = faces
        .iter()
        .flat_map(|f| (f.tri_start..f.tri_start + f.tri_count).filter_map(|t| mesh.triangle(t as usize)))
        .collect();
    if tris.is_empty() {
        return Err(CadpoleError::invalid(format!("pattern_on_surface: face {} has no triangles to lay copies on", face.name)));
    }
    let chart = Chart::of(face, &tris)?;
    let trim = Trim::new(&tris, topo, faces, chord);
    let (u_range, v_range) = chart.extent(&tris);
    let m = spec.margin;

    // The rows: positions across, in arc length along the across isoline
    // at the middle of the u extent.
    let u_mid = match u_range {
        Range::Open(a, b) => (a + b) * 0.5,
        Range::Closed(seam, period) => seam + period * 0.5,
    };
    let across = match v_range {
        Range::Open(a, b) => chart.across_table(u_mid, a, b),
        Range::Closed(seam, period) => chart.across_table(u_mid, seam, seam + period),
    };
    let closed_across = matches!(v_range, Range::Closed(..));
    let a_len = across.length();
    let a_window = if closed_across { a_len } else { a_len - 2.0 * m };
    if !closed_across && a_window < -STATION_EPS {
        return Err(CadpoleError::invalid(format!(
            "pattern_on_surface: face {} is {} mm across and a margin of {} mm on each side leaves nothing",
            face.name,
            fmt_num(a_len),
            fmt_num(m)
        )));
    }
    let a_window = a_window.max(0.0);
    let mid_row_len = {
        let v = across.param_at(if closed_across { 0.0 } else { m + a_window * 0.5 });
        row_table(&chart, &u_range, v).length()
    };
    let rows_wanted = match spec.grid {
        Grid::RowsCols { rows, .. } => rows,
        Grid::Count { rows: Some(rows), .. } => rows,
        Grid::Count { rows: None, count } => {
            // Near-square cells: the pitch across about the pitch along.
            let ratio = if mid_row_len > 0.0 { count as f64 * a_window / mid_row_len } else { 1.0 };
            ((0.5 + fmath::sqrt(0.25 + ratio)).round() as usize).clamp(1, count.max(1))
        }
        Grid::Spacing { dv, .. } => {
            if !(dv > 0.0) || !dv.is_finite() {
                return Err(CadpoleError::invalid(format!("pattern_on_surface: spacing between rows must be positive, got {}", fmt_num(dv))));
            }
            if closed_across {
                ((a_len / dv).round() as usize).max(1)
            } else {
                (a_window / dv + STATION_EPS).floor() as usize + 1
            }
        }
    };
    if rows_wanted == 0 {
        return Err(CadpoleError::invalid("pattern_on_surface: rows must be at least 1"));
    }
    let across_pitch = if rows_wanted <= 1 {
        0.0
    } else if closed_across {
        a_len / rows_wanted as f64
    } else if let Grid::Spacing { dv, .. } = spec.grid {
        dv
    } else {
        a_window / (rows_wanted - 1) as f64
    };
    let row_positions: Vec<f64> = (0..rows_wanted)
        .map(|i| {
            if closed_across {
                across_pitch * i as f64
            } else if rows_wanted == 1 {
                m + a_window * 0.5
            } else if let Grid::Spacing { .. } = spec.grid {
                m + (a_window - across_pitch * (rows_wanted - 1) as f64) * 0.5 + across_pitch * i as f64
            } else {
                m + across_pitch * i as f64
            }
        })
        .collect();
    let row_vs: Vec<f64> = row_positions.iter().map(|&s| across.param_at(s)).collect();

    // Each row's table and usable length.
    let closed_rows = matches!(u_range, Range::Closed(..));
    let tables: Vec<ArcTable> = row_vs.iter().map(|&v| row_table(&chart, &u_range, v)).collect();
    let usable: Vec<f64> = tables
        .iter()
        .map(|t| if closed_rows { t.length() } else { (t.length() - 2.0 * m).max(0.0) })
        .collect();

    // How many on each row.
    let per_row: Vec<usize> = match spec.grid {
        Grid::RowsCols { cols, .. } => vec![cols; rows_wanted],
        Grid::Count { count, .. } => share(count, &usable),
        Grid::Spacing { du, .. } => {
            if !(du > 0.0) || !du.is_finite() {
                return Err(CadpoleError::invalid(format!("pattern_on_surface: spacing along a row must be positive, got {}", fmt_num(du))));
            }
            usable
                .iter()
                .map(|&l| if closed_rows { ((l / du).round() as usize).max(1) } else { (l / du + STATION_EPS).floor() as usize + 1 })
                .collect()
        }
    };
    let total: usize = per_row.iter().sum();
    if total > spec.max_count {
        return Err(CadpoleError::invalid(format!(
            "pattern_on_surface: {total} copies is more than the limit of {}; use fewer rows or a larger spacing",
            spec.max_count
        )));
    }
    if total == 0 {
        return Err(CadpoleError::invalid("pattern_on_surface: the layout has no copies"));
    }

    // The copies.
    let mut out = SurfaceLayout {
        transforms: Vec::with_capacity(total),
        cells: Vec::with_capacity(total),
        points: Vec::with_capacity(total),
        rows: rows_wanted,
        per_row: per_row.clone(),
        row_pitch: Vec::with_capacity(rows_wanted),
        across_pitch,
        skipped: 0,
        chart: chart.name(),
        closed_rows,
        closed_across,
    };
    for (i, table) in tables.iter().enumerate() {
        let n = per_row[i];
        let len = usable[i];
        let pitch = if n <= 1 {
            0.0
        } else if closed_rows {
            table.length() / n as f64
        } else if let Grid::Spacing { du, .. } = spec.grid {
            du
        } else {
            len / (n - 1) as f64
        };
        out.row_pitch.push(pitch);
        if n == 0 {
            continue;
        }
        let odd = spec.stagger && i % 2 == 1;
        // Positions along the row in mm from the row's start (the seam, or
        // the start of the face's extent).
        let mut positions: Vec<f64> = if closed_rows {
            let shift = if odd { pitch * 0.5 } else { 0.0 };
            (0..n).map(|j| pitch * j as f64 + shift).collect()
        } else if n == 1 {
            vec![m + len * 0.5]
        } else if let Grid::Spacing { .. } = spec.grid {
            let lead = m + (len - pitch * (n - 1) as f64) * 0.5;
            (0..n).map(|j| lead + pitch * j as f64).collect()
        } else {
            (0..n).map(|j| m + pitch * j as f64).collect()
        };
        if odd && !closed_rows && n > 1 {
            // A brick course: the copies between the row below's, one fewer.
            positions = positions.windows(2).map(|w| (w[0] + w[1]) * 0.5).collect();
        }
        let mut col = 0usize;
        for s in positions {
            let u = table.param_at(s);
            let f = chart.frame(u, row_vs[i]);
            // The clearance is measured to the boundary's polyline, which
            // sits within the chord error of the true edge, so a copy the
            // layout put exactly `margin` from an end is kept: the test is
            // for what the face's trimmed region takes away.
            if !trim.inside(f.origin) || (m > 0.0 && trim.clearance(f.origin) < m - trim.in_tol) {
                out.skipped += 1;
                continue;
            }
            out.transforms.push(if spec.normal { f.to_transform() } else { Transform::translation(f.origin) });
            out.cells.push((i, col));
            out.points.push(f.origin);
            col += 1;
        }
    }
    if out.transforms.is_empty() {
        return Err(CadpoleError::invalid(format!(
            "pattern_on_surface: every one of the {total} copies fell outside face {} (or within {} mm of its \
             boundary); lower the margin or give the face more room",
            face.name,
            fmt_num(m)
        )));
    }
    Ok(out)
}

/// True when two faces' classes are one surface: the same kind with the
/// same axis, radius, centre or plane to a micron (a cylinder's two halves
/// carry different `axis_point`s, both on the one axis).
fn same_surface(a: &SurfaceClass, b: &SurfaceClass, ga: Option<&Arc<Surface>>, gb: Option<&Arc<Surface>>) -> bool {
    const EPS: f64 = 1e-6;
    let parallel = |p: Vec3, q: Vec3| p.normalized().dot(q.normalized()).abs() > 1.0 - 1e-9;
    let on_line = |p: Vec3, q: Vec3, axis: Vec3| {
        let d = p - q;
        (d - axis.normalized() * d.dot(axis.normalized())).length() <= EPS
    };
    match (a, b) {
        (SurfaceClass::Plane { origin: oa, normal: na }, SurfaceClass::Plane { origin: ob, normal: nb }) => {
            parallel(*na, *nb) && (*oa - *ob).dot(na.normalized()).abs() <= EPS
        }
        (
            SurfaceClass::Cylinder { axis_point: pa, axis: aa, radius: ra },
            SurfaceClass::Cylinder { axis_point: pb, axis: ab, radius: rb },
        ) => parallel(*aa, *ab) && (ra - rb).abs() <= EPS && on_line(*pa, *pb, *aa),
        (SurfaceClass::Cone { apex: pa, axis: aa, half_angle: ha }, SurfaceClass::Cone { apex: pb, axis: ab, half_angle: hb }) => {
            aa.normalized().dot(ab.normalized()) > 1.0 - 1e-9 && (ha - hb).abs() <= 1e-9 && (*pa - *pb).length() <= EPS
        }
        (SurfaceClass::Sphere { center: ca, radius: ra }, SurfaceClass::Sphere { center: cb, radius: rb }) => {
            (*ca - *cb).length() <= EPS && (ra - rb).abs() <= EPS
        }
        (
            SurfaceClass::Torus { center: ca, axis: aa, major: ma, minor: na },
            SurfaceClass::Torus { center: cb, axis: ab, major: mb, minor: nb },
        ) => (*ca - *cb).length() <= EPS && parallel(*aa, *ab) && (ma - mb).abs() <= EPS && (na - nb).abs() <= EPS,
        (SurfaceClass::Other, SurfaceClass::Other) => match (ga, gb) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            _ => false,
        },
        _ => false,
    }
}

/// `count` shared out in proportion to `weights`, to whole numbers that
/// add up to `count` exactly (largest remainders first; a tie goes to the
/// earlier row).
fn share(count: usize, weights: &[f64]) -> Vec<usize> {
    let total: f64 = weights.iter().filter(|w| w.is_finite()).sum();
    if weights.is_empty() {
        return Vec::new();
    }
    if !(total > 0.0) {
        // Every row of zero length (rows through a pole): spread evenly.
        let base = count / weights.len();
        let extra = count % weights.len();
        return (0..weights.len()).map(|i| base + usize::from(i < extra)).collect();
    }
    let exact: Vec<f64> = weights.iter().map(|w| count as f64 * w.max(0.0) / total).collect();
    let mut out: Vec<usize> = exact.iter().map(|e| e.floor() as usize).collect();
    let given: usize = out.iter().sum();
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| {
        let fa = exact[a] - exact[a].floor();
        let fb = exact[b] - exact[b].floor();
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    for &i in order.iter().take(count.saturating_sub(given)) {
        out[i] += 1;
    }
    out
}

/// The row's arc-length table over the face's extent (or its whole period).
fn row_table(chart: &Chart, u_range: &Range, v: f64) -> ArcTable {
    match *u_range {
        Range::Open(a, b) => chart.row_table(v, a, b),
        Range::Closed(seam, period) => chart.row_table(v, seam, seam + period),
    }
}

// ------------------------------------------------------------------ charts

/// One direction of a face's extent in the chart's parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Range {
    Open(f64, f64),
    /// The face goes all the way round: copies start at `seam` and the
    /// direction repeats after `period`.
    Closed(f64, f64),
}

/// Arc length along an isoline, as a table from the parameter: linear for
/// the analytic charts (two entries, exact), sampled for a free-form one.
#[derive(Clone, Debug)]
struct ArcTable {
    params: Vec<f64>,
    cum: Vec<f64>,
}

impl ArcTable {
    fn linear(p0: f64, p1: f64, scale: f64) -> ArcTable {
        ArcTable { params: vec![p0, p1], cum: vec![0.0, (p1 - p0) * scale] }
    }

    fn length(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// The parameter at arc length `s` from the start (clamped).
    fn param_at(&self, s: f64) -> f64 {
        let n = self.params.len();
        if n < 2 || !(self.length() > 0.0) {
            return self.params.first().copied().unwrap_or(0.0);
        }
        let s = s.clamp(0.0, self.length());
        let i = match self.cum.binary_search_by(|c| c.partial_cmp(&s).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(i) => return self.params[i.min(n - 1)],
            Err(i) => i.clamp(1, n - 1),
        };
        let (c0, c1) = (self.cum[i - 1], self.cum[i]);
        let f = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
        self.params[i - 1] + (self.params[i] - self.params[i - 1]) * f
    }
}

/// A face's surface as rows and columns: `u` along a row, `v` across.
enum Chart {
    Plane { origin: Vec3, x: Vec3, y: Vec3, n: Vec3 },
    Cylinder { axis_point: Vec3, axis: Vec3, x: Vec3, y: Vec3, radius: f64, sign: f64 },
    Cone { apex: Vec3, axis: Vec3, x: Vec3, y: Vec3, half: f64, sign: f64 },
    Sphere { center: Vec3, axis: Vec3, x: Vec3, y: Vec3, radius: f64, sign: f64 },
    Torus { center: Vec3, axis: Vec3, x: Vec3, y: Vec3, major: f64, minor: f64, sign: f64 },
    Free { surface: Arc<Surface>, sign: f64, u_period: Option<f64>, v_period: Option<f64> },
}

impl Chart {
    /// The chart of `face`: its recognised class, or its own surface when
    /// it has one, with the normal's sign read off the face's triangles
    /// (outward for a solid's mesh).
    fn of(face: &FaceInfo, tris: &[[Vec3; 3]]) -> Result<Chart> {
        let class = &face.surface;
        let (tri_centre, tri_normal) = largest_triangle(tris);
        let sign_against = |natural: Option<Vec3>| -> f64 {
            match natural {
                Some(nat) if nat.dot(tri_normal) < 0.0 => -1.0,
                _ => 1.0,
            }
        };
        Ok(match *class {
            SurfaceClass::Plane { origin, normal } => {
                let n = normal.normalized();
                let n = if n.dot(tri_normal) < 0.0 { -n } else { n };
                let x = stable_x(n);
                Chart::Plane { origin, x, y: n.cross(x), n }
            }
            SurfaceClass::Cylinder { axis_point, axis, radius } => {
                let axis = axis.normalized();
                let (x, y) = row_reference(axis);
                Chart::Cylinder { axis_point, axis, x, y, radius, sign: sign_against(class.normal_at(tri_centre)) }
            }
            SurfaceClass::Cone { apex, axis, half_angle } => {
                let axis = axis.normalized();
                let (x, y) = row_reference(axis);
                Chart::Cone { apex, axis, x, y, half: half_angle, sign: sign_against(class.normal_at(tri_centre)) }
            }
            SurfaceClass::Sphere { center, radius } => {
                let axis = Vec3::Z;
                let (x, y) = row_reference(axis);
                Chart::Sphere { center, axis, x, y, radius, sign: sign_against(class.normal_at(tri_centre)) }
            }
            SurfaceClass::Torus { center, axis, major, minor } => {
                let axis = axis.normalized();
                let (x, y) = row_reference(axis);
                Chart::Torus { center, axis, x, y, major, minor, sign: sign_against(class.normal_at(tri_centre)) }
            }
            SurfaceClass::Other => {
                let Some(surface) = face.geometry.clone() else {
                    return Err(CadpoleError::invalid(format!(
                        "pattern_on_surface: face {} is a mesh face with no surface, so it has no normal to place \
                         copies along; pattern on a face of a B-rep body (a mesh body's faces keep a class only \
                         where the boolean carried one through)",
                        face.name
                    )));
                };
                let natural = nearest_parameter(&surface, tri_centre)
                    .map(|(u, v)| vector_to_vec3(surface.normal(u, v)).normalized());
                let sign = sign_against(natural);
                let u_period = surface.u_period();
                let v_period = surface.v_period();
                Chart::Free { surface, sign, u_period, v_period }
            }
        })
    }

    fn name(&self) -> &'static str {
        match self {
            Chart::Plane { .. } => "plane",
            Chart::Cylinder { .. } => "cylinder",
            Chart::Cone { .. } => "cone",
            Chart::Sphere { .. } => "sphere",
            Chart::Torus { .. } => "torus",
            Chart::Free { .. } => "nurbs",
        }
    }

    fn u_period(&self) -> Option<f64> {
        match self {
            Chart::Plane { .. } => None,
            Chart::Cylinder { .. } | Chart::Cone { .. } | Chart::Sphere { .. } | Chart::Torus { .. } => Some(TAU),
            Chart::Free { u_period, .. } => *u_period,
        }
    }

    fn v_period(&self) -> Option<f64> {
        match self {
            Chart::Torus { .. } => Some(TAU),
            Chart::Free { v_period, .. } => *v_period,
            _ => None,
        }
    }

    /// The angle of `d` about `axis`, from `x` towards `y`, in (−π, π].
    fn angle(d: Vec3, x: Vec3, y: Vec3) -> f64 {
        fmath::atan2(d.dot(y), d.dot(x))
    }

    /// The chart parameters of a point on the surface; `None` where the
    /// point cannot be inverted (a free-form surface's Newton search).
    fn uv(&self, p: Vec3) -> Option<(f64, f64)> {
        Some(match self {
            Chart::Plane { origin, x, y, .. } => ((p - *origin).dot(*x), (p - *origin).dot(*y)),
            Chart::Cylinder { axis_point, axis, x, y, .. } => {
                let d = p - *axis_point;
                let h = d.dot(*axis);
                (Self::angle(d - *axis * h, *x, *y), h)
            }
            Chart::Cone { apex, axis, x, y, .. } => {
                let d = p - *apex;
                let h = d.dot(*axis);
                (Self::angle(d - *axis * h, *x, *y), d.length())
            }
            Chart::Sphere { center, axis, x, y, radius, .. } => {
                let d = p - *center;
                let r = if *radius > 0.0 { *radius } else { 1.0 };
                let lat = fmath::asin((d.dot(*axis) / r).clamp(-1.0, 1.0));
                (Self::angle(d - *axis * d.dot(*axis), *x, *y), lat)
            }
            Chart::Torus { center, axis, x, y, major, .. } => {
                let d = p - *center;
                let h = d.dot(*axis);
                let r_vec = d - *axis * h;
                (Self::angle(r_vec, *x, *y), fmath::atan2(h, r_vec.length() - *major))
            }
            Chart::Free { surface, .. } => nearest_parameter(surface, p)?,
        })
    }

    /// The point at `(u, v)`.
    fn point(&self, u: f64, v: f64) -> Vec3 {
        match self {
            Chart::Plane { origin, x, y, .. } => *origin + *x * u + *y * v,
            Chart::Cylinder { axis_point, axis, x, y, radius, .. } => {
                let (s, c) = fmath::sin_cos(u);
                *axis_point + *axis * v + (*x * c + *y * s) * *radius
            }
            Chart::Cone { apex, axis, x, y, half, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sh, ch) = fmath::sin_cos(*half);
                *apex + *axis * (v * ch) + (*x * c + *y * s) * (v * sh)
            }
            Chart::Sphere { center, axis, x, y, radius, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sl, cl) = fmath::sin_cos(v);
                *center + ((*x * c + *y * s) * cl + *axis * sl) * *radius
            }
            Chart::Torus { center, axis, x, y, major, minor, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sp, cp) = fmath::sin_cos(v);
                *center + (*x * c + *y * s) * (*major + *minor * cp) + *axis * (*minor * sp)
            }
            Chart::Free { surface, .. } => point_to_vec3(surface.subs(u, v)),
        }
    }

    /// The copy's frame at `(u, v)`: origin on the surface, `z` the outward
    /// normal, `x` along the row.
    fn frame(&self, u: f64, v: f64) -> Frame {
        let origin = self.point(u, v);
        let (z, x) = match self {
            Chart::Plane { x, n, .. } => (*n, *x),
            Chart::Cylinder { axis, x, y, sign, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let radial = *x * c + *y * s;
                (radial * *sign, axis.cross(radial))
            }
            Chart::Cone { axis, x, y, half, sign, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sh, ch) = fmath::sin_cos(*half);
                let radial = *x * c + *y * s;
                ((radial * ch - *axis * sh) * *sign, axis.cross(radial))
            }
            Chart::Sphere { axis, x, y, sign, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sl, cl) = fmath::sin_cos(v);
                let radial = *x * c + *y * s;
                let n = (radial * cl + *axis * sl) * *sign;
                let along = if cl.abs() > 1e-9 { axis.cross(radial) } else { stable_x(n) };
                (n, along)
            }
            Chart::Torus { axis, x, y, sign, .. } => {
                let (s, c) = fmath::sin_cos(u);
                let (sp, cp) = fmath::sin_cos(v);
                let radial = *x * c + *y * s;
                ((radial * cp + *axis * sp) * *sign, axis.cross(radial))
            }
            Chart::Free { surface, sign, .. } => {
                let n = vector_to_vec3(surface.normal(u, v)).normalized() * *sign;
                let du = vector_to_vec3(surface.uder(u, v));
                let along = du - n * du.dot(n);
                (n, if along.length() > 1e-9 { along.normalized() } else { stable_x(n) })
            }
        };
        let z = if z.length() > 0.0 { z.normalized() } else { Vec3::Z };
        let x = x - z * x.dot(z);
        let x = if x.length() > 1e-9 { x.normalized() } else { stable_x(z) };
        Frame { origin, z, x }
    }

    /// Arc length along the row at `v` from `u0` to `u1`.
    fn row_table(&self, v: f64, u0: f64, u1: f64) -> ArcTable {
        match self {
            Chart::Plane { .. } => ArcTable::linear(u0, u1, 1.0),
            Chart::Cylinder { radius, .. } => ArcTable::linear(u0, u1, *radius),
            Chart::Cone { half, .. } => ArcTable::linear(u0, u1, v * fmath::sin(*half)),
            Chart::Sphere { radius, .. } => ArcTable::linear(u0, u1, *radius * fmath::cos(v).max(0.0)),
            Chart::Torus { major, minor, .. } => ArcTable::linear(u0, u1, (*major + *minor * fmath::cos(v)).max(0.0)),
            Chart::Free { .. } => self.sampled(|t| (u0 + (u1 - u0) * t, v), u0, u1),
        }
    }

    /// Arc length across the rows along the isoline at `u`, from `v0` to `v1`.
    fn across_table(&self, u: f64, v0: f64, v1: f64) -> ArcTable {
        match self {
            Chart::Plane { .. } | Chart::Cylinder { .. } | Chart::Cone { .. } => ArcTable::linear(v0, v1, 1.0),
            Chart::Sphere { radius, .. } => ArcTable::linear(v0, v1, *radius),
            Chart::Torus { minor, .. } => ArcTable::linear(v0, v1, *minor),
            Chart::Free { .. } => self.sampled(|t| (u, v0 + (v1 - v0) * t), v0, v1),
        }
    }

    /// A chord-length table along the isoline `at(t)`, `t` in 0..1, whose
    /// parameter runs from `p0` to `p1`.
    fn sampled(&self, at: impl Fn(f64) -> (f64, f64), p0: f64, p1: f64) -> ArcTable {
        let n = FREEFORM_SAMPLES;
        let mut params = Vec::with_capacity(n + 1);
        let mut cum = Vec::with_capacity(n + 1);
        let mut prev = None;
        let mut s = 0.0;
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let (u, v) = at(t);
            let p = self.point(u, v);
            if let Some(q) = prev {
                s += p.dist(q);
            }
            prev = Some(p);
            params.push(p0 + (p1 - p0) * t);
            cum.push(s);
        }
        ArcTable { params, cum }
    }

    /// The face's extent in the chart: the smallest range of each parameter
    /// holding every corner of its triangles, and whether it goes all the
    /// way round a periodic direction.
    fn extent(&self, tris: &[[Vec3; 3]]) -> (Range, Range) {
        let uvs: Vec<[Option<(f64, f64)>; 3]> = tris.iter().map(|t| [self.uv(t[0]), self.uv(t[1]), self.uv(t[2])]).collect();
        let u = extent_of(&uvs, 0, self.u_period());
        let v = extent_of(&uvs, 1, self.v_period());
        (u, v)
    }
}

/// The extent of coordinate `which` (0 for `u`, 1 for `v`) over the
/// triangles' corners: min and max, or, for a periodic coordinate, whether
/// the triangles cover every bin of the period and, if not, the arc that
/// they do cover, read off the largest gap.
fn extent_of(uvs: &[[Option<(f64, f64)>; 3]], which: usize, period: Option<f64>) -> Range {
    let pick = |uv: (f64, f64)| if which == 0 { uv.0 } else { uv.1 };
    let Some(period) = period else {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for tri in uvs {
            for c in tri.iter().flatten() {
                let x = pick(*c);
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        if !lo.is_finite() {
            return Range::Open(0.0, 0.0);
        }
        return Range::Open(lo, hi);
    };
    // Rasterise every triangle's span (its corners unwrapped to within half
    // a period of the first) into bins over one period from −period/2.
    let base = -period * 0.5;
    let mut bins = vec![false; PERIOD_BINS];
    for tri in uvs {
        let corners: Vec<f64> = tri.iter().flatten().map(|c| pick(*c)).collect();
        if corners.is_empty() {
            continue;
        }
        let a0 = corners[0];
        let (mut lo, mut hi) = (a0, a0);
        for &c in &corners[1..] {
            let mut x = c;
            while x - a0 > period * 0.5 {
                x -= period;
            }
            while a0 - x > period * 0.5 {
                x += period;
            }
            lo = lo.min(x);
            hi = hi.max(x);
        }
        let b0 = ((lo - base) / period * PERIOD_BINS as f64).floor() as i64;
        let b1 = ((hi - base) / period * PERIOD_BINS as f64).ceil() as i64;
        for b in b0..=b1 {
            bins[b.rem_euclid(PERIOD_BINS as i64) as usize] = true;
        }
    }
    // The largest run of empty bins, circularly.
    let mut best: Option<(usize, usize)> = None; // (start bin, length)
    let mut start = None;
    for k in 0..PERIOD_BINS * 2 {
        let b = bins[k % PERIOD_BINS];
        match (b, start) {
            (false, None) => start = Some(k),
            (true, Some(s)) => {
                let len = k - s;
                if best.map_or(true, |(_, l)| len > l) {
                    best = Some((s % PERIOD_BINS, len));
                }
                start = None;
            }
            _ => {}
        }
        if let Some(s) = start {
            if k - s >= PERIOD_BINS {
                // Nothing marked at all.
                return Range::Open(0.0, 0.0);
            }
        }
    }
    let Some((gap_start, gap_len)) = best else {
        return Range::Closed(0.0, period);
    };
    // The face begins where the gap ends; unwrap every corner from there.
    let begin = base + (gap_start + gap_len) as f64 / PERIOD_BINS as f64 * period;
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for tri in uvs {
        for c in tri.iter().flatten() {
            let mut x = pick(*c);
            // Into [begin − a bin, begin − a bin + period).
            let floor = begin - period / PERIOD_BINS as f64;
            x -= ((x - floor) / period).floor() * period;
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    Range::Open(lo, hi)
}

/// The reference direction a closed row starts from and the direction a
/// quarter turn on: a stable rule (the world axis least along the
/// axis, ties X before Y before Z, so a classified axis's 1e-17 of noise
/// cannot swap them) — `+X` for a vertical axis, where a helix starts too.
fn row_reference(axis: Vec3) -> (Vec3, Vec3) {
    let x = stable_x(axis);
    (x, axis.cross(x))
}

/// The centroid and unit normal of the largest triangle, for the sign of
/// a surface's natural normal against the mesh's outward one.
fn largest_triangle(tris: &[[Vec3; 3]]) -> (Vec3, Vec3) {
    let mut best = (0.0, Vec3::ZERO, Vec3::Z);
    for t in tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let a = n.length();
        if a > best.0 {
            best = (a, (t[0] + t[1] + t[2]) * (1.0 / 3.0), n * (1.0 / a));
        }
    }
    (best.1, best.2)
}

/// The parameters of the point on `surface` nearest `p`.
fn nearest_parameter(surface: &Surface, p: Vec3) -> Option<(f64, f64)> {
    crate::ksurface::nearest_parameter(surface, p.point(), SPHint2D::None, 100)
}

// -------------------------------------------------------------------- trim

/// The face's trimmed region: its triangles, and its boundary polylines.
struct Trim<'a> {
    tris: &'a [[Vec3; 3]],
    boxes: Vec<(Vec3, Vec3)>,
    boundary: Vec<&'a [Vec3]>,
    in_tol: f64,
}

impl<'a> Trim<'a> {
    fn new(tris: &'a [[Vec3; 3]], topo: &'a Topology, faces: &[&FaceInfo], chord: f64) -> Trim<'a> {
        let boxes = tris.iter().map(|t| (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]))).collect();
        // The boundary is every edge of the faces that is not a seam
        // between two of them.
        let names: Vec<&str> = faces.iter().map(|f| f.name.as_str()).collect();
        let mut seen = std::collections::BTreeSet::new();
        let mut boundary: Vec<&[Vec3]> = Vec::new();
        for face in faces {
            for &e in &face.edge_ids {
                if !seen.insert(e) {
                    continue;
                }
                let Some(edge) = topo.edges.get(e as usize) else { continue };
                let seam = edge.faces.len() >= 2 && edge.faces.iter().all(|f| names.contains(&f.as_str()));
                if !seam && edge.polyline.len() >= 2 {
                    boundary.push(&edge.polyline);
                }
            }
        }
        // A point on the surface is within the chord error of the face's
        // triangles when it is over one of them; twice that, plus a floor,
        // is the slack.
        Trim { tris, boxes, boundary, in_tol: 2.0 * chord.max(0.0) + 1e-4 }
    }

    /// True when `p` lies over one of the face's triangles.
    fn inside(&self, p: Vec3) -> bool {
        let tol = self.in_tol;
        for (t, (lo, hi)) in self.tris.iter().zip(&self.boxes) {
            if p.x < lo.x - tol || p.x > hi.x + tol || p.y < lo.y - tol || p.y > hi.y + tol || p.z < lo.z - tol || p.z > hi.z + tol {
                continue;
            }
            if point_triangle_distance(p, t[0], t[1], t[2]) <= tol {
                return true;
            }
        }
        false
    }

    /// The distance from `p` to the nearest boundary edge of the face.
    fn clearance(&self, p: Vec3) -> f64 {
        let mut best = f64::INFINITY;
        for poly in &self.boundary {
            for w in poly.windows(2) {
                best = best.min(point_segment_distance(p, w[0], w[1]));
            }
        }
        best
    }
}

/// Distance from `p` to the segment `ab`.
fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

// ----------------------------------------------------------------- overlap

/// Pairs of copies of a body whose box (in the body's own frame) is
/// `source` that overlap by more than `eps`: a sweep along x over the
/// copies' world boxes finds the candidates, and each candidate pair is
/// then tested as two oriented boxes (the source's box under each copy's
/// transform, by separating axes), so a slender cutter leaning across its
/// neighbours' world boxes — effusion holes round a cone — is not called
/// an overlap. The first pair found, in copy order, and how many there are.
pub fn overlapping_pairs(source: (Vec3, Vec3), transforms: &[Transform], eps: f64) -> (Option<(usize, usize)>, usize) {
    let boxes: Vec<(Vec3, Vec3)> = transforms.iter().map(|t| transformed_box(source, t)).collect();
    let obbs: Vec<Obb> = transforms.iter().map(|t| Obb::of(source, t)).collect();
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0.x.partial_cmp(&boxes[b].0.x).unwrap_or(std::cmp::Ordering::Equal));
    let mut first = None;
    let mut count = 0usize;
    for (k, &i) in order.iter().enumerate() {
        let (lo_i, hi_i) = boxes[i];
        for &j in &order[k + 1..] {
            let (lo_j, hi_j) = boxes[j];
            if lo_j.x > hi_i.x - eps {
                break;
            }
            let over = hi_i.x.min(hi_j.x) - lo_i.x.max(lo_j.x) > eps
                && hi_i.y.min(hi_j.y) - lo_i.y.max(lo_j.y) > eps
                && hi_i.z.min(hi_j.z) - lo_i.z.max(lo_j.z) > eps
                && obbs[i].overlaps(&obbs[j], eps);
            if over {
                count += 1;
                if first.is_none() {
                    first = Some((i.min(j), i.max(j)));
                }
            }
        }
    }
    (first, count)
}

/// A box under a rigid transform: its centre, its three axes and its
/// half-extents along them.
struct Obb {
    centre: Vec3,
    axes: [Vec3; 3],
    half: [f64; 3],
}

impl Obb {
    fn of(b: (Vec3, Vec3), t: &Transform) -> Obb {
        let (lo, hi) = b;
        Obb {
            centre: t.point((lo + hi) * 0.5),
            axes: [t.direction(Vec3::X), t.direction(Vec3::Y), t.direction(Vec3::Z)],
            half: [(hi.x - lo.x) * 0.5, (hi.y - lo.y) * 0.5, (hi.z - lo.z) * 0.5],
        }
    }

    /// Half the box's extent along the unit direction `l`.
    fn radius_along(&self, l: Vec3) -> f64 {
        (0..3).map(|k| self.half[k] * self.axes[k].dot(l).abs()).sum()
    }

    /// True when the two boxes overlap by more than `eps` along every one
    /// of the fifteen separating axes (each box's axes and their cross
    /// products); boxes that touch do not overlap.
    fn overlaps(&self, o: &Obb, eps: f64) -> bool {
        let d = o.centre - self.centre;
        let mut axes: Vec<Vec3> = Vec::with_capacity(15);
        axes.extend(self.axes);
        axes.extend(o.axes);
        for a in self.axes {
            for b in o.axes {
                let c = a.cross(b);
                if c.length() > 1e-9 {
                    axes.push(c.normalized());
                }
            }
        }
        axes.into_iter().all(|l| d.dot(l).abs() < self.radius_along(l) + o.radius_along(l) - eps)
    }
}

/// The box `b` under `t`: the box of its eight corners.
pub fn transformed_box(b: (Vec3, Vec3), t: &Transform) -> (Vec3, Vec3) {
    let (lo, hi) = b;
    let mut out: Option<(Vec3, Vec3)> = None;
    for k in 0..8 {
        let c = Vec3::new(
            if k & 1 == 0 { lo.x } else { hi.x },
            if k & 2 == 0 { lo.y } else { hi.y },
            if k & 4 == 0 { lo.z } else { hi.z },
        );
        let p = t.point(c);
        out = Some(match out {
            None => (p, p),
            Some((a, b)) => (a.min(p), b.max(p)),
        });
    }
    out.unwrap_or((lo, hi))
}

// ----------------------------------------------------------------- context

impl Context {
    /// `pattern_along`: copies of `body` along `spec.curve`, placed
    /// by [`along`]. Without `merge`, one feature of kind `pattern_along`
    /// whose outputs are `<feature>/copy[k]`, each a rigid copy of the
    /// source sharing its tessellation; with `merge`, the copies
    /// fused by [`Context::pattern`] (one compose). The source is
    /// consumed either way: its copies are where it now is.
    #[allow(clippy::too_many_arguments)]
    pub fn pattern_along(
        &mut self,
        body: BodyId,
        spec: &AlongSpec<'_>,
        merge: bool,
        kernel: KernelChoice,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let layout = along(spec)?;
        args.insert("curve".into(), spec.curve.describe());
        args.insert("length".into(), fmt_num(layout.length));
        args.insert("frame".into(), spec.frame.name().to_owned());
        args.insert("align".into(), spec.align.to_string());
        if let Some(d) = spec.spacing {
            args.insert("spacing".into(), fmt_num(d));
        }
        if let Some(s) = spec.start {
            args.insert("start".into(), fmt_num(s));
        }
        if let Some(e) = spec.end {
            args.insert("end".into(), fmt_num(e));
        }
        if layout.closed {
            args.insert("closed".into(), "true".into());
        }
        let names: Vec<String> = (0..layout.transforms.len()).map(|k| format!("copy[{k}]")).collect();
        self.pattern_place("pattern_along", body, &layout.transforms, &names, merge, kernel, args, name, span)
    }

    /// `pattern_on_surface`: copies of `body` on the faces
    /// `face_names` of `on` (one surface, in one or more pieces), placed by
    /// [`on_surface`]; outputs `<feature>/row[i].col[j]`.
    #[allow(clippy::too_many_arguments)]
    pub fn pattern_on_surface(
        &mut self,
        body: BodyId,
        on: BodyId,
        face_names: &[String],
        spec: &SurfaceSpec,
        merge: bool,
        kernel: KernelChoice,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let chord = {
            let doc = self.tolerance;
            match &self.body(on)?.quality {
                Some(q) => q.chord.as_ref().map(|c| c.max).unwrap_or(q.tessellated_at).max(doc),
                None => doc,
            }
        };
        let face_name = face_names.join(", ");
        let layout = {
            let (mesh, topo) = self.body_mesh(on)?;
            let mut faces = Vec::with_capacity(face_names.len());
            for name in face_names {
                let face = topo.faces.iter().find(|f| &f.name == name || f.aliases.iter().any(|a| a == name)).ok_or_else(|| {
                    CadpoleError::invalid(format!("pattern_on_surface: face {name:?} is no longer on its body"))
                })?;
                faces.push(face);
            }
            on_surface(mesh, topo, &faces, chord, spec)?
        };
        args.insert("face".into(), face_name.clone());
        args.insert("surface".into(), layout.chart.to_owned());
        args.insert("rows".into(), layout.rows.to_string());
        args.insert("per_row".into(), summarise(&layout.per_row));
        args.insert("row_pitch".into(), summarise_mm(&layout.row_pitch));
        args.insert("across_pitch".into(), fmt_num(layout.across_pitch));
        args.insert("normal".into(), spec.normal.to_string());
        if spec.stagger {
            args.insert("stagger".into(), "true".into());
        }
        if spec.margin > 0.0 {
            args.insert("margin".into(), fmt_num(spec.margin));
        }
        if layout.skipped > 0 {
            args.insert("skipped".into(), layout.skipped.to_string());
        }
        if layout.closed_rows {
            args.insert("closed_rows".into(), "true".into());
        }
        let names: Vec<String> = layout.cells.iter().map(|(i, j)| format!("row[{i}].col[{j}]")).collect();
        let skipped = layout.skipped;
        let ids = self.pattern_place("pattern_on_surface", body, &layout.transforms, &names, merge, kernel, args, name, span.clone())?;
        if skipped > 0 {
            let fid = self.body(ids[0])?.feature.clone();
            self.info(
                format!(
                    "pattern_on_surface: {skipped} of {} copies fell outside face {face_name} (its trimmed region, \
                     or within the margin of its boundary) and were left out",
                    skipped + layout.transforms.len()
                ),
                &fid,
                span,
            );
        }
        Ok(ids)
    }

    /// Place the copies of a pattern that a curve or a surface laid out:
    /// as `count` rigid instances under one feature (one feature
    /// with N outputs, sharing one tessellation), or fused through
    /// [`Context::pattern`]. Copies whose boxes overlap are a warning
    /// naming the first pair.
    #[allow(clippy::too_many_arguments)]
    fn pattern_place(
        &mut self,
        kind: &str,
        body: BodyId,
        transforms: &[Transform],
        cell_names: &[String],
        merge: bool,
        kernel: KernelChoice,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        if transforms.is_empty() {
            return Err(CadpoleError::invalid(format!("{kind}: the layout has no copies")));
        }
        // The overlap check, on the source's box: the display mesh the
        // copies share anyway, so this is no extra tessellation.
        let overlap = {
            let bbox = self.body_mesh(body)?.0.bbox();
            bbox.map(|b| overlapping_pairs(b, transforms, self.tolerance))
        };
        let ids = if merge {
            self.pattern(kind, body, transforms, true, kernel, args, name, span.clone())?
        } else {
            args.insert("count".into(), transforms.len().to_string());
            if kernel != KernelChoice::Auto {
                args.insert("kernel".into(), kernel.name().to_owned());
            }
            let (material, inputs, src_key, layer) = {
                let src = self.body(body)?;
                (src.material.clone(), vec![src.feature.clone()], src.key, src.layer.clone())
            };
            let fid = self.new_feature(kind, name, inputs, args, span.clone())?;
            self.consume_body(body)?;
            let mut out = Vec::with_capacity(transforms.len());
            for (t, cell) in transforms.iter().zip(cell_names) {
                // A copy's identity is the source's and the matrix, as
                // `copy` and an instance are keyed: no kernel op.
                let key = {
                    let mut k = KeyBuilder::new("transform");
                    k.body(&src_key).put(t);
                    k.finish().body_key(0)
                };
                let moved = self.body(body)?.body.transformed(t);
                let id = self.add_body(&fid, moved, Some(format!("{}/{cell}", fid.0)))?;
                let b = self.body_mut(id)?;
                b.material = material.clone();
                b.layer = layer.clone();
                b.derived_from = Some((body, *t));
                b.key = key;
                out.push(id);
            }
            out
        };
        if let Some((Some((i, j)), n)) = overlap {
            let fid = self.body(ids[0])?.feature.clone();
            let (ci, cj) = (&cell_names[i], &cell_names[j]);
            self.warn(
                format!(
                    "{kind}: copies {ci} and {cj} overlap (their bounding boxes intersect){}: the pitch is smaller \
                     than the body{}",
                    if n > 1 { format!(", and {} more pairs do", n - 1) } else { String::new() },
                    if merge { "; the fuse joins them" } else { "" }
                ),
                &fid,
                span,
            );
        }
        Ok(ids)
    }
}

/// `60 × 10` for ten equal rows, else the counts as a list.
fn summarise(counts: &[usize]) -> String {
    if counts.iter().all(|c| Some(c) == counts.first()) {
        format!("{} × {}", counts.first().copied().unwrap_or(0), counts.len())
    } else {
        counts.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
    }
}

/// One number when every row's pitch is the same to a micron, else the
/// range.
fn summarise_mm(pitches: &[f64]) -> String {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &p in pitches {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if !lo.is_finite() {
        return "0".into();
    }
    if hi - lo < 1e-3 {
        fmt_num(lo)
    } else {
        format!("{}..{}", fmt_num(lo), fmt_num(hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: Vec3, b: Vec3) -> Curve {
        Curve::new(a).line_to(b).unwrap()
    }

    #[test]
    fn stations_along_a_line_are_at_equal_arc_length_and_z_follows_the_tangent() {
        let c = line(Vec3::ZERO, Vec3::new(0.0, 0.0, 30.0));
        let spec = AlongSpec {
            curve: &c,
            count: Some(4),
            spacing: None,
            frame: SweepFrame::Rmf,
            up: None,
            start: None,
            end: None,
            align: true,
            max_count: 10_000,
        };
        let l = along(&spec).unwrap();
        assert_eq!(l.stations, vec![0.0, 10.0, 20.0, 30.0]);
        for (k, t) in l.transforms.iter().enumerate() {
            let expect = Transform::translation(Vec3::new(0.0, 0.0, 10.0 * k as f64));
            assert_eq!(t.0, expect.0, "copy {k} is the translation to the bit");
        }
        // Along +X the body's z turns onto the tangent.
        let c = line(Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        let l = along(&AlongSpec { curve: &c, ..spec }).unwrap();
        for t in &l.transforms {
            assert!((t.direction(Vec3::Z) - Vec3::X).length() < 1e-12);
        }
        // Translation only keeps the body's axes.
        let l = along(&AlongSpec { curve: &c, align: false, ..spec }).unwrap();
        for t in &l.transforms {
            assert!((t.direction(Vec3::Z) - Vec3::Z).length() < 1e-12);
        }
    }

    #[test]
    fn spacing_and_count_are_checked_against_the_length() {
        let c = line(Vec3::ZERO, Vec3::new(0.0, 0.0, 30.0));
        let base = AlongSpec {
            curve: &c,
            count: None,
            spacing: None,
            frame: SweepFrame::Rmf,
            up: None,
            start: None,
            end: None,
            align: true,
            max_count: 10_000,
        };
        let l = along(&AlongSpec { spacing: Some(7.0), ..base }).unwrap();
        assert_eq!(l.stations, vec![0.0, 7.0, 14.0, 21.0, 28.0]);
        let e = along(&AlongSpec { count: Some(5), spacing: Some(10.0), ..base }).unwrap_err().to_string();
        assert!(e.contains("5 copies 10 mm apart span 40 mm, and the curve is 30 mm long"), "{e}");
        let e = along(&AlongSpec { ..base }).unwrap_err().to_string();
        assert!(e.contains("give count="), "{e}");
        let e = along(&AlongSpec { count: Some(3), start: Some(20.0), end: Some(10.0), ..base }).unwrap_err().to_string();
        assert!(e.contains("start=20 must come before end=10"), "{e}");
        let l = along(&AlongSpec { count: Some(3), start: Some(10.0), end: Some(20.0), ..base }).unwrap();
        assert_eq!(l.stations, vec![10.0, 15.0, 20.0]);
        let e = along(&AlongSpec { count: Some(20_000), ..base }).unwrap_err().to_string();
        assert!(e.contains("limit of 10000"), "{e}");
    }

    #[test]
    fn a_closed_curve_divides_its_length_like_a_full_turn() {
        // A square, closed: the copies go round its corners.
        let c = Curve::new(Vec3::ZERO)
            .line_to(Vec3::new(10.0, 0.0, 0.0))
            .unwrap()
            .line_to(Vec3::new(10.0, 10.0, 0.0))
            .unwrap()
            .line_to(Vec3::new(0.0, 10.0, 0.0))
            .unwrap()
            .close()
            .unwrap();
        let spec = AlongSpec {
            curve: &c,
            count: Some(8),
            spacing: None,
            frame: SweepFrame::Rmf,
            up: None,
            start: None,
            end: None,
            align: true,
            max_count: 10_000,
        };
        let l = along(&spec).unwrap();
        assert!(l.closed);
        assert_eq!(l.stations, (0..8).map(|k| 5.0 * k as f64).collect::<Vec<_>>());
        // z along each side's direction, and a proper rotation at every
        // station (no corner degenerates the frame).
        let dirs = [Vec3::X, Vec3::Y, -Vec3::X, -Vec3::Y];
        for (k, t) in l.transforms.iter().enumerate() {
            let want = dirs[k / 2];
            assert!((t.direction(Vec3::Z) - want).length() < 1e-12, "station {k}");
            let (x, y, z) = (t.direction(Vec3::X), t.direction(Vec3::Y), t.direction(Vec3::Z));
            assert!((x.cross(y) - z).length() < 1e-9, "right-handed at {k}");
        }
    }

    #[test]
    fn share_gives_whole_numbers_that_add_up() {
        assert_eq!(share(600, &[1.0; 10]), vec![60; 10]);
        let w: Vec<f64> = (0..10).map(|i| 10.0 + i as f64).collect();
        let s = share(600, &w);
        assert_eq!(s.iter().sum::<usize>(), 600);
        assert!(s.windows(2).all(|p| p[0] <= p[1]), "{s:?}");
        // Equal pitch to within one part in the smallest count.
        let pitches: Vec<f64> = w.iter().zip(&s).map(|(l, n)| l / *n as f64).collect();
        let (lo, hi) = pitches.iter().fold((f64::INFINITY, 0.0f64), |(a, b), &p| (a.min(p), b.max(p)));
        assert!((hi - lo) / lo < 1.0 / 40.0, "{pitches:?}");
        assert_eq!(share(5, &[0.0, 0.0]), vec![3, 2]);
        assert_eq!(share(3, &[1.0, 1.0, 1.0, 1.0]), vec![1, 1, 1, 0]);
    }

    #[test]
    fn overlaps_are_found_by_sweep_and_judged_as_oriented_boxes() {
        let unit = (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        let at = |x: f64| Transform::translation(Vec3::new(x, 0.0, 0.0));
        assert_eq!(overlapping_pairs(unit, &[at(0.0), at(2.0), at(2.5), at(10.0)], 1e-6), (Some((1, 2)), 1));
        // Touching is not overlapping.
        assert_eq!(overlapping_pairs(unit, &[at(0.0), at(1.0), at(2.0)], 1e-6), (None, 0));
        // Two slender cutters 2.6 mm apart, both leaning 45° in the plane
        // across the line between them: their world boxes overlap (each is
        // 3.2 mm wide), the bodies do not.
        let slender = (Vec3::new(-0.3, -0.3, -2.0), Vec3::new(0.3, 0.3, 2.0));
        let lean = Vec3::new(1.0, 1.0, 0.0).normalized();
        let side = Vec3::new(-1.0, 1.0, 0.0).normalized();
        let leaning = |origin: Vec3| Frame { origin, z: lean, x: Vec3::Z }.to_transform();
        let pair = [leaning(Vec3::ZERO), leaning(side * 2.6)];
        let (lo, hi) = transformed_box(slender, &pair[0]);
        assert!(hi.x - lo.x > 3.0, "the world box is the leaning cutter's: {lo:?}..{hi:?}");
        assert_eq!(overlapping_pairs(slender, &pair, 1e-6), (None, 0));
        // Brought within a diameter they do overlap.
        let pair = [leaning(Vec3::ZERO), leaning(side * 0.5)];
        assert_eq!(overlapping_pairs(slender, &pair, 1e-6), (Some((0, 1)), 1));
    }

    #[test]
    fn an_arc_table_inverts_its_own_lengths() {
        let t = ArcTable { params: vec![0.0, 1.0, 2.0], cum: vec![0.0, 3.0, 4.0] };
        assert_eq!(t.length(), 4.0);
        assert!((t.param_at(1.5) - 0.5).abs() < 1e-12);
        assert!((t.param_at(3.5) - 1.5).abs() < 1e-12);
        assert_eq!(t.param_at(-1.0), 0.0);
        assert_eq!(t.param_at(9.0), 2.0);
    }
}
