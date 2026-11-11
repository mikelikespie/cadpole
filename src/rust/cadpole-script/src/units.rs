//! The display convention: `units(...)` and `length(...)`.
//!
//! The kernel is millimetres, everywhere and always (docs/starlark-api.md).
//! What a house script wants is not a different kernel but a
//! different *readout*: `45'-6"` where a bracket script reads `13868.4`.
//! `units(units.ft_in)` records that wish on the context, and every place
//! that writes a length for a person — `cadpole inspect`, `distance()`'s
//! report, `drawing()` and the `export_*` calls when they are given no
//! `units=` of their own ([`document_format`]), the web UI — reads it from
//! there. Nothing here converts a coordinate; `ft(45)` and `inch(6)` in
//! `@cadpole/building.star` are plain multiplications.
//!
//! `units` is a callable vocabulary: `units.ft_in` is the string
//! `"ft_in"`, which `drawing(units=)` and the `export_*` calls take, and
//! `units(units.ft_in, fraction=8)` is the declaration. The members come
//! from `cadpole_drawing::units::UNIT_MEMBERS`, the attribute spellings of the
//! six units the drawing parser reads.
//!
//! `length("12'-6\"")` is the imperial parser the API doc promised (its v2
//! table: "the imperial parser is `length(...)`, not `len(...)`"): the forms
//! a person writes, to millimetres.

use cadpole_core::context::DisplayUnits;
use cadpole_drawing::units::{parse_length, UnitFormat, Units, UNIT_NAMES};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::none::NoneType;

use crate::store::store;

/// The natives behind the `units` vocabulary: what `units(...)` calls.
#[starlark_module]
pub(crate) fn natives(builder: &mut GlobalsBuilder) {
    /// Declare how this document writes lengths: `units(units.ft_in)` makes
    /// `cadpole inspect`, `distance()`, the drawings and the web UI read
    /// `45'-6"` where they read `13868.4` before. A display convention
    /// only — every number the model holds stays a millimetre, and
    /// `ft(45)` is still `45 * 304.8`.
    ///
    /// `unit` is one of `units.mm`, `units.cm`, `units.m`, `units.inch`,
    /// `units.ft` or `units.ft_in` (the strings are accepted too).
    /// `fraction=` is the fraction of an inch an imperial dimension rounds
    /// to — 16 by default, 8 for framing, 0 for decimal inches — and is
    /// ignored by the metric units. A clearance or a gap is always written
    /// as a decimal (`0.35"`), because a fit rounded to a sixteenth is a
    /// different number. Declared once per document; a library may not
    /// declare it, the document that loads the library does.
    fn units<'v>(
        #[starlark(require = pos)] unit: &str,
        #[starlark(require = named)] fraction: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        if let Some(at) = eval.call_stack_top_location()
            && crate::stdlib::is_library(at.filename())
        {
            anyhow::bail!(
                "units {unit:?}: a library ({}) may not declare the document's units; the document that \
                 loads it does",
                at.filename()
            );
        }
        let fraction = match fraction {
            Some(f) => {
                let f = crate::check::num(&f, "units: fraction")?;
                if f < 0.0 || f.fract() != 0.0 {
                    anyhow::bail!("units: fraction must be a whole number of parts of an inch, got {f}");
                }
                Some(f as u32)
            }
            None => None,
        };
        let u = match fraction {
            Some(f) => Units::parse(unit, f).map_err(|e| anyhow::anyhow!("units: {e}"))?,
            None => {
                let probe = Units::parse(unit, 0).map_err(|e| anyhow::anyhow!("units: {e}"))?;
                Units::of(probe.unit)
            }
        };
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        if let Some(prev) = &ctx.display_units {
            anyhow::bail!(
                "units {unit:?}: the document already declared units {:?}; a document has one display convention",
                prev.unit
            );
        }
        ctx.display_units = Some(DisplayUnits { unit: u.unit.name().to_owned(), fraction: u.fraction });
        Ok(NoneType)
    }
}

/// The document's display convention as the drawing pipeline's format, if
/// the script declared one: what `drawing()`, `export_svg()`, `export_dxf()`
/// and `export_pdf()` write their figures in when the call names no
/// `units=` of its own.
pub(crate) fn document_format(ctx: &cadpole_core::Context) -> Option<UnitFormat> {
    ctx.display_units.as_ref().and_then(|u| Units::parse(&u.unit, u.fraction).ok()).map(|u| u.format())
}

