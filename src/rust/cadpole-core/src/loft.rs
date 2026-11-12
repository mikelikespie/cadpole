//! Section matching for `loft`.
//!
//! A loft is only as good as the correspondence between its sections. This
//! module takes the sections a script gave, checks that they can be lofted at
//! all, and decides **which point of section *k* goes to which point of
//! section *k+1***. Nothing here touches a kernel: it produces a [`LoftPlan`]
//! of oriented loops, and `kernel::loft` turns that into a solid.
//!
//! ## The correspondence rule
//!
//! Sections correspond **loop by loop and segment by segment, in construction
//! order** — loop `l` of every section is the same loop, segment `s` of that
//! loop is the same segment. It is never a geometric search.
//!
//! That is the whole point. Two rounded rectangles with different insets and
//! different corner radii are both `Loop::rounded_rect`, so both are
//! `line, arc, line, arc, line, arc, line, arc` in the same order starting from
//! the same edge; pairing by index sends line to line and corner arc to corner
//! arc. A ruled surface between two concentric circular arcs of equal angular
//! span is an exact cone (the two arcs are rational curves with identical
//! weights, so interpolating their homogeneous control points interpolates
//! their Euclidean ones), which is precisely what a 45° chamfer of a filleted
//! vertical edge is. Gridfinity's foot is four such rounded rectangles and
//! needs no chamfer operation at all.
//!
//! A geometric matcher — "rotate the second loop until the vertices line up" —
//! would be worse in the way that matters: the argmin can jump when a parameter
//! moves, silently twisting the solid between one evaluation and the next.
//! Index pairing is a pure function of how the sketches were written, so the
//! same script always produces the same solid and a parameter nudge can only
//! move points, never re-pair them. What the geometric search is good for is
//! *telling the user they got it wrong*, so [`LoftPlan::new`] runs it and warns
//! when a different rotation would have fitted much better (see
//! [`twist_warning`]).
//!
//! ## When the counts differ: `match="arclen"`
//!
//! Construction order cannot pair a loop of 8 segments with a loop of 28, and
//! the shape that needs it is real: a rim shell that lofts a plain wall ring
//! up to a ring that detours in a U around every magnet boss. So a loft
//! may be asked — **never told automatically** — to *subdivide* its sections
//! until the counts agree, with `match="arclen"`. See [`SectionMatch`].
//!
//! The rule is: walk every section's loop `l` from its own **segment 0 start**
//! (the seam is still construction order — the matcher never rotates a loop),
//! take the union of all the sections' vertices at their **normalised arc
//! length**, and split every section's loop at every fraction in that union.
//! Splitting is exact — [`crate::sketch::Segment::sub`] keeps a line a line and
//! an arc a true arc of the same circle — so the corner of a plain ring lofted
//! to the corner of a detoured one is still a cone, not a fan of chords.
//!
//! Two properties make this safe to build on:
//!
//! * **It only ever refines.** Where two sections already agree on a segment
//!   count the matcher leaves them alone, so every loft that worked before
//!   `match=` existed builds byte for byte the same solid.
//! * **A parameter nudge moves points, it never re-pairs them.** The
//!   correspondence is a monotone map between two arc-length parameters, both
//!   anchored at a seam the *script* chose. There is no argmin over rotations
//!   to jump, and the one discrete decision in the algorithm — snapping two
//!   split points within [`SNAP_MM`] into one — is local and bounded: crossing
//!   that threshold changes the face count by one and moves a split point by at
//!   most half a millimetre. It cannot swap which part of one section goes to
//!   which part of the other.
//!
//! What it costs is worth knowing: arc length is
//! shared out over the *whole* loop, so a detour that exists on one section and
//! not the other spends a proportional share of the other section's perimeter.
//! On that rim shell it is exactly right — it is what splays the tab's support
//! ramp — but it also drifts the corners of the two rings apart by a few
//! millimetres. `match="arclen"` is opt-in because that is the author's call to
//! make, not the loft's.
//!
//! ## Sections in space: `Curve` sections
//!
//! A section may also be a closed [`Curve`] — one loop anywhere in space,
//! planar or not — and a loft with any such section takes the **ordered**
//! layout ([`Layout::Ordered`]): *list order is the order*. Nothing is
//! parallel, nothing is stacked along a normal, and the band index `b` in
//! `side[l.s]#b` is the script's list index and nothing else. Sketch sections
//! in such a loft are lifted onto their planes as drawn ([`Curve::from_loop`])
//! and must be a single loop, because a curve carries exactly one; a section
//! with holes is a diagnostic naming the workaround (loft the loops
//! separately and subtract). Construction-order pairing and `match="arclen"`
//! carry over unchanged. The two questions a planar loft never had to ask —
//! which way is *out*, and how to close an end that lies in no plane — are
//! answered by the kernel from the signed volume (`kernel::loft`) and by the
//! [`Cap`] the script chose.
//!
//! A loft of planar sketches with the default `cap="plane"` takes exactly the
//! path it always took ([`Layout::Stacked`]); the ordered layout is only
//! entered by a `Curve` section or a `cap="ruled"`.
//!
//! ## What is refused
//!
//! Planar sections must be parallel, ordered monotonically along the loft
//! direction, with matching loop counts and matching region structure. Curve
//! sections must be closed and single-loop. Segment counts must match too,
//! unless `match="arclen"` is asked for. A `cap="plane"` end must be planar.
//! Anything else is a diagnostic naming the section and the reason, never a
//! panic.

use crate::curve::{Curve, Segment3, PLANAR_TOL};
use crate::geom::{Plane, Vec3};
use crate::sketch::{Loop, Segment, Sketch};
use crate::{CadpoleError, Result};
use crate::fmath;

/// How far from parallel two section planes may be, as the sine of the angle
/// between their normals. Sections are built from `XY.offset(z)` and friends,
/// so in practice they are parallel to the last bit.
const PARALLEL_EPS: f64 = 1e-9;

/// The smallest gap between two consecutive sections, in mm. Below it the band
/// between them is degenerate and the kernel would make a zero-area face.
const MIN_GAP: f64 = 1e-9;

/// How much better an alternative seam rotation has to fit before the plan
/// warns that the sections are probably mis-paired. Rounded rectangles of
/// different sizes pair perfectly at rotation 0, so this only fires on loops
/// that genuinely start at different corners.
const TWIST_RATIO: f64 = 1.5;

/// How much narrower a ruled cap's ribbon could have been, with the seam
/// somewhere else on the loop, before the plan warns that the seam is in the
/// wrong place. A thin section closed from the middle of a flank
/// instead of its end rules the loop against itself lengthwise, and the
/// ribbon is then many times wider than the section is thick.
const SEAM_RATIO: f64 = 3.0;

/// Two split points this close together are one split point, in mm measured on
/// the **longest** loop taking part — so the bound holds on every section at
/// once, and snapping can never move a split by more than this much anywhere.
///
/// It is there for two reasons. A sliver piece a few microns long is a face the
/// kernel cannot build, and a vertex that lands *almost* on another section's
/// vertex is nearly always meant to be that vertex. Half a millimetre is an
/// empirical value.
pub const SNAP_MM: f64 = 0.5;

/// The shortest piece the matcher will hand to the kernel, in mm. Below this a
/// band face is degenerate; the matcher says so and names the loop rather than
/// letting `try_wire_homotopy` fail with a kernel message.
const MIN_PIECE_MM: f64 = 1e-3;

/// Tolerance for "this arc length is a segment boundary", in mm.
const BOUNDARY_EPS: f64 = 1e-9;

/// Points sampled round a loop when the plan looks for a better seam for a
/// ruled cap. A warning's precision, not a kernel's.
const SEAM_SAMPLES: usize = 128;

/// How a loft decides which part of one section goes to which part of the next.
///
/// The default is [`SectionMatch::Index`] and that is the contract: the
/// correspondence is a pure function of the *program*, so the same script twice
/// gives the same solid and a parameter nudge can only move points.
/// [`SectionMatch::ArcLength`] relaxes it only as far as is needed to pair
/// sections whose segment counts genuinely differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SectionMatch {
    /// Loop `l` to loop `l`, segment `s` to segment `s`, in construction order.
    /// The segment counts have to agree already; if they do not, that is a
    /// diagnostic.
    #[default]
    Index,
    /// Subdivide the sections' loops at the union of their vertices' normalised
    /// arc lengths until the counts agree, then pair in construction order as
    /// usual. Sections that already agree are left untouched.
    ArcLength,
}

impl SectionMatch {
    /// Every rule, in documentation order. `parse` reads this, and so does
    /// the `match` vocabulary cadpole-script exports, so the spellings
    /// the parser takes and the members the namespace offers are one list by
    /// construction. `all_lists_every_variant` below stops compiling when a
    /// variant is missing here.
    pub const ALL: [SectionMatch; 2] = [SectionMatch::Index, SectionMatch::ArcLength];

    /// The spelling a script uses.
    pub fn name(self) -> &'static str {
        match self {
            SectionMatch::Index => "index",
            SectionMatch::ArcLength => "arclen",
        }
    }

    /// Parse the `match=` argument. An unknown value is a diagnostic that lists
    /// the two that exist and says what each does.
    pub fn parse(s: &str) -> Result<SectionMatch> {
        match Self::ALL.into_iter().find(|m| m.name() == s) {
            Some(m) => Ok(m),
            None => Err(CadpoleError::invalid(format!(
                "loft(match={s:?}) is not a matching rule. It is \"index\" (the default: \
                 segment s of one section goes to segment s of the next, in the order the \
                 sketches were written, and the segment counts must agree) or \"arclen\" (split \
                 the sections' loops at each other's vertices, measured as a fraction of the way \
                 round the loop, until the counts agree)"
            ))),
        }
    }
}

/// How an end section of a loft is closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    /// A planar face on the section's plane. The section must be planar; a
    /// sketch always is, a curve may not be.
    #[default]
    Plane,
    /// The loop ruled against itself: split at its seam and half-way round by
    /// arc length, one half reversed, and a ruled band between the two —
    /// the loft's own band builder applied to one section. Exact for a blade
    /// tip, which physically *is* the ribbon between its two flanks.
    Ruled,
}

impl Cap {
    /// Every way to close an end, in documentation order; `parse` and the
    /// `cap` vocabulary both read it. See [`SectionMatch::ALL`].
    pub const ALL: [Cap; 2] = [Cap::Plane, Cap::Ruled];

    /// The spelling a script uses.
    pub fn name(self) -> &'static str {
        match self {
            Cap::Plane => "plane",
            Cap::Ruled => "ruled",
        }
    }

    /// Parse one `cap=` spelling. An unknown value lists the two that exist.
    pub fn parse(s: &str) -> Result<Cap> {
        match Self::ALL.into_iter().find(|c| c.name() == s) {
            Some(c) => Ok(c),
            None => Err(CadpoleError::invalid(format!(
                "loft(cap={s:?}) is not a way to close an end. It is \"plane\" (the default: a \
                 flat face, which needs a planar end section) or \"ruled\" (the loop ruled against \
                 itself: split at its seam and half-way round, and a ruled band between the two \
                 halves — for an end section that lies in no plane). A 2-tuple gives the start and \
                 the end separately: cap=(\"plane\", \"ruled\")"
            ))),
        }
    }
}

