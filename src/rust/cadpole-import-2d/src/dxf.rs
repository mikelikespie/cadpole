//! DXF into sketch loops, over the `dxf` crate (IxMilia, MIT).
//!
//! Two things make a real floor plan usable rather than merely readable:
//!
//! * **bulges stay arcs.** A `LWPOLYLINE` vertex's bulge is `tan(theta/4)` of
//!   the included angle, and Cadpole turns it into a [`Segment::Arc`] rather
//!   than a polyline, so a rounded corner extrudes to a real fillet and a
//!   `CIRCLE` to a cylinder.
//! * **joining.** Architectural DXFs are bags of unconnected `LINE`s that
//!   bound nothing. `join` chains endpoints within tolerance into loops, the
//!   explicit form of FreeCAD's "join geometry"; chains that do not close
//!   become construction paths and are reported rather than silently dropped.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use dxf::entities::EntityType;
use dxf::enums::Units;
use cadpole_core::sketch::{Loop, P2, Path, Segment};

use crate::curve;
use crate::{Import, Note};

/// Parser caps.
const MAX_ENTITIES: usize = 500_000;
const MAX_INSERT_DEPTH: usize = 8;

/// How `import_dxf` reads the file.
#[derive(Clone, Debug)]
pub struct Options {
    /// Layer names to import, case-insensitive, `*` and `?` allowed
    /// (`"A-WALL*"`). Empty means every layer.
    pub layers: Vec<String>,
    /// Block *definitions* to import at the origin, instead of the drawing's
    /// entities. This is how a furniture symbol comes in on its own.
    pub blocks: Vec<String>,
    /// `"auto"` reads `$INSUNITS`; a unit name overrides it.
    pub units: String,
    /// Multiplied on after units are resolved.
    pub scale: f64,
    /// Chain loose segments into loops within `tolerance`.
    pub join: bool,
    /// Join tolerance and flattening tolerance, in millimetres.
    pub tolerance: f64,
    /// What to do about chains that do not close: `"construction"` keeps them
    /// quietly, `"warn"` keeps them and says so, `"error"` fails the import.
    pub open: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            layers: Vec::new(),
            blocks: Vec::new(),
            units: "auto".into(),
            scale: 1.0,
            join: true,
            tolerance: 0.01,
            open: "construction".into(),
        }
    }
}

/// Insertion transform of a block reference: uniform-or-not scale, rotation
/// and translation, composed as INSERTs nest.
#[derive(Clone, Copy, Debug)]
struct Placement {
    sx: f64,
    sy: f64,
    cos: f64,
    sin: f64,
    tx: f64,
    ty: f64,
}

impl Placement {
    const ID: Placement = Placement { sx: 1.0, sy: 1.0, cos: 1.0, sin: 0.0, tx: 0.0, ty: 0.0 };

    fn apply(&self, p: P2) -> P2 {
        let (x, y) = (p.x * self.sx, p.y * self.sy);
        P2::new(self.cos * x - self.sin * y + self.tx, self.sin * x + self.cos * y + self.ty)
    }

    /// `self` inside `outer`.
    fn then(&self, outer: &Placement) -> Placement {
        // Compose by mapping the basis: exact for the rotate-scale-translate
        // family DXF INSERTs use.
        let o = outer;
        let origin = o.apply(P2::new(self.tx, self.ty));
        let ex = o.apply(P2::new(self.tx + self.sx * self.cos, self.ty + self.sx * self.sin));
        let ey = o.apply(P2::new(self.tx - self.sy * self.sin, self.ty + self.sy * self.cos));
        let (ax, ay) = (ex.x - origin.x, ex.y - origin.y);
        let (bx, by) = (ey.x - origin.x, ey.y - origin.y);
        let sx = (ax * ax + ay * ay).sqrt();
        let sy = (bx * bx + by * by).sqrt();
        let (cos, sin) = if sx > 1e-12 { (ax / sx, ay / sx) } else { (1.0, 0.0) };
        // Keep the handedness of the y axis.
        let sy = if ax * by - ay * bx < 0.0 { -sy } else { sy };
        Placement { sx, sy, cos, sin, tx: origin.x, ty: origin.y }
    }

    fn uniform(&self) -> bool {
        (self.sx.abs() - self.sy.abs()).abs() <= 1e-9 * self.sx.abs().max(1.0)
    }
}

