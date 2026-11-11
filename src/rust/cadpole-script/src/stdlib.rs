//! The standard library: `.star` files compiled into the engine and loaded
//! as `load("@cadpole/<name>.star", ...)`.
//!
//! A document's `load()` resolves only inside the document's own script list
//! (`docs/starlark-api.md`) — the sandbox has no I/O, and the
//! playground, the render host and wasm each receive one self-contained
//! document. The standard library keeps that property by living *in the
//! binary*: every module here is `include_str!`ed at build time from
//! `//stdlib`, so the CLI, `cadpole serve`, the C FFI and the wasm engine carry
//! the same files at the same version, `cadpole --version` is the library
//! version, and a document never ships a copy. The loader consults
//! [`source`] before the document's files for any path under [`PREFIX`], so
//! a document cannot shadow the library, and a document file may not be
//! named under `@` at all (`evaluate` refuses it).
//!
//! Loading a library must record nothing: no feature, no parameter, no
//! export. [`Loader`](crate) checks the store's counts before and after and
//! refuses with a diagnostic naming the library and the feature, and
//! `param()` refuses to run from inside a library file. That is what makes
//! `@cadpole/gears.star` a *library* and `examples/nightstand.star` a *part
//! file*: loading the second builds the nightstand, on purpose.
//!
//! What is deliberately not here:
//! a user-wide library directory, a URL `load()`, auto-loading, a version
//! solver. Third-party libraries under other `@<dep>/` prefixes are
//! not built.

use std::fmt::Write as _;

/// The reserved prefix. `load("@cadpole/gears.star", ...)` is the standard
/// library; nothing in a document may be named under it.
pub const PREFIX: &str = "@cadpole/";

/// One embedded module.
#[derive(Debug, Clone, Copy)]
pub struct Module {
    /// The label a script loads it by: `@cadpole/gears.star`.
    pub path: &'static str,
    /// Its source, exactly the file under `//stdlib`.
    pub source: &'static str,
    /// One line for a listing.
    pub summary: &'static str,
    /// A pure function the module exports, which the purity test loads.
    pub probe: &'static str,
}

