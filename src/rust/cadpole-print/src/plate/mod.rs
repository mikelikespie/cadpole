//! Plating: from a model's bodies to print-ready plates.
//!
//! `docs/plating.md` is the design. In short: bodies are joined into
//! **pieces** that move as one ([`pieces`]: what overlaps, what is a skin on
//! something else, what is captive in something else); each piece is
//! **oriented** by a cost in mm² of support equivalent built from the
//! support rules `print_check` judges ([`score`]); pieces are **packed** by
//! raster bottom-left fill onto plates the size of what every nozzle
//! reaches, a plate never holding more filaments than the AMS has slots
//! ([`pack`]); and every decision is written down with its numbers
//! ([`Plan::report`]).
//!
//! Meshes in, a [`Plan`] out: nothing here knows about documents or
//! scripts, and nothing is random or hash-ordered, so the same input gives
//! the same plan.

pub mod colour;
pub mod pack;
pub mod pieces;
pub mod score;

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::{Mesh, Topology};

use crate::{Profile, num};

pub use pieces::{Joint, JointKind};
pub use score::Scored;

/// One body the planner may place.
#[derive(Clone, Debug)]
pub struct PlanBody<'a> {
    pub name: String,
    pub mesh: &'a Mesh,
    /// Face names for the findings and the round-hole rule; optional.
    pub topo: Option<&'a Topology>,
    /// The filaments the body prints in, as `#rrggbb`: its own colour first,
    /// then its paint's, in the order the project assigns slots.
    pub colours: Vec<String>,
    /// Per triangle, an index into `colours`; empty means every triangle is
    /// `colours[0]`.
    pub tri_colours: Vec<u8>,
}

/// What a script pinned about one item. Every field is optional; what is
/// not pinned is decided and explained.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pin {
    /// The model direction that faces the bed.
    pub down: Option<Vec3>,
    /// Print as modelled.
    pub keep: bool,
    /// The model direction the piece's fibres must run in (a snap arm's
    /// length): kept within 30° of the bed plane when any candidate can.
    pub along: Option<Vec3>,
    /// The turn about z, in degrees; `None` lets the packer choose.
    pub spin: Option<f64>,
    /// No turn about z at all, not even the packer's.
    pub no_spin: bool,
    /// Pieces with the same group share a plate when they fit.
    pub group: Option<String>,
    /// A test coupon: printed first, on its own plate.
    pub coupon: bool,
    /// The plate of its variant (1-based) it must go on.
    pub plate: Option<u32>,
    /// A name for the piece.
    pub name: Option<String>,
}

/// One thing the script handed over: a body, or a list of bodies that is
/// one piece whatever the analysis finds.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// Indices into the plan's bodies.
    pub bodies: Vec<usize>,
    /// Given as a list: one piece, always.
    pub declared: bool,
    pub pin: Pin,
}

/// A set of plates made from every piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Each body in its own filaments.
    Colour,
    /// Every body in filament 1.
    Mono,
}

impl Variant {
    pub fn parse(s: &str) -> Option<Variant> {
        match s.trim().to_ascii_lowercase().as_str() {
            "colour" | "color" | "full" | "full colour" | "full color" | "multicolour" | "multicolor" => {
                Some(Variant::Colour)
            }
            "mono" | "monochrome" | "single" | "one colour" | "one color" => Some(Variant::Mono),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Colour => "colour",
            Variant::Mono => "mono",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Variant::Colour => "Colour",
            Variant::Mono => "Mono",
        }
    }
}

/// How to plan.
#[derive(Clone, Debug)]
pub struct Options {
    /// The checks' printer settings: bed height, support rules, layer.
    pub profile: Profile,
    /// The longest bridge, as `print_check`'s `max_bridge`.
    pub max_bridge: f64,
    /// Where pieces may stand, `[x0, y0, x1, y1]` in plate mm: what every
    /// nozzle reaches.
    pub region: [f64; 4],
    /// Rectangles of the plate nothing may stand in.
    pub exclude: Vec<[f64; 4]>,
    /// The side of the square a prime tower and its margins take on a plate
    /// of more than one filament; `None` when the project prints none.
    pub tower: Option<f64>,
    /// Filaments one plate may use; 0 is no limit.
    pub ams: u32,
    /// Clear space between pieces, in mm.
    pub spacing: f64,
    /// Bodies closer than this may be one print-in-place piece, in mm.
    pub joint_gap: f64,
    /// Raster cell, in mm.
    pub cell: f64,
    /// The plate sets to make, in order.
    pub variants: Vec<Variant>,
    /// The printer's name, for the report.
    pub printer: String,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            profile: Profile::default(),
            max_bridge: crate::overhang::DEFAULT_MAX_BRIDGE,
            region: [0.0, 0.0, 256.0, 256.0],
            exclude: Vec::new(),
            tower: Some(55.0),
            ams: 4,
            spacing: DEFAULT_SPACING,
            joint_gap: DEFAULT_JOINT_GAP,
            cell: 1.0,
            variants: vec![Variant::Colour],
            printer: "default".to_owned(),
        }
    }
}