/// One entity's contribution, tagged with where it came from.
struct Piece {
    layer: String,
    name: String,
    segments: Vec<Segment>,
    closed: bool,
}

/// Read a DXF into loops and construction paths, in millimetres.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Import, String> {
    let mut cursor = Cursor::new(bytes);
    let drawing = dxf::Drawing::load(&mut cursor).map_err(|e| format!("DXF could not be parsed: {e}"))?;

    let (unit_scale, unit_note) = resolve_units(&drawing, opts);
    let k = unit_scale * opts.scale;
    // Tolerances are quoted in millimetres; entity geometry is still in the
    // file's own units at this point.
    let tol_file = (opts.tolerance / k.max(1e-12)).max(1e-12);

    let blocks: HashMap<String, &dxf::Block> =
        drawing.blocks().map(|b| (b.name.to_ascii_uppercase(), b)).collect();

    let mut notes: Vec<Note> = unit_note.into_iter().collect();
    let mut pieces = Vec::new();
    let mut counters: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut ignored: BTreeMap<String, usize> = BTreeMap::new();
    let mut budget = MAX_ENTITIES;

    if opts.blocks.is_empty() {
        let entities: Vec<&dxf::entities::Entity> = drawing.entities().collect();
        collect(
            &entities,
            Placement::ID,
            &blocks,
            opts,
            tol_file,
            None,
            &mut Vec::new(),
            0,
            &mut pieces,
            &mut counters,
            &mut ignored,
            &mut notes,
            &mut budget,
        );
    } else {
        let mut found = Vec::new();
        for want in &opts.blocks {
            let Some(b) = blocks.get(&want.to_ascii_uppercase()) else { continue };
            found.push(want.clone());
            let entities: Vec<&dxf::entities::Entity> = b.entities.iter().collect();
            collect(
                &entities,
                Placement::ID,
                &blocks,
                opts,
                tol_file,
                None,
                &mut vec![b.name.to_ascii_uppercase()],
                0,
                &mut pieces,
                &mut counters,
                &mut ignored,
                &mut notes,
                &mut budget,
            );
        }
        let missing: Vec<&String> = opts.blocks.iter().filter(|b| !found.contains(b)).collect();
        if !missing.is_empty() {
            let mut have: Vec<&str> = drawing.blocks().map(|b| b.name.as_str()).collect();
            have.sort_unstable();
            return Err(format!(
                "DXF has no block named {} (blocks in the file: {})",
                missing.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", "),
                if have.is_empty() { "none".to_owned() } else { have.join(", ") }
            ));
        }
    }

    if !opts.layers.is_empty() && pieces.is_empty() {
        let mut have: Vec<String> = drawing.layers().map(|l| l.name.clone()).collect();
        have.sort();
        have.dedup();
        return Err(format!(
            "no DXF entity is on a layer matching {} (layers in the file: {})",
            opts.layers.iter().map(|l| format!("{l:?}")).collect::<Vec<_>>().join(", "),
            if have.is_empty() { "none".to_owned() } else { have.join(", ") }
        ));
    }

    for (kind, n) in ignored {
        notes.push(Note::info(format!("DXF {kind} ignored ({n})")));
    }

    let mut import = assemble(pieces, opts, tol_file, &mut notes)?;
    import.notes.append(&mut notes);
    import.map_points(move |p| P2::new(p.x * k, p.y * k));
    Ok(import)
}

/// `$INSUNITS` as millimetres per drawing unit.
fn resolve_units(drawing: &dxf::Drawing, opts: &Options) -> (f64, Option<Note>) {
    if opts.units != "auto" {
        return match crate::unit_scale(&opts.units) {
            Some(k) => (k, None),
            None => (
                1.0,
                Some(Note::warn(format!(
                    "units {:?} is not one of {}; treating drawing units as mm",
                    opts.units,
                    crate::UNIT_NAMES.join(", ")
                ))),
            ),
        };
    }
    match drawing.header.default_drawing_units {
        Units::Unitless => (
            1.0,
            Some(Note::warn(
                "DXF has no $INSUNITS; drawing units are read as millimetres \
                 (pass units=\"in\" or units=\"ft\" if the file is imperial)"
                    .to_string(),
            )),
        ),
        Units::Inches => (25.4, None),
        Units::Feet => (304.8, None),
        Units::Millimeters => (1.0, None),
        Units::Centimeters => (10.0, None),
        Units::Meters => (1000.0, None),
        Units::Mils => (0.0254, None),
        Units::Microinches => (25.4e-6, None),
        Units::Yards => (914.4, None),
        Units::Microns => (0.001, None),
        Units::Decimeters => (100.0, None),
        other => (
            1.0,
            Some(Note::warn(format!("DXF $INSUNITS is {other:?}, which Cadpole does not convert; read as mm"))),
        ),
    }
}