/// Every module, in listing order. The sources come in through `rustc_env`
/// (`//src/rust/cadpole-script:BUILD.bazel`), one variable per file, as the
/// built-in font does: an exec-root path rather than a source-relative one,
/// so a remote action's input root resolves it too.
pub const MODULES: &[Module] = &[
    Module {
        path: "@cadpole/gears.star",
        source: include_str!(env!("CADPOLE_STDLIB_GEARS")),
        summary: "involute gears: spur, helical, herringbone, internal, rack, bevel; \
                  profile, layout and tooth faces; pitch, centre-distance and planetary arithmetic",
        probe: "pitch_diameter",
    },
    Module {
        path: "@cadpole/mechanical.star",
        source: include_str!(env!("CADPOLE_STDLIB_MECHANICAL")),
        summary: "springs, threads, bolts and nuts on sweep_along and helix: \
                  spring, thread, bolt, nut; the ISO, square and ACME profiles",
        probe: "thread_depth",
    },
    Module {
        path: "@cadpole/site.star",
        source: include_str!(env!("CADPOLE_STDLIB_SITE")),
        summary: "site modelling: a local frame from latitude and longitude, parcels with \
                  named sides, setbacks, the envelope above grade, neighbour masses; ft, inch, to_mm",
        probe: "polygon_area",
    },
    Module {
        path: "@cadpole/building.star",
        source: include_str!(env!("CADPOLE_STDLIB_BUILDING")),
        summary: "architecture massing: ft/inch/ft_in, level and floor masses, hip, gable and \
                  shed roofs by straight skeleton, decks on posts, stairs to code, elevator shafts, \
                  floor areas with the 5 ft rule, lot coverage, height envelopes over grade with \
                  averaging, a floor-area cap",
        probe: "ft",
    },
    Module {
        path: "@cadpole/plan_light.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_LIGHT")),
        summary: "daylight for a plan: lit depth within 2H of facade glass, the sky a court, well or slot \
                  window sees by view_cone against a street window's, a sky table over court width, wall height and \
                  floor; the light rows the audit reports",
        probe: "lit_depth",
    },
    Module {
        path: "@cadpole/plan_audit.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_AUDIT")),
        summary: "the red pen for a plan: code (R304, R305, R310.1, R311.6, R311.7, CPC 402.5, R302.1), the \
                  door graph, plumbing stacks, filler rooms, daylight and furniture, as CHECKS-style rows and marks",
        probe: "ftin",
    },
    Module {
        path: "@cadpole/print.star",
        source: include_str!(env!("CADPOLE_STDLIB_PRINT")),
        summary: "printing a model in pieces: registration pegs and sockets sized for FDM, the places \
                  they fit, story bands cut, joined with a weld, pegged and dropped onto the bed",
        probe: "is_box",
    },
    Module {
        path: "@cadpole/plan.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN")),
        summary: "the floor-plan value every @cadpole/plan* module shares: plain-data records (floors, rooms, \
                  doors, windows, fixtures, stacks, courts) with constructors and validate(); areas, shared and \
                  exterior walls, the door graph and its paths, a room in its own frame; ftin and sqft",
        probe: "ftin",
    },
    Module {
        path: "@cadpole/plan_frame.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_FRAME")),
        summary: "a fixed plan's cheapest frame: reframe() picks bearing lines, joist sizes and spacing, \
                  beams, transfers and headers as a MILP over joist bays; members by name, span and size; framing \
                  plans; a planning aid, not engineering",
        probe: "frame_menu",
    },
    Module {
        path: "@cadpole/plan_spans.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_SPANS")),
        summary: "span tables as data: CRC/IRC R502.3.1(1) and (2) floor joists, R802.4.1(2) rafters, \
                  R602.7(1) and (2) headers and girders; I-joists, LVL and steel by rule of thumb; ftin",
        probe: "joist_span",
    },
    Module {
        path: "@cadpole/plan_furniture.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_FURNITURE")),
        summary: "the room stage's catalog as data: furniture and fixtures with sizes, use spaces, heights \
                  and sources (NKBA, CPC 402.5, 407 and 408, the IRC's R307), and the sets a room holds",
        probe: "catalog_probe",
    },
    Module {
        path: "@cadpole/plan_furnish.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_FURNISH")),
        summary: "the room stage: a room's furniture or fixtures packed by a MILP clear of door swings, \
                  windows and stacks, re-walked at its final size; least depths, fit tables and fit_row",
        probe: "furnish_probe",
    },
    Module {
        path: "@cadpole/plan_fit_tables.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_FIT_TABLES")),
        summary: "the room stage's fit tables as data: the least depth that holds each set by clear width \
                  and door class, 170 entries; generated by plan_fit_tables_gen",
        probe: "fit_tables_probe",
    },
    Module {
        path: "@cadpole/plan_fit_tables_gen.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_FIT_TABLES_GEN")),
        summary: "the generator of @cadpole/plan_fit_tables.star: the fit-table jobs, solved by the \
                  room stage and written out as Starlark",
        probe: "fit_jobs",
    },
    Module {
        path: "@cadpole/plan_kitchen.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_KITCHEN")),
        summary: "one kitchen run against NKBA's landing areas: the shortest compliant run, the least \
                  shortfall at a given length, a drawn run checked",
        probe: "kitchen_probe",
    },
    Module {
        path: "@cadpole/plan_roof.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_ROOF")),
        summary: "PV rows on a roof plane: modules round skylights and hatches, inside R324.6 / CFC 1205's \
                  pathways and ridge setbacks, with the low-slope and sprinklered rules",
        probe: "roof_probe",
    },
    Module {
        path: "@cadpole/plan_draw.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_DRAW")),
        summary: "a plan value drawn on sheets: every floor in layers -- rooms filled by kind, walls derived \
                  from the rooms, doors with their swings, windows, fixtures, stacks, a label per room placed clear \
                  of the rest -- a red pen of numbered check rows, plan sets and a comparison sheet with scorecards",
        probe: "kind_fill",
    },
    Module {
        path: "@cadpole/plan_parti.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_PARTI")),
        summary: "parti-first floor plans: a template's column grid, stair, court and rules fitted on every \
                  floor at once by one MILP; fit, compare, explain (a deletion filter by rule family), author, precheck",
        probe: "grid",
    },
    Module {
        path: "@cadpole/plan_templates.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_TEMPLATES")),
        summary: "the parti library for 25-ft row-house lots: side hall, cross stair, middle court and open \
                  core as data; mirror, column splits, a neighbour's light well, an optional court, four floors",
        probe: "side_hall",
    },
    Module {
        path: "@cadpole/plan_rules.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_RULES")),
        summary: "the vetted rule menu for generated plans: typology rules hard, size rules weighted, the \
                  standing set; the same rules as checks on any plan",
        probe: "rule",
    },
    Module {
        path: "@cadpole/plan_solve.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_SOLVE")),
        summary: "floor plans as MILPs on opt: free rectangles in zones, exact areas, walls between rooms, \
                  doors as a tree, escape walls, L-pieces, must-holds; stacks, bearing lines, beams, transfers, \
                  not-over, courts and moving faces across floors; solve, options by arrangement cuts, floor by floor",
        probe: "ftin",
    },
    Module {
        path: "@cadpole/plan_ask.star",
        source: include_str!(env!("CADPOLE_STDLIB_PLAN_ASK")),
        summary: "ask the solver about your own plan: pin its arrangement and size it, check it, cost a \
                  rule by paired solves, explain a clash by rule family, edit it moving the fewest feet of wall, \
                  local LP duals on the pinned plan; changes as data (set_room, move_zone, drop_family, ...)",
        probe: "describe",
    },
];

