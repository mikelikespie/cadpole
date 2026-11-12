//! Which way up each piece prints: a cost in mm² of support equivalent.
//!
//! The candidates are `orient_for_print`'s -- as modelled, upside down, the
//! four sides, the outward normals of the eight largest flat regions -- and
//! each is scored by an **absolute** cost, so the number means the same on
//! every part and the report can quote it:
//!
//! ```text
//! cost = support_area                          what the support rule still needs supported
//!      + ISLAND per island + ISLAND_DEPTH per mm the deepest hangs
//!      + CANTILEVER per mm a ledge sticks out past max_cantilever
//!      + WIDE_HOLE per wide round horizontal hole
//!      + TIPPY when height over √(first layer) passes max_height_to_base
//!      + ACROSS_LAYERS when the along= axis is more than 30° out of the bed plane
//!      + HEIGHT per mm of height               (print time: a tiebreak)
//!      + TURN for any turn at all              (a turn nobody needed is a cost)
//!      + MISFIT when it does not fit
//! ```
//!
//! The full support rules cost a weld and a sweep per candidate, so every
//! candidate is first measured by its raw overhang area and only the best
//! [`FULL`] (and "as modelled", the baseline the report compares with) get
//! the full rules.

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::{Mesh, Topology};

use super::{Options, Piece, PieceKind, Pin};
use crate::{Adjacency, num, orient, overhang, scale_of, support};

pub const ISLAND: f64 = 60.0;
pub const ISLAND_DEPTH: f64 = 20.0;
pub const CANTILEVER: f64 = 40.0;
pub const WIDE_HOLE: f64 = 50.0;
pub const TIPPY: f64 = 150.0;
pub const ACROSS_LAYERS: f64 = 400.0;
pub const HEIGHT: f64 = 0.05;
pub const TURN: f64 = 0.5;
pub const MISFIT: f64 = 1e6;
/// An `along=` axis further than this out of the bed plane, in degrees,
/// runs across the layers.
pub const ALONG_DEGREES: f64 = 30.0;
/// Candidates that get the full support rules.
pub const FULL: usize = 4;
/// Two orientations whose costs differ by less than this many mm², or this
/// share of the better one, are the same print.
pub const SAME_MM2: f64 = 2.0;
pub const SAME_SHARE: f64 = 0.02;

pub const AS_MODELLED: &str = "as modelled";

/// One orientation, measured.
#[derive(Clone, Debug)]
pub struct Scored {
    /// The model direction that faces the bed.
    pub down: Vec3,
    pub label: String,
    /// Rotation, then the drop onto z = 0.
    pub transform: Transform,
    /// Raw overhang area past the angle, off the bed, in mm².
    pub raw_overhang: f64,
    /// Whether the full support rules were run; the fields below are only
    /// meaningful then.
    pub full: bool,
    pub support_area: f64,
    pub islands: usize,
    pub deepest_island: f64,
    pub longest_cantilever: f64,
    pub wide_holes: usize,
    pub height_to_base: f64,
    pub height: f64,
    pub bed_contact: f64,
    /// How far the `along=` axis is out of the bed plane, in degrees.
    pub along_degrees: Option<f64>,
    pub fits: bool,
    pub cost: f64,
    pub pinned: bool,
}