/// One section as the script gave it.
#[derive(Clone, Debug)]
pub enum Section {
    /// A planar sketch. Every loop of it takes part in the loft.
    Profile(Sketch),
    /// A closed curve in space: one loop, planar or not.
    Curve(Curve),
    /// A single point: the loft ends in a tip. Only the first or the last
    /// section may be one — that is a cone's apex or a pyramid's point, and it
    /// is how `cone(r0, 0, h)` is built.
    Point(Vec3),
}

impl Section {
    /// The loops of a profile section as the script wrote them, before
    /// orienting or matching. Empty for a point or a curve section.
    pub fn profile_loops(&self) -> &[Loop] {
        match self {
            Section::Profile(sk) => &sk.loops,
            Section::Curve(_) | Section::Point(_) => &[],
        }
    }
}

/// A section after checking and orienting: outer loops counter-clockwise seen
/// from the loft direction, holes clockwise, indexed as in the sketch — or a
/// curve as drawn, or a point.
#[derive(Clone, Debug)]
pub enum PlannedSection {
    Profile {
        plane: Plane,
        /// One entry per sketch loop, in sketch order, correctly oriented.
        loops: Vec<Loop>,
        /// `(outer loop index, hole loop indices)` per material region.
        regions: Vec<(usize, Vec<usize>)>,
    },
    /// A closed curve, exactly as drawn (after matching, if any): loop 0 of
    /// the section, and its only one.
    Curve(Curve),
    Point(Vec3),
}

impl PlannedSection {
    pub fn loops(&self) -> &[Loop] {
        match self {
            PlannedSection::Profile { loops, .. } => loops,
            PlannedSection::Curve(_) | PlannedSection::Point(_) => &[],
        }
    }

    /// The curve of a curve section.
    pub fn curve(&self) -> Option<&Curve> {
        match self {
            PlannedSection::Curve(c) => Some(c),
            _ => None,
        }
    }
}

/// Where the sections of a loft are, relative to each other.
#[derive(Clone, Debug)]
pub enum Layout {
    /// Planar sections stacked along a common normal: the kernel
    /// builds them in ascending order along it, whichever way the script
    /// listed them.
    Stacked {
        /// Common section normal, pointing along the first section's plane normal.
        normal: Vec3,
        /// A point on the first profile section's plane; offsets are measured
        /// from it along `normal`.
        origin: Vec3,
        /// Signed offset of each section along `normal`, in script order.
        offsets: Vec<f64>,
        /// True when the script listed the sections from high to low, so the
        /// kernel builds them the other way round. Band *b* is still "between
        /// script sections b and b+1", which is what the names say.
        descending: bool,
    },
    /// Sections in space: list order is the order, full stop. There
    /// is no normal, no offset and no "up"; band `b` is between the section
    /// the script listed `b`-th and the next.
    Ordered,
}

/// How one end of a ruled cap's ribbon is closed. The ribbon between the two
/// halves of a loop has a ruling of zero length at the seam and at the
/// half-way point — the two corners of the lens — and truck has no surface
/// that is invertible at a collapsed corner (it tessellates to no
/// triangles). So each corner is closed by the two segments that
/// meet there instead, in one of two ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonEnd {
    /// The two segments are collinear straight lines through the corner:
    /// together they *are* the first ruling of the neighbouring ribbon face,
    /// which takes both as its cross boundary. No face of its own.
    Straight,
    /// The two segments share a plane with the corner: a planar face bounded
    /// by the two of them and the straight chord between their far ends —
    /// a triangle with curved sides. Two arcs of one circle, an arc and a
    /// line in its plane, a flat corner of a spline.
    Flat,
}

/// The layout of a ruled cap on one end section.
///
/// The ribbon needs vertices the section was not drawn with — the half-way
/// split, and each half's counterparts of the other half's vertices — and
/// they are planted on **this section's wire only**. The side face that
/// meets the capped section takes the pieces of a cut segment as its boundary
/// and keeps the segment's own curve as its surface, so nothing about any
/// other section, any band's surface or any face name changes; the
/// alternative, cutting every section at the same fraction so the counts
/// stay equal, multiplied faces everywhere and, with a ruled cap at each end,
/// never settled — each end's cuts moved the other's mirror points.
#[derive(Clone, Debug, PartialEq)]
pub struct RuledCap {
    /// The section's loop with the ribbon's vertices planted: the same
    /// geometry as the section, cut exactly ([`Segment3::sub`]).
    pub curve: Curve,
    /// For each segment of `curve`, the index of the section's own segment it
    /// is a piece of.
    pub owner: Vec<usize>,
    /// The index of the segment of `curve` that starts at the half-way point:
    /// segments `0..split` are the first half, `split..` the second, and the
    /// two halves have equal counts.
    pub split: usize,
    /// How the ribbon closes at the seam (`ends[0]`) and at the half-way
    /// point (`ends[1]`).
    pub ends: [RibbonEnd; 2],
}

/// Validated sections plus the correspondence between them.
#[derive(Clone, Debug)]
pub struct LoftPlan {
    /// Sections in **script order**.
    pub sections: Vec<PlannedSection>,
    /// Material regions, as `(outer loop index, hole loop indices)`. Identical
    /// in every profile section — that is checked. A curve loft has one
    /// region, `(0, [])`.
    pub regions: Vec<(usize, Vec<usize>)>,
    /// Stacked along a normal, or in list order.
    pub layout: Layout,
    /// How the start and the end are closed.
    pub caps: [Cap; 2],
    /// The ruled cap's layout at the start and at the end, for the ends that
    /// are curve sections closed by [`Cap::Ruled`].
    pub ruled_caps: [Option<RuledCap>; 2],
    pub ruled: bool,
    /// The rule that produced the correspondence above.
    pub matching: SectionMatch,
    /// Non-fatal findings: the caller turns these into warning diagnostics.
    pub warnings: Vec<String>,
}

impl LoftPlan {
    /// True when the script listed stacked sections from high to low.
    pub fn descending(&self) -> bool {
        matches!(self.layout, Layout::Stacked { descending: true, .. })
    }

    /// Section indices in the order the kernel stacks them in: ascending
    /// along the normal for a stacked loft, script order for an ordered one.
    pub fn ascending(&self) -> Vec<usize> {
        let n = self.sections.len();
        if self.descending() { (0..n).rev().collect() } else { (0..n).collect() }
    }

    /// How many side bands the loft has: one per adjacent section pair.
    pub fn bands(&self) -> usize {
        self.sections.len() - 1
    }

    /// True for a loft in the ordered layout: curve sections, or ruled caps.
    pub fn is_ordered(&self) -> bool {
        matches!(self.layout, Layout::Ordered)
    }

    /// Check the sections and work out the correspondence, with both ends
    /// closed by a plane.
    pub fn new(sections: &[Section], ruled: bool, matching: SectionMatch) -> Result<LoftPlan> {
        LoftPlan::with_caps(sections, ruled, matching, [Cap::Plane, Cap::Plane])
    }

    /// Check the sections and work out the correspondence.
    ///
    /// Planar sketch sections closed by planes take the stacked path, exactly
    /// as before curve sections existed. Any curve section, or any ruled cap,
    /// takes the ordered path.
    pub fn with_caps(sections: &[Section], ruled: bool, matching: SectionMatch, caps: [Cap; 2]) -> Result<LoftPlan> {
        if sections.len() < 2 {
            return Err(CadpoleError::invalid(format!(
                "loft needs at least two sections, got {}",
                sections.len()
            )));
        }
        // ---- point sections may only be the ends
        for (i, s) in sections.iter().enumerate() {
            if matches!(s, Section::Point(_)) && i != 0 && i != sections.len() - 1 {
                return Err(CadpoleError::invalid(format!(
                    "loft section {i} is a point, but only the first and last section may be one \
                     (a point in the middle would pinch the solid to nothing)"
                )));
            }
        }
        let stacked = sections.iter().all(|s| !matches!(s, Section::Curve(_))) && caps == [Cap::Plane, Cap::Plane];
        if stacked { plan_stacked(sections, ruled, matching) } else { plan_ordered(sections, ruled, matching, caps) }
    }

    /// A short description for the feature tree.
    pub fn describe(&self) -> String {
        let kinds: Vec<&'static str> = self
            .sections
            .iter()
            .map(|s| match s {
                PlannedSection::Profile { .. } => "profile",
                PlannedSection::Curve(_) => "curve",
                PlannedSection::Point(_) => "point",
            })
            .collect();
        format!("{} sections ({})", kinds.len(), kinds.join(", "))
    }
}