/// The globals of this module that are plain functions.
#[starlark_module]
pub(crate) fn units_globals(builder: &mut GlobalsBuilder) {
    /// Parse a length written the way a person writes it, to millimetres:
    /// `length("12'-6\"")` is `3810`, and so are `length("12' 6\"")`,
    /// `length("12ft 6in")` and `length("150in")`; `length("3.65 m")` is
    /// `3650`; a bare number is millimetres. Fractions are taken
    /// (`"6 1/2\""`), and `'`/`"` may be written `ft`/`in`. The result is
    /// snapped to the nanometre so `12'-6"` is exactly 3810. Anything else
    /// is an error rather than a guess: a comma (`"1,000"`), a second bare
    /// number (`"1 2"`), a unit repeated or out of order (`"6\" 2'"`), a
    /// whole number and a fraction with no unit (`"1 1/2"`).
    fn length(#[starlark(require = pos)] text: &str) -> anyhow::Result<f64> {
        parse_length(text).ok_or_else(|| {
            anyhow::anyhow!(
                "length: {text:?} is not a length; write it as 12'-6\", 12ft 6in, 150in, 3.65 m or 3810 (units: {})",
                UNIT_NAMES.join(", ")
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    /// `units(...)` is recorded once, as the parser's own name, with the
    /// unit's default fraction; the string and the member are the same call.
    #[test]
    fn units_are_declared_once_and_reach_the_context() {
        let ctx = run("units(units.ft_in)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let u = ctx.display_units.clone().expect("declared");
        assert_eq!((u.unit.as_str(), u.fraction), ("ft-in", 16));

        let ctx = run("units(\"ft-in\", fraction=8)\n");
        let u = ctx.display_units.clone().expect("declared");
        assert_eq!((u.unit.as_str(), u.fraction), ("ft-in", 8));

        let ctx = run("units(units.m)\n");
        let u = ctx.display_units.clone().expect("declared");
        assert_eq!((u.unit.as_str(), u.fraction), ("m", 0));

        let ctx = run("units(units.inch)\nunits(units.mm)\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("already declared units \"in\""), "{}", e[0]);

        let ctx = run("units(\"furlong\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("must be one of mm, cm, m, in, ft, ft-in"), "{}", e[0]);

        let ctx = run("units(units.ft_in, fraction=3)\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("fraction 3 must be one of"), "{}", e[0]);

        // A document that declares none has none.
        let ctx = run("b = box(1, 1, 1)\n");
        assert!(ctx.display_units.is_none());
    }

    /// The parser the API doc promised.
    #[test]
    fn length_parses_what_people_write() {
        let ctx = run(
            "assert_eq(length(\"12'-6\\\"\"), 3810.0)\n\
             assert_eq(length(\"12ft 6in\"), 3810.0)\n\
             assert_eq(length(\"3.65 m\"), 3650.0)\n\
             assert_eq(length(\"150\"), 150.0)\n\
             assert_close(length(\"6 1/2\\\"\"), 165.1)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run("length(\"nonsense\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("is not a length"), "{}", e[0]);
    }

    /// `drawing()` and the exports figure in the document's units unless the
    /// call names its own: the same slab drawn three ways.
    #[test]
    fn the_documents_units_are_the_drawings_default() {
        let svg_of = |src: &str| {
            let out = crate::evaluate_full(crate::EvalInput {
                scripts: vec![crate::ScriptFile { path: "main.star".into(), source: src.into() }],
                ..Default::default()
            });
            assert!(errors(&out.ctx).is_empty(), "{:?}", errors(&out.ctx));
            String::from_utf8(out.files[0].data.clone()).unwrap()
        };
        let slab = "b = box(3810, 2438.4, 100, center=True, name=\"slab\")\n";
        // Declared: the sheet and the export read in feet and inches.
        let svg = svg_of(&format!("units(units.ft_in)\n{slab}d = drawing(b, view=\"top\", sheet=\"A3\", name=\"plan\")\nexport_svg(d, \"plan.svg\")\n"));
        assert!(svg.contains("12&apos;-6&quot;"), "the width reads in feet and inches");
        // The document's fraction goes with it: eighths round 3 3/16\" to 3 1/4\".
        let svg = svg_of(&format!(
            "units(units.ft_in, fraction=8)\nb = box(80.9625, 50, 10, center=True, name=\"b\")\nd = drawing(b, view=\"top\", sheet=\"A4\", name=\"plan\")\nexport_svg(d, \"plan.svg\")\n"
        ));
        assert!(svg.contains("3 1/4&quot;"), "eighths: {}", svg.lines().filter(|l| l.contains("</text>")).collect::<Vec<_>>().join(" "));
        // The call's own `units=` wins over the document's.
        let svg = svg_of(&format!("units(units.ft_in)\n{slab}d = drawing(b, view=\"top\", sheet=\"A3\", units=\"mm\", name=\"plan\")\nexport_svg(d, \"plan.svg\")\n"));
        assert!(svg.contains(">3810</text>"), "millimetres when asked");
        // No declaration: millimetres, as before.
        let svg = svg_of(&format!("{slab}d = drawing(b, view=\"top\", sheet=\"A3\", name=\"plan\")\nexport_svg(d, \"plan.svg\")\n"));
        assert!(svg.contains(">3810</text>"), "millimetres by default");
        // A sketch exported on its own under the declaration is figured the same way.
        let svg = svg_of("units(units.ft_in)\ns = sketch(XY).rect(3810, 2438.4)\nexport_svg(s, \"s.svg\")\nbox(1, 1, 1, name=\"anything\")\n");
        assert!(svg.contains("viewBox"));
    }

    /// `distance()` reports in the document's units when it has them.
    #[test]
    fn a_clearance_reads_in_the_documents_units() {
        let ctx = run(
            "units(units.ft_in)\n\
             a = box(100, 100, 100, name=\"a\")\n\
             b = box(100, 100, 100, at=(108.89, 0, 0), name=\"b\")\n\
             c = distance(a, b)\n\
             assert_close(c.distance, 8.89, tol=1e-6)\n\
             assert_true(str(c).startswith(\"clearance(0.35\\\" between\"), msg=str(c))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // Without a declaration: millimetres, as before. (A gap that is
        // exact in the mesh's f32 positions, so the number reads whole.)
        let ctx = run(
            "a = box(100, 100, 100, name=\"a\")\n\
             b = box(100, 100, 100, at=(108, 0, 0), name=\"b\")\n\
             c = distance(a, b)\n\
             assert_true(str(c).startswith(\"clearance(8 mm between\"), msg=str(c))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }
}
