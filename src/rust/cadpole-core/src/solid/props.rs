//! Property tests on the booleans: does every body a boolean *returns* pass
//! [`super::validate`]?
//!
//! Primitives (boxes, cylinders, spheres) at random sizes, positions and
//! rotations from a seeded generator, combined by random sequences of union,
//! subtract and intersect, on both kernels. And the cases a random draw
//! rarely lands on, written out by hand: faces flush against each other,
//! feet one Gridfinity pitch apart, a cylinder grazing a wall, a sphere
//! touching a plane, a tool whose top is coplanar with the body's.
//!
//! The rule for each step is the one the kernel is held to everywhere else:
//! an `Err` is a *diagnostic* and passes — the exact kernel refuses tangent
//! quadrics and partial coincidence by design — while an `Ok`
//! that is not a solid is the failure. A result the kernel stands behind has
//! to be a closed, consistently oriented, embedded surface.
//!
//! No `proptest`: the workspace takes no new dependencies, and a splitmix64
//! with a printed seed reproduces a failure just as well. Every failure
//! message carries its seed and its kernel; `CADPOLE_PROP_CASES=500` runs more
//! random sequences than the default for a longer hunt.
//!
//! Known failures live in [`KNOWN_NON_SOLID`]: a list someone has to edit to
//! grow, and has to edit to shrink, because an entry that stops failing
//! fails the test until it is removed.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use super::{validate_with, SolidOptions, SolidReport};
use crate::context::{Body, Context};
use crate::geom::{Plane, Transform, Vec3};
use crate::ids::BodyId;
use crate::kernel::{BoolOp, KernelChoice};
use crate::sketch::{Loop, Segment, Sketch, P2};

/// Booleans the kernels return that are not solids:
/// `(case, kernel, failing check, why)`. The case name is the one the failure
/// message prints (`seed:N` for a random sequence), the kernel is `brep`,
/// `mesh` or `*`, and the check is a name from [`SolidReport::failing`].
///
/// It is empty, and the discipline is that it stays that way: a new entry
/// needs a reason written down, and an entry whose case starts passing fails
/// this suite until someone deletes it.
const KNOWN_NON_SOLID: &[(&str, &str, &str, &str)] = &[
    // Empty, and that is the point: every boolean result either passes
    // `validate` or is refused with a reason -- by `Context::check_mesh_result`
    // on the mesh kernel, by the tessellator's open-shell check on the exact one.
];

/// Random sequences run by default. `CADPOLE_PROP_CASES` overrides.
const DEFAULT_CASES: u64 = 24;

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

/// splitmix64: 64 bits of state, one multiply-xorshift per draw, good enough
/// to spread seeds and cheap enough to be obviously deterministic.
struct Rng(u64);

impl Rng {
    /// The state is the seed *mixed*, not the seed: with the raw seed as
    /// state, seed 1's fourth draw is seed 4's first, and every sequence is
    /// a window on one stream.
    fn new(seed: u64) -> Rng {
        let mut r = Rng(seed ^ 0x2545_F491_4F6C_DD1D);
        let state = r.next_u64();
        Rng(state)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    /// A length that lands on a 2 mm grid half the time, so faces of
    /// different primitives coincide more often than chance would have it.
    fn length(&mut self, lo: f64, hi: f64) -> f64 {
        let v = self.range(lo, hi);
        if self.chance(0.5) {
            (v / 2.0).round() * 2.0
        } else {
            (v * 100.0).round() / 100.0
        }
    }
}

#[derive(Clone, Debug)]
enum Shape {
    Box { w: f64, d: f64, h: f64 },
    Cylinder { r: f64, h: f64 },
    Sphere { r: f64 },
}

/// A primitive, built at the origin and then rotated and moved.
#[derive(Clone, Debug)]
struct Primitive {
    shape: Shape,
    at: Vec3,
    axis: Vec3,
    angle_deg: f64,
}

impl Primitive {
    fn r#box(w: f64, d: f64, h: f64, at: Vec3) -> Primitive {
        Primitive { shape: Shape::Box { w, d, h }, at, axis: Vec3::Z, angle_deg: 0.0 }
    }
    fn cylinder(r: f64, h: f64, at: Vec3) -> Primitive {
        Primitive { shape: Shape::Cylinder { r, h }, at, axis: Vec3::Z, angle_deg: 0.0 }
    }
    fn sphere(r: f64, at: Vec3) -> Primitive {
        Primitive { shape: Shape::Sphere { r }, at, axis: Vec3::Z, angle_deg: 0.0 }
    }
    fn rotated(mut self, axis: Vec3, angle_deg: f64) -> Primitive {
        self.axis = axis;
        self.angle_deg = angle_deg;
        self
    }
    fn curved(&self) -> bool {
        !matches!(self.shape, Shape::Box { .. })
    }