/// Clear space between pieces on a plate, in mm.
pub const DEFAULT_SPACING: f64 = 4.0;
/// The widest gap a print-in-place joint is looked for across, in mm:
/// print-in-place clearances are 0.2-0.6 mm, so 1 mm finds them all and
/// keeps the pair search small.
pub const DEFAULT_JOINT_GAP: f64 = 1.0;

/// What kind of piece it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// One body.
    Single,
    /// Bodies that overlap or lie on each other: one multi-material object.
    Fused,
    /// Bodies held captive in each other across a gap: printed assembled.
    PrintInPlace,
    /// A list the script gave: one piece.
    Declared,
}

impl PieceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PieceKind::Single => "single",
            PieceKind::Fused => "fused",
            PieceKind::PrintInPlace => "print-in-place",
            PieceKind::Declared => "declared",
        }
    }
}

/// One piece, oriented.
#[derive(Clone, Debug)]
pub struct Piece {
    pub name: String,
    /// Indices into the plan's bodies, in input order.
    pub bodies: Vec<usize>,
    pub kind: PieceKind,
    /// Why its bodies are together.
    pub joints: Vec<Joint>,
    pub pin: Pin,
    /// Its filaments, `#rrggbb`, in first-use order.
    pub colours: Vec<String>,
    /// The orientations scored, best first; `chosen` indexes it.
    pub candidates: Vec<Scored>,
    pub chosen: usize,
    /// "as modelled" among `candidates`, for the comparison.
    pub baseline: Option<usize>,
    /// Candidates the packer may use instead, as good a print as `chosen`.
    pub alternatives: Vec<usize>,
    /// The sentence about its orientation.
    pub reason: String,
}

impl Piece {
    pub fn best(&self) -> &Scored {
        &self.candidates[self.chosen]
    }
}

/// One piece on one plate.
#[derive(Clone, Debug)]
pub struct Placement {
    pub piece: usize,
    /// Index into [`Plan::plates`].
    pub plate: usize,
    /// Model to plate: orientation, turn about z, position.
    pub transform: Transform,
    /// The candidate used (an index into the piece's `candidates`).
    pub candidate: usize,
    /// The turn about z, in degrees.
    pub spin: f64,
    /// Footprint on the plate, `[x0, y0, x1, y1]` in mm, without spacing.
    pub footprint: [f64; 4],
}

/// One plate.
#[derive(Clone, Debug)]
pub struct Plate {
    pub name: String,
    /// `None` for the test coupons' plates.
    pub variant: Option<Variant>,
    /// Indices into [`Plan::placements`].
    pub placements: Vec<usize>,
    /// Filaments, `#rrggbb`, in first-use order; one entry for a mono plate.
    pub colours: Vec<String>,
    /// The colour-change estimate ([`colour::changes`]).
    pub colour_changes: usize,
    /// Layers the plate prints.
    pub layers: usize,
    /// A free square kept for the prime tower.
    pub tower: bool,
}

/// The whole plan.
#[derive(Clone, Debug)]
pub struct Plan {
    pub pieces: Vec<Piece>,
    pub plates: Vec<Plate>,
    pub placements: Vec<Placement>,
    /// Problems and remarks the report leads with.
    pub notes: Vec<String>,
}