/// Walk entities, expanding INSERTs, and turn each one into a [`Piece`].
#[allow(clippy::too_many_arguments)]
fn collect(
    entities: &[&dxf::entities::Entity],
    place: Placement,
    blocks: &HashMap<String, &dxf::Block>,
    opts: &Options,
    tol: f64,
    inherited_layer: Option<&str>,
    block_path: &mut Vec<String>,
    depth: usize,
    out: &mut Vec<Piece>,
    counters: &mut BTreeMap<(String, String), usize>,
    ignored: &mut BTreeMap<String, usize>,
    notes: &mut Vec<Note>,
    budget: &mut usize,
) {
    for e in entities {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        // Entities on layer "0" inside a block take the INSERT's layer, which
        // is how DXF block libraries are meant to be coloured.
        let layer = match (&e.common.layer, inherited_layer) {
            (l, Some(inh)) if l == "0" => inh.to_owned(),
            (l, _) => l.clone(),
        };

        if let EntityType::Insert(ins) = &e.specific {
            if depth >= MAX_INSERT_DEPTH {
                notes.push(Note::warn(format!(
                    "DXF INSERT nested deeper than {MAX_INSERT_DEPTH}; {:?} was not expanded",
                    ins.name
                )));
                continue;
            }
            let key = ins.name.to_ascii_uppercase();
            if block_path.contains(&key) {
                notes.push(Note::warn(format!("DXF block {:?} inserts itself; the cycle was cut", ins.name)));
                continue;
            }
            let Some(block) = blocks.get(&key) else {
                notes.push(Note::warn(format!("DXF INSERT names block {:?}, which the file does not define", ins.name)));
                continue;
            };
            let rot = ins.rotation.to_radians();
            let cols = ins.column_count.max(1) as usize;
            let rows = ins.row_count.max(1) as usize;
            let children: Vec<&dxf::entities::Entity> = block.entities.iter().collect();
            block_path.push(key);
            for cx in 0..cols {
                for ry in 0..rows {
                    let local = Placement {
                        sx: if ins.x_scale_factor == 0.0 { 1.0 } else { ins.x_scale_factor },
                        sy: if ins.y_scale_factor == 0.0 { 1.0 } else { ins.y_scale_factor },
                        cos: rot.cos(),
                        sin: rot.sin(),
                        tx: ins.location.x + cx as f64 * ins.column_spacing - block.base_point.x,
                        ty: ins.location.y + ry as f64 * ins.row_spacing - block.base_point.y,
                    };
                    collect(
                        &children,
                        local.then(&place),
                        blocks,
                        opts,
                        tol,
                        Some(&layer),
                        block_path,
                        depth + 1,
                        out,
                        counters,
                        ignored,
                        notes,
                        budget,
                    );
                }
            }
            block_path.pop();
            continue;
        }

        if !layer_wanted(&layer, opts) {
            continue;
        }

        let (etype, mut segments, closed) = match entity_geometry(&e.specific, tol) {
            Some(v) => v,
            None => {
                *ignored.entry(kind_name(&e.specific).to_owned()).or_insert(0) += 1;
                continue;
            }
        };
        if segments.is_empty() {
            continue;
        }
        if !place.uniform() {
            let mut flat = Vec::with_capacity(segments.len());
            for s in &segments {
                if let Segment::Arc { a, b, via } = s {
                    match cadpole_core::sketch::arc_params(*a, *b, *via) {
                        Some((c, r, t0, t1)) => flat.extend(curve::ellipse_cubics(c, r, r, 0.0, t0, t1 - t0, tol)),
                        None => flat.push(Segment::Line { a: *a, b: *b }),
                    }
                } else {
                    flat.push(s.clone());
                }
            }
            segments = flat;
        }
        for s in &mut segments {
            curve::map_segment(s, |p| place.apply(p));
        }
        let n = {
            let c = counters.entry((layer.clone(), etype.to_owned())).or_insert(0);
            let v = *c;
            *c += 1;
            v
        };
        let prefix = if block_path.is_empty() {
            String::new()
        } else {
            block_path.iter().map(|b| format!("{b}/")).collect::<String>()
        };
        // Handles survive a re-save in AutoCAD and ezdxf, so they are the
        // most stable name available; the index is the fallback.
        let local = if e.common.handle.0 != 0 {
            format!("h:{:X}", e.common.handle.0)
        } else {
            format!("{etype}[{n}]")
        };
        out.push(Piece { layer: layer.clone(), name: format!("{prefix}{local}"), segments, closed });
    }
}