/// The spellings `hand=` accepts in `@cadpole/gears.star` (`helical`,
/// `herringbone`, `internal`), exported as the `hand` vocabulary.
/// The library checks the string itself; this list is what the namespace
/// offers, and `gears_refuses_a_hand_the_vocabulary_does_not_list` holds
/// the two together.
pub const HANDS: &[&str] = &["right", "left"];

/// Is `path` a library label (any `@` prefix), as opposed to a document file?
pub fn is_library(path: &str) -> bool {
    path.starts_with('@')
}

/// The source of the module at `path`, if it is one of ours.
pub fn source(path: &str) -> Option<&'static str> {
    MODULES.iter().find(|m| m.path == path).map(|m| m.source)
}

/// The labels, in listing order.
pub fn names() -> Vec<&'static str> {
    MODULES.iter().map(|m| m.path).collect()
}

/// The diagnostic for a `load()` of a library that does not exist: which
/// prefix it used, what the prefix means, and what exists.
pub fn missing(path: &str) -> String {
    let mut msg = String::new();
    if path.starts_with(PREFIX) {
        let _ = write!(msg, "no standard library module named {path:?}; the modules built into this engine are: ");
        let list: Vec<String> = MODULES.iter().map(|m| format!("{} ({})", m.path, m.summary)).collect();
        msg.push_str(&list.join("; "));
    } else {
        let _ = write!(
            msg,
            "no library named {path:?}: only the standard library's `{PREFIX}` prefix resolves \
             (modules: {}); other `@<name>/` libraries are not supported yet, and a document's own \
             files are loaded by their bare names",
            names().join(", ")
        );
    }
    msg
}

/// What loading a library changed in the store when it should have changed
/// nothing: the message of the refusal, or `None` when the load was pure.
/// `before` is [`counts`] taken before the file ran.
pub fn impurity(path: &str, before: Counts, ctx: &cadpole_core::Context) -> Option<String> {
    let after = counts(ctx);
    let what = if after.features > before.features {
        let f = &ctx.features[before.features];
        format!("recorded feature {:?} ({})", f.id.0, f.kind)
    } else if after.params > before.params {
        format!("declared parameter {:?}", ctx.params[before.params].0.name)
    } else if after.exports > before.exports {
        format!("asked to export {:?}", ctx.exports[before.exports].path)
    } else {
        return None;
    };
    Some(format!(
        "{path} {what} at top level when it was loaded; a library builds geometry only inside \
         its functions and declares no parameters — the document that loads it does"
    ))
}