/// Score the candidates for one piece. Returns them best first, with the
/// index of the chosen one, of "as modelled", and of the alternatives the
/// packer may use instead of the chosen.
pub fn orient(
    mesh: &Mesh,
    topo: Option<&Topology>,
    pin: &Pin,
    fixed: bool,
    opts: &Options,
) -> (Vec<Scored>, usize, Option<usize>, Vec<usize>) {
    let profile = &opts.profile;
    let up = profile.up();
    let mut dirs: Vec<(Vec3, String)> = Vec::new();
    orient::push_unique(&mut dirs, -up, AS_MODELLED.to_owned());
    let mut pinned_dir = None;
    if let Some(d) = pin.down {
        if d.normalized().dot(-up) > 0.999_999 {
            pinned_dir = Some(0);
        } else {
            dirs.push((d.normalized(), format!("pinned down= {}", vec_words(d))));
            pinned_dir = Some(dirs.len() - 1);
        }
    } else if !(pin.keep || fixed) {
        for (n, area) in orient::flat_regions(mesh) {
            orient::push_unique(&mut dirs, n, format!("on the {} mm² flat face", num(area)));
        }
        for (d, name) in [
            (up, "upside down"),
            (Vec3::X, "on its +x side"),
            (-Vec3::X, "on its -x side"),
            (Vec3::Y, "on its +y side"),
            (-Vec3::Y, "on its -y side"),
        ] {
            orient::push_unique(&mut dirs, d, name.to_owned());
        }
    }
    let bed_eps = (scale_of(mesh) * 1e-4).max(1e-6);
    let region = [opts.region[2] - opts.region[0], opts.region[3] - opts.region[1]];
    let mut out: Vec<Scored> = dirs
        .iter()
        .enumerate()
        .map(|(i, (d, label))| {
            let r = orient::rotation_putting_down(mesh, *d, up);
            let m = mesh.transformed(&r);
            let floor = overhang::lowest(&m, up);
            let transform = r.then(Transform::translation(up * -floor));
            let (raw, contact) = overhang::unsupported_area(&m, up, profile.max_overhang_degrees, bed_eps);
            let (_, height) = orient::footprint_and_height(&m, up);
            let (_, w, dd) = super::pack::min_rect(&super::pack::plan_points(&m));
            let fits_plan = (w <= region[0] && dd <= region[1]) || (dd <= region[0] && w <= region[1]);
            let fits = fits_plan && height <= profile.bed[2] + 1e-6;
            let along_degrees = pin.along.map(|a| {
                let a = r.direction(a);
                a.dot(up).abs().clamp(0.0, 1.0).asin().to_degrees()
            });
            Scored {
                down: *d,
                label: label.clone(),
                transform,
                raw_overhang: raw,
                full: false,
                support_area: raw,
                islands: 0,
                deepest_island: 0.0,
                longest_cantilever: 0.0,
                wide_holes: 0,
                height_to_base: 0.0,
                height,
                bed_contact: contact,
                along_degrees,
                fits,
                cost: f64::INFINITY,
                pinned: Some(i) == pinned_dir,
            }
        })
        .collect();

    // The full rules on the best few by raw overhang, "as modelled" and the
    // pinned direction.
    let mut order: Vec<usize> = (0..out.len()).collect();
    let rough = |s: &Scored| s.raw_overhang + if s.fits { 0.0 } else { MISFIT } + along_cost(s);
    order.sort_by(|&a, &b| rough(&out[a]).total_cmp(&rough(&out[b])).then(a.cmp(&b)));
    let mut full: Vec<usize> = order.iter().copied().take(FULL).collect();
    for i in [Some(0), pinned_dir].into_iter().flatten() {
        if !full.contains(&i) {
            full.push(i);
        }
    }
    full.sort_unstable();
    for &i in &full {
        measure(mesh, topo, &mut out[i], opts);
        out[i].cost = cost(&out[i], i == 0, opts);
    }

    let chosen = match pinned_dir {
        Some(p) => p,
        None => {
            let mut best = 0usize;
            for &i in &full {
                if out[i].cost < out[best].cost - 1e-9 {
                    best = i;
                }
            }
            best
        }
    };
    let alternatives: Vec<usize> = if pinned_dir.is_some() || pin.keep || fixed {
        Vec::new()
    } else {
        let c = &out[chosen];
        let slack = SAME_MM2.max(SAME_SHARE * c.cost.abs());
        full.iter()
            .copied()
            .filter(|&i| {
                i != chosen
                    && out[i].fits
                    && out[i].islands <= c.islands
                    && out[i].support_area <= c.support_area + SAME_MM2
                    && out[i].cost <= c.cost + slack
            })
            .collect()
    };

    // Best first: the fully measured by cost, then the rest by raw overhang.
    let mut idx: Vec<usize> = (0..out.len()).collect();
    idx.sort_by(|&a, &b| {
        let (sa, sb) = (&out[a], &out[b]);
        (!sa.full).cmp(&!sb.full).then(if sa.full { sa.cost.total_cmp(&sb.cost) } else { rough(sa).total_cmp(&rough(sb)) }).then(a.cmp(&b))
    });
    let pos = |i: usize| idx.iter().position(|&k| k == i).expect("every index is in the order");
    let chosen_pos = pos(chosen);
    let baseline_pos = Some(pos(0));
    let alt_pos: Vec<usize> = {
        let mut v: Vec<usize> = alternatives.iter().map(|&i| pos(i)).collect();
        v.sort_unstable();
        v
    };
    let sorted: Vec<Scored> = idx.into_iter().map(|i| out[i].clone()).collect();
    (sorted, chosen_pos, baseline_pos, alt_pos)
}

fn along_cost(s: &Scored) -> f64 {
    match s.along_degrees {
        Some(d) if d > ALONG_DEGREES => ACROSS_LAYERS,
        _ => 0.0,
    }
}

/// The full support rules on one candidate.
fn measure(mesh: &Mesh, topo: Option<&Topology>, s: &mut Scored, opts: &Options) {
    let profile = &opts.profile;
    let m = mesh.transformed(&s.transform);
    let topo_t = topo.map(|t| t.transformed(&s.transform));
    let adj = Adjacency::new(&m);
    let mut issues = Vec::new();
    let (holes, skip) = support::horizontal_holes(&m, topo_t.as_ref(), profile, &mut issues);
    let ov = overhang::check_support(&m, &adj, topo_t.as_ref(), profile, None, opts.max_bridge, &skip, &mut issues);
    let sup = support::check(&m, &adj, topo_t.as_ref(), profile, holes, &mut issues);
    s.full = true;
    s.support_area = ov.support_area;
    s.longest_cantilever = ov.longest_cantilever;
    s.bed_contact = ov.bed_contact_area;
    s.islands = sup.islands;
    s.deepest_island = sup.deepest_island;
    s.wide_holes = sup.wide_holes;
    s.height_to_base = sup.height_to_base;
}