/// The stacked layout: parallel planar sections along one normal.
fn plan_stacked(sections: &[Section], ruled: bool, matching: SectionMatch) -> Result<LoftPlan> {
    let first_profile = sections.iter().position(|s| matches!(s, Section::Profile(_)));
    let Some(fp) = first_profile else {
        return Err(CadpoleError::invalid("loft needs at least one profile section; two points are not a solid"));
    };

    // ---- a common plane normal
    let Section::Profile(ref_sketch) = &sections[fp] else { unreachable!() };
    let normal = ref_sketch.plane.normal.normalized();
    let origin = ref_sketch.plane.origin;
    for (i, s) in sections.iter().enumerate() {
        let Section::Profile(sk) = s else { continue };
        let n = sk.plane.normal.normalized();
        let sin = n.cross(normal).length();
        if sin > PARALLEL_EPS || n.dot(normal) <= 0.0 {
            return Err(CadpoleError::invalid(format!(
                "loft section {i} is not parallel to section {fp}: its plane normal is \
                 ({:.6}, {:.6}, {:.6}) and section {fp}'s is ({:.6}, {:.6}, {:.6}). Sketch sections \
                 must be parallel and face the same way — use one plane offset to several heights \
                 (XY.offset(z)) — or pass the sections as curves (Sketch.curves(), Path.on(plane), \
                 or curve()), which may lie anywhere in space",
                n.x, n.y, n.z, normal.x, normal.y, normal.z
            )));
        }
    }

    // ---- offsets, strictly monotonic in script order
    let offsets: Vec<f64> = sections
        .iter()
        .map(|s| match s {
            Section::Profile(sk) => (sk.plane.origin - origin).dot(normal),
            Section::Point(p) => (*p - origin).dot(normal),
            Section::Curve(_) => unreachable!("a curve section takes the ordered layout"),
        })
        .collect();
    let dir = offsets[1] - offsets[0];
    for w in 0..sections.len() - 1 {
        let gap = offsets[w + 1] - offsets[w];
        if gap.abs() <= MIN_GAP {
            return Err(CadpoleError::invalid(format!(
                "loft sections {w} and {} are at the same height ({:.6} mm along the section \
                 normal), so the band between them has no thickness",
                w + 1,
                offsets[w]
            )));
        }
        if gap * dir < 0.0 {
            return Err(CadpoleError::invalid(format!(
                "loft section {} is at {:.4} mm but section {w} is at {:.4} mm, which reverses \
                 the direction the loft was going in: list the sections in order along the \
                 loft axis (a loft that doubles back would pass through itself)",
                w + 1,
                offsets[w + 1],
                offsets[w]
            )));
        }
    }
    let descending = dir < 0.0;

    // ---- loops and regions have to agree
    let mut oriented: Vec<Option<Oriented>> = Vec::with_capacity(sections.len());
    let mut regions: Option<Vec<(usize, Vec<usize>)>> = None;
    for (i, s) in sections.iter().enumerate() {
        match s {
            Section::Point(_) => oriented.push(None),
            Section::Curve(_) => unreachable!("a curve section takes the ordered layout"),
            Section::Profile(sk) => {
                let (loops, regs) = orient(sk, i)?;
                match &regions {
                    None => regions = Some(regs.clone()),
                    Some(r0) => {
                        let first = oriented
                            .iter()
                            .flatten()
                            .next()
                            .expect("a profile section came before this one");
                        if first.loops.len() != loops.len() {
                            return Err(CadpoleError::invalid(format!(
                                "loft section {i} has {} loop(s) but section {fp} has {}: every \
                                 section must draw the same loops in the same order, so the \
                                 loft knows which one goes to which",
                                loops.len(),
                                first.loops.len()
                            )));
                        }
                        if *r0 != regs {
                            return Err(CadpoleError::invalid(format!(
                                "loft section {i} nests its loops differently from section \
                                 {fp} (which loop is a hole in which). Every section must have \
                                 the same regions and holes"
                            )));
                        }
                    }
                }
                oriented.push(Some(Oriented { plane: sk.plane, loops, regions: regs }));
            }
        }
    }
    let regions = regions.expect("at least one profile section");

    // ---- segment counts: subdivide to make them agree if asked, then insist
    if matching == SectionMatch::ArcLength {
        match_by_arc_length(&mut oriented)?;
    }
    let mut seg_counts: Option<Vec<usize>> = None;
    for (i, o) in oriented.iter().enumerate() {
        let Some(o) = o else { continue };
        let counts: Vec<usize> = o.loops.iter().map(|l| l.segments.len()).collect();
        match &seg_counts {
            None => seg_counts = Some(counts),
            Some(c0) => {
                for (l, (a, b)) in c0.iter().zip(&counts).enumerate() {
                    if a != b {
                        return Err(count_mismatch(i, l, *b, fp, *a));
                    }
                }
            }
        }
    }
    let mut planned: Vec<PlannedSection> = Vec::with_capacity(sections.len());
    let mut oriented = oriented.into_iter();
    for s in sections {
        match (s, oriented.next().expect("one entry per section")) {
            (Section::Point(p), _) => planned.push(PlannedSection::Point(*p)),
            (Section::Profile(_), Some(o)) => planned.push(PlannedSection::Profile {
                plane: o.plane,
                loops: o.loops,
                regions: o.regions,
            }),
            (Section::Profile(_), None) => unreachable!("a profile section was oriented"),
            (Section::Curve(_), _) => unreachable!("a curve section takes the ordered layout"),
        }
    }
    if regions.is_empty() {
        return Err(CadpoleError::invalid("loft section 0 has no closed region to loft"));
    }
    check_point_sections(sections, &regions)?;

    // Run the twist check on the *matched* loops: after `match="arclen"`
    // the counts agree, so a loft whose sections start at different corners
    // is caught here too, and the answer is the same one — draw them from
    // the same corner — because arc length is measured from the seam.
    let warnings = twist_warning(&planned);
    Ok(LoftPlan {
        sections: planned,
        regions,
        layout: Layout::Stacked { normal, origin, offsets, descending },
        caps: [Cap::Plane, Cap::Plane],
        ruled_caps: [None, None],
        ruled,
        matching,
        warnings,
    })
}

/// The diagnostic for two sections whose loop `l` has different segment counts.
fn count_mismatch(i: usize, l: usize, got: usize, fp: usize, want: usize) -> CadpoleError {
    CadpoleError::invalid(format!(
        "loft section {i}'s loop {l} has {got} segment(s) but section {fp}'s has {want}: a ruled \
         loft joins segment to segment, so the counts must match. Either draw both loops the same \
         way (a rounded_rect with r = 0 is a 4-segment rectangle, not an 8-segment rounded one — \
         give it a small radius instead), or pass match=\"arclen\" to have the loft split the \
         coarser loop at the other's vertices, measured as a fraction of the way round"
    ))
}

/// A point section is one point, so it can only cap one region, and a hole
/// has nowhere to go.
fn check_point_sections(sections: &[Section], regions: &[(usize, Vec<usize>)]) -> Result<()> {
    let any_point = sections.iter().any(|s| matches!(s, Section::Point(_)));
    if any_point && regions.len() != 1 {
        return Err(CadpoleError::invalid(format!(
            "loft ends in a point but its profile sections have {} separate regions: a single \
             point cannot close more than one of them",
            regions.len()
        )));
    }
    if any_point && !regions[0].1.is_empty() {
        return Err(CadpoleError::invalid(
            "loft ends in a point but its profile has a hole in it: the hole has nowhere to go",
        ));
    }
    Ok(())
}

/// The ordered layout: sections in space, in list order.
fn plan_ordered(sections: &[Section], ruled: bool, matching: SectionMatch, caps: [Cap; 2]) -> Result<LoftPlan> {
    let n = sections.len();
    let first_curve = sections.iter().position(|s| !matches!(s, Section::Point(_)));
    let Some(fc) = first_curve else {
        return Err(CadpoleError::invalid("loft needs at least one profile section; two points are not a solid"));
    };

    // ---- every section is one closed curve, or a point
    let mut curves: Vec<Option<Curve>> = Vec::with_capacity(n);
    for (i, s) in sections.iter().enumerate() {
        match s {
            Section::Point(_) => curves.push(None),
            Section::Curve(c) => {
                if !c.closed {
                    return Err(CadpoleError::invalid(format!(
                        "loft section {i} is an open curve: close it with .close() first. A Cadpole body \
                         is a solid, so every section must be a closed loop; open sections would make \
                         a surface, which Cadpole does not have"
                    )));
                }
                if c.segments.len() < 2 {
                    return Err(CadpoleError::invalid(format!(
                        "loft section {i} has {} segment(s); a closed section needs at least two",
                        c.segments.len()
                    )));
                }
                curves.push(Some(c.clone()));
            }
            Section::Profile(sk) => {
                if sk.loops.len() != 1 {
                    return Err(CadpoleError::invalid(format!(
                        "loft section {i} is a sketch with {} loops, but this loft has a section in \
                         space (a curve), which is exactly one loop, so the sections cannot pair. A \
                         curve section carries no holes: loft each loop separately and subtract the \
                         inner solids, or make every section a sketch on a parallel plane",
                        sk.loops.len()
                    )));
                }
                if !sk.loops[0].is_closed(1e-9) {
                    return Err(CadpoleError::invalid(format!("loft section {i}: loop 0 is not closed")));
                }
                curves.push(Some(Curve::from_loop(&sk.loops[0], &sk.plane)));
            }
        }
    }

    // ---- consecutive sections must not coincide
    for w in 0..n - 1 {
        let (Some(a), Some(b)) = (&curves[w], &curves[w + 1]) else { continue };
        let (pa, pb) = (a.sample_fractions(16), b.sample_fractions(16));
        let apart = pa.iter().zip(&pb).map(|(p, q)| (*p - *q).length()).fold(0.0f64, f64::max);
        if apart <= MIN_GAP {
            return Err(CadpoleError::invalid(format!(
                "loft sections {w} and {} are the same curve, so the band between them has no \
                 thickness",
                w + 1
            )));
        }
    }

    // ---- segment counts: subdivide to make them agree if asked, then insist
    if matching == SectionMatch::ArcLength {
        match_curves_by_arc_length(&mut curves)?;
    }
    let want = curves[fc].as_ref().expect("a curve").segments.len();
    for (i, c) in curves.iter().enumerate() {
        let Some(c) = c else { continue };
        if c.segments.len() != want {
            return Err(count_mismatch(i, 0, c.segments.len(), fc, want));
        }
    }

    // ---- caps: a plane needs a planar end; a ruled cap needs the halves to pair
    let ends = [0usize, n - 1];
    for (e, &i) in ends.iter().enumerate() {
        let Some(c) = &curves[i] else { continue };
        if caps[e] == Cap::Plane && c.plane().is_none() {
            let which = if e == 0 { "start" } else { "end" };
            let other = if e == 0 { "(\"ruled\", \"plane\")" } else { "(\"plane\", \"ruled\")" };
            return Err(CadpoleError::invalid(format!(
                "loft section {i} (the {which}) does not lie in a plane (its points leave their best-fit \
                 plane by up to {:.4} mm), so cap=\"plane\" cannot close it. Pass cap=\"ruled\" to rule \
                 the loop against itself — or cap={other} to close only this end that way",
                planar_deviation(c)
            )));
        }
    }
    let mut ruled_caps: [Option<RuledCap>; 2] = [None, None];
    for (e, &i) in ends.iter().enumerate() {
        if caps[e] != Cap::Ruled {
            continue;
        }
        if let Some(c) = &curves[i] {
            ruled_caps[e] = Some(ruled_cap_layout(c, i)?);
        }
    }

    // ---- warnings
    let planned: Vec<PlannedSection> = sections
        .iter()
        .zip(curves)
        .map(|(s, c)| match (s, c) {
            (Section::Point(p), _) => PlannedSection::Point(*p),
            (_, Some(c)) => PlannedSection::Curve(c),
            (_, None) => unreachable!("every non-point section is a curve"),
        })
        .collect();
    let mut warnings = twist_warning_ordered(&planned);
    for (e, &i) in ends.iter().enumerate() {
        if caps[e] != Cap::Ruled {
            continue;
        }
        if let Some(c) = planned[i].curve() {
            warnings.extend(seam_warning(c, i));
        }
    }
    check_point_sections(sections, &[(0, Vec::new())])?;
    Ok(LoftPlan {
        sections: planned,
        regions: vec![(0, Vec::new())],
        layout: Layout::Ordered,
        caps,
        ruled_caps,
        ruled,
        matching,
        warnings,
    })
}

/// How far a curve's control points stray from their best-fit plane, in mm.
fn planar_deviation(c: &Curve) -> f64 {
    let pts: Vec<Vec3> = c.segments.iter().flat_map(|s| s.control_points()).collect();
    if pts.len() < 3 {
        return 0.0;
    }
    let centroid = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / pts.len() as f64);
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        n = n + (pts[i] - centroid).cross(pts[(i + 1) % pts.len()] - centroid);
    }
    if n.length() < 1e-12 {
        return 0.0;
    }
    let n = n.normalized();
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        let h = (*p - centroid).dot(n);
        (lo.min(h), hi.max(h))
    });
    hi - lo
}