fn layer_wanted(layer: &str, opts: &Options) -> bool {
    if opts.layers.is_empty() {
        return true;
    }
    opts.layers.iter().any(|pat| glob_match(&pat.to_ascii_lowercase(), &layer.to_ascii_lowercase()))
}

/// `*` and `?` globbing, case folded by the caller. One definition, shared
/// with the entity-name selector (`cadpole_core::select`).
use cadpole_core::select::glob_match;

fn kind_name(e: &EntityType) -> &'static str {
    match e {
        EntityType::Text(_) => "TEXT",
        EntityType::MText(_) => "MTEXT",
        EntityType::Attribute(_) | EntityType::AttributeDefinition(_) => "ATTRIB",
        EntityType::RotatedDimension(_)
        | EntityType::RadialDimension(_)
        | EntityType::DiameterDimension(_)
        | EntityType::AngularThreePointDimension(_)
        | EntityType::OrdinateDimension(_) => "DIMENSION",
        EntityType::Leader(_) => "LEADER",
        EntityType::ModelPoint(_) => "POINT",
        EntityType::Face3D(_) => "3DFACE",
        EntityType::Solid(_) => "SOLID",
        EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Body(_) => "3D solid",
        _ => "entity",
    }
}

/// Geometry of one entity in the file's own units: its kind name, its
/// segments and whether it closes on itself.
fn entity_geometry(e: &EntityType, tol: f64) -> Option<(&'static str, Vec<Segment>, bool)> {
    let p2 = |p: &dxf::Point| P2::new(p.x, p.y);
    match e {
        EntityType::Line(l) => {
            Some(("line", vec![Segment::Line { a: p2(&l.p1), b: p2(&l.p2) }], false))
        }
        EntityType::Arc(a) => {
            if a.radius <= 0.0 {
                return None;
            }
            let segs = curve::arc_from_center(
                p2(&a.center),
                a.radius,
                a.start_angle.to_radians(),
                a.end_angle.to_radians(),
            );
            Some(("arc", segs, false))
        }
        EntityType::Circle(c) => {
            if c.radius <= 0.0 {
                return None;
            }
            Some(("circle", curve::circle_loop(p2(&c.center), c.radius).segments, true))
        }
        EntityType::Ellipse(el) => {
            let (mx, my) = (el.major_axis.x, el.major_axis.y);
            let rx = (mx * mx + my * my).sqrt();
            let ry = rx * el.minor_axis_ratio;
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let rot = my.atan2(mx);
            let (t0, t1) = (el.start_parameter, el.end_parameter);
            let sweep = t1 - t0;
            let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-6;
            let c = p2(&el.center);
            let segs = if (rx - ry).abs() <= 1e-9 * rx && rot.abs() < 1e-12 {
                if full {
                    curve::circle_loop(c, rx).segments
                } else {
                    curve::arc_from_center(c, rx, t0, t1)
                }
            } else {
                curve::ellipse_cubics(c, rx, ry, rot, t0, sweep, tol)
            };
            Some(("ellipse", segs, full))
        }
        EntityType::LwPolyline(p) => {
            let closed = p.flags & 1 == 1;
            let pts: Vec<(P2, f64)> =
                p.vertices.iter().map(|v| (P2::new(v.x, v.y), v.bulge)).collect();
            Some(("lwpolyline", polyline_segments(&pts, closed), closed))
        }
        EntityType::Polyline(p) => {
            // 3D meshes and polyface meshes are not 2D geometry.
            if p.flags & 0b0111_0000 != 0 {
                return None;
            }
            let closed = p.flags & 1 == 1;
            let pts: Vec<(P2, f64)> = p
                .vertices()
                .map(|v| (P2::new(v.location.x, v.location.y), v.bulge))
                .collect();
            Some(("polyline", polyline_segments(&pts, closed), closed))
        }
        EntityType::Spline(s) => {
            let closed = s.flags & 1 == 1;
            let ctrl: Vec<P2> = s.control_points.iter().map(p2).collect();
            let segs = if ctrl.len() >= 2 {
                curve::bspline_segments(
                    s.degree_of_curve.max(1) as usize,
                    &ctrl,
                    &s.knot_values,
                    &s.weight_values,
                    tol,
                )
            } else if s.fit_points.len() >= 2 {
                // Fit-point-only splines: the same Catmull-Rom that
                // `Path.spline` uses, which is what most viewers draw.
                let pts: Vec<P2> = s.fit_points.iter().map(p2).collect();
                cadpole_core::sketch::catmull_rom(&pts, closed, 0.5)
            } else {
                Vec::new()
            };
            Some(("spline", segs, closed))
        }
        // Everything else — TEXT, MTEXT, DIMENSION, LEADER, POINT, 3D
        // entities, and HATCH, which the `dxf` crate does not model at all —
        // is counted and reported rather than guessed at.
        _ => None,
    }
}

