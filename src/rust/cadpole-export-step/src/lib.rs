//! STEP writer: ISO 10303-21 part 21 text, AP214 schema, from truck B-rep
//! solids.
//!
//! This is the format the FreeCAD Gridfinity generator ships and the one a
//! Cadpole user hands to somebody running FreeCAD, Fusion, SolidWorks, Onshape or
//! Rhino. Unlike STL or 3MF it carries the *exact* surfaces — a hole stays a
//! cylinder — which is the whole reason Cadpole keeps a B-rep kernel at all.
//!
//! # Why this crate writes the file rather than calling `truck-stepio`
//!
//! `truck-stepio` 0.3 is Apache-2.0 and already in the dependency graph, so
//! licensing was never the question; coverage and correctness were. Its own
//! documentation opens with *"Shapes created by set operations cannot be
//! output yet"*, and every Cadpole body of interest is a set operation. Its
//! source has three concrete blockers:
//!
//! * its header declares `FILE_SCHEMA(('ISO-10303-042'))` — part 42 is the
//!   *geometry resource*, not an application protocol, so a reader that
//!   dispatches on the schema name has nothing to dispatch to;
//! * `impl DisplayByStep for IntersectionCurve` writes its second surface at
//!   the first surface's entity id (`src/out/geometry.rs`), so a file with any
//!   boolean edge in it has a duplicated id and a dangling reference;
//! * its `brep_with_voids` writes `ORIENTED_CLOSED_SHELL(..., .T.)`, which
//!   ISO 10303-42's `WR1` forbids outright — every void must be `.F.`.
//!
//! It is still used, in the *other* direction: the tests parse this crate's
//! output back with `truck_stepio::r#in` and compare geometry, which is a far
//! better use for it than trusting it to write.
//!
//! # What is written
//!
//! | truck | STEP |
//! | --- | --- |
//! | `Surface::Plane` | `PLANE` |
//! | a swept arc, recognised | `CYLINDRICAL_SURFACE` |
//! | a revolved line, recognised | `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE` or `PLANE` |
//! | a revolved arc, recognised | `SPHERICAL_SURFACE` or `TOROIDAL_SURFACE` |
//! | any other `RevolutedCurve` | `SURFACE_OF_REVOLUTION` |
//! | `BSplineSurface` / `NurbsSurface` | `B_SPLINE_SURFACE_WITH_KNOTS`, rational when it has weights |
//! | `Curve::Line` | `LINE` |
//! | an arc, recognised | `CIRCLE` |
//! | `BSplineCurve` / `NurbsCurve` | `B_SPLINE_CURVE_WITH_KNOTS`, rational when it has weights |
//! | a boolean's `IntersectionCurve` | a refitted `B_SPLINE_CURVE_WITH_KNOTS`, or the exact `POLYLINE` |
//! | topology | `VERTEX_POINT`, `EDGE_CURVE`, `ORIENTED_EDGE`, `EDGE_LOOP`, `FACE_OUTER_BOUND`/`FACE_BOUND`, `ADVANCED_FACE`, `CLOSED_SHELL` |
//! | a solid | `MANIFOLD_SOLID_BREP`, or `BREP_WITH_VOIDS` when it has internal voids |
//!
//! Units are millimetres, always, as everywhere else in Cadpole: the
//! representation context assigns `SI_UNIT(.MILLI.,.METRE.)`.
//!
//! Recognition never guesses. Every analytic surface is *fitted and then
//! verified* against samples of the truck surface, in position and in facing,
//! and a fit that does not verify falls back to the exact B-spline
//! ([`analytic`]). Anything that cannot be written at all comes back as
//! [`Error::Unsupported`] naming the type, because a file that is quietly the
//! wrong shape is worse than no file.
//!
//! # Verifying the output
//!
//! [`validate`] re-reads the text this crate just produced and checks it as a
//! graph: every `#n` referenced is defined, every entity is reachable from the
//! file's roots, no shell has an edge used other than exactly twice in
//! opposite directions, and no entity refers to itself. `//src/rust/cadpole-export-step`'s
//! tests additionally parse the output with `truck-stepio` and compare the
//! rebuilt solid's face count, volume and bounding box with the original.

pub mod analytic;
pub mod geometry;
pub mod text;
pub mod topology;
pub mod validate;

#[cfg(test)]
mod tests;

use cadpole_core::kernel::KSolid;

use crate::text::{real, string, Emitter, Id};