/// The ruled cap on section `i`: the section's loop with the
/// ribbon's vertices planted, where it splits, and how the two corners close.
///
/// The split is the vertex nearest half-way round by arc length when one is
/// within [`SNAP_MM`] of it, else a new vertex exactly half-way,
/// so a section drawn with a vertex on its nose keeps it. The two
/// halves are then matched the way two sections are ([`match_positions`]):
/// each walked from the seam, the first forwards to the split and the second
/// backwards to it, and cut at the union of each other's vertices, snapped.
/// Every cut is exact, and every cut is on this loop only.
fn ruled_cap_layout(c: &Curve, i: usize) -> Result<RuledCap> {
    let m = c.segments.len();
    let cum = c.cumulative_lengths();
    let total = cum[m];
    if !(total > MIN_PIECE_MM) {
        return Err(CadpoleError::invalid(format!(
            "loft section {i} is {total:.6} mm round, which is nothing to close with a ruled cap"
        )));
    }
    let what = format!("loft section {i}'s loop 0");
    // ---- the split: an existing vertex near half-way, or a new one there
    let half = total / 2.0;
    let nearest = (1..m).min_by(|a, b| {
        (cum[*a] - half).abs().partial_cmp(&(cum[*b] - half).abs()).expect("finite lengths")
    });
    let split_pos = match nearest {
        Some(k) if (cum[k] - half).abs() <= SNAP_MM => cum[k],
        _ => half,
    };
    let is_at = |list: &[f64], s: f64| list.iter().any(|c| (c - s).abs() <= BOUNDARY_EPS);
    let mut positions: Vec<f64> = cum[..m].to_vec();
    if !is_at(&positions, split_pos) {
        positions.push(split_pos);
        positions.sort_by(|a, b| a.partial_cmp(b).expect("finite lengths"));
    }
    let cut1 = c.split_at(&positions, &what)?;
    let cum1 = cut1.cumulative_lengths();
    let m1 = cut1.segments.len();
    let k1 = (1..m1)
        .find(|&v| (cum1[v] - split_pos).abs() <= BOUNDARY_EPS)
        .ok_or_else(|| CadpoleError::kernel(format!("{what}: the half-way vertex went missing")))?;

    // ---- the two halves, matched like two sections
    let cum_a: Vec<f64> = cum1[..=k1].to_vec();
    let cum_b: Vec<f64> = (k1..=m1).rev().map(|v| total - cum1[v]).collect();
    let names = [
        format!("loft section {i}'s first half (seam to half-way point)"),
        format!("loft section {i}'s second half (half-way point back to the seam)"),
    ];
    let matched = match_positions(&[cum_a, cum_b], &names)?;
    // Both halves' positions on the loop: the first half's as they are, the
    // second half's measured back from the seam. Each list starts at the
    // seam (0), which on the second half is the loop's end, so the seam is
    // kept once, at 0, and the split once.
    let mut all: Vec<f64> = vec![0.0];
    all.extend(matched[0].iter().copied());
    all.extend(matched[1].iter().map(|s| total - s));
    all.push(split_pos);
    all.retain(|s| *s > BOUNDARY_EPS && *s < total - BOUNDARY_EPS);
    all.insert(0, 0.0);
    all.sort_by(|a, b| a.partial_cmp(b).expect("finite lengths"));
    all.dedup_by(|a, b| (*a - *b).abs() <= BOUNDARY_EPS);
    let curve = cut1.split_at(&all, &what)?;
    let cum2 = curve.cumulative_lengths();
    let m2 = curve.segments.len();
    let split = (1..m2)
        .find(|&v| (cum2[v] - split_pos).abs() <= BOUNDARY_EPS)
        .ok_or_else(|| CadpoleError::kernel(format!("{what}: the half-way vertex went missing")))?;
    if split != m2 - split {
        return Err(CadpoleError::kernel(format!(
            "{what}: cap=\"ruled\" left halves of {split} and {} segments after matching",
            m2 - split
        )));
    }
    // Which of the section's own segments each piece belongs to.
    let owner: Vec<usize> = (0..m2)
        .map(|v| cum.partition_point(|s| *s <= cum2[v] + BOUNDARY_EPS).saturating_sub(1).min(m - 1))
        .collect();

    // ---- how the two corners close
    let seam = ribbon_end(&curve.segments[0], &curve.segments[m2 - 1].reversed(), i, "seam")?;
    let far = ribbon_end(&curve.segments[split - 1].reversed(), &curve.segments[split], i, "half-way point")?;
    let straight = seam == RibbonEnd::Straight || far == RibbonEnd::Straight;
    if split < 2 || (straight && split < 3) {
        return Err(CadpoleError::invalid(format!(
            "loft section {i}: cap=\"ruled\" needs at least {} segments in each half of the loop \
             to build the ribbon, and this loop has {split}. Draw the section with more segments \
             (a spline through more points), or close this end with a plane",
            if straight { 3 } else { 2 }
        )));
    }
    Ok(RuledCap { curve, owner, split, ends: [seam, far] })
}

/// How a ribbon corner closes, from the two segments that leave it (`x` and
/// `y` both start at the corner).
fn ribbon_end(x: &Segment3, y: &Segment3, i: usize, where_: &str) -> Result<RibbonEnd> {
    let corner = x.start();
    if let (Segment3::Line { b: xb, .. }, Segment3::Line { b: yb, .. }) = (x, y) {
        let (dx, dy) = ((*xb - corner).normalized(), (*yb - corner).normalized());
        if dx.cross(dy).length() <= 1e-9 {
            if dx.dot(dy) < 0.0 {
                return Ok(RibbonEnd::Straight);
            }
            return Err(CadpoleError::invalid(format!(
                "loft section {i}: at the {where_} of the loop the two segments run back along each \
                 other, so the loop touches itself there and no cap can close it"
            )));
        }
    }
    // Coplanar with the corner: a plane through the corner that holds every
    // control point of both, within the kernel's tolerance. The face's
    // outline is the corner, out along `x`, across the chord, and back along
    // `y`, so that polygon's area vector is the plane's normal.
    let pts: Vec<Vec3> = std::iter::once(corner)
        .chain(x.control_points().into_iter().skip(1))
        .chain(y.control_points().into_iter().skip(1).rev())
        .collect();
    let mut n = Vec3::ZERO;
    for p in 0..pts.len() {
        n = n + (pts[p] - corner).cross(pts[(p + 1) % pts.len()] - corner);
    }
    let scale = pts.iter().map(|p| (*p - corner).length()).fold(0.0f64, f64::max).max(1.0);
    if n.length() <= 1e-12 * scale * scale {
        return Err(CadpoleError::invalid(format!(
            "loft section {i}: at the {where_} of the loop the two segments are collinear, so the \
             face closing that corner of the ruled cap has no area"
        )));
    }
    let n = n.normalized();
    let off = pts.iter().map(|p| (*p - corner).dot(n).abs()).fold(0.0f64, f64::max);
    if off > PLANAR_TOL {
        return Err(CadpoleError::invalid(format!(
            "loft section {i}: cap=\"ruled\" closes the ribbon's corner at the {where_} with a flat \
             face, but the two segments meeting there do not share a plane (they leave any common \
             plane by {off:.4} mm), so that corner has no exact surface. Start the loop on a straight \
             segment or at a flat corner — a blade section drawn from the middle of its trailing edge, \
             with a circular nose, satisfies this at both ends — or close this end with a plane"
        )));
    }
    Ok(RibbonEnd::Flat)
}

/// Would the ruled cap's ribbon have been much narrower with the seam
/// somewhere else on the loop? Sampled at [`SEAM_SAMPLES`] points by arc
/// length: the ribbon for a seam at sample `k` pairs sample `k + j` with
/// sample `k - j`, and its width is the sum of those rulings. A thin section
/// drawn from one end of its thin direction has a ribbon as wide as it is
/// thick; drawn from the middle of a flank it rules the loop against itself
/// lengthwise, and the ratio says so. A warning, never a re-seam: the seam is
/// the script's.
fn seam_warning(c: &Curve, i: usize) -> Vec<String> {
    let pts = c.sample_fractions(SEAM_SAMPLES);
    let n = pts.len();
    let width = |k: usize| -> f64 {
        (1..n / 2).map(|j| (pts[(k + j) % n] - pts[(k + n - j) % n]).length()).sum()
    };
    let w0 = width(0);
    let (mut best_k, mut best) = (0usize, w0);
    for k in 1..n {
        let w = width(k);
        if w < best {
            best = w;
            best_k = k;
        }
    }
    if best_k != 0 && w0 > best * SEAM_RATIO && best > 1e-12 {
        let p = pts[best_k];
        vec![format!(
            "loft: cap=\"ruled\" on section {i} rules the loop against itself from its seam, and the \
             ribbon that closes it is {:.1}x wider than it would be with the loop started at \
             ({:.3}, {:.3}, {:.3}), {:.0}% of the way round — the seam is in the wrong place. Start \
             the loop at one end of the section's thin direction (a blade section at the middle of \
             its trailing edge)",
            w0 / best,
            p.x,
            p.y,
            p.z,
            100.0 * best_k as f64 / n as f64
        )]
    } else {
        Vec::new()
    }
}

/// A profile section after [`orient`], before it becomes a [`PlannedSection`].
/// The matcher rewrites `loops` in place, which is the only thing that ever
/// changes between a section as written and a section as built.
struct Oriented {
    plane: Plane,
    loops: Vec<Loop>,
    regions: Vec<(usize, Vec<usize>)>,
}

/// Subdivide the sections' loops until every section's loop `l` has the same
/// number of segments. `None` entries are point sections
/// and take no part.
///
/// Done **per loop index across every section at once**, not pairwise: a
/// section in the middle of a stack is one wire shared by the band below it and
/// the band above, so the two bands have to agree about where its vertices are.
fn match_by_arc_length(sections: &mut [Option<Oriented>]) -> Result<()> {
    let ids: Vec<usize> = sections
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|_| i))
        .collect();
    if ids.len() < 2 {
        return Ok(());
    }
    let loop_count = sections[ids[0]].as_ref().expect("a profile").loops.len();
    for l in 0..loop_count {
        let counts: Vec<usize> = ids
            .iter()
            .map(|&k| sections[k].as_ref().expect("a profile").loops[l].segments.len())
            .collect();
        // Sections that already agree are left exactly as written: `match=`
        // only ever refines, so an existing loft is bit-for-bit unchanged.
        if counts.iter().all(|c| *c == counts[0]) {
            continue;
        }
        let originals: Vec<Loop> = ids
            .iter()
            .map(|&k| sections[k].as_ref().expect("a profile").loops[l].clone())
            .collect();
        let matched = match_one_loop(&originals, &ids, l)?;
        for (&k, nl) in ids.iter().zip(matched) {
            sections[k].as_mut().expect("a profile").loops[l] = nl;
        }
    }
    Ok(())
}