/// The store counts the purity check compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub features: usize,
    pub params: usize,
    pub exports: usize,
}

pub fn counts(ctx: &cadpole_core::Context) -> Counts {
    Counts { features: ctx.features.len(), params: ctx.params.len(), exports: ctx.exports.len() }
}

/// An error raised inside a library, attributed to the document: the
/// message gains the library file and line, and the span moves to the
/// innermost call that is *in* the document, so an editor points at the
/// line the user wrote. `None` when the error is not in a library, or no
/// document frame is on the stack.
pub fn attribute(e: &starlark::Error, msg: &str, span: Option<&cadpole_core::context::Span>) -> Option<(String, cadpole_core::context::Span)> {
    let inner = span?;
    if !is_library(&inner.path) {
        return None;
    }
    let frames = &e.call_stack().frames;
    let user = frames.iter().rev().find_map(|f| {
        let loc = f.location.as_ref()?;
        if is_library(loc.filename()) {
            return None;
        }
        let r = loc.resolve_span();
        Some((
            f.name.clone(),
            cadpole_core::context::Span {
                path: loc.filename().to_owned(),
                start_line: r.begin.line as u32 + 1,
                start_col: r.begin.column as u32 + 1,
                end_line: r.end.line as u32 + 1,
                end_col: r.end.column as u32 + 1,
            },
        ))
    })?;
    let (callee, site) = user;
    let msg = format!(
        "{msg} (in {}:{}, called as {callee} from {}:{}:{})",
        inner.path, inner.start_line, site.path, site.start_line, site.start_col
    );
    Some((msg, site))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{errors, run};
    use crate::{EvalInput, ScriptFile, evaluate, run_tests};

    fn run_files(files: &[(&str, &str)]) -> cadpole_core::Context {
        evaluate(EvalInput {
            scripts: files.iter().map(|(p, s)| ScriptFile { path: p.to_string(), source: s.to_string() }).collect(),
            ..Default::default()
        })
    }

    /// The library loads, defines and builds nothing, and its functions
    /// build when called from the document.
    #[test]
    fn the_gears_library_loads_pure_and_builds_on_call() {
        let ctx = run(
            "load(\"@cadpole/gears.star\", \"spur\", \"pitch_diameter\")\n\
             assert_close(pitch_diameter(2, 20), 40)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.features.is_empty(), "loading recorded {:?}", ctx.features.iter().map(|f| &f.id).collect::<Vec<_>>());
        assert!(ctx.params.is_empty());

        let ctx = run(
            "load(\"@cadpole/gears.star\", \"spur\")\n\
             g = spur(2, 20, 6, bore=5, name=\"g\")\n\
             assert_solid(g)\n\
             assert_eq(g.kind, \"brep\")\n\
             lo, hi = g.bbox()\n\
             assert_close(hi[0], 22, tol=0.02)\n\
             assert_close(hi[2], 6, tol=1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.features.iter().any(|f| f.id.0 == "g"));
        assert!(ctx.features.iter().any(|f| f.id.0 == "g_profile"));
    }

    /// A module that does not exist says what does, with the prefix rule.
    #[test]
    fn a_missing_module_lists_what_exists() {
        let ctx = run("load(\"@cadpole/gearz.star\", \"spur\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("no standard library module named \"@cadpole/gearz.star\""), "{}", e[0]);
        assert!(e[0].contains("@cadpole/gears.star (involute gears"), "{}", e[0]);

        let ctx = run("load(\"@gridfinity/bin.star\", \"bin\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("only the standard library's `@cadpole/` prefix resolves"), "{}", e[0]);
        assert!(e[0].contains("@cadpole/gears.star"), "{}", e[0]);
    }

    /// A document may not name a file under `@`, and cannot shadow the
    /// library by trying: the embedded module wins and the document is
    /// refused up front.
    #[test]
    fn the_prefix_is_reserved() {
        let ctx = run_files(&[
            ("main.star", "load(\"@cadpole/gears.star\", \"spur\")\n"),
            ("@cadpole/gears.star", "def spur(*a, **k):\n    fail(\"shadowed\")\n"),
        ]);
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("\"@cadpole/gears.star\"") && e[0].contains("reserved"), "{}", e[0]);
        assert!(ctx.features.is_empty());
    }

    /// Loading a library that builds or declares something at top level is
    /// refused, naming the file and what it did — the rule that separates a
    /// library from a part file, which `load("nightstand.star")` still is.
    #[test]
    fn an_impure_library_is_refused_and_a_part_file_is_not() {
        // Simulate a library with a document file under the prefix rule
        // lifted: the check keys on the `@` path, which only the loader's
        // embedded table can satisfy, so exercise `impurity` directly and the
        // `param()` refusal through the real path.
        let empty = cadpole_core::Context::new();
        let before = counts(&empty);
        assert!(impurity("@cadpole/x.star", before, &empty).is_none());
        let ctx = run("b = box(10, 10, 10, name=\"lib_box\")\n");
        let m = impurity("@cadpole/x.star", before, &ctx).expect("a feature was recorded");
        assert!(m.starts_with("@cadpole/x.star recorded feature \"lib_box\" (box) at top level"), "{m}");
        let ctx = run("w = param(\"lib_wall\", 2)\n");
        let m = impurity("@cadpole/x.star", before, &ctx).expect("a parameter was declared");
        assert!(m.starts_with("@cadpole/x.star declared parameter \"lib_wall\" at top level"), "{m}");

        // A part file keeps building on load.
        let ctx = run_files(&[
            ("main.star", "load(\"part.star\", \"PART\")\nassert_solid(PART)\n"),
            ("part.star", "PART = box(10, 10, 10, name=\"part\")\n"),
        ]);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.bodies.len(), 1);
    }

    /// `param()` inside a library function is refused when the function
    /// runs, naming the library: a library takes values, the document
    /// declares parameters.
    #[test]
    fn the_real_library_declares_no_parameter_and_none_may() {
        // Every module: load it alone and check the store is untouched.
        for m in MODULES {
            let ctx = run(&format!("load({:?}, {:?})\n", m.path, m.probe));
            assert!(errors(&ctx).is_empty(), "{}: {:?}", m.path, errors(&ctx));
            assert!(ctx.features.is_empty() && ctx.params.is_empty() && ctx.exports.is_empty(), "{} is not pure", m.path);
            assert!(!m.source.contains("param("), "{} calls param(); a library takes values", m.path);
        }
    }

    /// An error raised inside a library function reports the library file
    /// and line and the user's call site, and its span is the call site.
    #[test]
    fn an_error_inside_the_library_names_both_places() {
        let ctx = run(
            "load(\"@cadpole/gears.star\", \"spur\")\n\
             ok = spur(2, 20, 6, name=\"ok\")\n\
             bad = spur(2, 20, 6, fillet=3, name=\"bad\")\n",
        );
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].starts_with("fail: gears.spur: the root fillet does not fit between the teeth"), "{}", e[0]);
        assert!(e[0].contains("(in @cadpole/gears.star:"), "{}", e[0]);
        assert!(e[0].contains("called as spur from main.star:3:7)"), "{}", e[0]);
        let d = ctx.diagnostics.iter().find(|d| d.message == e[0]).unwrap();
        let span = d.span.as_ref().expect("a span");
        assert_eq!((span.path.as_str(), span.start_line), ("main.star", 3), "the span is the user's line");

        // The same inside a test body.
        let run = run_tests(
            EvalInput {
                scripts: vec![ScriptFile {
                    path: "main.star".into(),
                    source: "load(\"@cadpole/gears.star\", \"planetary\")\n\
                             def four():\n    planetary(9, 6, planets=4)\n\
                             test(\"four\", four)\n"
                        .into(),
                }],
                ..Default::default()
            },
            &[],
            1,
        );
        assert_eq!(run.outcomes.len(), 1);
        assert!(!run.outcomes[0].ok);
        assert!(run.outcomes[0].message.contains("4 planets cannot be spaced equally"), "{}", run.outcomes[0].message);
        assert!(run.outcomes[0].message.contains("called as planetary from main.star:3:5"), "{}", run.outcomes[0].message);
        assert_eq!(run.outcomes[0].span.as_ref().map(|s| s.start_line), Some(3));
    }

    /// `hand.right` is the string the library checks for; a spelling the
    /// namespace does not list is refused by the library in the `; valid: ...` shape.
    #[test]
    fn gears_refuses_a_hand_the_vocabulary_does_not_list() {
        let ctx = run(
            "load(\"@cadpole/gears.star\", \"helical\")\n\
             assert_eq(hand.right, \"right\")\n\
             assert_eq(hand.left, \"left\")\n\
             assert_eq(dir(hand), [\"left\", \"right\"])\n\
             g = helical(2, 17, 8, 20, hand=hand.left, name=\"g\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        for h in HANDS {
            let ctx = run(&format!("load(\"@cadpole/gears.star\", \"helical\")\nhelical(2, 17, 8, 20, hand={h:?}, name=\"g\")\n"));
            assert!(errors(&ctx).is_empty(), "{h}: {:?}", errors(&ctx));
        }
        let ctx = run("load(\"@cadpole/gears.star\", \"helical\")\nhelical(2, 17, 8, 20, hand=\"up\", name=\"g\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("hand=\"up\" is not a hand; valid: right, left"), "{}", e[0]);
    }

    /// `@cadpole/building.star` refuses by number: a stair over the
    /// code's rise or under its width, a placed ridge on a footprint that is
    /// not four-sided, a floor-area plate the setbacks leave nothing of. A
    /// script cannot catch `fail()`, so the refusals are checked here rather
    /// than in `stdlib/building_test.star`.
    #[test]
    fn the_building_library_refuses_by_number() {
        for (call, expect) in [
            (
                "stair(0, ft(10), ft(3), rise=inch(8), name=\"s\")",
                "building.stair: a rise of 203.2 mm (8.0\") is over the irc limit of 196.85 mm (7.75\") per step",
            ),
            (
                "stair(0, ft(10), inch(30), name=\"s\")",
                "building.stair: a width of 762.0 mm (30.0\") is under the irc minimum of 914.4 mm (36.0\")",
            ),
            (
                "hip_roof([(0, 0), (10, 0), (10, 10), (5, 15), (0, 10)], pitch=0.5, ridge=((5, 2), (5, 8)), name=\"r\")",
                "building.hip_roof: ridge= is for a four-sided footprint; this one has 5 sides",
            ),
            (
                "floor_area_cap(ft(25), ft(100), front_setback=ft(60), rear_yard=ft(45), floors=3)",
                "building.floor_area_cap: the front setback (18288.0) and the rear yard (13716.0) leave no plate on a lot 30480.0 deep",
            ),
            ("units(units.ft_in)\nunits(units.mm)", "already declared units \"ft-in\""),
        ] {
            let ctx = run(&format!(
                "load(\"@cadpole/building.star\", \"stair\", \"hip_roof\", \"floor_area_cap\", \"ft\", \"inch\")\n{call}\n"
            ));
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{call}: {e:?}");
            assert!(e[0].contains(expect), "{call}:\n  got  {}\n  want {expect}", e[0]);
        }
        // And the library itself may not declare the document's units.
        let ctx = run("load(\"@cadpole/building.star\", \"ft\")\nassert_eq(ft(1), 304.8)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.display_units.is_none(), "loading the library declared units");
    }

    /// `@cadpole/print.star` refuses a peg that does not fit, pieces that
    /// do not meet, more pegs than there is room for, and an option it does
    /// not have -- each saying what to do instead.
    #[test]
    fn the_print_library_refuses_what_does_not_fit() {
        let stories = "s0 = union(box(60, 40, 2, name=\"slab0\"), box(60, 5, 18, at=(0, 0, 2), name=\"w0\"), \
                       box(60, 5, 18, at=(0, 35, 2), name=\"w1\"), weld=0.01, name=\"s0\")\n\
                       s1 = union(box(60, 40, 4, at=(0, 0, 20), name=\"slab1\"), weld=0.01, name=\"s1\")\n";
        for (call, expect) in [
            ("pegs(s0, s1, at=[(30, 20)])", "a 3.0 peg at (30, 20) does not fit"),
            ("pegs(s0, translate(s1, (0, 0, 5), name=\"s1_up\"))", "the pieces do not meet"),
            ("pegs(s0, s1, count=40)", "pass count="),
            ("pegs(s0, s1, colour=\"red\")", "unknown option \"colour\""),
            ("pegs(s0, s1, shape=\"hex\")", "shape is \"square\" or \"round\""),
            ("pegs(s0, s1, size=1, chamfer=0.5)", "leaves nothing of a 1 x 2.0 peg"),
            // Key columns and bodies per filament.
            ("pegs(s0, s1, at=[(30, 20)], column=2)", "a key column 2 across is narrower than the 3.0 peg"),
            ("pegs(s0, s1, at=[(30, 20)], column=3.2)", "or a room corner for its column"),
            ("pegs([s0], [s1], at=[(30, 37.5)], onto=3)", "onto=3 is not one of the 1 lower bodies"),
            ("pegs([s0], [s1], at=[(30, 37.5)], onto=s1)", "onto= is not one of the lower bodies"),
            ("story_bands([s0], [0])", "levels needs two heights or more"),
            ("story_bands([s0], [10, 0])", "levels go up"),
        ] {
            let ctx = run(&format!("load(\"@cadpole/print.star\", \"pegs\", \"story_bands\")\n{stories}{call}\n"));
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{call}: {e:?}");
            assert!(e[0].contains(expect), "{call}:\n  got  {}\n  want {expect}", e[0]);
        }
    }

    /// The library's own test document, `stdlib/gears_test.star`: the
    /// closed-form checks, the meshing checks and the sweeps, run exactly as
    /// `every_example_test_passes` runs an example's.
    #[test]
    fn every_stdlib_test_passes() {
        // `cadpole test stdlib/opt_test.star` solves with the CLI's HiGHS.
        cadpole_opt::install();
        let dir = std::env::var("CADPOLE_STDLIB").unwrap_or_else(|_| "stdlib".into());
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{dir}: {e}"))
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with("_test.star")).unwrap_or(false))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no *_test.star under {dir}");
        let mut failures = Vec::new();
        for p in &files {
            let file = p.file_name().unwrap().to_str().unwrap().to_owned();
            let src = std::fs::read_to_string(p).unwrap();
            let run = run_tests(
                EvalInput { scripts: vec![ScriptFile { path: "main.star".into(), source: src }], ..Default::default() },
                &[],
                0,
            );
            let errs = errors(&run.base.ctx);
            assert!(errs.is_empty(), "{file}: {errs:?}");
            assert!(!run.base.tests.is_empty(), "{file} declares no test");
            eprintln!(
                "{file}: {} tests, {} results, {} failed",
                run.base.tests.len(),
                run.outcomes.len(),
                run.outcomes.iter().filter(|o| !o.ok).count()
            );
            for o in run.outcomes.iter().filter(|o| !o.ok) {
                let at = if o.point.is_empty() { String::new() } else { format!(" at {}", crate::point_label(&o.point)) };
                failures.push(format!("- {file}: {:?}{at}: {}", o.name, o.message));
            }
        }
        assert!(failures.is_empty(), "stdlib tests failed:\n{}", failures.join("\n"));
    }
}