    fn random(rng: &mut Rng) -> Primitive {
        let shape = match rng.below(3) {
            0 => Shape::Box { w: rng.length(2.0, 30.0), d: rng.length(2.0, 30.0), h: rng.length(2.0, 30.0) },
            1 => Shape::Cylinder { r: rng.length(1.0, 12.0), h: rng.length(2.0, 30.0) },
            _ => Shape::Sphere { r: rng.length(1.5, 12.0) },
        };
        let at = Vec3::new(rng.length(-15.0, 15.0), rng.length(-15.0, 15.0), rng.length(-15.0, 15.0));
        // Half the time axis-aligned, so coplanar contact is common; a fifth
        // of the time a quarter turn, which keeps faces axis-aligned but
        // swaps which; otherwise a general rotation.
        let (axis, angle_deg) = match rng.below(10) {
            0..=4 => (Vec3::Z, 0.0),
            5..=6 => ([Vec3::X, Vec3::Y, Vec3::Z][rng.below(3)], 90.0),
            _ => (
                Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)).normalized(),
                (rng.range(1.0, 179.0) * 10.0).round() / 10.0,
            ),
        };
        let axis = if axis.length() < 0.5 { Vec3::Z } else { axis };
        Primitive { shape, at, axis, angle_deg }
    }

    /// Build the primitive as a body of `ctx`: a box or cylinder is an
    /// extrude, a sphere is a half-disc revolved a full turn about Z.
    fn build(&self, ctx: &mut Context) -> crate::Result<BodyId> {
        let ids = match self.shape {
            Shape::Box { w, d, h } => {
                let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
                ctx.extrude("box", &s, None, h, BTreeMap::new(), None, None)?
            }
            Shape::Cylinder { r, h } => {
                let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), r));
                ctx.extrude("cylinder", &s, None, h, BTreeMap::new(), None, None)?
            }
            Shape::Sphere { r } => {
                // On the XZ plane sketch (x, y) is world (x, z): the arc runs
                // from the south pole over (r, 0) to the north pole and the
                // line closes it down the axis.
                let (south, north) = (P2::new(0.0, -r), P2::new(0.0, r));
                let profile = Loop {
                    segments: vec![
                        Segment::Arc { a: south, b: north, via: P2::new(r, 0.0) },
                        Segment::Line { a: north, b: south },
                    ],
                };
                let s = Sketch::new(Plane::XZ).with_loop(profile);
                ctx.revolve("sphere", &s, None, Vec3::ZERO, Vec3::Z, TAU, None, None)?
            }
        };
        let id = ids[0];
        if self.angle_deg == 0.0 && self.at.length() == 0.0 {
            return Ok(id);
        }
        let t = Transform::rotation(Vec3::ZERO, self.axis, self.angle_deg.to_radians())
            .then(Transform::translation(self.at));
        ctx.transform(id, &t, "move", BTreeMap::new(), None, None)
    }

    fn describe(&self) -> String {
        let shape = match self.shape {
            Shape::Box { w, d, h } => format!("box {w}x{d}x{h}"),
            Shape::Cylinder { r, h } => format!("cylinder r{r} h{h}"),
            Shape::Sphere { r } => format!("sphere r{r}"),
        };
        let rot = if self.angle_deg == 0.0 {
            String::new()
        } else {
            format!(" rotated {}° about ({:.3}, {:.3}, {:.3})", self.angle_deg, self.axis.x, self.axis.y, self.axis.z)
        };
        format!("{shape} at ({}, {}, {}){rot}", self.at.x, self.at.y, self.at.z)
    }
}

#[derive(Clone, Debug)]
struct Step {
    op: BoolOp,
    tool: Primitive,
}

#[derive(Clone, Debug)]
struct Case {
    name: String,
    base: Primitive,
    steps: Vec<Step>,
}

