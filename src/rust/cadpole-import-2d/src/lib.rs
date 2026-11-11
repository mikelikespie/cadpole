//! Reading SVG and DXF into Cadpole sketches.
//!
//! Both importers hand back an [`Import`]: named loops, named construction
//! paths and diagnostics. Neither touches the file system, records a feature
//! or knows what a Starlark value is — they take bytes and options and return
//! geometry, so the same code serves the CLI, the session server and wasm.
//!
//! The through-line is **exact where the file is exact**:
//! a DXF `ARC`, a bulged polyline vertex
//! and an SVG `A` command with equal radii all become [`Segment::Arc`], so a
//! hole in the drawing extrudes to a real cylinder that `faces(kind=
//! "cylinder")` can find. Only the genuinely inexact — elliptical arcs,
//! splines, arcs under a non-uniform transform — is approximated, and then
//! within `tolerance`.

pub mod curve;
pub mod dxf;
pub mod svg;

mod fixtures;

use cadpole_core::sketch::{Loop, P2, Path};

/// Severity of an importer diagnostic. Mirrors `cadpole_core::context::Severity`
/// without depending on the evaluation context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
}

/// Something worth telling the user about the file, short of failing.
#[derive(Clone, Debug)]
pub struct Note {
    pub level: Level,
    pub message: String,
}

impl Note {
    pub fn info(message: impl Into<String>) -> Note {
        Note { level: Level::Info, message: message.into() }
    }
    pub fn warn(message: impl Into<String>) -> Note {
        Note { level: Level::Warning, message: message.into() }
    }
}

/// What an importer produces: loops and open chains, each under a name
/// derived from the file's own identifiers, plus the
/// diagnostics the caller should surface.
#[derive(Clone, Debug, Default)]
pub struct Import {
    pub loops: Vec<(String, Loop)>,
    pub paths: Vec<(String, Path)>,
    pub notes: Vec<Note>,
}

impl Import {
    pub fn is_empty(&self) -> bool {
        self.loops.is_empty() && self.paths.is_empty()
    }

    /// Bounding box of everything imported, for `align=`.
    ///
    /// Curves are sampled finely rather than solved for their extremes: 2 µm
    /// is far below any placement anyone can see, and it keeps one code path
    /// for lines, arcs and Béziers.
    pub fn bbox(&self) -> Option<(P2, P2)> {
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        let mut any = false;
        let mut eat = |p: P2| {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        };
        for (_, l) in &self.loops {
            for p in l.sample_tolerance(0.002) {
                any = true;
                eat(p);
            }
        }
        for (_, path) in &self.paths {
            eat(path.start);
            any = true;
            for s in &path.segments {
                eat(s.start());
                eat(s.end());
            }
        }
        if any { Some((lo, hi)) } else { None }
    }

    /// Apply `f` to every point of every segment. Used for units, scaling,
    /// the y flip and the final placement, all folded into one pass.
    pub fn map_points(&mut self, f: impl Fn(P2) -> P2 + Copy) {
        for (_, l) in &mut self.loops {
            for s in &mut l.segments {
                curve::map_segment(s, f);
            }
        }
        for (_, p) in &mut self.paths {
            p.start = f(p.start);
            for s in &mut p.segments {
                curve::map_segment(s, f);
            }
        }
    }
}

/// A length unit a file (or the user) can name, as a factor to millimetres.
/// Cadpole models are in millimetres; imperial files are converted on the way in
/// and presented back in feet and inches by `display_unit`.
pub fn unit_scale(name: &str) -> Option<f64> {
    Some(match name {
        "mm" | "millimeter" | "millimetre" => 1.0,
        "cm" | "centimeter" | "centimetre" => 10.0,
        "m" | "meter" | "metre" => 1000.0,
        "in" | "inch" => 25.4,
        "ft" | "feet" | "foot" => 304.8,
        "pt" | "point" => 25.4 / 72.0,
        "pc" | "pica" => 25.4 / 6.0,
        "px" => 25.4 / 96.0,
        _ => return None,
    })
}

/// The unit names `units=` accepts, for the "expected one of" diagnostic.
pub const UNIT_NAMES: &[&str] = &["auto", "mm", "cm", "m", "in", "ft", "pt", "pc", "px"];