/// [`match_by_arc_length`] for curve sections: the same rule on the same
/// parameter, cutting with [`Segment3::sub`].
fn match_curves_by_arc_length(curves: &mut [Option<Curve>]) -> Result<()> {
    let ids: Vec<usize> = curves
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_ref().map(|_| i))
        .collect();
    if ids.len() < 2 {
        return Ok(());
    }
    let counts: Vec<usize> = ids.iter().map(|&k| curves[k].as_ref().expect("a curve").segments.len()).collect();
    if counts.iter().all(|c| *c == counts[0]) {
        return Ok(());
    }
    let cums: Vec<Vec<f64>> = ids.iter().map(|&k| curves[k].as_ref().expect("a curve").cumulative_lengths()).collect();
    let names: Vec<String> = ids.iter().map(|k| format!("loft section {k}'s loop 0")).collect();
    let positions = match_positions(&cums, &names)?;
    for (n, &k) in ids.iter().enumerate() {
        let c = curves[k].as_ref().expect("a curve");
        curves[k] = Some(c.split_at(&positions[n], &names[n])?);
    }
    Ok(())
}

/// One vertex of one section's loop, placed on the shared parameter.
///
/// `fraction` is arc length from the loop's own seam divided by the loop's own
/// perimeter. That is the whole of the correspondence: two points of two
/// sections are the same point of the loft when they are the same fraction of
/// the way round.
#[derive(Clone, Copy, Debug)]
struct Mark {
    fraction: f64,
    section: usize,
    vertex: usize,
}

/// Rebuild one loop of every section so they all have the same segment count.
///
/// `ls` are the loops in section order, `ids` their script section indices (for
/// diagnostics), `l` the loop index.
fn match_one_loop(ls: &[Loop], ids: &[usize], l: usize) -> Result<Vec<Loop>> {
    let cums: Vec<Vec<f64>> = ls.iter().map(Loop::cumulative_lengths).collect();
    let names: Vec<String> = ids.iter().map(|k| format!("loft section {k}'s loop {l}")).collect();
    let positions = match_positions(&cums, &names)?;
    let mut out = Vec::with_capacity(ls.len());
    for k in 0..ls.len() {
        out.push(split_loop(&ls[k], &cums[k], &positions[k], ids[k], l)?);
    }
    Ok(out)
}

/// Where each of several loops is cut so that they all have the same number
/// of pieces: the arc lengths, per loop, ascending from 0. `cums` are
/// the loops' cumulative lengths (`n + 1` entries each), `names` describe them
/// for diagnostics. This is the matcher with the geometry left out, so a 2D
/// loop and a 3D curve match by the same code.
fn match_positions(cums: &[Vec<f64>], names: &[String]) -> Result<Vec<Vec<f64>>> {
    let n = cums.len();
    let totals: Vec<f64> = cums.iter().map(|c| *c.last().expect("a closed loop")).collect();
    for (k, total) in totals.iter().enumerate() {
        if !total.is_finite() || *total <= MIN_PIECE_MM {
            return Err(CadpoleError::invalid(format!(
                "{} is {total:.6} mm round, which is nothing to loft; matching sections by arc \
                 length needs a loop with a length",
                names[k]
            )));
        }
    }

    // Every vertex of every section, on the shared parameter.
    let mut marks: Vec<Mark> = Vec::new();
    for (k, cum) in cums.iter().enumerate() {
        for (v, s) in cum[..cum.len() - 1].iter().enumerate() {
            marks.push(Mark { fraction: s / totals[k], section: k, vertex: v });
        }
    }
    // Sorted by fraction, and by section then vertex when two fractions are
    // equal to the bit. The tie-break is what makes the result reproducible:
    // it never depends on the order `Vec::sort` happened to leave equal keys
    // in, only on which section the script listed first.
    marks.sort_by(|a, b| {
        a.fraction
            .partial_cmp(&b.fraction)
            .expect("finite arc lengths")
            .then(a.section.cmp(&b.section))
            .then(a.vertex.cmp(&b.vertex))
    });

    // Snap: vertices within SNAP_MM of each other are one split point. The
    // threshold is a fraction of the *longest* loop, so the promise "a split
    // moves by at most SNAP_MM" holds on every section, not just the big one.
    let longest = totals.iter().copied().fold(0.0f64, f64::max);
    let snap = SNAP_MM / longest;
    let mut clusters: Vec<Vec<Mark>> = Vec::new();
    for (i, m) in marks.iter().copied().enumerate() {
        let joins = clusters.last().is_some_and(|c: &Vec<Mark>| {
            // A cluster is measured from its *first* member, never from a
            // running mean, so it can never be wider than `snap` however many
            // vertices fall into it — no chain of near-misses can drag a split
            // point away.
            if m.fraction > c[0].fraction + snap {
                return false;
            }
            // A section contributes at most one vertex to a cluster. Two
            // vertices of one loop are two points of that loop whatever their
            // spacing, and collapsing them would silently delete a segment.
            if c.iter().any(|x| x.section == m.section) {
                return false;
            }
            // And a vertex goes to the split point it is *closest* to. Without
            // this it could be pulled back into a cluster it is further from
            // and leave the one it belongs to with no vertex of its own to sit
            // on, which collapses a piece to zero length.
            match marks.get(i + 1) {
                Some(next) if next.section != m.section => {
                    m.fraction - c[0].fraction <= next.fraction - m.fraction
                }
                _ => true,
            }
        });
        if joins {
            clusters.last_mut().expect("a cluster").push(m);
        } else {
            clusters.push(vec![m]);
        }
    }

    // Where each section is cut. A section that owns a vertex in a cluster
    // keeps its *own* vertex exactly — the matcher never moves geometry that
    // was already there, it only adds cuts to the sections that lack one.
    let events = clusters.len();
    let mut pos: Vec<Vec<f64>> = vec![Vec::with_capacity(events); n];
    for c in &clusters {
        let rep = c[0].fraction;
        for (k, p) in pos.iter_mut().enumerate() {
            p.push(match c.iter().find(|m| m.section == k) {
                Some(m) => cums[k][m.vertex],
                None => rep * totals[k],
            });
        }
    }

    for k in 0..n {
        for e in 0..events {
            let a = pos[k][e];
            let b = if e + 1 < events { pos[k][e + 1] } else { totals[k] };
            if b - a < MIN_PIECE_MM {
                return Err(CadpoleError::invalid(format!(
                    "{} would be cut into a piece {:.6} mm long, {a:.4} mm round a loop of \
                     {:.4} mm, which is not a face the kernel can build. Two of the sections' \
                     vertices land within {SNAP_MM} mm of each other without being close enough to \
                     be treated as one: draw the sections so their vertices either coincide or are \
                     further apart than that, or give every section the same segment count and \
                     match in construction order (match=\"index\")",
                    names[k],
                    b - a,
                    totals[k]
                )));
            }
        }
    }
    Ok(pos)
}

/// Cut one loop at the arc lengths in `pos` (ascending, `pos[0] == 0`), giving
/// one segment per entry. Every piece is an exact piece of the segment it came
/// from — see [`Segment::sub`].
fn split_loop(
    l: &Loop,
    cum: &[f64],
    pos: &[f64],
    section: usize,
    loop_index: usize,
) -> Result<Loop> {
    let total = *cum.last().expect("a closed loop");
    let mut segments = Vec::with_capacity(pos.len());
    for e in 0..pos.len() {
        let s0 = pos[e];
        let s1 = if e + 1 < pos.len() { pos[e + 1] } else { total };
        // Every vertex of this loop is one of the split points, so the piece
        // between two consecutive split points lies inside a single segment.
        let j = cum
            .partition_point(|c| *c <= s0 + BOUNDARY_EPS)
            .saturating_sub(1)
            .min(l.segments.len() - 1);
        if s1 > cum[j + 1] + BOUNDARY_EPS {
            return Err(CadpoleError::invalid(format!(
                "loft section {section}'s loop {loop_index}: matching by arc length produced a \
                 piece from {s0:.6} mm to {s1:.6} mm that spans more than one segment of the \
                 loop, so it has no single exact curve. This is a loop whose vertices the \
                 matcher could not order; give every section the same segment count and match \
                 in construction order (match=\"index\")"
            )));
        }
        let piece = l.segments[j].sub(s0 - cum[j], s1 - cum[j]).ok_or_else(|| {
            CadpoleError::invalid(format!(
                "loft section {section}'s loop {loop_index}: segment {j} cannot be cut between \
                 {:.6} mm and {:.6} mm of its length",
                s0 - cum[j],
                s1 - cum[j]
            ))
        })?;
        segments.push(piece);
    }
    Ok(Loop { segments })
}

/// Orient a sketch's loops for lofting and report its region structure.
/// Outer loops come back counter-clockwise seen from `+normal`, holes
/// clockwise, at their original sketch indices.
fn orient(sk: &Sketch, which: usize) -> Result<(Vec<Loop>, Vec<(usize, Vec<usize>)>)> {
    let regions = sk
        .regions_indexed()
        .map_err(|e| CadpoleError::invalid(format!("loft section {which}: {e}")))?;
    let mut loops = sk.loops.clone();
    let mut structure = Vec::new();
    for r in &regions {
        let (oi, ol) = &r.outer;
        loops[*oi] = ol.clone();
        let mut holes = Vec::new();
        for (hi, hl) in &r.holes {
            loops[*hi] = hl.clone();
            holes.push(*hi);
        }
        holes.sort_unstable();
        structure.push((*oi, holes));
    }
    structure.sort_unstable();
    Ok((loops, structure))
}

/// Would a different seam have fitted much better? Index pairing is still what
/// gets built — a geometric matcher that could change its mind between two
/// evaluations is exactly what we are avoiding — but if rotating one loop by
/// *k* segments would have halved the corresponding-vertex distance, the script
/// almost certainly started the two loops at different corners, and saying so
/// is more useful than quietly building a twisted solid.
fn twist_warning(sections: &[PlannedSection]) -> Vec<String> {
    let mut out = Vec::new();
    for w in 0..sections.len().saturating_sub(1) {
        let (a, b) = (&sections[w], &sections[w + 1]);
        let (PlannedSection::Profile { loops: la, .. }, PlannedSection::Profile { loops: lb, .. }) = (a, b)
        else {
            continue;
        };
        for (l, (loop_a, loop_b)) in la.iter().zip(lb).enumerate() {
            let n = loop_a.segments.len();
            if n < 3 || n != loop_b.segments.len() {
                continue;
            }
            let va: Vec<(f64, f64)> = loop_a.segments.iter().map(|s| (s.start().x, s.start().y)).collect();
            let vb: Vec<(f64, f64)> = loop_b.segments.iter().map(|s| (s.start().x, s.start().y)).collect();
            let ca = centroid(&va);
            let cb = centroid(&vb);
            let cost = |k: usize| -> f64 {
                (0..n)
                    .map(|j| {
                        let p = (va[j].0 - ca.0, va[j].1 - ca.1);
                        let q = (vb[(j + k) % n].0 - cb.0, vb[(j + k) % n].1 - cb.1);
                        fmath::powi(p.0 - q.0, 2) + fmath::powi(p.1 - q.1, 2)
                    })
                    .sum()
            };
            let c0 = cost(0);
            let (mut best_k, mut best) = (0usize, c0);
            for k in 1..n {
                let c = cost(k);
                if c < best {
                    best = c;
                    best_k = k;
                }
            }
            if best_k != 0
                && c0 > best * TWIST_RATIO
                && c0 > 1e-12
                && !deliberate_turn(&va, ca, &vb, cb, best_k)
            {
                out.push(rotation_warning(l, w, best_k, c0 / best.max(1e-12)));
            }
        }
    }
    out
}