/// Why an export could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A geometry type Cadpole cannot express in STEP, named so the message is
    /// actionable. Never raised for anything truck currently produces; it
    /// exists so that a future truck type fails loudly instead of silently.
    Unsupported { what: String, detail: String },
    /// The model itself is not exportable: a non-finite coordinate, a
    /// degenerate placement, a shell with no faces.
    Invalid(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unsupported { what, detail } => {
                write!(f, "STEP export does not support {what}: {detail}")
            }
            Error::Invalid(m) => write!(f, "STEP export: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// One body to write.
pub struct Part<'a> {
    /// The body's name, which becomes the `MANIFOLD_SOLID_BREP` name.
    pub name: &'a str,
    pub solid: &'a KSolid,
}

/// Everything the writer needs that is not geometry.
#[derive(Clone, Debug)]
pub struct Options {
    /// The `PRODUCT` name, shown as the part name in the receiving CAD tool.
    pub product_name: String,
    /// The `FILE_NAME` first field. Conventionally the file's own name.
    pub file_name: String,
    pub author: String,
    pub organization: String,
    /// The `FILE_NAME` time stamp. Defaults to a **fixed** value so two
    /// exports of the same document are byte-identical, which is what the
    /// golden tests and content-addressed caches want; a front-end that
    /// prefers a real date passes one.
    pub timestamp: String,
    /// The tool that wrote the file, for `FILE_NAME.originating_system`.
    pub originating_system: String,
    /// Geometric tolerance in mm. Written into the representation context as
    /// the `distance_accuracy_value`, and used as the acceptance threshold for
    /// every analytic fit and every intersection-curve refit.
    pub tolerance: f64,
    /// Recognise cylinders, cones, spheres and tori instead of writing the
    /// B-spline truck actually holds. On by default: a real cylinder is what
    /// makes the result useful in a CAD tool. Turning it off is exact and
    /// slower to open.
    pub analytic_surfaces: bool,
    /// Recognise circular arcs and write `CIRCLE` instead of a rational
    /// B-spline.
    pub analytic_curves: bool,
    /// Refit a boolean's intersection polyline as a cubic B-spline when the
    /// fit tracks the polyline within [`Options::tolerance`].
    pub refit_intersections: bool,
}

/// The default tolerance, in mm. Matches `cadpole_core::kernel::DEFAULT_TOLERANCE`
/// -- the tessellation tolerance the rest of the document is built at -- so an
/// intersection curve refitted at this tolerance is no coarser than the mesh
/// the same model exports as STL.
pub const DEFAULT_TOLERANCE: f64 = cadpole_core::kernel::DEFAULT_TOLERANCE;

impl Default for Options {
    fn default() -> Options {
        Options {
            product_name: "cadpole".into(),
            file_name: "cadpole.step".into(),
            author: String::new(),
            organization: String::new(),
            timestamp: "1970-01-01T00:00:00".into(),
            originating_system: "Cadpole".into(),
            tolerance: DEFAULT_TOLERANCE,
            analytic_surfaces: true,
            analytic_curves: true,
            refit_intersections: true,
        }
    }
}

/// What the writer produced, for diagnostics and tests. Every field is a count
/// of entities of that kind actually written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub solids: usize,
    pub voids: usize,
    pub shells: usize,
    pub faces: usize,
    pub loops: usize,
    pub edges: usize,
    pub vertices: usize,
    pub planes: usize,
    pub cylinders: usize,
    pub cones: usize,
    pub spheres: usize,
    pub tori: usize,
    pub surfaces_of_revolution: usize,
    pub bspline_surfaces: usize,
    pub rational_surfaces: usize,
    pub lines: usize,
    pub circles: usize,
    pub bspline_curves: usize,
    pub rational_curves: usize,
    /// Boolean edges written as the exact polyline truck computed.
    pub polyline_intersections: usize,
    /// Boolean edges refitted as a cubic B-spline within tolerance.
    pub refitted_intersections: usize,
}

impl Stats {
    /// A one-line summary for a diagnostic.
    pub fn summary(&self) -> String {
        format!(
            "{} solid(s), {} face(s), {} edge(s); surfaces: {} plane, {} cylinder, {} cone, {} sphere, \
             {} torus, {} revolved, {} b-spline",
            self.solids,
            self.faces,
            self.edges,
            self.planes,
            self.cylinders,
            self.cones,
            self.spheres,
            self.tori,
            self.surfaces_of_revolution,
            self.bspline_surfaces + self.rational_surfaces,
        )
    }
}

/// The writer's state: the entity table, the options and the tally.
pub struct Writer {
    pub(crate) em: Emitter,
    pub(crate) options: Options,
    pub(crate) stats: Stats,
}

impl Writer {
    pub(crate) fn new(options: Options) -> Writer {
        Writer { em: Emitter::new(), options, stats: Stats::default() }
    }
}