impl Case {
    fn random(seed: u64) -> Case {
        let mut rng = Rng::new(seed);
        let base = Primitive::random(&mut rng);
        let n = 1 + rng.below(3);
        let steps = (0..n)
            .map(|_| Step {
                op: [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect][rng.below(3)],
                tool: Primitive::random(&mut rng),
            })
            .collect();
        Case { name: format!("seed:{seed}"), base, steps }
    }

    fn curved(&self) -> bool {
        self.base.curved() || self.steps.iter().any(|s| s.tool.curved())
    }

    fn describe(&self) -> String {
        let mut s = format!("{}: {}", self.name, self.base.describe());
        for st in &self.steps {
            s.push_str(&format!("\n    {} {}", st.op.kind_name(), st.tool.describe()));
        }
        s
    }

    /// Run the sequence on one kernel. Stops at the first step that is not a
    /// solid, or at the first refusal: after a refusal there is nothing to
    /// continue from, and that is a pass.
    fn run(&self, kernel: KernelChoice) -> Outcome {
        let mut ctx = Context::new();
        let mut acc = match self.base.build(&mut ctx) {
            Ok(id) => id,
            Err(e) => return Outcome::Refused(format!("building the base: {e}")),
        };
        // The primitives themselves must be solids, or nothing after them
        // means anything.
        if let Err(report) = solid_of(&mut ctx, acc) {
            return Outcome::NotSolid { step: 0, report, note: String::new() };
        }
        for (i, st) in self.steps.iter().enumerate() {
            let tool = match st.tool.build(&mut ctx) {
                Ok(id) => id,
                Err(e) => return Outcome::Refused(format!("building tool {}: {e}", i + 1)),
            };
            if let Err(report) = solid_of(&mut ctx, tool) {
                return Outcome::NotSolid { step: 0, report, note: String::new() };
            }
            acc = match ctx.boolean(st.op, acc, &[tool], kernel, None, None) {
                Ok(id) => id,
                Err(e) => return Outcome::Refused(format!("step {}: {e}", i + 1)),
            };
            if let Err(report) = solid_of(&mut ctx, acc) {
                let note = at_other_tolerances(&ctx, acc);
                return Outcome::NotSolid { step: i + 1, report, note };
            }
        }
        Outcome::Solid
    }
}

enum Outcome {
    Solid,
    /// The kernel said no, with a reason. A pass.
    Refused(String),
    /// The kernel returned a body that is not a solid. `step` 0 is a
    /// primitive before any boolean; `note` says whether an exact result is
    /// a solid at other tessellation tolerances.
    NotSolid { step: usize, report: SolidReport, note: String },
}

fn solid_of(ctx: &mut Context, id: BodyId) -> Result<(), SolidReport> {
    let (mesh, _) = ctx.body_mesh(id).expect("a body that exists tessellates");
    let r = validate_with(mesh, &SolidOptions { pair_limit: usize::MAX, ..Default::default() });
    if r.is_solid() {
        Ok(())
    } else {
        Err(r)
    }
}

/// For a B-rep result, whether the defect is in the solid or in one
/// tessellation of it: re-tessellate finer and coarser and validate again.
fn at_other_tolerances(ctx: &Context, id: BodyId) -> String {
    let body = &ctx.body(id).expect("the body exists").body;
    if !matches!(body, Body::Brep(_)) {
        return String::new();
    }
    let mut out = Vec::new();
    for tol in [0.02, 0.1] {
        let verdict = match body.to_mesh(tol) {
            Ok((mesh, _)) => {
                let r = validate_with(&mesh, &SolidOptions { pair_limit: usize::MAX, ..Default::default() });
                if r.is_solid() {
                    "a solid".to_owned()
                } else {
                    r.summary()
                }
            }
            Err(e) => format!("tessellation failed: {e}"),
        };
        out.push(format!("at tolerance {tol}: {verdict}"));
    }
    format!("re-tessellated {}", out.join("; "))
}

// ---------------------------------------------------------------------------
// The deliberate cases
// ---------------------------------------------------------------------------

