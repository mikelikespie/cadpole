//! STEP (ISO 10303-21) into `KSolid`s, over `truck-stepio`'s lexer and
//! typed holder table.
//!
//! What `truck-stepio` is used for: the Part 21 lexer (`ruststep`), the
//! typed holders for the Part 42 geometry entities, and the trimming of an
//! edge's curve between its two vertices (`EdgeCurve::parse_curve3d`).
//! What it is **not** used for: `to_compressed_shell`, which drops every
//! face or edge it cannot convert with an `eprintln!` and returns the shell
//! with a hole. This module walks each `CLOSED_SHELL` itself, in the file's
//! own order, and turns the first thing it cannot convert into a diagnostic
//! that names the face by index and the entity by id.
//!
//! **The geometry mapping**, every row exact:
//!
//! | STEP | here |
//! | --- | --- |
//! | `PLANE` | `Surface::Plane` |
//! | `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE` | a `RevolutedCurve` of a `Line` — the form Cadpole's own revolves take, which `SurfaceClass` recognises |
//! | `SPHERICAL_SURFACE`, `TOROIDAL_SURFACE` | a `RevolutedCurve` of a rational quadratic arc: the meridian, the tube's circle |
//! | `SURFACE_OF_REVOLUTION` | a `RevolutedCurve` of the converted profile |
//! | `SURFACE_OF_LINEAR_EXTRUSION` | a NURBS surface of degree (curve, 1) |
//! | `B_SPLINE_SURFACE*`, rational forms | moved |
//! | `LINE`, `B_SPLINE_CURVE*`, rational forms | moved; `POLYLINE` as a degree-1 B-spline |
//! | `CIRCLE`, `ELLIPSE` | a rational quadratic NURBS arc between the edge's vertices, pieces of at most 90° |
//! | `SURFACE_CURVE`, `SEAM_CURVE` | their 3D curve |
//! | `HYPERBOLA`, `PARABOLA`, `PCURVE`-only edges, `OFFSET_SURFACE`, `CURVE_BOUNDED_SURFACE`, `RECTANGULAR_TRIMMED_SURFACE`, `DEGENERATE_TOROIDAL_SURFACE`, `FACETED_BREP` | refused, by name and entity id |
//!
//! Every elementary surface is read from its own record here rather than
//! through the crate's `From` impls, for two reasons: the
//! crate normalises `ref_direction - (ref_direction·axis) axis` without
//! checking it is non-zero (NaN on real vendor files, its issues #123/#128),
//! and it cannot say which face failed. The seam of a surface of revolution
//! is placed where the face's boundary has an edge along it or, failing
//! that, in the middle of the largest angular gap of the boundary, so a
//! face never straddles the seam; the axial extent of a cylinder or cone is
//! taken from the boundary with a margin.
//!
//! **Topology.** Vertices are shared by `VERTEX_POINT` id, edges by
//! `EDGE_CURVE` id. An edge that starts and ends at one vertex — what an
//! OCCT-based exporter writes for a full circle — is split at its parametric
//! middle, because truck refuses `Edge::try_new(&v, &v, …)`; a zero-length
//! edge on one vertex is dropped. Faces disagreeing about orientation are
//! made consistent by a flood fill over shared edges, the solid is turned
//! outward by its signed volume, and `Solid::try_new` then holds it to the
//! same standard as every other Cadpole body.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::{PI, TAU};
use std::panic::{AssertUnwindSafe, catch_unwind};

use cadpole_core::geom::Transform;
use cadpole_core::kernel::{self, KSolid};
use cadpole_core::fmath;
use truck_modeling::*;
// The glob brings truck's one-argument `Result` alias; this module speaks in `String` errors.
use std::result::Result;
use truck_stepio::r#in::alias::{Conic3D, Curve3D, ElementarySurface, Surface as StepSurface, SweptCurve};
use truck_stepio::r#in::{
    Axis2Placement3dHolder, CartesianPointHolder, DirectionHolder, EdgeAnyHolder, EdgeCurveHolder, FaceAnyHolder,
    FaceBoundHolder, FaceSurfaceHolder,
    SurfaceAnyHolder, Table, VertexPointHolder,
};
use truck_stepio::r#in::ruststep::ast::{EntityInstance, Name, Parameter, Record};
use truck_stepio::r#in::ruststep::tables::{EntityTable, IntoOwned, PlaceHolder};
use truck_topology::compress::{CompressedEdge, CompressedEdgeIndex, CompressedFace, CompressedShell};
use truck_topology::shell::ShellCondition;

use crate::label;
use crate::{Note, bounded, prescan};

/// What `import` takes beside the bytes.
#[derive(Clone, Debug)]
pub struct Options {
    /// The vertex-matching distance to trust, in mm; `None` reads the file's
    /// `distance_accuracy_value`, clamped to `[1e-6, 0.1]`.
    pub tolerance: Option<f64>,
    /// The wall-clock budget for parse and conversion, in milliseconds
    /// ([`crate::bounded`]); 0 means the panic guard alone.
    pub budget_ms: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options { tolerance: None, budget_ms: crate::IMPORT_BUDGET_MS }
    }
}

/// One face of an imported solid, in the solid's face order — which is
/// `CLOSED_SHELL.cfs_faces` order, except that a face split at a seam
/// (see `ShellBuilder::split_at_meridian`) is two consecutive entries with
/// the same `index` and `piece` 0 and 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceLabel {
    /// The `ADVANCED_FACE` entity id.
    pub entity: u64,
    /// Position in the file's `cfs_faces` list: the `k` of `face[k]`.
    pub index: usize,
    /// 0, or 1 for the second half of a split face (`face[k]#1`).
    pub piece: usize,
    /// The file's own `name`, sanitised, when non-empty and unique within
    /// the shell; empty otherwise. What the caller keeps as an alias.
    pub label: String,
    /// What the file actually said, for `cadpole inspect`.
    pub raw: String,
}

/// One solid the file held.
#[derive(Clone, Debug)]
pub struct ImportedSolid {
    pub solid: KSolid,
    /// The entity that named it: `MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`
    /// or a bare `CLOSED_SHELL`.
    pub entity: u64,
    pub kind: &'static str,
    /// Its label: the `PRODUCT` name, else the solid's, else the shell's,
    /// sanitised; empty when the file gave none.
    pub label: String,
    pub raw_label: String,
    pub faces: Vec<FaceLabel>,
    /// Faces the flood fill had to turn to make the shell consistent.
    pub flipped_faces: usize,
    /// The whole shell was inside out and was turned.
    pub inverted: bool,
    /// Full-circle edges split in two, and zero-length edges dropped.
    pub split_edges: usize,
    pub dropped_edges: usize,
    /// Edges the file wrote off the surfaces they bound (a boolean's
    /// intersection curve refitted within a tolerance) and that were lifted
    /// onto both, as the intersection curves Cadpole's own booleans make.
    pub lifted_edges: usize,
    /// Faces whose loop touched itself at a seam edge (a vendor's full
    /// cylinder, walked up one side of the seam and down the other), each
    /// split in two at the meridian opposite: `face[k]` and `face[k]#1`.
    pub seam_faces: usize,
    /// Void shells (`BREP_WITH_VOIDS`).
    pub voids: usize,
}

/// What an import produced.
#[derive(Clone, Debug)]
pub struct Import {
    pub solids: Vec<ImportedSolid>,
    pub notes: Vec<Note>,
    /// Millimetres per file length unit, and the unit's name.
    pub unit_scale: f64,
    pub unit: String,
    /// The tolerance used, in mm.
    pub tolerance: f64,
    pub entities: usize,
    pub schema: String,
    pub product: String,
}

/// Read a STEP file under the pre-scan and the deadline.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Import, String> {
    let entities = prescan::step(bytes)?;
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_owned(),
        Err(e) => return Err(format!("not valid UTF-8 at byte {}: a STEP file is text", e.valid_up_to())),
    };
    let opts = opts.clone();
    match bounded::run(opts.budget_ms, move || convert(&text, &opts, entities)) {
        Ok(r) => r,
        Err(stopped) => Err(format!("STEP import {stopped}")),
    }
}

/// [`import`] without the deadline or the panic guard: for tests that want a
/// panic to be a failure with a backtrace.
pub fn import_unbounded(text: &str, opts: &Options) -> Result<Import, String> {
    let entities = prescan::step(text.as_bytes())?;
    convert(text, opts, entities)
}

// ---------------------------------------------------------------- records

/// The data section's records by id: what the holder table drops
/// (`PRODUCT`, the solids, the unit context) and what it holds without its
/// fields being reachable (every elementary surface's parameters).
struct Records<'a> {
    simple: BTreeMap<u64, &'a Record>,
    complex: BTreeMap<u64, Vec<&'a Record>>,
}