/// The best rigid turn of `a` (about `ca`) onto `b` (about `cb`) with vertex
/// `j` of `a` paired to vertex `j + k` of `b`: its angle in radians and the
/// summed squared distance left after it.
fn rigid_turn(a: &[(f64, f64)], ca: (f64, f64), b: &[(f64, f64)], cb: (f64, f64), k: usize) -> (f64, f64) {
    let n = a.len();
    let (mut dot, mut cross, mut aa, mut bb) = (0.0, 0.0, 0.0, 0.0);
    for j in 0..n {
        let p = (a[j].0 - ca.0, a[j].1 - ca.1);
        let q = (b[(j + k) % n].0 - cb.0, b[(j + k) % n].1 - cb.1);
        dot += p.0 * q.0 + p.1 * q.1;
        cross += p.0 * q.1 - p.1 * q.0;
        aa += p.0 * p.0 + p.1 * p.1;
        bb += q.0 * q.0 + q.1 * q.1;
    }
    let angle = fmath::atan2(cross, dot);
    let left = (aa + bb - 2.0 * fmath::hypot(dot, cross)).max(0.0);
    (angle, left / aa.max(1e-300))
}

/// Is section `b` section `a` turned as a whole, vertex for vertex, by a
/// smaller angle than the one the better-fitting seam `best_k` would need?
/// That is a loft twisted on purpose -- a helical gear's outline turned a
/// little at every section -- and pairing it in construction order is right,
/// however well a shifted seam happens to fit a fine, nearly regular loop. A
/// loop drawn from another corner is still caught: its shifted seam fits
/// with no turn at all.
fn deliberate_turn(a: &[(f64, f64)], ca: (f64, f64), b: &[(f64, f64)], cb: (f64, f64), best_k: usize) -> bool {
    const RIGID: f64 = 1e-9; // squared residual relative to the loop's own spread
    let (turn0, left0) = rigid_turn(a, ca, b, cb, 0);
    if left0 > RIGID {
        return false;
    }
    let (turn_k, left_k) = rigid_turn(a, ca, b, cb, best_k);
    left_k > RIGID || turn0.abs() < turn_k.abs()
}

fn rotation_warning(l: usize, w: usize, best_k: usize, ratio: f64) -> String {
    format!(
        "loft: loop {l} of section {w} and of section {} are paired segment 0 to \
         segment 0, but they would fit {ratio:.1}x better rotated by {best_k} segment(s) — \
         the loft is probably twisted. Sections are always paired in construction \
         order (never by a geometric search, which could re-pair itself when a \
         parameter moves), so draw the two loops starting from the same corner",
        w + 1
    )
}

/// [`twist_warning`] for curve sections: the same test on the vertices in
/// space — total squared distance between paired vertices, centroid-relative,
/// for every rotation by *k* segments — and one more that only a 3D section
/// can get wrong, because nothing normalises its direction: the loop drawn
/// the other way round. That fits better reversed than any rotation, and the
/// fix is `.reverse()`, not a different corner.
fn twist_warning_ordered(sections: &[PlannedSection]) -> Vec<String> {
    let mut out = Vec::new();
    for w in 0..sections.len().saturating_sub(1) {
        let (Some(a), Some(b)) = (sections[w].curve(), sections[w + 1].curve()) else { continue };
        let va = a.vertices();
        let vb = b.vertices();
        let n = va.len();
        if n < 3 || n != vb.len() {
            continue;
        }
        let ca = va.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / n as f64);
        let cb = vb.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / n as f64);
        let cost = |k: usize, reversed: bool| -> f64 {
            (0..n)
                .map(|j| {
                    let p = va[j] - ca;
                    let idx = if reversed { (n - ((j + k) % n)) % n } else { (j + k) % n };
                    let q = vb[idx] - cb;
                    (p - q).dot(p - q)
                })
                .sum()
        };
        let c0 = cost(0, false);
        let (mut best_k, mut best) = (0usize, c0);
        for k in 1..n {
            let c = cost(k, false);
            if c < best {
                best = c;
                best_k = k;
            }
        }
        let (mut rev_k, mut rev_best) = (0usize, f64::MAX);
        for k in 0..n {
            let c = cost(k, true);
            if c < rev_best {
                rev_best = c;
                rev_k = k;
            }
        }
        if c0 <= 1e-12 {
            continue;
        }
        if rev_best < best && c0 > rev_best * TWIST_RATIO {
            out.push(format!(
                "loft: section {} runs the opposite way round from section {w}: paired as drawn the \
                 sections fit {:.1}x worse than with section {} reversed{}, so the bands between them \
                 would cross — the loft is probably twisted. Sections are paired in construction \
                 order and in the direction they were drawn (nothing in space says which way round is \
                 right), so draw both loops the same way round, or reverse one with .reverse()",
                w + 1,
                c0 / rev_best.max(1e-12),
                w + 1,
                if rev_k == 0 { String::new() } else { format!(" and rotated by {rev_k} segment(s)") }
            ));
        } else if best_k != 0 && c0 > best * TWIST_RATIO {
            out.push(rotation_warning(0, w, best_k, c0 / best.max(1e-12)));
        }
    }
    out
}

fn centroid(pts: &[(f64, f64)]) -> (f64, f64) {
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    (sx / n, sy / n)
}

/// Points on every face of a ruled cap on `c`, in the order the kernel builds
/// them (`kernel::ribbon`), so the namer can tell the pieces of `start` and
/// `end` apart: `per_segment` samples along each paired segment, spread across
/// the ribbon at `fractions`. A corner closed by a flat face gets the points
/// between its two segments; a corner that is straight has no face and
/// contributes nothing.
pub fn ruled_cap_points(rc: &RuledCap, per_segment: usize, fractions: &[f64]) -> Vec<Vec<Vec3>> {
    let c = &rc.curve;
    let m = c.segments.len();
    let k = rc.split;
    if m != 2 * k || k < 2 {
        return Vec::new();
    }
    let along = |seg: &Segment3, reversed: bool| -> Vec<Vec3> {
        (0..per_segment)
            .map(|i| {
                let t = (i as f64 + 0.5) / per_segment as f64;
                seg.point_at_param(if reversed { 1.0 - t } else { t })
            })
            .collect()
    };
    let pair = |i: usize| -> (Vec<Vec3>, Vec<Vec3>) { (along(&c.segments[i], false), along(&c.segments[m - 1 - i], true)) };
    let spread = |(pa, pb): (Vec<Vec3>, Vec<Vec3>)| -> Vec<Vec3> {
        let mut pts = Vec::with_capacity(pa.len() * fractions.len());
        for &f in fractions {
            for (a, b) in pa.iter().zip(&pb) {
                pts.push(*a + (*b - *a) * f);
            }
        }
        pts
    };
    let [seam, far] = rc.ends;
    let mut out = Vec::with_capacity(k + 2);
    for i in 0..k {
        if (i == 0 && seam == RibbonEnd::Straight) || (i == k - 1 && far == RibbonEnd::Straight) {
            continue;
        }
        out.push(spread(pair(i)));
    }
    out
}

/// Is this segment kind pair one a ruled band can join without a surprise?
/// Purely informational; used by tests.
pub fn same_kind(a: &Segment, b: &Segment) -> bool {
    matches!(
        (a, b),
        (Segment::Line { .. }, Segment::Line { .. })
            | (Segment::Arc { .. }, Segment::Arc { .. })
            | (Segment::Bezier { .. }, Segment::Bezier { .. })
    )
}

#[cfg(test)]
mod vocabulary_tests {
    use super::{Cap, SectionMatch};