/// The contact cases, each with the three operations unless the shape only
/// makes sense for some.
fn deliberate_cases() -> Vec<Case> {
    let all = [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect];
    let mut out = Vec::new();
    let mut add = |name: &str, base: Primitive, tool: Primitive, ops: &[BoolOp]| {
        for op in ops {
            out.push(Case {
                name: format!("{name}/{}", op.kind_name()),
                base: base.clone(),
                steps: vec![Step { op: *op, tool: tool.clone() }],
            });
        }
    };
    let o = Vec3::ZERO;
    let cube = Primitive::r#box(10.0, 10.0, 10.0, o);

    // Flush faces: the second box shares the x = 10 face of the first.
    add("flush-face", cube.clone(), Primitive::r#box(10.0, 10.0, 10.0, Vec3::new(10.0, 0.0, 0.0)), &all);
    // A smaller box standing on the top face, touching in part of it.
    add("partial-face", cube.clone(), Primitive::r#box(4.0, 4.0, 5.0, Vec3::new(3.0, 3.0, 10.0)), &all);
    // Touching along one edge only.
    add("edge-touch", cube.clone(), Primitive::r#box(10.0, 10.0, 10.0, Vec3::new(10.0, 10.0, 0.0)), &all);
    // Touching at one corner only.
    add("corner-touch", cube.clone(), Primitive::r#box(10.0, 10.0, 10.0, Vec3::new(10.0, 10.0, 10.0)), &all);
    // The same box twice.
    add("identical", cube.clone(), cube.clone(), &all);
    // One strictly inside the other: subtract makes a void.
    add(
        "box-inside-box",
        Primitive::r#box(20.0, 20.0, 20.0, o),
        Primitive::r#box(10.0, 10.0, 10.0, Vec3::new(5.0, 5.0, 5.0)),
        &all,
    );
    // A tool that shares five faces of the body and leaves a 0.03 mm wall:
    // the sliver the Gridfinity feet showed.
    add("sliver-wall", Primitive::r#box(20.0, 20.0, 20.0, o), Primitive::r#box(19.97, 20.0, 20.0, o), &[BoolOp::Subtract]);
    // A through-cylinder whose two ends are coplanar with the body's top
    // and bottom: the over-extension case for subtract.
    add(
        "through-hole-flush",
        Primitive::r#box(20.0, 20.0, 20.0, o),
        Primitive::cylinder(5.0, 20.0, Vec3::new(10.0, 10.0, 0.0)),
        &all,
    );
    // A pocket from the bottom, flush at z = 0, closed at the top.
    add(
        "pocket-flush-bottom",
        Primitive::r#box(20.0, 20.0, 20.0, o),
        Primitive::cylinder(5.0, 10.0, Vec3::new(10.0, 10.0, 0.0)),
        &all,
    );
    // Two cylinders whose surfaces are tangent along a line.
    add("grazing-cylinders", Primitive::cylinder(5.0, 10.0, o), Primitive::cylinder(5.0, 10.0, Vec3::new(10.0, 0.0, 0.0)), &all);
    // A cylinder inscribed in a box: tangent to all four walls from inside.
    add(
        "cylinder-inscribed",
        Primitive::r#box(20.0, 20.0, 20.0, o),
        Primitive::cylinder(10.0, 20.0, Vec3::new(10.0, 10.0, 0.0)),
        &all,
    );
    // A cylinder inscribed and standing proud of the top.
    add(
        "cylinder-inscribed-tall",
        Primitive::r#box(20.0, 20.0, 20.0, o),
        Primitive::cylinder(10.0, 30.0, Vec3::new(10.0, 10.0, -5.0)),
        &all,
    );
    // A sphere resting on a plane: one point of contact.
    add("sphere-on-plane", Primitive::r#box(20.0, 20.0, 10.0, o), Primitive::sphere(5.0, Vec3::new(10.0, 10.0, 15.0)), &all);
    // A sphere inscribed in a cube: six points of contact.
    add("sphere-inscribed", Primitive::r#box(20.0, 20.0, 20.0, o), Primitive::sphere(10.0, Vec3::new(10.0, 10.0, 10.0)), &all);
    // A sphere through a sphere, and a cylinder through a sphere.
    add("sphere-sphere", Primitive::sphere(8.0, o), Primitive::sphere(8.0, Vec3::new(8.0, 0.0, 0.0)), &all);
    add("cylinder-through-sphere", Primitive::sphere(10.0, o), Primitive::cylinder(3.0, 30.0, Vec3::new(0.0, 0.0, -15.0)), &all);
    // Two spheres tangent at a point.
    add("kissing-spheres", Primitive::sphere(5.0, o), Primitive::sphere(5.0, Vec3::new(10.0, 0.0, 0.0)), &all);
    // A cylinder lying on a box, tangent along a line.
    add(
        "cylinder-on-box",
        Primitive::r#box(20.0, 20.0, 10.0, o),
        Primitive::cylinder(5.0, 20.0, Vec3::new(0.0, 10.0, 15.0)).rotated(Vec3::Y, 90.0),
        &all,
    );
    // A rotated box crossing a cylinder through its edge.
    add(
        "rotated-box-cylinder",
        Primitive::cylinder(6.0, 12.0, o),
        Primitive::r#box(8.0, 8.0, 8.0, Vec3::new(3.0, -4.0, 2.0)).rotated(Vec3::Z, 45.0),
        &all,
    );

    // Feet one Gridfinity pitch apart under a plate: 41.5 mm feet at 42 mm
    // pitch, a 0.5 mm gap between them, each flush against the plate.
    let plate = Primitive::r#box(84.0, 42.0, 3.0, Vec3::new(0.0, 0.0, 0.0));
    let foot = |x: f64| Primitive::r#box(41.5, 41.5, 5.0, Vec3::new(x + 0.25, 0.25, -5.0));
    out.push(Case {
        name: "feet-one-pitch-apart".into(),
        base: plate.clone(),
        steps: vec![Step { op: BoolOp::Union, tool: foot(0.0) }, Step { op: BoolOp::Union, tool: foot(42.0) }],
    });
    // Feet that fill the pitch, so adjacent feet share a face with each
    // other as well as with the plate.
    let full = |x: f64| Primitive::r#box(42.0, 42.0, 5.0, Vec3::new(x, 0.0, -5.0));
    out.push(Case {
        name: "feet-fused-at-pitch".into(),
        base: plate.clone(),
        steps: vec![Step { op: BoolOp::Union, tool: full(0.0) }, Step { op: BoolOp::Union, tool: full(42.0) }],
    });
    // The feet first, then the plate on top of both.
    out.push(Case {
        name: "feet-then-plate".into(),
        base: full(0.0),
        steps: vec![Step { op: BoolOp::Union, tool: full(42.0) }, Step { op: BoolOp::Union, tool: plate }],
    });
    out
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Failure {
    case: String,
    kernel: &'static str,
    failing: Vec<&'static str>,
    detail: String,
}

/// Run every case on both kernels. `verbose` also prints each refusal's
/// reason, which for the deliberate cases is the record of what the exact
/// kernel declines and why.
fn run_cases(cases: &[Case], verbose: bool) -> (Vec<Failure>, usize, usize) {
    let mut failures = Vec::new();
    let mut solids = 0;
    let mut refusals = 0;
    let mut timings: Vec<(f64, String)> = Vec::new();
    for case in cases {
        for kernel in [KernelChoice::Brep, KernelChoice::Mesh] {
            let t0 = std::time::Instant::now();
            let outcome = case.run(kernel);
            timings.push((t0.elapsed().as_secs_f64(), format!("{} on {}", case.name, kernel.name())));
            match outcome {
                Outcome::Solid => solids += 1,
                Outcome::Refused(why) => {
                    refusals += 1;
                    if verbose {
                        let first = why.lines().next().unwrap_or("");
                        let short: String = first.chars().take(160).collect();
                        eprintln!("  refused  {} on {}: {short}", case.name, kernel.name());
                    }
                }
                Outcome::NotSolid { step, report, note } => {
                    let detail = format!(
                        "{}\n  after step {step} on the {} kernel: {}\n  ({} triangles, {} components, euler {}){}",
                        case.describe(),
                        kernel.name(),
                        report.summary(),
                        report.triangles,
                        report.components,
                        report.euler,
                        if note.is_empty() { String::new() } else { format!("\n  {note}") }
                    );
                    // Known or not: the log is the record of what the kernels
                    // return today.
                    eprintln!("  not solid  {detail}");
                    failures.push(Failure {
                        case: case.name.clone(),
                        kernel: kernel.name(),
                        failing: report.failing(),
                        detail,
                    });
                }
            }
        }
    }
    timings.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let total: f64 = timings.iter().map(|t| t.0).sum();
    eprintln!("{:.1} s in all; slowest:", total);
    for (secs, what) in timings.iter().take(5) {
        eprintln!("  {secs:6.2} s  {what}");
    }
    (failures, solids, refusals)
}

/// Compare what failed with what is known to fail. New failures and stale
/// entries both fail the test, so the list is edited in both directions.
/// `covered` says which entries this run could have reproduced.
fn judge(failures: Vec<Failure>, covered: impl Fn(&str) -> bool) {
    let matches = |entry: &(&str, &str, &str, &str), f: &Failure| {
        entry.0 == f.case && (entry.1 == f.kernel || entry.1 == "*") && f.failing.contains(&entry.2)
    };
    let new: Vec<&Failure> = failures.iter().filter(|f| !KNOWN_NON_SOLID.iter().any(|e| matches(e, f))).collect();
    let stale: Vec<_> =
        KNOWN_NON_SOLID.iter().filter(|e| covered(e.0) && !failures.iter().any(|f| matches(e, f))).collect();
    let mut msg = String::new();
    if !new.is_empty() {
        msg.push_str(&format!("{} boolean result(s) that are not solids and not in KNOWN_NON_SOLID:\n", new.len()));
        for f in &new {
            msg.push_str(&format!("- [{}] {:?} {}\n", f.kernel, f.failing, f.detail));
        }
    }
    if !stale.is_empty() {
        msg.push_str("KNOWN_NON_SOLID entries that no longer fail that way; edit them:\n");
        for (c, k, check, why) in &stale {
            msg.push_str(&format!("- {c} on {k} ({check}): {why}\n"));
        }
    }
    // Printed as well as panicked: `kernel::guarded` swaps the panic hook
    // while a kernel call runs, and a test thread that panics in that window
    // loses its message.
    if !msg.is_empty() {
        eprintln!("{msg}");
    }
    assert!(msg.is_empty(), "{msg}");
}

fn run_deliberate(curved: bool) {
    let cases: Vec<Case> = deliberate_cases().into_iter().filter(|c| c.curved() == curved).collect();
    let (failures, solids, refusals) = run_cases(&cases, true);
    eprintln!(
        "{} contact cases: {} solids, {} refusals, {} not solids",
        if curved { "curved" } else { "planar" },
        solids,
        refusals,
        failures.len()
    );
    judge(failures, |name| cases.iter().any(|c| c.name == name));
}

#[test]
fn the_primitives_are_solids_on_their_own() {
    let mut ctx = Context::new();
    for p in [
        Primitive::r#box(10.0, 8.0, 6.0, Vec3::new(1.0, 2.0, 3.0)).rotated(Vec3::new(1.0, 1.0, 0.0), 30.0),
        Primitive::cylinder(4.0, 9.0, Vec3::new(-3.0, 0.0, 1.0)).rotated(Vec3::X, 90.0),
        Primitive::sphere(5.0, Vec3::new(2.0, -2.0, 2.0)),
    ] {
        let id = p.build(&mut ctx).unwrap_or_else(|e| panic!("{}: {e}", p.describe()));
        if let Err(r) = solid_of(&mut ctx, id) {
            panic!("{} is not a solid: {}", p.describe(), r.summary());
        }
    }
}

/// Boxes only: flush, partial and edge contact, identical operands, one
/// inside the other, the sliver wall, and the Gridfinity feet.
#[test]
fn planar_contact_cases_return_solids_or_refuse() {
    run_deliberate(false);
}

/// With cylinders and spheres: tangent lines and points, inscribed shapes,
/// flush-ended holes.
#[test]
fn curved_contact_cases_return_solids_or_refuse() {
    run_deliberate(true);
}

#[test]
fn random_boolean_sequences_return_solids_or_refuse() {
    let n = std::env::var("CADPOLE_PROP_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_CASES);
    let cases: Vec<Case> = (1..=n).map(Case::random).collect();
    let (failures, solids, refusals) = run_cases(&cases, false);
    eprintln!("{n} random sequences: {} solids, {} refusals, {} not solids", solids, refusals, failures.len());
    // A seed past the count that was run is not a stale entry.
    judge(failures, |c| c.strip_prefix("seed:").and_then(|s| s.parse::<u64>().ok()).map(|s| s <= n).unwrap_or(false));
}