/// [`Scored::cost`]: see the module comment.
pub fn cost(s: &Scored, as_modelled: bool, opts: &Options) -> f64 {
    let rules = &opts.profile.support;
    let mut c = s.support_area;
    c += ISLAND * s.islands as f64 + ISLAND_DEPTH * s.deepest_island;
    if s.longest_cantilever > rules.max_cantilever {
        c += CANTILEVER * (s.longest_cantilever - rules.max_cantilever);
    }
    c += WIDE_HOLE * s.wide_holes as f64;
    if rules.max_height_to_base > 0.0 && s.height_to_base > rules.max_height_to_base {
        c += TIPPY;
    }
    c += along_cost(s);
    c += HEIGHT * s.height;
    if !as_modelled {
        c += TURN;
    }
    if !s.fits {
        c += MISFIT;
    }
    c
}

fn vec_words(v: Vec3) -> String {
    format!("({}, {}, {})", num(v.x), num(v.y), num(v.z))
}

fn support_words(s: &Scored) -> String {
    let mut w = format!("{} mm² support", num(s.support_area));
    if s.islands > 0 {
        w.push_str(&format!(", {} island{}", s.islands, if s.islands == 1 { "" } else { "s" }));
    }
    if s.wide_holes > 0 {
        w.push_str(&format!(", {} wide round hole{}", s.wide_holes, if s.wide_holes == 1 { "" } else { "s" }));
    }
    w
}

/// The sentence about a piece's orientation.
pub fn explain(p: &Piece, opts: &Options) -> String {
    let c = p.best();
    let base = p.baseline.map(|b| &p.candidates[b]);
    let mut s = String::new();
    match p.kind {
        PieceKind::PrintInPlace => s.push_str("print-in-place, "),
        PieceKind::Fused => s.push_str("one multi-material piece, "),
        PieceKind::Declared => {}
        PieceKind::Single => {}
    }
    if c.pinned {
        s.push_str(&format!("{} as pinned; {}", c.label, support_words(c)));
    } else if p.kind == PieceKind::PrintInPlace && p.pin.down.is_none() {
        s.push_str(&format!("kept as modelled so its gaps stay on the layer grid; {}", support_words(c)));
    } else if p.pin.keep {
        s.push_str(&format!("kept as modelled (orient=\"keep\"); {}", support_words(c)));
    } else if c.label == AS_MODELLED {
        let n = p.candidates.iter().filter(|k| k.full).count();
        s.push_str(&format!("as modelled; {}, the best of {} orientations scored", support_words(c), n.max(1)));
    } else {
        s.push_str(&format!("{}: {}", c.label, support_words(c)));
        if let Some(b) = base {
            s.push_str(&format!(" vs {} as modelled", support_words(b)));
            let mut why = Vec::new();
            if !b.fits && c.fits {
                why.push("as modelled it does not fit".to_owned());
            }
            if along_cost(b) > 0.0 && along_cost(c) == 0.0 {
                why.push(format!(
                    "its along= axis now runs along the layers ({}° out of the bed plane, was {}°)",
                    num(c.along_degrees.unwrap_or(0.0)),
                    num(b.along_degrees.unwrap_or(0.0))
                ));
            }
            if b.support_area <= c.support_area + SAME_MM2 && b.islands <= c.islands && why.is_empty() {
                if b.height > c.height + 1e-6 {
                    why.push(format!("{} mm tall instead of {}", num(c.height), num(b.height)));
                }
                if rules_tippy(b, opts) && !rules_tippy(c, opts) {
                    why.push("it stands steady on the bed".to_owned());
                }
            }
            if !why.is_empty() {
                s.push_str(&format!(" ({})", why.join("; ")));
            }
        }
    }
    if p.pin.along.is_some() && along_cost(c) > 0.0 {
        s.push_str("; its along= axis still runs across the layers in every orientation scored");
    }
    if !c.fits {
        s.push_str(&format!("; it does not fit the {} in any orientation scored", opts.printer));
    }
    if !p.joints.is_empty() && p.kind == PieceKind::PrintInPlace {
        let gaps: Vec<f64> =
            p.joints.iter().filter_map(|j| if let super::JointKind::Captive(g) = j.kind { Some(g) } else { None }).collect();
        let min = gaps.iter().copied().fold(f64::INFINITY, f64::min);
        s.push_str(&format!(
            "; {} captive joint{}, the tightest {} mm",
            gaps.len(),
            if gaps.len() == 1 { "" } else { "s" },
            num(min)
        ));
    }
    s
}

fn rules_tippy(s: &Scored, opts: &Options) -> bool {
    let r = &opts.profile.support;
    r.max_height_to_base > 0.0 && s.height_to_base > r.max_height_to_base
}