fn polyline_segments(pts: &[(P2, f64)], closed: bool) -> Vec<Segment> {
    if pts.len() < 2 {
        return Vec::new();
    }
    let n = if closed { pts.len() } else { pts.len() - 1 };
    (0..n)
        .filter_map(|i| {
            let (a, bulge) = pts[i];
            let (b, _) = pts[(i + 1) % pts.len()];
            if (a.x - b.x).abs() < curve::EPS && (a.y - b.y).abs() < curve::EPS {
                return None;
            }
            Some(curve::bulge_segment(a, b, bulge))
        })
        .collect()
}

/// Group pieces by layer, keep the entities that already close, and join the
/// rest.
fn assemble(pieces: Vec<Piece>, opts: &Options, tol: f64, notes: &mut Vec<Note>) -> Result<Import, String> {
    let mut out = Import::default();
    let mut by_layer: BTreeMap<String, Vec<Piece>> = BTreeMap::new();
    for p in pieces {
        by_layer.entry(p.layer.clone()).or_default().push(p);
    }
    let mut open_chains = 0usize;
    for (layer, pieces) in by_layer {
        let mut loose = Vec::new();
        for p in pieces {
            if p.closed && p.segments.len() >= 2 {
                out.loops.push((format!("{layer}/{}", p.name), Loop { segments: p.segments }));
            } else if opts.join {
                loose.extend(p.segments);
            } else if !p.segments.is_empty() {
                let start = p.segments[0].start();
                open_chains += 1;
                out.paths.push((format!("{layer}/{}", p.name), Path { start, segments: p.segments }));
            }
        }
        if !loose.is_empty() {
            let (loops, paths) = curve::join(loose, tol);
            for (i, l) in loops.into_iter().enumerate() {
                out.loops.push((format!("{layer}/loop[{i}]"), l));
            }
            open_chains += paths.len();
            for (i, p) in paths.into_iter().enumerate() {
                out.paths.push((format!("{layer}/path[{i}]"), p));
            }
        }
    }
    if open_chains > 0 {
        let msg = format!(
            "{open_chains} chain{} did not close within tolerance {} mm and became construction \
             paths (they cannot be extruded; raise tolerance= or fix the drawing)",
            if open_chains == 1 { "" } else { "s" },
            opts.tolerance
        );
        match opts.open.as_str() {
            "error" => return Err(msg),
            "warn" => notes.push(Note::warn(msg)),
            _ => notes.push(Note::info(msg)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::sketch::arc_params;

    /// Minimal ASCII DXF: header with `$INSUNITS`, then an ENTITIES section.
    fn dxf_file(insunits: i16, entities: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{insunits}\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn run(src: &str, opts: Options) -> Import {
        import(src.as_bytes(), &opts).unwrap()
    }

    #[test]
    fn a_bulged_polyline_keeps_its_arc_radius() {
        // A 20 x 20 square whose top-right corner is a quarter-circle fillet
        // of radius 5: the bulge for a 90 degree arc is tan(22.5 deg).
        let b = (22.5f64).to_radians().tan();
        let e = format!(
            "0\nLWPOLYLINE\n8\nOUTLINE\n70\n1\n90\n5\n\
             10\n0\n20\n0\n\
             10\n20\n20\n0\n\
             10\n20\n20\n15\n42\n{b}\n\
             10\n15\n20\n20\n\
             10\n0\n20\n20\n"
        );
        let im = run(&dxf_file(4, &e), Options::default());
        assert_eq!(im.loops.len(), 1, "the closed flag makes it a loop on its own");
        let l = &im.loops[0].1;
        let arcs: Vec<&Segment> = l.segments.iter().filter(|s| matches!(s, Segment::Arc { .. })).collect();
        assert_eq!(arcs.len(), 1, "one bulge, one arc");
        let Segment::Arc { a, b, via } = arcs[0] else { unreachable!() };
        let (c, r, _, _) = arc_params(*a, *b, *via).unwrap();
        assert!((r - 5.0).abs() < 1e-9, "fillet radius {r}");
        assert!((c.x - 15.0).abs() < 1e-9 && (c.y - 15.0).abs() < 1e-9, "centre {c:?}");
        assert!(im.loops[0].0.starts_with("OUTLINE/"), "named by layer: {}", im.loops[0].0);
    }

    #[test]
    fn unconnected_lines_are_joined_into_a_loop() {
        // A square as four LINEs in a shuffled order, one of them reversed.
        let e = "0\nLINE\n8\nA-WALL\n10\n0\n20\n0\n11\n10\n21\n0\n\
                 0\nLINE\n8\nA-WALL\n10\n10\n20\n10\n11\n0\n21\n10\n\
                 0\nLINE\n8\nA-WALL\n10\n10\n20\n0\n11\n10\n21\n10\n\
                 0\nLINE\n8\nA-WALL\n10\n0\n20\n10\n11\n0\n21\n0\n";
        let im = run(&dxf_file(4, e), Options::default());
        assert_eq!(im.loops.len(), 1, "join geometry closes the square");
        assert_eq!(im.paths.len(), 0);
        assert!((im.loops[0].1.signed_area().abs() - 100.0).abs() < 1e-9);
        assert!(im.loops[0].1.is_closed(1e-9));

        // With join=False the same file is four open chains.
        let im = run(&dxf_file(4, e), Options { join: false, ..Default::default() });
        assert!(im.loops.is_empty());
        assert_eq!(im.paths.len(), 4);
        assert!(im.notes.iter().any(|n| n.message.contains("construction paths")));
    }

    #[test]
    fn a_chain_that_cannot_close_is_reported_not_dropped() {
        let e = "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
                 0\nLINE\n8\n0\n10\n10\n20\n0\n11\n10\n21\n10\n";
        let im = run(&dxf_file(4, e), Options::default());
        assert!(im.loops.is_empty());
        assert_eq!(im.paths.len(), 1, "the two lines chain but do not close");
        let err = import(dxf_file(4, e).as_bytes(), &Options { open: "error".into(), ..Default::default() })
            .unwrap_err();
        assert!(err.contains("did not close"), "{err}");
    }

    #[test]
    fn insunits_inches_scale_to_millimetres_and_units_overrides() {
        let e = "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n1\n";
        let im = run(&dxf_file(1, e), Options::default());
        let (lo, hi) = im.bbox().unwrap();
        assert!((hi.x - lo.x - 50.8).abs() < 0.01, "a 1 inch radius circle is 50.8 mm across: {}", hi.x - lo.x);
        // Unitless files warn and are read as mm.
        let im = run(&dxf_file(0, e), Options::default());
        let (lo, hi) = im.bbox().unwrap();
        assert!((hi.x - lo.x - 2.0).abs() < 0.01);
        assert!(im.notes.iter().any(|n| n.message.contains("$INSUNITS")));
        // An explicit unit wins over the header.
        let im = run(&dxf_file(1, e), Options { units: "mm".into(), ..Default::default() });
        let (lo, hi) = im.bbox().unwrap();
        assert!((hi.x - lo.x - 2.0).abs() < 0.01);
    }

    #[test]
    fn layers_are_selected_by_name_and_by_glob() {
        let e = "0\nCIRCLE\n8\nA-WALL\n10\n0\n20\n0\n40\n5\n\
                 0\nCIRCLE\n8\nA-DOOR\n10\n30\n20\n0\n40\n5\n\
                 0\nCIRCLE\n8\nNOTES\n10\n60\n20\n0\n40\n5\n";
        let im = run(&dxf_file(4, e), Options { layers: vec!["A-*".into()], ..Default::default() });
        assert_eq!(im.loops.len(), 2, "the glob takes both A- layers");
        let im = run(&dxf_file(4, e), Options { layers: vec!["notes".into()], ..Default::default() });
        assert_eq!(im.loops.len(), 1, "layer names are case-insensitive");
        let err = import(
            dxf_file(4, e).as_bytes(),
            &Options { layers: vec!["B-WALL".into()], ..Default::default() },
        )
        .unwrap_err();
        assert!(err.contains("A-WALL"), "the error lists the layers: {err}");
    }

    #[test]
    fn inserts_expand_with_their_transform_and_array() {
        let blocks = "0\nSECTION\n2\nBLOCKS\n\
             0\nBLOCK\n2\nTILE\n10\n0\n20\n0\n\
             0\nLWPOLYLINE\n8\n0\n70\n1\n90\n4\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n10\n\
             0\nENDBLK\n0\nENDSEC\n";
        let src = format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n{blocks}\
             0\nSECTION\n2\nENTITIES\n\
             0\nINSERT\n8\nFURNITURE\n2\nTILE\n10\n100\n20\n0\n41\n2\n42\n2\n70\n3\n44\n50\n\
             0\nENDSEC\n0\nEOF\n"
        );
        let im = run(&src, Options::default());
        assert_eq!(im.loops.len(), 3, "a 3-column array of the block");
        // The block's entities are on layer 0, so they take the INSERT's layer.
        assert!(im.loops[0].0.starts_with("FURNITURE/"), "{}", im.loops[0].0);
        assert!(im.loops[0].0.contains("TILE/"), "the block name is in the path: {}", im.loops[0].0);
        // scale 2 makes each 20 mm square.
        assert!((im.loops[0].1.signed_area().abs() - 400.0).abs() < 1e-6);
        let (lo, hi) = im.bbox().unwrap();
        assert!((lo.x - 100.0).abs() < 1e-6 && (hi.x - 220.0).abs() < 1e-6, "{lo:?} {hi:?}");

        // Importing the block definition instead puts it at the origin, unscaled.
        let im = run(&src, Options { blocks: vec!["TILE".into()], ..Default::default() });
        assert_eq!(im.loops.len(), 1);
        assert!((im.loops[0].1.signed_area().abs() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_self_inserting_block_does_not_recurse_forever() {
        let src = "0\nSECTION\n2\nBLOCKS\n\
             0\nBLOCK\n2\nLOOPY\n10\n0\n20\n0\n\
             0\nINSERT\n8\n0\n2\nLOOPY\n10\n1\n20\n1\n\
             0\nENDBLK\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n0\nINSERT\n8\n0\n2\nLOOPY\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
        let im = run(src, Options::default());
        assert!(im.notes.iter().any(|n| n.message.contains("cycle")), "{:?}", im.notes);
    }

    #[test]
    fn a_spline_becomes_beziers() {
        // A clamped cubic with one span: four control points, eight knots.
        let e = "0\nSPLINE\n8\n0\n70\n8\n71\n3\n\
                 40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
                 10\n0\n20\n0\n30\n0\n\
                 10\n10\n20\n20\n30\n0\n\
                 10\n30\n20\n20\n30\n0\n\
                 10\n40\n20\n0\n30\n0\n";
        let im = run(&dxf_file(4, e), Options::default());
        assert_eq!(im.paths.len(), 1, "an open spline is a construction path");
        assert!(im.paths[0].1.segments.iter().all(|s| matches!(s, Segment::Bezier { .. })));
        // Clamped: it starts and ends on the outer control points.
        let p = &im.paths[0].1;
        assert!((p.start.x).abs() < 1e-9 && (p.start.y).abs() < 1e-9, "{:?}", p.start);
        let end = p.segments.last().unwrap().end();
        assert!((end.x - 40.0).abs() < 1e-9 && end.y.abs() < 1e-9, "{end:?}");
    }

    #[test]
    fn globs_behave() {
        assert!(glob_match("a-*", "a-wall"));
        assert!(glob_match("*wall*", "a-wall-1"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a-*", "b-wall"));
        assert!(glob_match("*", "anything"));
    }
}