    /// `ALL` is written by hand, so this is what keeps it complete: the
    /// `match` arms are exhaustive, and a new variant fails to compile here
    /// until it is listed. Every listed spelling parses back to its variant,
    /// and a spelling outside the list is refused with the list in the
    /// message.
    #[test]
    fn all_lists_every_variant_and_parse_reads_it() {
        for m in SectionMatch::ALL {
            match m {
                SectionMatch::Index | SectionMatch::ArcLength => {}
            }
            assert_eq!(SectionMatch::parse(m.name()).unwrap(), m);
        }
        for c in Cap::ALL {
            match c {
                Cap::Plane | Cap::Ruled => {}
            }
            assert_eq!(Cap::parse(c.name()).unwrap(), c);
        }
        let e = SectionMatch::parse("arclength").unwrap_err().to_string();
        assert!(e.contains("\"index\"") && e.contains("\"arclen\""), "{e}");
        let e = Cap::parse("flat").unwrap_err().to_string();
        assert!(e.contains("\"plane\"") && e.contains("\"ruled\""), "{e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::P2;

    fn rr(z: f64, size: f64, r: f64) -> Section {
        Section::Profile(
            Sketch::new(Plane::XY.offset(z)).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), size, size, r)),
        )
    }

    #[test]
    fn the_gridfinity_foot_matches_segment_for_segment() {
        // The four sections of a Gridfinity foot.
        let plan = LoftPlan::new(
            &[rr(-5.0, 35.6, 0.8), rr(-4.2, 37.2, 1.6), rr(-2.4, 37.2, 1.6), rr(0.0, 42.0, 4.0)],
            true,
            SectionMatch::Index,
        )
        .unwrap();
        assert_eq!(plan.bands(), 3);
        assert!(!plan.descending());
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        for s in &plan.sections {
            assert_eq!(s.loops()[0].segments.len(), 8, "4 lines + 4 corner arcs");
        }
        // Every corner arc is paired with a corner arc, which is what makes the
        // 45-degree band a cone rather than a set of flats.
        for w in 0..3 {
            let (a, b) = (&plan.sections[w].loops()[0], &plan.sections[w + 1].loops()[0]);
            for s in 0..8 {
                assert!(same_kind(&a.segments[s], &b.segments[s]), "band {w} segment {s}");
            }
        }
    }

    #[test]
    fn a_tilted_section_is_a_diagnostic_naming_it() {
        let tilted = Section::Profile(
            Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0)),
        );
        let e =
            LoftPlan::new(&[rr(0.0, 10.0, 1.0), tilted], true, SectionMatch::Index).unwrap_err().to_string();
        assert!(e.contains("section 1 is not parallel"), "{e}");
    }

    #[test]
    fn unequal_segment_counts_say_which_section_and_why() {
        let e = LoftPlan::new(&[rr(0.0, 10.0, 1.0), rr(5.0, 6.0, 0.0)], true, SectionMatch::Index)
            .unwrap_err()
            .to_string();
        assert!(e.contains("section 1's loop 0 has 4 segment(s)"), "{e}");
        assert!(e.contains("segment to segment"), "{e}");
    }

    #[test]
    fn sections_at_the_same_height_are_refused() {
        let e = LoftPlan::new(&[rr(3.0, 10.0, 1.0), rr(3.0, 6.0, 1.0)], true, SectionMatch::Index)
            .unwrap_err()
            .to_string();
        assert!(e.contains("same height"), "{e}");
    }

    #[test]
    fn sections_that_double_back_are_refused() {
        let e =
            LoftPlan::new(&[rr(0.0, 10.0, 1.0), rr(5.0, 8.0, 1.0), rr(2.0, 6.0, 1.0)], true, SectionMatch::Index)
                .unwrap_err()
                .to_string();
        assert!(e.contains("reverses the direction"), "{e}");
    }

    #[test]
    fn descending_sections_are_fine_and_recorded() {
        let plan =
            LoftPlan::new(&[rr(5.0, 10.0, 1.0), rr(0.0, 6.0, 1.0)], true, SectionMatch::Index).unwrap();
        assert!(plan.descending());
        assert_eq!(plan.ascending(), vec![1, 0]);
    }

    #[test]
    fn a_point_in_the_middle_is_refused() {
        let e = LoftPlan::new(
            &[rr(0.0, 10.0, 1.0), Section::Point(Vec3::new(0.0, 0.0, 3.0)), rr(6.0, 4.0, 1.0)],
            true,
            SectionMatch::Index,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("only the first and last section"), "{e}");
    }

    #[test]
    fn a_loop_started_at_a_different_corner_warns_about_the_twist() {
        let a = Section::Profile(
            Sketch::new(Plane::XY).with_loop(Loop::polygon(&[
                P2::new(-5.0, -5.0),
                P2::new(5.0, -5.0),
                P2::new(5.0, 5.0),
                P2::new(-5.0, 5.0),
            ])),
        );
        // Same square, but written starting from the opposite corner.
        let b = Section::Profile(
            Sketch::new(Plane::XY.offset(4.0)).with_loop(Loop::polygon(&[
                P2::new(5.0, 5.0),
                P2::new(-5.0, 5.0),
                P2::new(-5.0, -5.0),
                P2::new(5.0, -5.0),
            ])),
        );
        let plan = LoftPlan::new(&[a, b], true, SectionMatch::Index).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("probably twisted")), "{:?}", plan.warnings);
    }

    /// A fine, nearly regular loop: `count` shallow scallops on a circle, each
    /// drawn with several points, turned by `turn` radians.
    fn scalloped(z: f64, turn: f64) -> Section {
        let (count, per) = (36, 9);
        let mut pts = Vec::new();
        for i in 0..count * per {
            let t = turn + std::f64::consts::TAU * (i as f64) / ((count * per) as f64);
            let dip = if i % per == 0 { 0.0 } else { 0.8 * fmath::sin(std::f64::consts::PI * ((i % per) as f64) / (per as f64)) };
            let r = 48.0 - dip;
            pts.push(P2::new(r * fmath::cos(t), r * fmath::sin(t)));
        }
        Section::Profile(Sketch::new(Plane::XY.offset(z)).with_loop(Loop::polygon(&pts)))
    }

    #[test]
    fn a_loop_turned_on_purpose_does_not_warn() {
        // A helical outline: each section the last one turned by about two of
        // its segments. A seam shifted by two fits the vertices better, but
        // the turn is the design, and pairing in order builds it.
        let step = 2.0 * std::f64::consts::TAU / (36.0 * 9.0) * 0.95;
        let sections: Vec<Section> = (0..5).map(|k| scalloped(2.0 * k as f64, step * k as f64)).collect();
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn a_loop_turned_and_restarted_still_warns() {
        // The same loop restarted three segments on: the seam fits with no
        // turn at all, so the turn this pairing would build is a mistake.
        let a = scalloped(0.0, 0.0);
        let b = match scalloped(4.0, 0.0) {
            Section::Profile(sk) => {
                let segs = &sk.loops[0].segments;
                let pts: Vec<P2> = (0..segs.len()).map(|j| segs[(j + 3) % segs.len()].start()).collect();
                Section::Profile(Sketch::new(Plane::XY.offset(4.0)).with_loop(Loop::polygon(&pts)))
            }
            _ => unreachable!(),
        };
        let plan = LoftPlan::new(&[a, b], true, SectionMatch::Index).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("probably twisted")), "{:?}", plan.warnings);
    }

    #[test]
    fn concentric_rounded_rects_do_not_warn() {
        let plan =
            LoftPlan::new(&[rr(0.0, 42.0, 4.0), rr(-2.4, 37.2, 1.6)], true, SectionMatch::Index).unwrap();
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    // ------------------------------------------------ the arc-length matcher

    /// A point rotated `q` quarter-turns counter-clockwise about the origin.
    fn rot(p: P2, q: usize) -> P2 {
        match q % 4 {
            0 => p,
            1 => P2::new(-p.y, p.x),
            2 => P2::new(-p.x, -p.y),
            _ => P2::new(p.y, -p.x),
        }
    }

    fn rot_seg(s: &Segment, q: usize) -> Segment {
        match s {
            Segment::Line { a, b } => Segment::Line { a: rot(*a, q), b: rot(*b, q) },
            Segment::Arc { a, b, via } => {
                Segment::Arc { a: rot(*a, q), b: rot(*b, q), via: rot(*via, q) }
            }
            Segment::Bezier { a, b, ctrl } => Segment::Bezier {
                a: rot(*a, q),
                b: rot(*b, q),
                ctrl: ctrl.iter().map(|c| rot(*c, q)).collect(),
            },
        }
    }

    /// The bossed ring of a rim shell: a square ring of half-width `h` with
    /// corner radius `r` that detours in a U around one magnet boss of radius
    /// `rb` in the middle of each of the four walls, the boss sitting `d` in
    /// from the wall.
    ///
    /// Six segments per quarter — wall, flank, boss arc, flank, wall, corner
    /// arc — so 24 in all, against the plain ring's 8.
    fn brim_ring(h: f64, r: f64, rb: f64, d: f64) -> Loop {
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let quarter = [
            Segment::Line { a: P2::new(-h + r, -h), b: P2::new(-rb, -h) },
            Segment::Line { a: P2::new(-rb, -h), b: P2::new(-rb, -h + d) },
            Segment::Arc {
                a: P2::new(-rb, -h + d),
                b: P2::new(rb, -h + d),
                via: P2::new(0.0, -h + d + rb),
            },
            Segment::Line { a: P2::new(rb, -h + d), b: P2::new(rb, -h) },
            Segment::Line { a: P2::new(rb, -h), b: P2::new(h - r, -h) },
            Segment::Arc {
                a: P2::new(h - r, -h),
                b: P2::new(h, -h + r),
                via: P2::new(h - r + r * k, -h + r - r * k),
            },
        ];
        Loop {
            segments: (0..4).flat_map(|q| quarter.iter().map(move |s| rot_seg(s, q))).collect(),
        }
    }

    /// The two sections of a rim shell one grid cell wide: a plain
    /// body-wall ring at inset 1.2 lofted 15 mm up to the
    /// inset-2.6 ring that detours round four R4.025 bosses whose apex plane is
    /// 11.775 in from the wall.
    fn brim_sections() -> [Section; 2] {
        let plain = Section::Profile(
            Sketch::new(Plane::XY).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), 39.6, 39.6, 6.3)),
        );
        let detoured = Section::Profile(
            Sketch::new(Plane::XY.offset(15.0)).with_loop(brim_ring(18.4, 4.9, 4.025, 5.15)),
        );
        [plain, detoured]
    }

    /// Signed distance from a rounded rectangle of half-width `h` and corner
    /// radius `r`, centred at the origin: exactly zero on its boundary. A point
    /// still on it after splitting is a point the split did not move.
    fn rounded_rect_sdf(p: P2, h: f64, r: f64) -> f64 {
        let (dx, dy) = ((p.x.abs() - (h - r)).max(0.0), (p.y.abs() - (h - r)).max(0.0));
        (dx * dx + dy * dy).sqrt() - r
    }

    fn arcs(l: &Loop) -> usize {
        l.segments.iter().filter(|s| matches!(s, Segment::Arc { .. })).count()
    }

    /// 8 segments and 24 pair up by arc length, and the
    /// pieces are still lines and true arcs.
    #[test]
    fn the_base_brim_ring_matches_the_plain_ring_by_arc_length() {
        let s = brim_sections();
        assert_eq!(s[0].profile_loops()[0].segments.len(), 8);
        assert_eq!(s[1].profile_loops()[0].segments.len(), 24);

        let plan = LoftPlan::new(&s, true, SectionMatch::ArcLength).unwrap();
        let (a, b) = (&plan.sections[0].loops()[0], &plan.sections[1].loops()[0]);
        assert_eq!(a.segments.len(), b.segments.len(), "the counts have to agree now");
        // 4 shared seams + 4 of the plain ring's own vertices + 20 of the
        // detoured ring's: one cut per vertex of either, nothing else.
        assert_eq!(a.segments.len(), 28, "8 + 24 - 4 vertices that coincide");
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);

        // Exact: every point of the split plain ring is still *on* the plain
        // ring, to the last few bits, because a line piece is a line and an arc
        // piece is an arc of the same circle. A chorded pairing would be off by
        // the sagitta — about 0.1 mm here.
        let mut worst: f64 = 0.0;
        for seg in &a.segments {
            let len = seg.length();
            for t in [0.0, 0.17, 0.5, 0.83, 1.0] {
                worst = worst.max(rounded_rect_sdf(seg.point_at_length(len * t), 19.8, 6.3).abs());
            }
        }
        assert!(worst < 1e-9, "the split plain ring drifted off the original by {worst} mm");

        // Nothing was flattened, and no piece was lost: the perimeters agree.
        assert_eq!(arcs(a), 8, "4 corner arcs, one of them cut in two by a boss vertex");
        assert_eq!(arcs(b), 8, "4 corner arcs + 4 boss arcs, none turned into chords");
        for (orig, matched) in [(&s[0], a), (&s[1], b)] {
            let before = orig.profile_loops()[0].length();
            assert!((matched.length() - before).abs() < 1e-9, "{before} -> {}", matched.length());
        }

        // The detoured ring kept every vertex it was drawn with: the matcher
        // only ever *adds* cuts, it never moves a point the author placed.
        let before: Vec<P2> = s[1].profile_loops()[0].segments.iter().map(Segment::start).collect();
        for v in &before {
            assert!(
                b.segments.iter().any(|s| {
                    let p = s.start();
                    fmath::hypot(p.x - v.x, p.y - v.y) < 1e-9
                }),
                "vertex ({:.4}, {:.4}) went missing",
                v.x,
                v.y
            );
        }
    }

    /// The correspondence is a pure function of the two sketches: same script,
    /// same split points, bit for bit. There is no argmin over rotations in it
    /// to change its mind.
    #[test]
    fn matching_the_same_sections_twice_gives_the_same_loops() {
        let one = LoftPlan::new(&brim_sections(), true, SectionMatch::ArcLength).unwrap();
        let two = LoftPlan::new(&brim_sections(), true, SectionMatch::ArcLength).unwrap();
        for (x, y) in one.sections.iter().zip(&two.sections) {
            assert_eq!(x.loops(), y.loops());
        }
    }

    /// ...and a parameter nudge moves the split points, it does not re-pair the
    /// sections: the segment count and the seam both stay put.
    #[test]
    fn a_parameter_nudge_does_not_re_pair_matched_sections() {
        let base = LoftPlan::new(&brim_sections(), true, SectionMatch::ArcLength).unwrap();
        let n = base.sections[0].loops()[0].segments.len();
        let seam = base.sections[1].loops()[0].segments[0].start();
        for nudge in [0.01, 0.02, 0.05, 0.1] {
            let s = [
                Section::Profile(
                    Sketch::new(Plane::XY)
                        .with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), 39.6, 39.6, 6.3)),
                ),
                Section::Profile(
                    Sketch::new(Plane::XY.offset(15.0))
                        .with_loop(brim_ring(18.4, 4.9, 4.025 + nudge, 5.15)),
                ),
            ];
            let plan = LoftPlan::new(&s, true, SectionMatch::ArcLength).unwrap();
            assert_eq!(plan.sections[0].loops()[0].segments.len(), n, "the face count moved");
            let now = plan.sections[1].loops()[0].segments[0].start();
            assert!(
                (now.x - seam.x).abs() < 1e-9 && (now.y - seam.y).abs() < 1e-9,
                "the seam rotated to ({:.4}, {:.4})",
                now.x,
                now.y
            );
        }
    }

    /// `match=` only ever refines: sections that already agree come out of the
    /// matcher exactly as they went in, so no existing loft changes shape.
    #[test]
    fn matching_leaves_sections_that_already_agree_alone() {
        let sections = [rr(-5.0, 35.6, 0.8), rr(-4.2, 37.2, 1.6), rr(0.0, 42.0, 4.0)];
        let index = LoftPlan::new(&sections, true, SectionMatch::Index).unwrap();
        let arclen = LoftPlan::new(&sections, true, SectionMatch::ArcLength).unwrap();
        for (a, b) in index.sections.iter().zip(&arclen.sections) {
            assert_eq!(a.loops(), b.loops());
        }
    }

    /// Three sections, and the middle one is the wire *both* bands hang off, so
    /// it has to be cut once for all of them — not once per band.
    #[test]
    fn a_stack_of_three_is_matched_across_every_section_at_once() {
        let sections = [
            Section::Profile(
                Sketch::new(Plane::XY).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), 39.6, 39.6, 6.3)),
            ),
            Section::Profile(
                Sketch::new(Plane::XY.offset(8.0)).with_loop(brim_ring(18.4, 4.9, 4.025, 5.15)),
            ),
            Section::Profile(
                Sketch::new(Plane::XY.offset(15.0))
                    .with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), 30.0, 30.0, 3.0)),
            ),
        ];
        let plan = LoftPlan::new(&sections, true, SectionMatch::ArcLength).unwrap();
        let counts: Vec<usize> =
            plan.sections.iter().map(|s| s.loops()[0].segments.len()).collect();
        assert_eq!(counts, vec![28, 28, 28], "one cut list shared by both bands");
    }

    /// Each loop is matched on its own: an outer boundary that already agrees
    /// stays untouched while the hole inside it is subdivided.
    #[test]
    fn a_hole_is_matched_independently_of_the_boundary_around_it() {
        let a = Section::Profile(
            Sketch::new(Plane::XY)
                .with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0))
                .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0)),
        );
        let b = Section::Profile(
            Sketch::new(Plane::XY.offset(6.0))
                .with_loop(Loop::rect(P2::new(0.0, 0.0), 14.0, 14.0))
                .with_loop(Loop::rect(P2::new(0.0, 0.0), 5.0, 5.0)),
        );
        let plan = LoftPlan::new(&[a, b], true, SectionMatch::ArcLength).unwrap();
        assert_eq!(plan.regions.len(), 1, "one region with one hole");
        assert_eq!(plan.regions[0].1.len(), 1);
        for s in &plan.sections {
            assert_eq!(s.loops()[0].segments.len(), 4, "the boundaries already agreed");
            // The circle's 2 arcs and the square's 4 corners share the seam and
            // the half-way point, so the hole becomes 4 pieces.
            assert_eq!(s.loops()[1].segments.len(), 4);
        }
        // The circle is still two half-arcs plus two quarter-arcs, not chords.
        assert_eq!(arcs(&plan.sections[0].loops()[1]), 4);
    }

    /// Without `match="arclen"` the mismatch is refused, and the diagnostic
    /// says which argument turns the matcher on.
    #[test]
    fn the_count_diagnostic_points_at_the_matcher() {
        let e = LoftPlan::new(&brim_sections(), true, SectionMatch::Index).unwrap_err().to_string();
        assert!(e.contains("section 1's loop 0 has 24 segment(s)"), "{e}");
        assert!(e.contains("match=\"arclen\""), "{e}");
    }

    /// An unknown `match=` names both rules rather than guessing.
    #[test]
    fn an_unknown_matching_rule_is_a_diagnostic() {
        let e = SectionMatch::parse("nearest").unwrap_err().to_string();
        assert!(e.contains("\"index\"") && e.contains("\"arclen\""), "{e}");
    }

    /// Splitting a circular arc gives circular arcs **of the same circle**, not
    /// chords: that is what keeps a lofted corner an exact cone.
    #[test]
    fn splitting_an_arc_keeps_it_on_its_circle() {
        let quarter = Segment::Arc {
            a: P2::new(5.0, 0.0),
            b: P2::new(0.0, 5.0),
            via: P2::new(
                5.0 * std::f64::consts::FRAC_1_SQRT_2,
                5.0 * std::f64::consts::FRAC_1_SQRT_2,
            ),
        };
        let len = quarter.length();
        assert!((len - 5.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12, "{len}");
        let piece = quarter.sub(len * 0.31, len * 0.77).unwrap();
        assert!(matches!(piece, Segment::Arc { .. }), "a split arc must stay an arc");
        let plen = piece.length();
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p = piece.point_at_length(plen * t);
            assert!((fmath::hypot(p.x, p.y) - 5.0).abs() < 1e-12, "({}, {}) left the circle", p.x, p.y);
        }
        // ...and the pieces still add up to the whole.
        let a = quarter.sub(0.0, len * 0.31).unwrap().length();
        let c = quarter.sub(len * 0.77, len).unwrap().length();
        assert!((a + plen + c - len).abs() < 1e-9, "{a} + {plen} + {c} != {len}");
    }

    /// A Bézier splits by de Casteljau, which reproduces the curve exactly
    /// rather than approximating it.
    #[test]
    fn splitting_a_bezier_stays_on_the_curve() {
        let curve = Segment::Bezier {
            a: P2::new(0.0, 0.0),
            b: P2::new(10.0, 0.0),
            ctrl: vec![P2::new(2.0, 8.0), P2::new(8.0, -8.0)],
        };
        let len = curve.length();
        let piece = curve.sub(len * 0.2, len * 0.6).unwrap();
        let plen = piece.length();
        assert!((plen - len * 0.4).abs() < 1e-6, "{plen} vs {}", len * 0.4);
        for t in [0.0, 0.3, 0.6, 1.0] {
            let want = curve.point_at_length(len * (0.2 + 0.4 * t));
            let got = piece.point_at_length(plen * t);
            assert!(
                (want.x - got.x).abs() < 1e-6 && (want.y - got.y).abs() < 1e-6,
                "({}, {}) vs ({}, {})",
                got.x,
                got.y,
                want.x,
                want.y
            );
        }
    }

    /// Vertices that fall a hair apart are one split point, so the matcher does
    /// not hand the kernel a sliver face.
    #[test]
    fn near_coincident_vertices_are_snapped_into_one() {
        // A square, and the same square with an extra vertex 0.1 mm past one
        // corner: 4 against 5, and the near-corner pair is inside SNAP_MM.
        let square = Loop::polygon(&[
            P2::new(-10.0, -10.0),
            P2::new(10.0, -10.0),
            P2::new(10.0, 10.0),
            P2::new(-10.0, 10.0),
        ]);
        let nearly = Loop::polygon(&[
            P2::new(-10.0, -10.0),
            P2::new(10.0, -10.0),
            P2::new(10.0, 10.0),
            P2::new(-9.9, 10.0),
            P2::new(-10.0, 10.0),
        ]);
        let plan = LoftPlan::new(
            &[
                Section::Profile(Sketch::new(Plane::XY).with_loop(square)),
                Section::Profile(Sketch::new(Plane::XY.offset(5.0)).with_loop(nearly)),
            ],
            true,
            SectionMatch::ArcLength,
        )
        .unwrap();
        let (a, b) = (&plan.sections[0].loops()[0], &plan.sections[1].loops()[0]);
        assert_eq!(a.segments.len(), 5);
        assert_eq!(b.segments.len(), 5);
        // Every piece is a real face, not a few microns of nothing.
        for l in [a, b] {
            for s in &l.segments {
                assert!(s.length() > 1e-3, "a sliver piece {} mm long", s.length());
            }
        }
    }

    /// A loop drawn with a genuinely degenerate segment cannot be matched, and
    /// the diagnostic says which section, which loop and what to do.
    #[test]
    fn a_sliver_the_matcher_cannot_absorb_is_a_diagnostic() {
        // The second loop has a 2 micron edge at a place the first loop also
        // wants to cut, and 2 microns is below what a face can be built from.
        let square = Loop::polygon(&[
            P2::new(-10.0, -10.0),
            P2::new(10.0, -10.0),
            P2::new(10.0, 10.0),
            P2::new(-10.0, 10.0),
        ]);
        let slivered = Loop::polygon(&[
            P2::new(-10.0, -10.0),
            P2::new(10.0, -10.0),
            P2::new(10.0, 10.0),
            P2::new(-9.999_998, 10.0),
            P2::new(-10.0, 10.0),
        ]);
        let e = LoftPlan::new(
            &[
                Section::Profile(Sketch::new(Plane::XY).with_loop(square)),
                Section::Profile(Sketch::new(Plane::XY.offset(5.0)).with_loop(slivered)),
            ],
            true,
            SectionMatch::ArcLength,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("loft section 0's loop 0"), "{e}");
        assert!(e.contains("not a face the kernel can build"), "{e}");
        assert!(e.contains("match=\"index\""), "{e}");
    }
}