/// Plan `items` of `bodies`.
pub fn plan(bodies: &[PlanBody<'_>], items: &[Item], opts: &Options) -> Plan {
    let mut notes = Vec::new();
    let groups = pieces::join(bodies, items, opts, &mut notes);
    let mut out: Vec<Piece> = Vec::with_capacity(groups.len());
    for g in groups {
        let mesh = if g.bodies.len() > 1 {
            pieces::without_contacts(&g.bodies.iter().map(|&b| bodies[b].mesh).collect::<Vec<_>>())
        } else {
            bodies[g.bodies[0]].mesh.clone()
        };
        let topo = if g.bodies.len() == 1 { bodies[g.bodies[0]].topo } else { None };
        let fixed = g.kind == PieceKind::PrintInPlace && g.pin.down.is_none();
        let (candidates, chosen, baseline, alternatives) = score::orient(&mesh, topo, &g.pin, fixed, opts);
        let mut colours: Vec<String> = Vec::new();
        for &b in &g.bodies {
            for c in &bodies[b].colours {
                if !colours.contains(c) {
                    colours.push(c.clone());
                }
            }
        }
        let mut piece = Piece {
            name: g.name,
            bodies: g.bodies,
            kind: g.kind,
            joints: g.joints,
            pin: g.pin,
            colours,
            candidates,
            chosen,
            baseline,
            alternatives,
            reason: String::new(),
        };
        piece.reason = score::explain(&piece, opts);
        if opts.ams > 0 && piece.colours.len() as u32 > opts.ams && opts.variants.contains(&Variant::Colour) {
            notes.push(format!(
                "{} prints in {} filaments, more than the {} AMS slots a plate has; its plate will need a filament \
                 swapped by hand",
                piece.name,
                piece.colours.len(),
                opts.ams
            ));
        }
        out.push(piece);
    }
    let (plates, placements) = pack::pack(bodies, &out, opts, &mut notes);
    let mut plan = Plan { pieces: out, plates, placements, notes };
    colour::count(bodies, &mut plan, opts);
    plan
}

impl Plan {
    /// Every decision, a line each, for a person or an agent to read.
    pub fn report(&self, opts: &Options) -> String {
        let mut s = String::new();
        let w = opts.region[2] - opts.region[0];
        let d = opts.region[3] - opts.region[1];
        s.push_str(&format!(
            "{} piece{} on {} plate{} for the {} ({} x {} mm where every nozzle reaches{}).\n",
            self.pieces.len(),
            plural(self.pieces.len()),
            self.plates.len(),
            plural(self.plates.len()),
            opts.printer,
            num(w),
            num(d),
            if opts.ams > 0 { format!(", {} AMS slots a plate", opts.ams) } else { String::new() }
        ));
        for n in &self.notes {
            s.push_str(&format!("note: {n}\n"));
        }
        for (i, p) in self.plates.iter().enumerate() {
            s.push_str(&format!(
                "plate {} \"{}\": {} piece{}, {} filament{}{}{}, {} layers\n",
                i + 1,
                p.name,
                p.placements.len(),
                plural(p.placements.len()),
                p.colours.len(),
                plural(p.colours.len()),
                if p.colours.len() > 1 { format!(" ({})", p.colours.join(", ")) } else { String::new() },
                if p.colours.len() > 1 {
                    format!(", ~{} colour change{}", p.colour_changes, plural(p.colour_changes))
                } else {
                    String::new()
                },
                p.layers
            ));
            for &k in &p.placements {
                let pl = &self.placements[k];
                let piece = &self.pieces[pl.piece];
                let mut line = format!("  {}: {}", piece.name, piece.reason);
                if pl.candidate != piece.chosen {
                    line.push_str(&format!(
                        "; placed {} instead (the same print: {} mm² support) so it fits this plate",
                        piece.candidates[pl.candidate].label,
                        num(piece.candidates[pl.candidate].support_area)
                    ));
                }
                if pl.spin.abs() > 1e-6 {
                    line.push_str(&format!("; turned {}° about z", num(pl.spin)));
                }
                s.push_str(&line);
                s.push('\n');
            }
        }
        s
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The meshes as one: positions and indices appended, face ids kept.
pub fn concat<'a>(meshes: impl Iterator<Item = &'a Mesh>) -> Mesh {
    let mut out = Mesh::default();
    for m in meshes {
        let base = (out.positions.len() / 3) as u32;
        out.positions.extend_from_slice(&m.positions);
        if m.normals.len() == m.positions.len() {
            out.normals.extend_from_slice(&m.normals);
        } else {
            out.normals.extend(std::iter::repeat_n(0.0, m.positions.len()));
        }
        out.indices.extend(m.indices.iter().map(|i| i + base));
        if m.face_ids.len() == m.triangle_count() {
            out.face_ids.extend_from_slice(&m.face_ids);
        } else {
            out.face_ids.extend(std::iter::repeat_n(0, m.triangle_count()));
        }
    }
    out
}

#[cfg(test)]
mod tests;