/// The AP214 schema identifier. AP214 ("automotive_design") rather than AP203
/// ("config_control_design") because AP203 requires a whole approval and
/// security-classification apparatus around the shape — person, organization,
/// approval, date/time roles, `CC_DESIGN_*` — that says nothing about the
/// geometry, while AP214's product structure is six entities; and because
/// AP214 is the schema every tool Cadpole users name reads first.
pub const SCHEMA: &str = "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }";

/// Write `parts` as one STEP file.
pub fn write(parts: &[Part<'_>], options: &Options) -> Result<String> {
    Ok(write_with_stats(parts, options)?.0)
}

/// [`write`], also returning what was written.
pub fn write_with_stats(parts: &[Part<'_>], options: &Options) -> Result<(String, Stats)> {
    if parts.is_empty() {
        return Err(Error::Invalid("there are no bodies to write".into()));
    }
    if !(options.tolerance > 0.0 && options.tolerance.is_finite()) {
        return Err(Error::Invalid(format!("the tolerance must be a positive number, got {}", options.tolerance)));
    }
    let mut w = Writer::new(options.clone());

    // The product structure refers forward to the solids and the solids refer
    // back to the representation context, so one side has to be reserved.
    const PREAMBLE: usize = 16;
    let p: Vec<Id> = (0..PREAMBLE).map(|_| w.em.reserve()).collect();

    // The representation's own origin placement. AP214's
    // `advanced_brep_shape_representation` allows exactly four item types and
    // this is one of them; every writer includes it and some readers expect a
    // placement to hang the shape on.
    let origin = w.axis2(
        cadpole_core::geom::Point3::new(0.0, 0.0, 0.0),
        cadpole_core::geom::Vector3::new(0.0, 0.0, 1.0),
        cadpole_core::geom::Vector3::new(1.0, 0.0, 0.0),
    )?;

    let mut items = vec![origin];
    for part in parts {
        items.push(w.solid(part.name, part.solid.compress())?);
    }

    let name = string(&options.product_name);
    let preamble: [String; PREAMBLE] = [
        format!("APPLICATION_PROTOCOL_DEFINITION('international standard', 'automotive_design', 1994, #{})", p[1]),
        "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into(),
        format!("PRODUCT_CONTEXT('', #{}, 'mechanical')", p[1]),
        format!("PRODUCT({name}, {name}, '', (#{}))", p[2]),
        format!("PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('', '', #{}, .NOT_KNOWN.)", p[3]),
        format!("PRODUCT_DEFINITION_CONTEXT('part definition', #{}, 'design')", p[1]),
        format!("PRODUCT_DEFINITION('design', '', #{}, #{})", p[4], p[5]),
        format!("PRODUCT_DEFINITION_SHAPE('', '', #{})", p[6]),
        format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part', '', (#{}))", p[3]),
        "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI., .METRE.) )".into(),
        "( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($, .RADIAN.) )".into(),
        "( NAMED_UNIT(*) SI_UNIT($, .STERADIAN.) SOLID_ANGLE_UNIT() )".into(),
        format!(
            "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}), #{}, 'distance_accuracy_value', 'confusion accuracy')",
            real(options.tolerance),
            p[9]
        ),
        format!(
            "( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{})) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#{}, #{}, #{})) \
             REPRESENTATION_CONTEXT('Cadpole', '3D model in millimetres') )",
            p[12], p[9], p[10], p[11]
        ),
        format!("SHAPE_DEFINITION_REPRESENTATION(#{}, #{})", p[7], p[15]),
        format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION({name}, {}, #{})",
            crate::text::refs(items),
            p[13]
        ),
    ];
    for (id, body) in p.iter().zip(preamble) {
        w.em.fill(*id, body);
    }

    let mut out = String::with_capacity(w.em.len() * 48 + 512);
    out.push_str("ISO-10303-21;\n");
    out.push_str("HEADER;\n");
    out.push_str("FILE_DESCRIPTION(('Cadpole B-rep model'), '2;1');\n");
    out.push_str(&format!(
        "FILE_NAME({}, {}, ({}), ({}), {}, {}, '');\n",
        string(&options.file_name),
        string(&options.timestamp),
        string(&options.author),
        string(&options.organization),
        string(&format!("Cadpole {}", env!("CARGO_PKG_VERSION"))),
        string(&options.originating_system),
    ));
    out.push_str(&format!("FILE_SCHEMA(({}));\n", string(SCHEMA)));
    out.push_str("ENDSEC;\n");
    out.push_str("DATA;\n");
    out.push_str(&w.em.render());
    out.push_str("ENDSEC;\n");
    out.push_str("END-ISO-10303-21;\n");
    Ok((out, w.stats))
}