impl<'a> Records<'a> {
    fn of(entities: &'a [EntityInstance]) -> Records<'a> {
        let mut simple = BTreeMap::new();
        let mut complex = BTreeMap::new();
        for e in entities {
            match e {
                EntityInstance::Simple { id, record } => {
                    simple.insert(*id, record);
                }
                EntityInstance::Complex { id, subsuper } => {
                    complex.insert(*id, subsuper.0.iter().collect());
                }
            }
        }
        Records { simple, complex }
    }

    fn name_of(&self, id: u64) -> Option<&str> {
        self.simple.get(&id).map(|r| r.name.as_str()).or_else(|| {
            self.complex.get(&id).map(|rs| rs.last().map(|r| r.name.as_str()).unwrap_or("complex"))
        })
    }

    fn named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = (u64, &'a Record)> + 's {
        self.simple.iter().filter(move |(_, r)| r.name == name).map(|(id, r)| (*id, *r))
    }
}

fn params(r: &Record) -> &[Parameter] {
    match &r.parameter {
        Parameter::List(v) => v,
        _ => &[],
    }
}

fn as_ref(p: &Parameter) -> Option<u64> {
    match p {
        Parameter::Ref(Name::Entity(id)) => Some(*id),
        _ => None,
    }
}

fn as_str(p: &Parameter) -> Option<&str> {
    match p {
        Parameter::String(s) => Some(s),
        _ => None,
    }
}

fn as_real(p: &Parameter) -> Option<f64> {
    match p {
        Parameter::Real(v) => Some(*v),
        Parameter::Integer(i) => Some(*i as f64),
        Parameter::Typed { parameter, .. } => as_real(parameter),
        _ => None,
    }
}

/// An enumeration value without its dots: `ruststep` keeps `.MILLI.` as
/// `MILLI`, and either spelling reads the same here.
fn as_enum(p: &Parameter) -> Option<&str> {
    match p {
        Parameter::Enumeration(s) => Some(s.trim_matches('.')),
        _ => None,
    }
}

fn as_list(p: &Parameter) -> &[Parameter] {
    match p {
        Parameter::List(v) => v,
        _ => &[],
    }
}

// ---------------------------------------------------------------- units

/// Millimetres per unit of a `(LENGTH_UNIT NAMED_UNIT SI_UNIT)` or a
/// `CONVERSION_BASED_UNIT` instance; `None` when `id` is neither.
fn length_unit_scale(recs: &Records, id: u64, depth: usize) -> Option<(f64, String)> {
    if depth > 2 {
        return None;
    }
    let parts = recs.complex.get(&id)?;
    let names: HashSet<&str> = parts.iter().map(|r| r.name.as_str()).collect();
    if !names.contains("LENGTH_UNIT") {
        return None;
    }
    if let Some(si) = parts.iter().find(|r| r.name == "SI_UNIT") {
        let ps = params(si);
        let prefix = ps.first().and_then(as_enum).unwrap_or("");
        let name = ps.get(1).and_then(as_enum).unwrap_or("");
        if name != "METRE" {
            return None;
        }
        let factor = match prefix {
            "" | "$" => 1.0,
            "MILLI" => 1e-3,
            "CENTI" => 1e-2,
            "DECI" => 1e-1,
            "MICRO" => 1e-6,
            "KILO" => 1e3,
            "NANO" => 1e-9,
            _ => return None,
        };
        let label = match prefix {
            "" | "$" => "m",
            "MILLI" => "mm",
            "CENTI" => "cm",
            "DECI" => "dm",
            "MICRO" => "um",
            "KILO" => "km",
            _ => "nm",
        };
        return Some((factor * 1000.0, label.to_owned()));
    }
    if let Some(cb) = parts.iter().find(|r| r.name == "CONVERSION_BASED_UNIT") {
        let ps = params(cb);
        let name = ps.first().and_then(as_str).unwrap_or("").to_owned();
        if let Some(mid) = ps.get(1).and_then(as_ref)
            && let Some(m) = recs.simple.get(&mid)
        {
            let mp = params(m);
            let value = mp.first().and_then(as_real);
            let base = mp.get(1).and_then(as_ref).and_then(|u| length_unit_scale(recs, u, depth + 1));
            if let (Some(v), Some((b, _))) = (value, base) {
                return Some((v * b, name));
            }
        }
        // The measure did not resolve: the name is enough for the two
        // units anyone writes.
        return match name.to_ascii_uppercase().as_str() {
            "INCH" => Some((25.4, name)),
            "FOOT" => Some((304.8, name)),
            _ => None,
        };
    }
    None
}

/// The file's length unit: the one the representation context assigns
/// (`GLOBAL_UNIT_ASSIGNED_CONTEXT`), else the first `LENGTH_UNIT` by id
/// that is not merely the base of a `CONVERSION_BASED_UNIT`.
fn length_unit(recs: &Records, notes: &mut Vec<Note>) -> (f64, String) {
    let mut found: Vec<(f64, String)> = Vec::new();
    for parts in recs.complex.values() {
        if let Some(ctx) = parts.iter().find(|r| r.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for u in params(ctx).first().map(as_list).unwrap_or(&[]) {
                if let Some(uid) = as_ref(u)
                    && let Some(unit) = length_unit_scale(recs, uid, 0)
                {
                    found.push(unit);
                }
            }
        }
    }
    if found.is_empty() {
        let bases: HashSet<u64> = recs
            .named("LENGTH_MEASURE_WITH_UNIT")
            .filter_map(|(_, r)| params(r).get(1).and_then(as_ref))
            .collect();
        for (&id, _) in &recs.complex {
            if !bases.contains(&id)
                && let Some(u) = length_unit_scale(recs, id, 0)
            {
                found.push(u);
            }
        }
    }
    match found.first() {
        Some((s, n)) => {
            if found.iter().any(|(t, _)| (t - s).abs() > 1e-12) {
                notes.push(Note::warning(format!(
                    "the file declares more than one length unit; the first ({n}) is used for everything"
                )));
            }
            (*s, n.clone())
        }
        None => {
            notes.push(Note::info("the file declares no length unit; its coordinates are read as millimetres"));
            (1.0, "mm (assumed)".into())
        }
    }
}

/// Radians per unit of a `PLANE_ANGLE_UNIT` instance: 1 for the SI radian,
/// or the conversion factor a `CONVERSION_BASED_UNIT` (`DEGREE`) carries;
/// `None` when `id` is not a plane angle unit.
fn angle_unit_scale(recs: &Records, id: u64) -> Option<f64> {
    let parts = recs.complex.get(&id)?;
    if !parts.iter().any(|r| r.name == "PLANE_ANGLE_UNIT") {
        return None;
    }
    if let Some(cb) = parts.iter().find(|r| r.name == "CONVERSION_BASED_UNIT") {
        let ps = params(cb);
        if let Some(mid) = ps.get(1).and_then(as_ref)
            && let Some(m) = recs.simple.get(&mid)
            && let Some(v) = params(m).first().and_then(as_real)
        {
            return Some(v);
        }
        if ps.first().and_then(as_str).is_some_and(|n| n.eq_ignore_ascii_case("degree")) {
            return Some(PI / 180.0);
        }
    }
    Some(1.0)
}

/// Radians per unit of the file's plane angle unit (`CONICAL_SURFACE`'s
/// `semi_angle` is written in it): the one the representation context
/// assigns (`GLOBAL_UNIT_ASSIGNED_CONTEXT`), else the first
/// `PLANE_ANGLE_UNIT` by id that is not merely the base a conversion is
/// measured in, else radians. As [`length_unit`]: a `DEGREE` unit is
/// defined from an SI radian, which is often the lower id, so the first by
/// id alone reads degrees as radians.
fn angle_unit(recs: &Records) -> f64 {
    for parts in recs.complex.values() {
        if let Some(ctx) = parts.iter().find(|r| r.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for u in params(ctx).first().map(as_list).unwrap_or(&[]) {
                if let Some(uid) = as_ref(u)
                    && let Some(scale) = angle_unit_scale(recs, uid)
                {
                    return scale;
                }
            }
        }
    }
    let bases: HashSet<u64> = recs
        .named("PLANE_ANGLE_MEASURE_WITH_UNIT")
        .filter_map(|(_, r)| params(r).get(1).and_then(as_ref))
        .collect();
    recs.complex
        .keys()
        .filter(|id| !bases.contains(id))
        .find_map(|&id| angle_unit_scale(recs, id))
        .unwrap_or(1.0)
}

/// The `distance_accuracy_value` the file declares, in file units.
fn file_tolerance(recs: &Records) -> Option<f64> {
    for (_, r) in recs.named("UNCERTAINTY_MEASURE_WITH_UNIT") {
        let ps = params(r);
        if let Some(v) = ps.first().and_then(as_real) {
            return Some(v);
        }
    }
    // The uncertainty is usually a complex instance
    // `(LENGTH_MEASURE_WITH_UNIT MEASURE_WITH_UNIT UNCERTAINTY_MEASURE_WITH_UNIT)`.
    for parts in recs.complex.values() {
        if parts.iter().any(|r| r.name == "UNCERTAINTY_MEASURE_WITH_UNIT") {
            for r in parts {
                if r.name == "MEASURE_WITH_UNIT"
                    && let Some(v) = params(r).first().and_then(as_real)
                {
                    return Some(v);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------- solids

struct SolidRef {
    entity: u64,
    kind: &'static str,
    raw_label: String,
    outer: u64,
    /// `(shell id, orientation)` per void.
    voids: Vec<(u64, bool)>,
}

/// The solids the file names, in ascending entity id: every
/// `MANIFOLD_SOLID_BREP` and `BREP_WITH_VOIDS`, or, when there are none,
/// every `CLOSED_SHELL` that is not a void of something.
fn solids(recs: &Records, table: &Table, notes: &mut Vec<Note>) -> Vec<SolidRef> {
    let mut out = Vec::new();
    for (id, r) in recs.simple.iter().map(|(id, r)| (*id, *r)) {
        match r.name.as_str() {
            "MANIFOLD_SOLID_BREP" => {
                let ps = params(r);
                if let Some(outer) = ps.get(1).and_then(as_ref) {
                    out.push(SolidRef {
                        entity: id,
                        kind: "MANIFOLD_SOLID_BREP",
                        raw_label: ps.first().and_then(as_str).unwrap_or("").to_owned(),
                        outer,
                        voids: Vec::new(),
                    });
                }
            }
            "BREP_WITH_VOIDS" => {
                let ps = params(r);
                if let Some(outer) = ps.get(1).and_then(as_ref) {
                    let mut voids = Vec::new();
                    for v in ps.get(2).map(as_list).unwrap_or(&[]) {
                        if let Some(vid) = as_ref(v) {
                            if let Some(os) = table.oriented_shell.get(&vid) {
                                if let PlaceHolder::Ref(Name::Entity(sid)) = os.shell_element {
                                    voids.push((sid, os.orientation));
                                }
                            } else if table.shell.contains_key(&vid) {
                                voids.push((vid, true));
                            }
                        }
                    }
                    out.push(SolidRef {
                        entity: id,
                        kind: "BREP_WITH_VOIDS",
                        raw_label: ps.first().and_then(as_str).unwrap_or("").to_owned(),
                        outer,
                        voids,
                    });
                }
            }
            "FACETED_BREP" => notes.push(Note::warning(format!(
                "FACETED_BREP #{id} skipped: a faceted B-rep (POLY_LOOP faces) has no surfaces; export the part \
                 as STL or 3MF and use import_mesh"
            ))),
            _ => {}
        }
    }
    if out.is_empty() {
        let used: HashSet<u64> = out.iter().flat_map(|s| s.voids.iter().map(|v| v.0)).collect();
        for (id, r) in recs.named("CLOSED_SHELL") {
            if !used.contains(&id) {
                out.push(SolidRef {
                    entity: id,
                    kind: "CLOSED_SHELL",
                    raw_label: params(r).first().and_then(as_str).unwrap_or("").to_owned(),
                    outer: id,
                    voids: Vec::new(),
                });
            }
        }
        let open = recs.named("OPEN_SHELL").count();
        if open > 0 {
            notes.push(Note::warning(format!(
                "{open} OPEN_SHELL{} skipped: an open shell is not a solid",
                if open == 1 { "" } else { "s" }
            )));
        }
    }
    out
}

fn product_name(recs: &Records) -> String {
    recs.named("PRODUCT").next().and_then(|(_, r)| params(r).get(1).and_then(as_str)).unwrap_or("").to_owned()
}

fn schema_name(header: &[Record]) -> String {
    header
        .iter()
        .find(|r| r.name == "FILE_SCHEMA")
        .and_then(|r| params(r).first())
        .map(as_list)
        .and_then(|l| l.first())
        .and_then(as_str)
        .map(|s| s.split(['{', ' ']).next().unwrap_or(s).to_owned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- the walk

/// The text with every `''` inside a string literal written as
/// `\X2\0027\X0\`, the other Part 21 spelling of an apostrophe.
/// `ruststep`'s lexer ends a string at the first `'`, so the doubled form —
/// the one every writer uses, Cadpole's included — made the whole file
/// untokenisable. The replacement holds no quote, survives the lexer as
/// string text, and [`label::decode_step`] turns it back into `'`.
/// Comments are skipped so an apostrophe in one opens no string. Borrowed
/// unchanged when there is nothing to replace.
fn escape_doubled_apostrophes(text: &str) -> std::borrow::Cow<'_, str> {
    let b = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    let mut i = 0usize;
    let mut in_string = false;
    while i < b.len() {
        if in_string {
            if b[i] == b'\'' {
                if b.get(i + 1) == Some(&b'\'') {
                    let o = out.get_or_insert_with(|| String::with_capacity(text.len() + 64));
                    o.push_str(&text[copied..i]);
                    o.push_str("\\X2\\0027\\X0\\");
                    i += 2;
                    copied = i;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b[i] {
            b'\'' => in_string = true,
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    match out {
        Some(mut o) => {
            o.push_str(&text[copied..]);
            std::borrow::Cow::Owned(o)
        }
        None => std::borrow::Cow::Borrowed(text),
    }
}

fn convert(text: &str, opts: &Options, entities: usize) -> Result<Import, String> {
    // The pre-scan allows a UTF-8 byte order mark before the magic; the
    // lexer does not.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = &*escape_doubled_apostrophes(text);
    let exchange = truck_stepio::r#in::ruststep::parser::parse(text).map_err(|e| {
        // The parser's message quotes the input around the error; cap and
        // strip it so a hostile file cannot write through it.
        let m: String = e.to_string().chars().filter(|c| !c.is_control()).take(240).collect();
        format!("not a well-formed STEP file: {m}")
    })?;
    let mut notes = Vec::new();
    if exchange.data.is_empty() {
        return Err("the file has no DATA section".into());
    }
    if exchange.data.len() > 1 {
        notes.push(Note::warning(format!(
            "the file has {} DATA sections; only the first is read",
            exchange.data.len()
        )));
    }
    let data = &exchange.data[0];
    let recs = Records::of(&data.entities);
    let mut table = Table::default();
    let mut unreadable = 0usize;
    let mut first_bad: Option<(u64, String)> = None;
    for e in &data.entities {
        let id = match e {
            EntityInstance::Simple { id, .. } | EntityInstance::Complex { id, .. } => *id,
        };
        // A holder that fails to deserialise is a malformed entity: it stays
        // out of the table, and a shell that needs it reports it by id.
        let r = catch_unwind(AssertUnwindSafe(|| table.push_instance(e)));
        match r {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                unreadable += 1;
                if first_bad.is_none() {
                    let m: String = err.to_string().chars().filter(|c| !c.is_control()).take(160).collect();
                    first_bad = Some((id, m));
                }
            }
            Err(_) => {
                unreadable += 1;
                if first_bad.is_none() {
                    first_bad = Some((id, "the entity could not be read".into()));
                }
            }
        }
    }
    if unreadable > 0 {
        let (id, m) = first_bad.clone().unwrap_or((0, String::new()));
        notes.push(Note::warning(format!(
            "{unreadable} entit{} could not be read (first: #{id}: {m}); a shell that needs one is refused",
            if unreadable == 1 { "y" } else { "ies" }
        )));
    }

    let (unit_scale, unit) = length_unit(&recs, &mut notes);
    let angle_scale = angle_unit(&recs);
    let tolerance = match opts.tolerance {
        Some(t) if t.is_finite() && t > 0.0 => t,
        _ => file_tolerance(&recs).map(|t| (t * unit_scale).clamp(1e-6, 0.1)).unwrap_or(1e-4),
    };
    let product = product_name(&recs);
    let schema = schema_name(&exchange.header);
    let refs = solids(&recs, &table, &mut notes);
    if refs.is_empty() {
        return Err(format!(
            "the file holds no solid: no MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS or CLOSED_SHELL among its \
             {entities} entities (schema {})",
            if schema.is_empty() { "unknown" } else { schema.as_str() }
        ));
    }

    let mut solids_out = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (k, sr) in refs.iter().enumerate() {
        let mut b = ShellBuilder::new(&table, &recs, tolerance / unit_scale, angle_scale);
        match b.solid(sr) {
            Ok(mut built) => {
                if (unit_scale - 1.0).abs() > 1e-12 {
                    built.solid = kernel::transformed(&built.solid, &Transform(Matrix4::from_scale(unit_scale)));
                }
                let raw_label = if k == 0 && !product.is_empty() && refs.len() == 1 {
                    product.clone()
                } else if !sr.raw_label.is_empty() {
                    sr.raw_label.clone()
                } else {
                    table.shell.get(&sr.outer).map(|s| s.label.clone()).unwrap_or_default()
                };
                built.label = label::sanitise(&raw_label);
                built.raw_label = raw_label;
                built.entity = sr.entity;
                built.kind = sr.kind;
                solids_out.push(built);
            }
            Err(e) => failures.push(format!("{} #{} ({}): {e}", sr.kind, sr.entity, label::quoted(&sr.raw_label))),
        }
    }
    if !failures.is_empty() {
        let shown: Vec<&String> = failures.iter().take(6).collect();
        let more = failures.len().saturating_sub(6);
        return Err(format!(
            "{} of {} solid{} could not be imported:\n  {}{}",
            failures.len(),
            refs.len(),
            if refs.len() == 1 { "" } else { "s" },
            shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  "),
            if more > 0 { format!("\n  … and {more} more") } else { String::new() }
        ));
    }
    Ok(Import { solids: solids_out, notes, unit_scale, unit, tolerance, entities, schema, product })
}

/// An orthonormal placement frame from an `AXIS2_PLACEMENT_3D`.
#[derive(Clone, Copy, Debug)]
struct Frame {
    o: Point3,
    x: Vector3,
    y: Vector3,
    z: Vector3,
}

impl Frame {
    fn local(&self, p: Point3) -> Vector3 {
        let d = p - self.o;
        Vector3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
    /// The radial unit vector at angle `phi` from `x` towards `y`.
    fn radial(&self, phi: f64) -> Vector3 {
        self.x * fmath::cos(phi) + self.y * fmath::sin(phi)
    }
}

/// Where the seam of a surface of revolution goes, how far along the axis
/// a face reaches, and which side of its boundary the face is on, read off
/// the boundary's sample points.
struct Boundary {
    /// Samples per edge, each edge's samples in the direction the loop
    /// walks it.
    edges: Vec<Vec<Point3>>,
}

/// The margin a profile arc is extended past the boundary's extent, in
/// radians: enough that a boundary point sits inside the parameter range
/// after rounding, small enough that the arc is the one Cadpole built to
/// within 1e-7 (its tessellation is then the same).
const ARC_MARGIN: f64 = 1e-7;

/// How far past a boundary vertex the seam of a surface of revolution is
/// placed, in radians, so that no vertex ever lies *on* the seam. Cadpole's
/// own primitives put a vertex exactly on the seam and get away with it
/// because the vertex is bit-identical to the profile's start; a decimal
/// file cannot reproduce that, and truck's boundary-to-parameter mapping
/// then decides by rounding which side of the seam the vertex is on.
///
/// On Cadpole's own spheres, rotated and not, at 2 mm and 7 mm, on
/// and off the axis: the seam this far past the *outer loop's first
/// vertex* tessellates every one — the boundary then starts at
/// `u = 2π − ε`, runs the whole way round inside `(0, 2π)` and crosses the
/// seam only where it closes on itself; the same distance *before* that
/// vertex, or past any other vertex, or in the middle of the largest gap
/// between boundary points, fails some of them with a hole along the
/// seam. The size does not matter between 1e-14 and 1e-6 rad and it does
/// not move the tessellation (that divides by knot span), so it is
/// far above rounding and far below anything a tolerance sees.
const SEAM_NUDGE: f64 = 1e-9;

impl Boundary {
    fn all(&self) -> impl Iterator<Item = Point3> + '_ {
        self.edges.iter().flatten().copied()
    }

    /// The first step the loop takes: two consecutive samples of its first
    /// edge, in walking order.
    fn first_step(&self) -> Option<(Point3, Point3)> {
        self.edges.iter().find(|e| e.len() >= 2).map(|e| (e[0], e[1]))
    }

    /// Which way the face lies from its first edge, along `toward`: the
    /// loop runs counter-clockwise about the face normal, so the interior
    /// is on the loop's left. Positive means along `toward(p)`.
    fn interior_sign(&self, face_normal: impl Fn(Point3) -> Vector3, toward: impl Fn(Point3) -> Vector3) -> Option<f64> {
        let (p0, p1) = self.first_step()?;
        let d = p1 - p0;
        let left = face_normal(p0).cross(d);
        let s = left.dot(toward(p0));
        (s.is_finite() && s.abs() > 1e-12).then_some(s)
    }

    /// The smallest interval `[lo, hi]` (in radians, `hi - lo < 2π`) that
    /// holds every `angle(p)` of the boundary, found as the complement of
    /// the largest gap; `None` when no sample has an angle.
    fn angle_interval(&self, angle: impl Fn(Point3) -> Option<f64>) -> Option<(f64, f64)> {
        let mut angles: Vec<f64> = self.all().filter_map(angle).map(|a| a.rem_euclid(TAU)).collect();
        if angles.is_empty() {
            return None;
        }
        angles.sort_by(|a, b| a.total_cmp(b));
        // The largest gap, as (size, angle it starts at).
        let mut best = (angles[0] + TAU - angles[angles.len() - 1], angles[angles.len() - 1]);
        for w in angles.windows(2) {
            let gap = w[1] - w[0];
            if gap > best.0 {
                best = (gap, w[0]);
            }
        }
        let lo = best.1 + best.0;
        let hi = best.1 + TAU;
        Some((lo, hi))
    }

    /// The seam angle about `frame.z`: a hair past an edge that runs along
    /// a meridian (a seam edge, all its samples at one angle), else a hair
    /// past the outer loop's first vertex, else 0. Never on a vertex, and
    /// never crossed by the boundary except where the loop closes
    /// ([`SEAM_NUDGE`] says what was measured).
    fn seam(&self, frame: &Frame) -> f64 {
        let angle = |p: Point3| {
            let l = frame.local(p);
            let r = (l.x * l.x + l.y * l.y).sqrt();
            (r > 1e-9).then(|| fmath::atan2(l.y, l.x))
        };
        for e in &self.edges {
            let angles: Vec<f64> = e.iter().filter_map(|&p| angle(p)).collect();
            if angles.len() >= 2 && angles.iter().all(|a| angular_gap(*a, angles[0]) < 1e-6) {
                return angles[0] + SEAM_NUDGE;
            }
        }
        // The outer loop's first vertex (the loop starts there and ends
        // there, so the seam a hair past it is crossed only at the closure).
        for e in &self.edges {
            if let Some(a) = e.first().and_then(|&p| angle(p)) {
                return a + SEAM_NUDGE;
            }
        }
        0.0
    }

    /// `(min, max)` of the boundary along `frame.z`, widened by a margin.
    fn extent(&self, frame: &Frame, margin: f64) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for p in self.all() {
            let h = frame.local(p).z;
            lo = lo.min(h);
            hi = hi.max(h);
        }
        if !lo.is_finite() {
            return (-1.0, 1.0);
        }
        let pad = (hi - lo).abs() * 0.02 + margin;
        (lo - pad, hi + pad)
    }

    /// The seam angle around a torus's tube: the angle in the (radial, z)
    /// half-plane, treated like [`Boundary::seam`].
    fn tube_seam(&self, frame: &Frame, major: f64) -> f64 {
        let angle = |p: Point3| {
            let l = frame.local(p);
            let r = (l.x * l.x + l.y * l.y).sqrt() - major;
            ((r * r + l.z * l.z).sqrt() > 1e-9).then(|| fmath::atan2(l.z, r))
        };
        for e in &self.edges {
            let angles: Vec<f64> = e.iter().filter_map(|&p| angle(p)).collect();
            if angles.len() >= 2 && angles.iter().all(|a| angular_gap(*a, angles[0]) < 1e-6) {
                return angles[0];
            }
        }
        let mut angles: Vec<f64> = self.all().filter_map(angle).map(|a| a.rem_euclid(TAU)).collect();
        if angles.is_empty() {
            return 0.0;
        }
        angles.sort_by(|a, b| a.total_cmp(b));
        let mut best = (angles[0] + TAU - angles[angles.len() - 1], angles[angles.len() - 1]);
        for w in angles.windows(2) {
            let gap = w[1] - w[0];
            if gap > best.0 {
                best = (gap, w[0]);
            }
        }
        best.1 + best.0 / 2.0
    }
}

fn angular_gap(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

/// A rational quadratic NURBS for `c + cos t · a + sin t · b`, `t ∈ [t0,
/// t1]`, in pieces of at most 90°: exact for a circle (`|a| = |b|`, `a ⟂
/// b`) and for an ellipse (any `a`, `b`), since the construction is affine.
fn conic_arc(c: Point3, a: Vector3, b: Vector3, t0: f64, t1: f64) -> NurbsCurve<Vector4> {
    let span = (t1 - t0).max(1e-12);
    let n = ((span / (PI / 2.0 + 1e-9)).ceil() as usize).clamp(1, 8);
    let d = span / n as f64;
    let w = fmath::cos(d / 2.0);
    let pt = |t: f64| c + a * fmath::cos(t) + b * fmath::sin(t);
    let mut knots = vec![0.0; 3];
    let mut cps = Vec::with_capacity(2 * n + 1);
    cps.push(pt(t0).to_homogeneous());
    for i in 0..n {
        let ta = t0 + d * i as f64;
        let tb = ta + d;
        let tm = (ta + tb) / 2.0;
        // The middle control point is where the end tangents meet: the
        // arc's midpoint pushed out by 1/cos(d/2).
        let m = c + (a * fmath::cos(tm) + b * fmath::sin(tm)) * (1.0 / w);
        cps.push(Vector4::new(m.x * w, m.y * w, m.z * w, w));
        cps.push(pt(tb).to_homogeneous());
        if i + 1 < n {
            knots.push((i + 1) as f64);
            knots.push((i + 1) as f64);
        }
    }
    knots.extend([n as f64; 3]);
    NurbsCurve::new(BSplineCurve::new(KnotVec::from(knots), cps))
}

/// truck's own circular arc — one rational quadratic Bézier with its
/// quarter knots, `truck_modeling::builder::circle_arc` — from `p0` to
/// `p1` through `transit`. A circle Cadpole wrote comes back as the curve Cadpole
/// built it from, so the tessellation divides it identically (it divides
/// per knot span) and a Cadpole→Cadpole round trip is exact.
fn truck_arc(p0: Point3, p1: Point3, transit: Point3) -> Curve {
    builder::circle_arc(&builder::vertex(p0), &builder::vertex(p1), transit).curve()
}

/// A circle's centre, radius vectors and trimmed range, when the conic is a
/// circle (`|a| = |b|`, `a ⟂ b`).
fn circle_of(proc: &truck_stepio::r#in::alias::Ellipse<Point3, Matrix4>) -> Option<(Point3, Vector3, Vector3, f64, f64)> {
    let (t0, t1) = proc.entity().range_tuple();
    let m = proc.transform();
    let c = Point3::from_homogeneous(m[3]);
    let a = m[0].truncate();
    let b = m[1].truncate();
    if !finite3(a) || !finite3(b) || !t0.is_finite() || !t1.is_finite() {
        return None;
    }
    let (la, lb) = (a.magnitude(), b.magnitude());
    let circle = la > 1e-12 && (la - lb).abs() <= 1e-9 * la && a.dot(b).abs() <= 1e-9 * la * la;
    circle.then_some((c, a, b, t0, t1))
}

/// Length of a curve, estimated from `n` samples.
fn curve_length(c: &Curve, n: usize) -> f64 {
    let (t0, t1) = c.range_tuple();
    let mut l = 0.0;
    let mut prev = c.subs(t0);
    for i in 1..=n {
        let p = c.subs(t0 + (t1 - t0) * i as f64 / n as f64);
        l += prev.distance(p);
        prev = p;
    }
    l
}

fn samples(c: &Curve, n: usize) -> Vec<Point3> {
    let (t0, t1) = c.range_tuple();
    (0..=n).map(|i| c.subs(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
}

fn finite3(v: Vector3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// What `surface` knows about an analytic surface of revolution it built,
/// kept for the seam split: the frame, where the seam is, and the profile's
/// kind and extent.
#[derive(Clone, Copy, Debug)]
struct Revolved {
    frame: Frame,
    /// The seam's angle about `frame.z` from `frame.x`.
    seam: f64,
    kind: RevolvedKind,
}

#[derive(Clone, Copy, Debug)]
enum RevolvedKind {
    /// A straight profile: the meridian between two points is a line.
    Ruled,
    Sphere { r: f64 },
    /// The tube's angle the face's profile starts at.
    Torus { major: f64, minor: f64, lo: f64 },
}

/// An analytic surface as its record gave it, so the distance of a point
/// to it is a formula rather than a parameter search (which truck cannot
/// seed on a surface of revolution and gets wrong near a pole).
#[derive(Clone, Copy, Debug)]
enum Analytic {
    Plane { o: Point3, n: Vector3 },
    Cylinder { f: Frame, r: f64 },
    Cone { f: Frame, r: f64, alpha: f64 },
    Sphere { f: Frame, r: f64 },
    Torus { f: Frame, major: f64, minor: f64 },
}

impl Analytic {
    fn distance(&self, p: Point3) -> f64 {
        match *self {
            Analytic::Plane { o, n } => (p - o).dot(n).abs(),
            Analytic::Cylinder { f, r } => {
                let l = f.local(p);
                ((l.x * l.x + l.y * l.y).sqrt() - r).abs()
            }
            Analytic::Cone { f, r, alpha } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                ((rho - r - l.z * fmath::tan(alpha)) * fmath::cos(alpha)).abs()
            }
            Analytic::Sphere { f, r } => ((p - f.o).magnitude() - r).abs(),
            Analytic::Torus { f, major, minor } => {
                let l = f.local(p);
                let rr = (l.x * l.x + l.y * l.y).sqrt() - major;
                ((rr * rr + l.z * l.z).sqrt() - minor).abs()
            }
        }
    }
}

/// The per-shell walk.
struct ShellBuilder<'t> {
    table: &'t Table,
    recs: &'t Records<'t>,
    /// In file units.
    tol: f64,
    angle_scale: f64,
    /// Set by `surface` for the analytic surfaces of revolution, read by
    /// `face` when a loop has to be split.
    revolved: std::cell::Cell<Option<Revolved>>,
    /// Set by `surface` for every analytic surface, read by `shell` for the
    /// edge lift.
    analytic: std::cell::Cell<Option<Analytic>>,
    vertices: Vec<Point3>,
    vmap: HashMap<u64, usize>,
    edges: Vec<CompressedEdge<Curve>>,
    /// `EDGE_CURVE` id → the indices of its sub-edges, start to end.
    emap: HashMap<u64, Vec<usize>>,
    split_edges: usize,
    dropped_edges: usize,
    lifted_edges: usize,
}

/// A face that could not be converted: its index in the shell and why.
struct FaceFailure {
    index: usize,
    entity: u64,
    reason: String,
}

impl<'t> ShellBuilder<'t> {
    fn new(table: &'t Table, recs: &'t Records<'t>, tol: f64, angle_scale: f64) -> Self {
        ShellBuilder {
            table,
            recs,
            tol,
            angle_scale,
            revolved: std::cell::Cell::new(None),
            analytic: std::cell::Cell::new(None),
            vertices: Vec::new(),
            vmap: HashMap::new(),
            edges: Vec::new(),
            emap: HashMap::new(),
            split_edges: 0,
            dropped_edges: 0,
            lifted_edges: 0,
        }
    }

    fn solid(&mut self, sr: &SolidRef) -> Result<ImportedSolid, String> {
        let (mut outer, faces, flipped, mut seam_faces) = self.shell(sr.outer)?;
        let mut shells = vec![];
        let mut voids = 0;
        for (vid, orientation) in &sr.voids {
            let (mut v, _, _, seams) = self.shell(*vid).map_err(|e| format!("void shell #{vid}: {e}"))?;
            seam_faces += seams;
            if !orientation {
                for f in v.iter_mut() {
                    f.invert();
                }
            }
            shells.push(v);
            voids += 1;
        }
        // Outward, by the signed volume of a coarse tessellation (the sign
        // is all that is read). A void's faces point into the void, so they
        // subtract, which is what makes this the material's volume.
        let mut inverted = false;
        let probe = Solid::new_unchecked(std::iter::once(outer.clone()).chain(shells.iter().cloned()).collect());
        // The builder's vertex table was moved into the shell; read the
        // extent off the shell itself.
        let pts: Vec<Point3> = outer.vertex_iter().map(|v| v.point()).collect();
        let diag = bbox_diagonal(&pts).max(1e-6);
        let volume = signed_volume(&probe, (diag * 0.01).clamp(1e-4, 0.5))
            .map_err(|e| format!("the shell could not be tessellated: {e}"))?;
        if volume < 0.0 {
            inverted = true;
            for f in outer.iter_mut() {
                f.invert();
            }
            for s in shells.iter_mut() {
                for f in s.iter_mut() {
                    f.invert();
                }
            }
        }
        let mut all = vec![outer];
        all.extend(shells);
        let solid = Solid::try_new(all).map_err(|e| format!("not a solid: {e}"))?;
        Ok(ImportedSolid {
            solid,
            entity: sr.entity,
            kind: sr.kind,
            label: String::new(),
            raw_label: String::new(),
            faces,
            flipped_faces: flipped,
            inverted,
            split_edges: self.split_edges,
            dropped_edges: self.dropped_edges,
            lifted_edges: self.lifted_edges,
            seam_faces,
            voids,
        })
    }

    /// One `CLOSED_SHELL` as a closed, consistently oriented truck shell,
    /// with its face labels in file order, how many faces were turned and
    /// how many loops touch themselves at a seam.
    fn shell(&mut self, shell_id: u64) -> Result<(Shell, Vec<FaceLabel>, usize, usize), String> {
        let holder = self.table.shell.get(&shell_id).ok_or_else(|| {
            format!(
                "#{shell_id} is not a CLOSED_SHELL the reader could load ({})",
                self.recs.name_of(shell_id).unwrap_or("no such entity")
            )
        })?;
        let face_refs: Vec<PlaceHolder<FaceAnyHolder>> = holder.cfs_faces.clone();
        if face_refs.is_empty() {
            return Err(format!("CLOSED_SHELL #{shell_id} has no faces"));
        }
        let mut cfaces: Vec<CompressedFace<Surface>> = Vec::with_capacity(face_refs.len());
        let mut analytics: Vec<Option<Analytic>> = Vec::with_capacity(face_refs.len());
        let mut labels = Vec::with_capacity(face_refs.len());
        let mut failures: Vec<FaceFailure> = Vec::new();
        let mut split_faces = 0usize;
        for (index, fr) in face_refs.iter().enumerate() {
            let (entity, oriented, fh) = match self.face_holder(fr) {
                Ok(x) => x,
                Err(e) => {
                    failures.push(FaceFailure { index, entity: 0, reason: e });
                    continue;
                }
            };
            match self.face(&fh, oriented) {
                Ok(pieces) => {
                    if pieces.len() > 1 {
                        split_faces += 1;
                    }
                    let an = self.analytic.take();
                    for (piece, cf) in pieces.into_iter().enumerate() {
                        labels.push(FaceLabel { entity, index, piece, label: String::new(), raw: fh.label.clone() });
                        cfaces.push(cf);
                        analytics.push(an);
                    }
                }
                Err(reason) => failures.push(FaceFailure { index, entity, reason }),
            }
        }
        if !failures.is_empty() {
            let shown: Vec<String> = failures
                .iter()
                .take(8)
                .map(|f| format!("face[{}] (#{}): {}", f.index, f.entity, f.reason))
                .collect();
            let more = failures.len().saturating_sub(8);
            return Err(format!(
                "{} of {} faces could not be converted — a shell with a face missing is a hole, so the whole \
                 solid is refused: {}{}",
                failures.len(),
                face_refs.len(),
                shown.join("; "),
                if more > 0 { format!("; and {more} more") } else { String::new() }
            ));
        }
        self.lift_edges_onto_faces(&cfaces, &analytics);
        // Names: the file's, when non-empty and unique among the file's
        // faces; the second piece of a split face carries `#1`.
        let names: Vec<String> = labels.iter().map(|l| label::sanitise(&l.raw)).collect();
        let mut owners: HashMap<&str, HashSet<usize>> = HashMap::new();
        for (n, l) in names.iter().zip(&labels) {
            owners.entry(n.as_str()).or_default().insert(l.index);
        }
        for (l, n) in labels.iter_mut().zip(names.iter()) {
            l.label = if !n.is_empty() && owners[n.as_str()].len() == 1 {
                if l.piece == 0 { n.clone() } else { format!("{n}#{}", l.piece) }
            } else {
                String::new()
            };
        }

        let cshell = CompressedShell {
            vertices: std::mem::take(&mut self.vertices),
            edges: std::mem::take(&mut self.edges),
            faces: cfaces,
        };
        // The builder's tables are per shell: a void shares no vertex with
        // the outer shell in the file either.
        self.vmap.clear();
        self.emap.clear();
        let mut shell = Shell::extract(cshell).map_err(|e| format!("the shell's topology is not well formed: {e}"))?;
        let flipped = if shell.shell_condition() == ShellCondition::Closed {
            0
        } else {
            let n = orient_by_adjacency(&mut shell)?;
            match shell.shell_condition() {
                ShellCondition::Closed => n,
                cond => {
                    let open = shell.extract_boundaries();
                    let edges: usize = open.iter().map(|w| w.len()).sum();
                    return Err(format!(
                        "the shell is not closed after orienting its faces ({cond:?}): {} boundary edge{} in {} \
                         loop{} — the file's CLOSED_SHELL is open, or one face's loop runs the wrong way",
                        edges,
                        if edges == 1 { "" } else { "s" },
                        open.len(),
                        if open.len() == 1 { "" } else { "s" }
                    ));
                }
            }
        };
        Ok((shell, labels, flipped, split_faces))
    }

    /// An edge a file writes is the curve its author had, and for the edge
    /// a boolean made that is a *fit*: Cadpole's own writer refits every
    /// intersection curve as a cubic B-spline within its tolerance, so the
    /// rim of a bored hole sits a few 1e-5 mm off the cylinder it bounds.
    /// truck maps a face's boundary into the surface's parameter space to a
    /// tolerance of 1e-6 and drops a face whose boundary is not on it — and
    /// Cadpole's own booleans never hit that, because their edges are
    /// `IntersectionCurve`s, evaluated by projecting a leader onto *both*
    /// surfaces. So every edge shared by two *analytic* faces (the planes,
    /// cylinders, cones, spheres and tori this reader built itself, whose
    /// distance to a point is a formula and whose parameter search truck
    /// gets right) that measures off either surface is lifted into that
    /// form, its file curve as the leader; an edge that lies on both (a line
    /// on two planes, an arc on the cylinder it came from) is left as it
    /// is, so a Cadpole primitive still re-imports to the identical
    /// tessellation. An edge whose two faces have the same normal there —
    /// the meridian a seam split adds between two halves of one cylinder —
    /// is left too: the projection onto "both" surfaces is singular. And an
    /// edge on a free-form face is never lifted: truck's parameter search on
    /// a B-spline surface can fail, and an `IntersectionCurve` whose
    /// projection fails is a panic inside the tessellator, not a
    /// diagnostic.
    fn lift_edges_onto_faces(&mut self, cfaces: &[CompressedFace<Surface>], analytics: &[Option<Analytic>]) {
        let mut uses: HashMap<usize, Vec<usize>> = HashMap::new();
        for (k, cf) in cfaces.iter().enumerate() {
            for l in &cf.boundaries {
                for e in l {
                    let v = uses.entry(e.index).or_default();
                    if !v.contains(&k) {
                        v.push(k);
                    }
                }
            }
        }
        let diag = bbox_diagonal(&self.vertices);
        let on_surface = 1e-9 * (1.0 + diag);
        let off = |k: usize, p: Point3| -> bool {
            match analytics[k] {
                Some(a) => a.distance(p) > on_surface,
                None => false,
            }
        };
        for (ei, faces) in uses {
            let [a, b] = faces[..] else { continue };
            if analytics[a].is_none() || analytics[b].is_none() {
                continue;
            }
            if matches!(self.edges[ei].curve, Curve::IntersectionCurve(_)) {
                continue;
            }
            let pts = samples(&self.edges[ei].curve, 9);
            let (sa, sb) = (&cfaces[a].surface, &cfaces[b].surface);
            if !pts.iter().any(|&p| off(a, p) || off(b, p)) {
                continue;
            }
            let normal_at = |s: &Surface, p: Point3| s.search_nearest_parameter(p, None, 100).map(|(u, v)| s.normal(u, v));
            let mid = pts[pts.len() / 2];
            match (normal_at(sa, mid), normal_at(sb, mid)) {
                (Some(na), Some(nb)) if na.cross(nb).magnitude() > 1e-6 => {}
                _ => continue,
            }
            let leader = match &self.edges[ei].curve {
                Curve::Line(l) => Leader::BSpline(l.to_bspline()),
                Curve::BSplineCurve(c) => Leader::BSpline(c.clone()),
                Curve::NurbsCurve(_) => Leader::Polyline(PolylineCurve(samples(&self.edges[ei].curve, 64))),
                Curve::IntersectionCurve(_) => continue,
            };
            self.edges[ei].curve = Curve::IntersectionCurve(IntersectionCurve::new_unchecked(
                Box::new(sa.clone()),
                Box::new(sb.clone()),
                leader,
                self.tol,
            ));
            self.lifted_edges += 1;
        }
    }

    fn face_holder(&self, fr: &PlaceHolder<FaceAnyHolder>) -> Result<(u64, bool, FaceSurfaceHolder), String> {
        match fr {
            PlaceHolder::Ref(Name::Entity(id)) => {
                if let Some(of) = self.table.oriented_face.get(id) {
                    let inner = match &of.face_element {
                        PlaceHolder::Ref(Name::Entity(fid)) => self
                            .table
                            .face_surface
                            .get(fid)
                            .cloned()
                            .ok_or_else(|| format!("ORIENTED_FACE #{id} refers to #{fid}, which is not a face"))?,
                        PlaceHolder::Owned(h) => h.clone(),
                        _ => return Err(format!("ORIENTED_FACE #{id} has no face element")),
                    };
                    return Ok((*id, of.orientation, inner));
                }
                if let Some(f) = self.table.face_surface.get(id) {
                    return Ok((*id, true, f.clone()));
                }
                Err(format!(
                    "#{id} is not a face the reader could load ({})",
                    self.recs.name_of(*id).unwrap_or("no such entity")
                ))
            }
            PlaceHolder::Owned(FaceAnyHolder::FaceSurface(h)) => Ok((0, true, h.clone())),
            PlaceHolder::Owned(FaceAnyHolder::OrientedFace(of)) => match &of.face_element {
                PlaceHolder::Owned(h) => Ok((0, of.orientation, h.clone())),
                PlaceHolder::Ref(Name::Entity(fid)) => Ok((
                    0,
                    of.orientation,
                    self.table.face_surface.get(fid).cloned().ok_or_else(|| format!("#{fid} is not a face"))?,
                )),
                _ => Err("an oriented face has no face element".into()),
            },
            _ => Err("a shell face is neither a reference nor an inline face".into()),
        }
    }

    /// One face: its loops as sub-edge indices, its surface converted and
    /// oriented as `same_sense` says. Two faces when a loop touches itself
    /// at a seam ([`ShellBuilder::split_at_meridian`]).
    fn face(&mut self, fh: &FaceSurfaceHolder, oriented: bool) -> Result<Vec<CompressedFace<Surface>>, String> {
        let mut boundaries: Vec<Vec<CompressedEdgeIndex>> = Vec::with_capacity(fh.bounds.len());
        let mut boundary = Boundary { edges: Vec::new() };
        for (bi, bref) in fh.bounds.iter().enumerate() {
            let (bid, bh) = self.bound_holder(bref)?;
            let (loop_id, edge_refs) = match &bh.bound {
                PlaceHolder::Ref(Name::Entity(lid)) => (
                    *lid,
                    self.table
                        .edge_loop
                        .get(lid)
                        .map(|l| l.edge_list.clone())
                        .ok_or_else(|| format!("bound #{bid} refers to #{lid}, which is not an EDGE_LOOP"))?,
                ),
                PlaceHolder::Owned(l) => (0, l.edge_list.clone()),
                _ => return Err(format!("bound #{bid} has no loop")),
            };
            let mut list: Vec<CompressedEdgeIndex> = Vec::new();
            for er in &edge_refs {
                let (eid, orientation) = self.edge_ref(er)?;
                let forward = orientation == bh.orientation;
                let subs = self.edge(eid)?;
                if forward {
                    list.extend(subs.iter().map(|&i| CompressedEdgeIndex { index: i, orientation: true }));
                } else {
                    list.extend(subs.iter().rev().map(|&i| CompressedEdgeIndex { index: i, orientation: false }));
                }
                for &i in &subs {
                    let mut pts = samples(&self.edges[i].curve, 8);
                    if !forward {
                        pts.reverse();
                    }
                    boundary.edges.push(pts);
                }
                if !bh.orientation {
                    // The loop as a whole runs the other way: the edges'
                    // order reverses with their directions.
                    let n = subs.len();
                    let len = boundary.edges.len();
                    boundary.edges[len - n..].reverse();
                }
            }
            if !bh.orientation {
                list.reverse();
            }
            if list.is_empty() {
                return Err(format!("loop {bi} (#{loop_id}) has no edges"));
            }
            // Closed, and each edge takes off where the last landed.
            let ends = |e: &CompressedEdgeIndex| {
                let (a, b) = self.edges[e.index].vertices;
                if e.orientation { (a, b) } else { (b, a) }
            };
            for w in list.windows(2) {
                if ends(&w[0]).1 != ends(&w[1]).0 {
                    return Err(format!("loop {bi} (#{loop_id}) is not a chain: consecutive edges do not meet"));
                }
            }
            if ends(list.last().unwrap()).1 != ends(&list[0]).0 {
                return Err(format!("loop {bi} (#{loop_id}) does not close"));
            }
            boundaries.push(list);
        }
        if boundaries.is_empty() {
            return Err("the face has no bounds".into());
        }
        let mut surface = self.surface(&fh.face_geometry, &boundary, fh.same_sense)?;
        let revolved = self.revolved.get();
        if !fh.same_sense {
            surface.invert();
        }
        if boundaries.iter().all(|l| self.is_simple(l)) {
            return Ok(vec![CompressedFace { boundaries, orientation: oriented, surface }]);
        }
        let rev = revolved.ok_or_else(|| {
            "the loop touches itself (a seam edge walked both ways), and only a cylinder, cone, sphere or torus \
             face can be split at a meridian here"
                .to_string()
        })?;
        let (a, b) = self.split_at_meridian(boundaries, &rev)?;
        Ok(vec![
            CompressedFace { boundaries: a, orientation: oriented, surface: surface.clone() },
            CompressedFace { boundaries: b, orientation: oriented, surface },
        ])
    }

    /// Start and end vertex of a loop entry.
    fn ends(&self, e: &CompressedEdgeIndex) -> (usize, usize) {
        let (a, b) = self.edges[e.index].vertices;
        if e.orientation { (a, b) } else { (b, a) }
    }

    /// No vertex visited twice: what truck asks of a face's wire.
    fn is_simple(&self, l: &[CompressedEdgeIndex]) -> bool {
        let mut seen = HashSet::with_capacity(l.len());
        l.iter().all(|e| seen.insert(self.ends(e).0))
    }

    /// Split a face whose loop touches itself — a full cylinder, cone,
    /// sphere band or torus written with one seam edge, which the loop
    /// walks up on one side and down on the other — into two faces along
    /// a meridian through two of its vertices, the way Cadpole's own primitives
    /// never write such a face (a full turn is several faces). The
    /// meridian is chosen among the loop's vertices off the seam so that no
    /// edge crosses it anywhere but at a vertex; a new edge on the surface
    /// joins the two, and the holes go to whichever side holds them.
    fn split_at_meridian(
        &mut self,
        loops: Vec<Vec<CompressedEdgeIndex>>,
        rev: &Revolved,
    ) -> Result<(Vec<Vec<CompressedEdgeIndex>>, Vec<Vec<CompressedEdgeIndex>>), String> {
        let f = rev.frame;
        // Angle about the axis from the seam, in [0, 2π).
        let angle = |p: Point3| {
            let l = f.local(p);
            ((l.x * l.x + l.y * l.y).sqrt() > 1e-9).then(|| (fmath::atan2(l.y, l.x) - rev.seam).rem_euclid(TAU))
        };
        let signed = |a: f64, c: f64| {
            let d = (a - c).rem_euclid(TAU);
            if d > PI { d - TAU } else { d }
        };
        let pinched = loops
            .iter()
            .position(|l| !self.is_simple(l))
            .ok_or_else(|| "no loop touches itself".to_string())?;
        let outer = loops[pinched].clone();
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for e in &outer {
            *counts.entry(self.ends(e).0).or_default() += 1;
        }
        // Candidates: the loop's once-visited vertices off the seam.
        let mut candidates: Vec<f64> = Vec::new();
        for e in &outer {
            let v = self.ends(e).0;
            if counts[&v] != 1 {
                continue;
            }
            if let Some(a) = angle(self.vertices[v])
                && angular_gap(a, 0.0) > 1e-6
                && !candidates.iter().any(|c| angular_gap(*c, a) < 1e-9)
            {
                candidates.push(a);
            }
        }
        let mut chosen: Option<(f64, Vec<usize>)> = None;
        'candidates: for &phic in &candidates {
            let mut on_cut: Vec<usize> = Vec::new();
            for (li, l) in loops.iter().enumerate() {
                for e in l {
                    let (v0, _) = self.ends(e);
                    let on = angle(self.vertices[v0]).is_some_and(|a| angular_gap(a, phic) < 1e-6);
                    if on {
                        if li != pinched {
                            continue 'candidates; // a hole's vertex on the cut
                        }
                        on_cut.push(v0);
                    }
                    // No crossing away from a vertex.
                    let pts = samples(&self.edges[e.index].curve, 16);
                    let ds: Vec<Option<f64>> = pts.iter().map(|&p| angle(p).map(|a| signed(a, phic))).collect();
                    for (k, w) in ds.windows(2).enumerate() {
                        if let (Some(d0), Some(d1)) = (w[0], w[1]) {
                            if d0 * d1 < 0.0 && d0.abs() < PI / 2.0 && d1.abs() < PI / 2.0 {
                                continue 'candidates;
                            }
                            if k > 0 && d0.abs() < 1e-9 {
                                continue 'candidates; // through the cut at an interior sample
                            }
                        }
                    }
                }
            }
            if on_cut.len() == 2 {
                chosen = Some((phic, on_cut));
                break;
            }
        }
        let Some((phic, on_cut)) = chosen else {
            return Err(
                "the loop touches itself at its seam and no meridian through its vertices cuts the face cleanly \
                 (split the face in the source, or export it as two faces)"
                    .into(),
            );
        };
        let (va, vb) = (on_cut[0], on_cut[1]);
        let (pa, pb) = (self.vertices[va], self.vertices[vb]);
        let curve = meridian_curve(rev, phic, pa, pb);
        self.edges.push(CompressedEdge { vertices: (va, vb), curve });
        let ne = self.edges.len() - 1;
        // The loop from va round to vb, and from vb round to va.
        let n = outer.len();
        let start = (0..n).find(|&i| self.ends(&outer[i]).0 == va).expect("va is on the loop");
        let mut p1 = Vec::new();
        let mut i = start;
        loop {
            p1.push(outer[i]);
            i = (i + 1) % n;
            if self.ends(&outer[i]).0 == vb {
                break;
            }
            if i == start {
                return Err("the seam split lost its way round the loop".into());
            }
        }
        let mut p2 = Vec::new();
        let mut j = i;
        loop {
            p2.push(outer[j]);
            j = (j + 1) % n;
            if self.ends(&outer[j]).0 == va {
                break;
            }
            if j == i {
                return Err("the seam split lost its way round the loop".into());
            }
        }
        p1.push(CompressedEdgeIndex { index: ne, orientation: false });
        p2.push(CompressedEdgeIndex { index: ne, orientation: true });
        // Which angular side each half covers, from a point inside its
        // first edge; a hole goes with the side its own first edge is on.
        let side = |l: &[CompressedEdgeIndex]| -> Option<bool> {
            let c = &self.edges[l[0].index].curve;
            let (t0, t1) = c.range_tuple();
            angle(c.subs((t0 + t1) / 2.0)).map(|a| a < phic)
        };
        let a_low = side(&p1).unwrap_or(true);
        let (mut fa, mut fb) = (vec![p1], vec![p2]);
        for (k, l) in loops.into_iter().enumerate() {
            if k == pinched {
                continue;
            }
            let low = side(&l).unwrap_or(a_low);
            if low == a_low { fa.push(l) } else { fb.push(l) }
        }
        Ok((fa, fb))
    }

    fn bound_holder(&self, bref: &PlaceHolder<FaceBoundHolder>) -> Result<(u64, FaceBoundHolder), String> {
        match bref {
            PlaceHolder::Ref(Name::Entity(id)) => self
                .table
                .face_bound
                .get(id)
                .cloned()
                .map(|b| (*id, b))
                .ok_or_else(|| format!("#{id} is not a FACE_BOUND the reader could load")),
            PlaceHolder::Owned(b) => Ok((0, b.clone())),
            _ => Err("a face bound is neither a reference nor inline".into()),
        }
    }

    /// `(EDGE_CURVE id, orientation)` of a loop entry.
    fn edge_ref(&self, er: &PlaceHolder<EdgeAnyHolder>) -> Result<(u64, bool), String> {
        match er {
            PlaceHolder::Ref(Name::Entity(id)) => {
                if let Some(oe) = self.table.oriented_edge.get(id) {
                    return match oe.edge_element {
                        PlaceHolder::Ref(Name::Entity(eid)) => Ok((eid, oe.orientation)),
                        _ => Err(format!("ORIENTED_EDGE #{id} has an inline edge, which is not supported")),
                    };
                }
                if self.table.edge_curve.contains_key(id) {
                    return Ok((*id, true));
                }
                Err(format!(
                    "#{id} is not an edge the reader could load ({})",
                    self.recs.name_of(*id).unwrap_or("no such entity")
                ))
            }
            _ => Err("an inline edge in a loop is not supported".into()),
        }
    }

    fn vertex(&mut self, vref: &PlaceHolder<VertexPointHolder>, edge: u64) -> Result<usize, String> {
        let id = match vref {
            PlaceHolder::Ref(Name::Entity(id)) => *id,
            _ => return Err(format!("EDGE_CURVE #{edge} has an inline vertex, which is not supported")),
        };
        if let Some(&i) = self.vmap.get(&id) {
            return Ok(i);
        }
        let vp = EntityTable::<VertexPointHolder>::get_owned(self.table, id)
            .map_err(|e| format!("VERTEX_POINT #{id}: {}", short(&e.to_string())))?;
        let c = &vp.vertex_geometry.coordinates;
        if c.len() < 2 || c.len() > 3 || c.iter().any(|v| !v.is_finite()) {
            return Err(format!("VERTEX_POINT #{id} has {} coordinates, or a non-finite one", c.len()));
        }
        let p = Point3::new(c[0], c[1], c.get(2).copied().unwrap_or(0.0));
        let i = self.vertices.len();
        self.vertices.push(p);
        self.vmap.insert(id, i);
        Ok(i)
    }

    /// The sub-edges of `EDGE_CURVE` `id`, start to end, converting it on
    /// first use.
    fn edge(&mut self, id: u64) -> Result<Vec<usize>, String> {
        if let Some(v) = self.emap.get(&id) {
            return Ok(v.clone());
        }
        let holder: EdgeCurveHolder =
            self.table.edge_curve.get(&id).cloned().ok_or_else(|| format!("#{id} is not an EDGE_CURVE"))?;
        let v0 = self.vertex(&holder.edge_start, id)?;
        let v1 = self.vertex(&holder.edge_end, id)?;
        let owned = catch_unwind(AssertUnwindSafe(|| EntityTable::<EdgeCurveHolder>::get_owned(self.table, id)))
            .map_err(|_| format!("EDGE_CURVE #{id}: its curve could not be read"))?
            .map_err(|e| format!("EDGE_CURVE #{id}: {}", short(&e.to_string())))?;
        let curve3d = catch_unwind(AssertUnwindSafe(|| owned.parse_curve3d()))
            .map_err(|_| format!("EDGE_CURVE #{id}: its curve could not be converted"))?
            .map_err(|e| format!("EDGE_CURVE #{id}: {}", short(&e.to_string())))?;
        let curve = curve_of(&curve3d).map_err(|e| format!("EDGE_CURVE #{id}: {e}"))?;
        let p0 = self.vertices[v0];
        let p1 = self.vertices[v1];
        let mut subs = Vec::new();
        if v0 == v1 {
            // One vertex at both ends: a full circle (split in two) or a
            // zero-length edge on a pole (dropped).
            let len = curve_length(&curve, 16);
            if len <= self.tol * 4.0 {
                self.dropped_edges += 1;
                self.emap.insert(id, Vec::new());
                return Ok(Vec::new());
            }
            let (t0, t1) = curve.range_tuple();
            // A full circle: two of truck's arcs, so each half is the curve
            // Cadpole's own writer would have given it; anything else is cut
            // at its parametric middle.
            let (head, tail, mid) = match &curve3d {
                Curve3D::Conic(Conic3D::Ellipse(proc)) if circle_of(proc).is_some() => {
                    let (c, a, b, t0, t1) = circle_of(proc).expect("checked");
                    let pt = |t: f64| c + a * fmath::cos(t) + b * fmath::sin(t);
                    let tm = (t0 + t1) / 2.0;
                    let (mut head, mut tail) =
                        (truck_arc(pt(t0), pt(tm), pt((t0 + tm) / 2.0)), truck_arc(pt(tm), pt(t1), pt((tm + t1) / 2.0)));
                    if !proc.orientation() {
                        head.invert();
                        tail.invert();
                        std::mem::swap(&mut head, &mut tail);
                    }
                    let mid = head.subs(head.range_tuple().1);
                    (head, tail, mid)
                }
                _ => {
                    let mut head = curve;
                    let tail = head.cut((t0 + t1) / 2.0);
                    let mid = head.subs(head.range_tuple().1);
                    (head, tail, mid)
                }
            };
            let vm = self.vertices.len();
            self.vertices.push(mid);
            self.edges.push(CompressedEdge { vertices: (v0, vm), curve: head });
            subs.push(self.edges.len() - 1);
            self.edges.push(CompressedEdge { vertices: (vm, v1), curve: tail });
            subs.push(self.edges.len() - 1);
            self.split_edges += 1;
        } else {
            let (t0, t1) = curve.range_tuple();
            let (a, b) = (curve.subs(t0), curve.subs(t1));
            let slack = (self.tol * 100.0).max(1e-6 * (p0.distance(p1) + 1.0));
            if a.distance(p0) > slack || b.distance(p1) > slack {
                return Err(format!(
                    "EDGE_CURVE #{id}: its curve runs from ({:.4}, {:.4}, {:.4}) to ({:.4}, {:.4}, {:.4}) but its \
                     vertices are at ({:.4}, {:.4}, {:.4}) and ({:.4}, {:.4}, {:.4})",
                    a.x, a.y, a.z, b.x, b.y, b.z, p0.x, p0.y, p0.z, p1.x, p1.y, p1.z
                ));
            }
            self.edges.push(CompressedEdge { vertices: (v0, v1), curve });
            subs.push(self.edges.len() - 1);
        }
        self.emap.insert(id, subs.clone());
        Ok(subs)
    }

    /// A placement frame, checked: the axis non-zero, the reference
    /// direction not parallel to it (the crate's own conversion divides by
    /// zero there — its issues #123/#128).
    fn placement(&self, id: u64) -> Result<Frame, String> {
        let h: &Axis2Placement3dHolder = self
            .table
            .axis2_placement_3d
            .get(&id)
            .ok_or_else(|| format!("#{id} is not an AXIS2_PLACEMENT_3D the reader could load"))?;
        let point = |p: &PlaceHolder<CartesianPointHolder>| -> Result<Point3, String> {
            match p {
                PlaceHolder::Ref(Name::Entity(pid)) => {
                    let c = &self
                        .table
                        .cartesian_point
                        .get(pid)
                        .ok_or_else(|| format!("AXIS2_PLACEMENT_3D #{id}: #{pid} is not a CARTESIAN_POINT"))?
                        .coordinates;
                    if c.len() != 3 || c.iter().any(|v| !v.is_finite()) {
                        return Err(format!("CARTESIAN_POINT #{pid} is not a finite 3D point"));
                    }
                    Ok(Point3::new(c[0], c[1], c[2]))
                }
                PlaceHolder::Owned(cp) => {
                    let c = &cp.coordinates;
                    if c.len() != 3 || c.iter().any(|v| !v.is_finite()) {
                        return Err(format!("AXIS2_PLACEMENT_3D #{id} has a non-finite location"));
                    }
                    Ok(Point3::new(c[0], c[1], c[2]))
                }
                _ => Err(format!("AXIS2_PLACEMENT_3D #{id} has no location")),
            }
        };
        let dir = |d: &Option<PlaceHolder<DirectionHolder>>| -> Result<Option<Vector3>, String> {
            let Some(d) = d else { return Ok(None) };
            let r = match d {
                PlaceHolder::Ref(Name::Entity(did)) => self
                    .table
                    .direction
                    .get(did)
                    .ok_or_else(|| format!("AXIS2_PLACEMENT_3D #{id}: #{did} is not a DIRECTION"))?
                    .direction_ratios
                    .clone(),
                PlaceHolder::Owned(dh) => dh.direction_ratios.clone(),
                _ => return Ok(None),
            };
            if r.len() != 3 || r.iter().any(|v| !v.is_finite()) {
                return Err(format!("AXIS2_PLACEMENT_3D #{id} has a direction that is not a finite 3-vector"));
            }
            let v = Vector3::new(r[0], r[1], r[2]);
            if v.magnitude() < 1e-12 {
                return Err(format!("AXIS2_PLACEMENT_3D #{id} has a zero direction"));
            }
            Ok(Some(v.normalize()))
        };
        let o = point(&h.location)?;
        let z = dir(&h.axis)?.unwrap_or(Vector3::unit_z());
        let xr = dir(&h.ref_direction)?.unwrap_or_else(|| perpendicular(z));
        let x = xr - z * xr.dot(z);
        if x.magnitude() < 1e-9 {
            return Err(format!(
                "AXIS2_PLACEMENT_3D #{id}: ref_direction is parallel to axis, so the frame has no x direction"
            ));
        }
        let x = x.normalize();
        let y = z.cross(x);
        Ok(Frame { o, x, y, z })
    }

    /// The face's surface, with the STEP entity's own natural orientation.
    fn surface(&self, sref: &PlaceHolder<SurfaceAnyHolder>, boundary: &Boundary, same_sense: bool) -> Result<Surface, String> {
        let sid = match sref {
            PlaceHolder::Ref(Name::Entity(id)) => Some(*id),
            _ => None,
        };
        let name = sid.and_then(|id| self.recs.name_of(id)).unwrap_or("");
        let margin = self.tol * 10.0;
        self.revolved.set(None);
        self.analytic.set(None);
        match (sid, name) {
            (Some(id), "PLANE") => {
                let f = self.frame_param(id, 1)?;
                self.analytic.set(Some(Analytic::Plane { o: f.o, n: f.z }));
                Ok(Surface::Plane(Plane::new(f.o, f.o + f.x, f.o + f.y)))
            }
            (Some(id), "CYLINDRICAL_SURFACE") => {
                let f = self.frame_param(id, 1)?;
                let r = self.real_param(id, 2, "radius")?;
                if r <= 0.0 {
                    return Err(format!("CYLINDRICAL_SURFACE #{id}: radius {r} is not positive"));
                }
                let phi = boundary.seam(&f);
                let (h0, h1) = boundary.extent(&f, margin);
                let e1 = f.radial(phi);
                self.revolved.set(Some(Revolved { frame: f, seam: phi, kind: RevolvedKind::Ruled }));
                self.analytic.set(Some(Analytic::Cylinder { f, r }));
                let line = Line(f.o + e1 * r + f.z * h0, f.o + e1 * r + f.z * h1);
                let s = Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(Curve::Line(line), f.o, f.z)));
                Ok(oriented_like(s, |p| {
                    let l = f.local(p);
                    f.x * l.x + f.y * l.y
                }))
            }
            (Some(id), "CONICAL_SURFACE") => {
                let f = self.frame_param(id, 1)?;
                let r = self.real_param(id, 2, "radius")?;
                let alpha = self.real_param(id, 3, "semi_angle")? * self.angle_scale;
                if r < 0.0 || !(alpha.abs() > 1e-9 && alpha.abs() < PI / 2.0 - 1e-9) {
                    return Err(format!("CONICAL_SURFACE #{id}: radius {r}, semi-angle {alpha} rad is not a cone"));
                }
                let phi = boundary.seam(&f);
                let (h0, h1) = boundary.extent(&f, margin);
                let e1 = f.radial(phi);
                self.revolved.set(Some(Revolved { frame: f, seam: phi, kind: RevolvedKind::Ruled }));
                self.analytic.set(Some(Analytic::Cone { f, r, alpha }));
                let at = |h: f64| f.o + e1 * (r + h * fmath::tan(alpha)) + f.z * h;
                let line = Line(at(h0), at(h1));
                let s = Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(Curve::Line(line), f.o, f.z)));
                let (sa, ca) = fmath::sin_cos(alpha);
                Ok(oriented_like(s, |p| {
                    let l = f.local(p);
                    let rad = f.x * l.x + f.y * l.y;
                    let rad = if rad.magnitude() > 1e-12 { rad.normalize() } else { f.x };
                    rad * ca - f.z * sa
                }))
            }
            (Some(id), "SPHERICAL_SURFACE") => {
                let f = self.frame_param(id, 1)?;
                let r = self.real_param(id, 2, "radius")?;
                if r <= 0.0 {
                    return Err(format!("SPHERICAL_SURFACE #{id}: radius {r} is not positive"));
                }
                let phi = boundary.seam(&f);
                let e1 = f.radial(phi);
                // Latitude of a point, -π/2 at the south pole.
                let lat = |p: Point3| {
                    let l = f.local(p);
                    fmath::atan2(l.z, (l.x * l.x + l.y * l.y).sqrt())
                };
                let sign = if same_sense { 1.0 } else { -1.0 };
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                for p in boundary.all() {
                    let l = lat(p);
                    lo = lo.min(l);
                    hi = hi.max(l);
                }
                // Which pole the face holds: the loop's left is its inside.
                let north = boundary
                    .interior_sign(
                        |p| (p - f.o) * sign,
                        |p| {
                            let l = f.local(p);
                            let rad = f.x * l.x + f.y * l.y;
                            let rad = if rad.magnitude() > 1e-12 { rad.normalize() } else { f.x };
                            let t = lat(p);
                            f.z * fmath::cos(t) - rad * fmath::sin(t)
                        },
                    )
                    .map(|s| s > 0.0);
                self.revolved.set(Some(Revolved { frame: f, seam: phi, kind: RevolvedKind::Sphere { r } }));
                self.analytic.set(Some(Analytic::Sphere { f, r }));
                let at = |t: f64| f.o + e1 * (r * fmath::cos(t)) + f.z * (r * fmath::sin(t));
                let meridian = match north {
                    // The meridian from the boundary's own latitude to that
                    // pole, as Cadpole's caps are built (a quarter arc from the
                    // equator), with the pole interior to the face.
                    Some(true) if lo.is_finite() && lo < PI / 2.0 - 1e-6 => {
                        let a = (lo - ARC_MARGIN).max(-PI / 2.0 + 1e-9);
                        truck_arc(at(a), at(PI / 2.0), at((a + PI / 2.0) / 2.0))
                    }
                    Some(false) if hi.is_finite() && hi > -PI / 2.0 + 1e-6 => {
                        let a = (hi + ARC_MARGIN).min(PI / 2.0 - 1e-9);
                        truck_arc(at(a), at(-PI / 2.0), at((a - PI / 2.0) / 2.0))
                    }
                    // No usable boundary: the whole meridian, pole to pole.
                    _ => Curve::NurbsCurve(conic_arc(f.o, e1 * r, f.z * r, -PI / 2.0, PI / 2.0)),
                };
                let s = Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(meridian, f.o, f.z)));
                Ok(oriented_like(s, |p| p - f.o))
            }
            (Some(id), "TOROIDAL_SURFACE") => {
                let f = self.frame_param(id, 1)?;
                let major = self.real_param(id, 2, "major_radius")?;
                let minor = self.real_param(id, 3, "minor_radius")?;
                if major <= 0.0 || minor <= 0.0 || minor >= major {
                    return Err(format!(
                        "TOROIDAL_SURFACE #{id}: major {major}, minor {minor} is not a ring torus (a spindle or \
                         horn torus crosses its own axis)"
                    ));
                }
                let phi = boundary.seam(&f);
                let e1 = f.radial(phi);
                self.analytic.set(Some(Analytic::Torus { f, major, minor }));
                // Angle around the tube: 0 at the outermost point, π/2 at
                // the top.
                let tube_angle = |p: Point3| {
                    let l = f.local(p);
                    let rr = (l.x * l.x + l.y * l.y).sqrt() - major;
                    ((rr * rr + l.z * l.z).sqrt() > 1e-9).then(|| fmath::atan2(l.z, rr))
                };
                let sign = if same_sense { 1.0 } else { -1.0 };
                let radial_at = |p: Point3| {
                    let l = f.local(p);
                    let rad = f.x * l.x + f.y * l.y;
                    if rad.magnitude() > 1e-12 { rad.normalize() } else { f.x }
                };
                let at = |t: f64| f.o + e1 * (major + minor * fmath::cos(t)) + f.z * (minor * fmath::sin(t));
                let tube = match boundary.angle_interval(tube_angle) {
                    Some((lo, hi)) if hi - lo < TAU - 1e-6 => {
                        // The interval, or its complement: whichever side of
                        // the first edge the face is on.
                        let side = boundary.interior_sign(
                            |p| (p - (f.o + radial_at(p) * major)) * sign,
                            |p| {
                                let t = tube_angle(p).unwrap_or(0.0);
                                f.z * fmath::cos(t) - radial_at(p) * fmath::sin(t)
                            },
                        );
                        let (lo, hi) = match (boundary.first_step(), side) {
                            (Some((p0, _)), Some(s)) => {
                                let t0 = tube_angle(p0).unwrap_or(lo).rem_euclid(TAU);
                                let at_lo = angular_gap(t0, lo) < 1e-6;
                                let at_hi = angular_gap(t0, hi) < 1e-6;
                                if (at_lo && s < 0.0) || (at_hi && s > 0.0) { (hi, lo + TAU) } else { (lo, hi) }
                            }
                            _ => (lo, hi),
                        };
                        self.revolved.set(Some(Revolved {
                            frame: f,
                            seam: phi,
                            kind: RevolvedKind::Torus { major, minor, lo },
                        }));
                        // From the top down, as Cadpole draws its tube halves.
                        let (a, b) = (hi + ARC_MARGIN, lo - ARC_MARGIN);
                        truck_arc(at(a), at(b), at((a + b) / 2.0))
                    }
                    _ => {
                        let psi = boundary.tube_seam(&f, major);
                        self.revolved.set(Some(Revolved {
                            frame: f,
                            seam: phi,
                            kind: RevolvedKind::Torus { major, minor, lo: psi },
                        }));
                        Curve::NurbsCurve(conic_arc(f.o + e1 * major, e1 * minor, f.z * minor, psi, psi + TAU))
                    }
                };
                let s = Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(tube, f.o, f.z)));
                Ok(oriented_like(s, |p| {
                    let l = f.local(p);
                    let rad = f.x * l.x + f.y * l.y;
                    let rad = if rad.magnitude() > 1e-12 { rad.normalize() } else { f.x };
                    p - (f.o + rad * major)
                }))
            }
            (Some(id), n @ ("OFFSET_SURFACE" | "CURVE_BOUNDED_SURFACE" | "RECTANGULAR_TRIMMED_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE")) => {
                Err(format!("{n} #{id} is not supported"))
            }
            _ => self.surface_via_stepio(sref, sid, boundary),
        }
    }

    fn frame_param(&self, id: u64, index: usize) -> Result<Frame, String> {
        let r = self.recs.simple.get(&id).ok_or_else(|| format!("#{id} is not a simple entity"))?;
        let pid = params(r).get(index).and_then(as_ref).ok_or_else(|| format!("{} #{id} has no placement", r.name))?;
        self.placement(pid)
    }

    fn real_param(&self, id: u64, index: usize, what: &str) -> Result<f64, String> {
        let r = self.recs.simple.get(&id).ok_or_else(|| format!("#{id} is not a simple entity"))?;
        let v = params(r).get(index).and_then(as_real).ok_or_else(|| format!("{} #{id} has no {what}", r.name))?;
        if !v.is_finite() {
            return Err(format!("{} #{id}: {what} is not finite", r.name));
        }
        Ok(v)
    }

    /// B-spline, rational and swept surfaces, through the crate's typed
    /// conversion, then re-expressed in `truck_modeling`'s types.
    fn surface_via_stepio(
        &self,
        sref: &PlaceHolder<SurfaceAnyHolder>,
        sid: Option<u64>,
        boundary: &Boundary,
    ) -> Result<Surface, String> {
        let tag = sid.map(|i| format!("#{i}")).unwrap_or_else(|| "inline surface".into());
        let name = sid.and_then(|i| self.recs.name_of(i)).unwrap_or("surface").to_owned();
        let owned = catch_unwind(AssertUnwindSafe(|| sref.clone().into_owned(self.table)))
            .map_err(|_| format!("{name} {tag}: the surface could not be read"))?
            .map_err(|e| format!("{name} {tag}: {}", short(&e.to_string())))?;
        let step_surface = catch_unwind(AssertUnwindSafe(|| StepSurface::try_from(&owned)))
            .map_err(|_| format!("{name} {tag}: the surface could not be converted (a malformed knot vector?)"))?
            .map_err(|e| format!("{name} {tag}: {}", short(&e.to_string())))?;
        match step_surface {
            StepSurface::BSplineSurface(b) => Ok(Surface::BSplineSurface(*b)),
            StepSurface::NurbsSurface(n) => Ok(Surface::NurbsSurface(*n)),
            StepSurface::ElementarySurface(e) => match *e {
                // Reached only for an inline elementary surface.
                ElementarySurface::Plane(p) => Ok(Surface::Plane(p)),
                _ => Err(format!("{name} {tag}: an inline elementary surface is not supported")),
            },
            StepSurface::SweptCurve(sc) => match *sc {
                SweptCurve::RevolutedCurve(proc) => {
                    let rev = proc.entity();
                    let profile = curve_of(rev.entity_curve()).map_err(|e| format!("{name} {tag}: profile: {e}"))?;
                    let mut s = Processor::new(RevolutedCurve::by_revolution(profile, rev.origin(), rev.axis()));
                    s.transform_by(*proc.transform());
                    // ISO parameterises a surface of revolution as (angle,
                    // curve), truck as (curve, angle): the normals are
                    // opposite, which is the inversion the crate applies
                    // and Cadpole's writer undoes.
                    if !proc.orientation() {
                        s.invert();
                    }
                    Ok(Surface::RevolutedCurve(s))
                }
                SweptCurve::ExtrudedCurve(ec) => {
                    let d = ec.extruding_vector();
                    if !finite3(d) || d.magnitude() < 1e-12 {
                        return Err(format!("{name} {tag}: the extrusion vector is zero"));
                    }
                    let profile = curve_of(ec.entity_curve()).map_err(|e| format!("{name} {tag}: profile: {e}"))?;
                    let lifted = profile.clone().lift_up();
                    let uknots = lifted.knot_vec().clone();
                    // The face's reach along the vector, minus the profile's
                    // own, with a margin.
                    let dl = d.magnitude();
                    let dh = d / dl;
                    let proj = |p: Point3| (p - Point3::origin()).dot(dh);
                    let (mut pl, mut ph) = (f64::INFINITY, f64::NEG_INFINITY);
                    for p in samples(&profile, 16) {
                        pl = pl.min(proj(p));
                        ph = ph.max(proj(p));
                    }
                    let (mut bl, mut bh) = (pl, ph);
                    for p in boundary.all() {
                        bl = bl.min(proj(p));
                        bh = bh.max(proj(p));
                    }
                    let v0 = (bl - ph) / dl - 0.02 * (bh - bl).abs() / dl - self.tol * 10.0 / dl;
                    let v1 = (bh - pl) / dl + 0.02 * (bh - bl).abs() / dl + self.tol * 10.0 / dl;
                    let rows: Vec<Vec<Vector4>> = lifted
                        .control_points()
                        .iter()
                        .map(|cp| {
                            let w = cp.w;
                            let shift = |v: f64| Vector4::new(cp.x + d.x * v * w, cp.y + d.y * v * w, cp.z + d.z * v * w, w);
                            vec![shift(v0), shift(v1)]
                        })
                        .collect();
                    let vknots = KnotVec::from(vec![v0, v0, v1, v1]);
                    Ok(Surface::NurbsSurface(NurbsSurface::new(BSplineSurface::new((uknots, vknots), rows))))
                }
            },
        }
    }
}

/// `s` turned, if need be, so its normal agrees with `natural` (the STEP
/// entity's own outward direction at a point): the test is made at the
/// middle of the surface's parameter range.
fn oriented_like(mut s: Surface, natural: impl Fn(Point3) -> Vector3) -> Surface {
    let (ur, vr) = s.parameter_range();
    let mid = |r: ParameterRange| match r {
        (std::ops::Bound::Included(a) | std::ops::Bound::Excluded(a), std::ops::Bound::Included(b) | std::ops::Bound::Excluded(b)) => (a + b) / 2.0,
        _ => 0.5,
    };
    let (u, v) = (mid(ur), mid(vr));
    let p = s.subs(u, v);
    let n = s.normal(u, v);
    let want = natural(p);
    if finite3(n) && finite3(want) && n.dot(want) < 0.0 {
        s.invert();
    }
    s
}

/// A unit vector perpendicular to `z`.
fn perpendicular(z: Vector3) -> Vector3 {
    let a = if z.x.abs() < 0.9 { Vector3::unit_x() } else { Vector3::unit_y() };
    (a - z * a.dot(z)).normalize()
}

/// The crate's edge curve as a `truck_modeling::Curve`.
fn curve_of(c: &Curve3D) -> Result<Curve, String> {
    Ok(match c {
        Curve3D::Line(l) => Curve::Line(*l),
        Curve3D::BSplineCurve(b) => Curve::BSplineCurve(b.clone()),
        Curve3D::NurbsCurve(n) => Curve::NurbsCurve(n.clone()),
        Curve3D::Polyline(pl) => {
            let pts = pl.0.clone();
            if pts.len() < 2 {
                return Err("a POLYLINE with fewer than two points".into());
            }
            Curve::BSplineCurve(BSplineCurve::new(KnotVec::uniform_knot(1, pts.len() - 1), pts))
        }
        Curve3D::Conic(Conic3D::Ellipse(proc)) => {
            let (t0, t1) = proc.entity().range_tuple();
            let m = proc.transform();
            let c = Point3::from_homogeneous(m[3]);
            let a = m[0].truncate();
            let b = m[1].truncate();
            if !finite3(a) || !finite3(b) || !t0.is_finite() || !t1.is_finite() {
                return Err("a CIRCLE or ELLIPSE with a non-finite placement".into());
            }
            let mut arc = match circle_of(proc) {
                // A circular arc short of a full turn: truck's own form.
                Some((c, a, b, t0, t1)) if t1 - t0 < TAU - 1e-9 => {
                    let pt = |t: f64| c + a * fmath::cos(t) + b * fmath::sin(t);
                    truck_arc(pt(t0), pt(t1), pt((t0 + t1) / 2.0))
                }
                _ => Curve::NurbsCurve(conic_arc(c, a, b, t0, t1)),
            };
            if !proc.orientation() {
                arc.invert();
            }
            arc
        }
        Curve3D::Conic(Conic3D::Hyperbola(_)) => return Err("a HYPERBOLA edge is not supported".into()),
        Curve3D::Conic(Conic3D::Parabola(_)) => return Err("a PARABOLA edge is not supported".into()),
        Curve3D::PCurve(_) => {
            return Err("an edge defined only in surface parameter space (PCURVE) is not supported".into());
        }
    })
}

/// The meridian of a surface of revolution at angle `phic` from its seam,
/// between two points on it: a line on a cylinder or cone, an arc through
/// the middle latitude on a sphere, an arc through the middle of the tube
/// on a torus.
fn meridian_curve(rev: &Revolved, phic: f64, pa: Point3, pb: Point3) -> Curve {
    let f = rev.frame;
    let e_c = f.radial(rev.seam + phic);
    match rev.kind {
        RevolvedKind::Ruled => Curve::Line(Line(pa, pb)),
        RevolvedKind::Sphere { r } => {
            let lat = |p: Point3| {
                let l = f.local(p);
                fmath::atan2(l.z, (l.x * l.x + l.y * l.y).sqrt())
            };
            let lm = (lat(pa) + lat(pb)) / 2.0;
            truck_arc(pa, pb, f.o + e_c * (r * fmath::cos(lm)) + f.z * (r * fmath::sin(lm)))
        }
        RevolvedKind::Torus { major, minor, lo } => {
            let tube = |p: Point3| {
                let l = f.local(p);
                let rr = (l.x * l.x + l.y * l.y).sqrt() - major;
                (fmath::atan2(l.z, rr) - lo).rem_euclid(TAU) + lo
            };
            let tm = (tube(pa) + tube(pb)) / 2.0;
            truck_arc(pa, pb, f.o + e_c * (major + minor * fmath::cos(tm)) + f.z * (minor * fmath::sin(tm)))
        }
    }
}

/// Make the faces of `shell` agree along every shared edge by flipping the
/// minority, starting from face 0 and spreading over adjacency. Returns how
/// many were flipped; fails on a non-orientable shell, naming a position.
fn orient_by_adjacency(shell: &mut Shell) -> Result<usize, String> {
    let n = shell.len();
    // Per face: the ids of its boundary edges with the direction it walks
    // them, and one point on each for a diagnostic.
    let mut uses: Vec<Vec<(truck_topology::EdgeID<Curve>, bool, Point3)>> = Vec::with_capacity(n);
    for face in shell.iter() {
        let mut v = Vec::new();
        for wire in face.boundaries() {
            for e in wire.iter() {
                v.push((e.id(), e.orientation(), e.front().point()));
            }
        }
        uses.push(v);
    }
    let mut by_edge: HashMap<truck_topology::EdgeID<Curve>, Vec<(usize, bool)>> = HashMap::new();
    for (i, u) in uses.iter().enumerate() {
        for (id, o, _) in u {
            by_edge.entry(*id).or_default().push((i, *o));
        }
    }
    let mut flip: Vec<Option<bool>> = vec![None; n];
    let mut queue = std::collections::VecDeque::new();
    for start in 0..n {
        if flip[start].is_some() {
            continue;
        }
        flip[start] = Some(false);
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            let fi = flip[i].unwrap();
            for (id, oi, at) in &uses[i] {
                let dir_i = *oi ^ fi;
                for &(j, oj) in &by_edge[id] {
                    if j == i {
                        continue;
                    }
                    // Consistent: the two faces walk the edge opposite ways.
                    let need = dir_i; // flipped direction of j must equal !dir_i, i.e. oj ^ fj == !dir_i
                    let fj_needed = oj == need;
                    match flip[j] {
                        None => {
                            flip[j] = Some(fj_needed);
                            queue.push_back(j);
                        }
                        Some(fj) if fj != fj_needed => {
                            return Err(format!(
                                "the shell is not orientable at ({:.3}, {:.3}, {:.3}): faces {i} and {j} cannot both \
                                 be turned to agree with their neighbours",
                                at.x, at.y, at.z
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let mut flipped = 0;
    for (f, do_flip) in shell.iter_mut().zip(flip) {
        if do_flip == Some(true) {
            f.invert();
            flipped += 1;
        }
    }
    Ok(flipped)
}

fn bbox_diagonal(pts: &[Point3]) -> f64 {
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    if pts.is_empty() { 0.0 } else { lo.distance(hi) }
}

/// The signed volume of a tessellation of `s` at `tol`: positive when its
/// faces point out of the material.
fn signed_volume(s: &KSolid, tol: f64) -> Result<f64, String> {
    let r = catch_unwind(AssertUnwindSafe(|| cadpole_core::tessellate::tessellate_reporting(s, tol)));
    match r {
        Ok(Ok(((mesh, _), _))) => Ok(mesh.volume()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("truck failed inside the tessellation".into()),
    }
}

/// A third-party error message, control characters out and capped.
fn short(m: &str) -> String {
    m.chars().filter(|c| !c.is_control()).take(160).collect()
}
