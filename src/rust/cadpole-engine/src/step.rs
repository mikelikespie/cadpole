//! STEP export: the part that knows about documents.
//!
//! The file format lives in `cadpole-export-step`; this module picks the bodies,
//! turns a mesh body into a diagnostic instead of a bad file, and validates
//! what was written before handing it back.
//!
//! **A mesh body cannot be exported.** STEP carries surfaces, and a body the
//! mesh kernel produced has only triangles; rebuilding surfaces from them is a
//! fit, not a conversion, and Cadpole does not do it. The
//! diagnostic names the body and says what to change, because the usual cause
//! is a boolean that `kernel="auto"` routed to the mesh kernel for speed.

use cadpole_core::Context;
use cadpole_core::ids::BodyId;
use cadpole_export_step::{validate, Options, Part};

/// Write the given bodies as one STEP file. `stem` becomes the `PRODUCT` name.
pub fn export_step(ctx: &mut Context, ids: &[BodyId], stem: &str, tolerance: f64) -> anyhow::Result<Vec<u8>> {
    if ids.is_empty() {
        anyhow::bail!("STEP export: no visible bodies");
    }
    let mut mesh_bodies = Vec::new();
    for id in ids {
        let b = ctx.body(*id)?;
        if b.body.to_brep().is_err() {
            mesh_bodies.push(b.name.clone());
        }
    }
    if !mesh_bodies.is_empty() {
        anyhow::bail!(
            "STEP export needs exact surfaces, and {} {} on the mesh kernel's triangles, which cannot become a \
             B-rep again. Put {} back on the B-rep path with kernel=\"brep\" on the boolean that \
             made {}, or export STL/3MF instead.",
            plural_list(&mesh_bodies),
            if mesh_bodies.len() == 1 { "is" } else { "are" },
            if mesh_bodies.len() == 1 { "it" } else { "them" },
            if mesh_bodies.len() == 1 { "it" } else { "them" },
        );
    }

    // Collect the solids first: `Part` borrows them, and `ctx.body` cannot be
    // called while a borrow is live.
    let named: Vec<(String, cadpole_core::kernel::KSolid)> = ids
        .iter()
        .map(|id| {
            let b = ctx.body(*id)?;
            Ok((b.name.clone(), b.body.to_brep()?.clone()))
        })
        .collect::<cadpole_core::Result<Vec<_>>>()?;
    let parts: Vec<Part<'_>> = named.iter().map(|(n, s)| Part { name: n.as_str(), solid: s }).collect();

    let options = Options {
        product_name: if stem.is_empty() { "cadpole".to_owned() } else { stem.to_owned() },
        file_name: format!("{}.step", if stem.is_empty() { "cadpole" } else { stem }),
        originating_system: format!("Cadpole {}", crate::VERSION),
        tolerance: if tolerance > 0.0 { tolerance } else { cadpole_export_step::DEFAULT_TOLERANCE },
        ..Options::default()
    };
    let (text, stats) = cadpole_export_step::write_with_stats(&parts, &options).map_err(|e| anyhow::anyhow!("{e}"))?;
    // Never hand out a file that does not pass our own reader. This is cheap
    // (one linear pass over the text) next to the modelling that produced it.
    if let Err(problems) = validate::check(&text) {
        anyhow::bail!("STEP export produced a file that fails its own structural checks:\n{problems}");
    }
    let _ = stats;
    Ok(text.into_bytes())
}

/// `"a"`, `"a and b"`, `"a, b and c"`.
fn plural_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => format!("{a:?}"),
        [rest @ .., last] => format!(
            "{} and {last:?}",
            rest.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::geom::Plane;
    use cadpole_core::kernel::KernelChoice;
    use cadpole_core::sketch::{Loop, Sketch, P2};
    use std::collections::{BTreeMap, HashMap};
    use cadpole_script::{EvalInput, ScriptFile};

    fn prism(ctx: &mut Context, w: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, w));
        ctx.extrude("extrude", &s, None, h, BTreeMap::new(), Some(name.to_owned()), None).unwrap()[0]
    }

    #[test]
    fn a_brep_body_exports_a_valid_step_file() {
        let mut ctx = Context::new();
        let b = prism(&mut ctx, 20.0, 10.0, "block");
        let bytes = export_step(&mut ctx, &[b], "doc", 0.0).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("ISO-10303-21;"));
        assert!(text.contains("MANIFOLD_SOLID_BREP('block'"), "{text}");
        assert!(text.contains("PRODUCT('doc'"), "the document name becomes the product name");
        assert!(validate::check(&text).is_ok());
    }

    #[test]
    fn a_mesh_body_is_a_diagnostic_that_says_what_to_change() {
        let mut ctx = Context::new();
        let a = prism(&mut ctx, 20.0, 10.0, "a");
        let s = Sketch::new(Plane::XY.offset(4.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 8.0, 40.0));
        let b = ctx.extrude("extrude", &s, None, 10.0, BTreeMap::new(), Some("cutter".into()), None).unwrap()[0];
        // Force the mesh kernel, which is what `kernel="auto"` does to a big
        // boolean.
        let r = ctx.boolean(cadpole_core::kernel::BoolOp::Subtract, a, &[b], KernelChoice::Mesh, Some("m".into()), None).unwrap();
        let e = export_step(&mut ctx, &[r], "doc", 0.0).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("\"m\""), "{msg}");
        assert!(msg.contains("kernel=\"brep\""), "{msg}");
        assert!(msg.contains("STL/3MF"), "{msg}");
    }

    #[test]
    fn exporting_no_bodies_is_an_error() {
        let mut ctx = Context::new();
        assert!(export_step(&mut ctx, &[], "doc", 0.0).is_err());
    }

    #[test]
    fn several_bodies_go_into_one_file() {
        let mut ctx = Context::new();
        let a = prism(&mut ctx, 20.0, 10.0, "a");
        let b = prism(&mut ctx, 5.0, 5.0, "b");
        let text = String::from_utf8(export_step(&mut ctx, &[a, b], "doc", 0.0).unwrap()).unwrap();
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 2);
    }

    #[test]
    fn the_names_of_several_mesh_bodies_are_all_listed() {
        assert_eq!(plural_list(&["a".into()]), "\"a\"");
        assert_eq!(plural_list(&["a".into(), "b".into()]), "\"a\" and \"b\"");
        assert_eq!(plural_list(&["a".into(), "b".into(), "c".into()]), "\"a\", \"b\" and \"c\"");
    }

    // ---- the Gridfinity acceptance test -----------------------------------

    /// `examples/gridfinity_bin.star` has to open in FreeCAD as one valid
    /// solid. Nothing in this repository runs FreeCAD, so
    /// what is asserted here is the closest available proxy — that a
    /// *different* STEP implementation (`truck-stepio`'s reader) parses the
    /// file, rebuilds one closed shell from it, and that the shell it rebuilds
    /// encloses the same volume, the right way out, as the model Cadpole
    /// tessellated.
    ///
    /// A 1x1x2 bin, because the point is the shape of the file, not its size,
    /// and a 2x3x6 takes six times as long on the exact kernel.
    #[test]
    fn the_gridfinity_bin_exports_a_step_file_a_separate_reader_rebuilds() {
        use truck_meshalgo::prelude::*;

        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("gridfinity_bin.star"))
            .expect("examples/gridfinity_bin.star");
        let doc = crate::document_from_scripts("gridfinity_bin", vec![("main.star".into(), src)]);
        let overrides = [
            ("width", cadpole_core::ParamValue::Number(1.0)),
            ("length", cadpole_core::ParamValue::Number(1.0)),
            ("height", cadpole_core::ParamValue::Number(2.0)),
            // STEP needs exact surfaces; `auto` would send the foot union to
            // the mesh kernel and the export would (correctly) refuse.
            ("kernel", cadpole_core::ParamValue::Text("brep".into())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        let (mut ctx, _) = crate::evaluate_document_with(&doc, &overrides, 0.0, 0.0);
        let errors: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(errors.is_empty(), "the bin must evaluate cleanly: {errors:?}");

        let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        assert_eq!(ids.len(), 1, "one bin");
        let want = ctx.body_mesh(ids[0]).unwrap().0.clone();
        let text = String::from_utf8(export_step(&mut ctx, &ids, "gridfinity_bin", 0.0).unwrap()).unwrap();

        // It is structurally valid (export_step already checked, but this is
        // the assertion the test is making, so it makes it).
        validate::check(&text).expect("structurally valid");
        assert!(text.contains("MANIFOLD_SOLID_BREP('bin'"), "one named solid");
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 1);

        // A separate implementation reads it back.
        let table = truck_stepio::r#in::Table::from_step(&text).expect("truck-stepio parses it");
        assert_eq!(table.shell.len(), 1, "one shell");
        let shell = table.shell.values().next().unwrap();
        let cshell = table.to_compressed_shell(shell).expect("the shell rebuilds");
        assert_eq!(
            cshell.faces.len(),
            text.matches("= ADVANCED_FACE").count(),
            "every face written comes back"
        );
        let poly = cshell.robust_triangulation(0.05).to_polygon();
        let pos = poly.positions();
        let mut volume = 0.0;
        for tri in poly.faces().triangle_iter() {
            let (a, b, c) = (pos[tri[0].pos], pos[tri[1].pos], pos[tri[2].pos]);
            volume += a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0;
        }
        let expected = want.volume();
        assert!(expected > 0.0, "the source model is not inside out");
        let rel = (volume - expected).abs() / expected;
        assert!(
            rel < 0.02,
            "the round-tripped bin encloses {volume:.1} mm3, the model {expected:.1} mm3 ({:.2}% apart)",
            rel * 100.0
        );
    }

    // ---- the STEP round trip of every example --------------------

    /// One example evaluated the way `cadpole eval examples/x.star` evaluates
    /// it: the entry as `main.star`, its siblings loadable, the fixture
    /// assets, and `kernel=brep` where the script offers a kernel (STEP
    /// needs exact surfaces).
    fn example_context(dir: &str, entry: &std::path::Path) -> Context {
        let src = std::fs::read_to_string(entry).expect("the example");
        let mut scripts = vec![ScriptFile { path: "main.star".into(), source: src.clone() }];
        let mut sibs: Vec<_> = std::fs::read_dir(dir)
            .expect("the examples directory")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p != entry && p.extension().and_then(|e| e.to_str()) == Some("star"))
            .collect();
        sibs.sort();
        for p in sibs {
            if let (Some(name), Ok(source)) = (p.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(&p)) {
                scripts.push(ScriptFile { path: name.to_owned(), source });
            }
        }
        let (assets, warnings) = cadpole_assets::load_dir(std::path::Path::new(dir).join("assets").as_path());
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut overrides = HashMap::new();
        if src.contains("param(\"kernel\"") {
            overrides.insert("kernel".to_owned(), cadpole_core::ParamValue::Text("brep".into()));
        }
        cadpole_script::evaluate(EvalInput {
            scripts,
            assets: Some(std::sync::Arc::new(assets)),
            overrides,
            ..Default::default()
        })
    }

    /// Faces by class, `plane:6 cylinder:2`, in a fixed order.
    fn class_census(topo: &cadpole_core::tessellate::Topology) -> String {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for f in &topo.faces {
            *counts.entry(f.surface.kind_name()).or_default() += 1;
        }
        counts.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(" ")
    }

    /// Export one B-rep body, read it back with `cadpole_import`, and compare
    /// the two tessellations at `tol`: `(faces, classes, relative volume
    /// difference)`, or why it could not be done.
    fn round_trip_body(ctx: &mut Context, id: BodyId, stem: &str) -> Result<(usize, String, String, f64), String> {
        let tol = ctx.tolerance;
        let solid = match &ctx.body(id).map_err(|e| e.to_string())?.body {
            cadpole_core::context::Body::Brep(s) => s.clone(),
            cadpole_core::context::Body::Mesh(_) => return Err("a mesh body: STEP export refuses it".into()),
        };
        let text = String::from_utf8(export_step(ctx, &[id], stem, 0.0).map_err(|e| format!("export: {e}"))?).unwrap();
        let im = cadpole_import::step::import_unbounded(&text, &cadpole_import::step::Options::default()).map_err(|e| format!("import: {e}"))?;
        if im.solids.len() != 1 {
            return Err(format!("import: {} solids from one body", im.solids.len()));
        }
        let back = &im.solids[0];
        let (m0, t0) = cadpole_core::tessellate::tessellate(&solid, tol).map_err(|e| format!("tessellate source: {e}"))?;
        let (m1, t1) = cadpole_core::tessellate::tessellate(&back.solid, tol).map_err(|e| format!("tessellate import: {e}"))?;
        let (v0, v1) = (m0.volume(), m1.volume());
        let rel = (v1 - v0).abs() / v0.abs().max(1e-9);
        let c0 = class_census(&t0);
        let c1 = class_census(&t1);
        if back.faces.len() != solid.face_iter().count() + back.seam_faces {
            return Err(format!("{} faces went out, {} came back", solid.face_iter().count(), back.faces.len()));
        }
        Ok((back.faces.len(), c0, c1, rel))
    }

    /// Every example's B-rep bodies go out as STEP and come back through
    /// Cadpole's own reader to the same volume and the same face classes. The
    /// table on stderr is the record; the assertion is on the examples
    /// known to round-trip, so an example that legitimately cannot (a mesh
    /// body, a kernel that refuses at the baseline) is a row, not a broken
    /// gate — and a known-good example that stops round-tripping is.
    #[test]
    fn every_example_round_trips_through_step() {
        // Examples whose every B-rep body round-trips within 1e-3 of its
        // volume with its face classes intact. Extend it when a new example
        // does; an entry that stops holding fails the test.
        const KNOWN_GOOD: &[&str] = &[
            "bearing_608.star",
            "bracket.star",
            "charging_stand.star",
            "constrained_bracket.star",
            "constrained_lprofile.star",
            "drawing_sheet.star",
            "filleted_bracket.star",
            "gears.star",
            "knurled_knob.star",
            "material_bar.star",
            "materials.star",
            "nightstand.star",
            "nightstand_scene.star",
            "peg_board.star",
            "planter.star",
            "print_ready.star",
            "showcase.star",
            "split_for_bed.star",
            "table_lamp.star",
            "text_label.star",
            "vase.star",
            "vendor_bearing.star",
        ];
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("the examples directory")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("star"))
            .filter(|p| !p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with("_future.star")).unwrap_or(false))
            .collect();
        files.sort();
        let mut rows: Vec<String> = Vec::new();
        let mut failed: Vec<(String, String)> = Vec::new();
        let mut clean: Vec<String> = Vec::new();
        for p in &files {
            let file = p.file_name().unwrap().to_str().unwrap().to_owned();
            let t = std::time::Instant::now();
            let mut ctx = example_context(&dir, p);
            let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
            let mut all_good = !ids.is_empty();
            for id in ids {
                let name = ctx.body(id).unwrap().name.clone();
                match round_trip_body(&mut ctx, id, &file) {
                    Ok((faces, c0, c1, rel)) => {
                        let ok = rel < 1e-3 && c0 == c1;
                        all_good &= ok;
                        rows.push(format!(
                            "| {file} | {name} | {faces} | {c1} | {rel:.1e} | {} |",
                            if ok { "ok" } else if c0 != c1 { "classes differ" } else { "volume differs" }
                        ));
                        if !ok {
                            failed.push((file.clone(), format!("{name}: {c0} -> {c1}, volume {rel:.1e} apart")));
                        }
                    }
                    Err(e) => {
                        all_good = false;
                        rows.push(format!("| {file} | {name} | - | - | - | {e} |"));
                        failed.push((file.clone(), format!("{name}: {e}")));
                    }
                }
            }
            if all_good {
                clean.push(file.clone());
            }
            eprintln!("{file}: {:.1} s", t.elapsed().as_secs_f64());
        }
        eprintln!("\nSTEP round trip of every example (Cadpole writes, Cadpole reads; volumes at the display tolerance):");
        eprintln!("| example | body | faces | classes | volume | status |\n| --- | --- | --- | --- | --- | --- |");
        for r in &rows {
            eprintln!("{r}");
        }
        let broken: Vec<&(String, String)> = failed.iter().filter(|(f, _)| KNOWN_GOOD.contains(&f.as_str())).collect();
        let could_add: Vec<&String> = clean.iter().filter(|f| !KNOWN_GOOD.contains(&f.as_str())).collect();
        eprintln!("\nnot round-tripping ({}):", failed.len());
        for (f, why) in &failed {
            eprintln!("- {f}: {why}");
        }
        if !could_add.is_empty() {
            eprintln!("\nround-trip cleanly but are not in KNOWN_GOOD: {could_add:?}");
        }
        assert!(broken.is_empty(), "known-good examples no longer round-trip: {broken:?}");
        assert!(rows.iter().any(|r| r.ends_with("| ok |")), "no example round-tripped at all");
    }

    /// The 608 bearing, Cadpole's own STEP read back by Cadpole's own reader: nine
    /// solids, each to a millionth of its volume on the same tessellation,
    /// the races' faces cylinders and planes, the balls' spheres, in the
    /// file's order.
    #[test]
    fn the_608_bearing_round_trips_to_a_millionth() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let mut ctx = example_context(&dir, &std::path::Path::new(&dir).join("bearing_608.star"));
        let errors: Vec<&str> = ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Error).map(|d| d.message.as_str()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        assert_eq!(ids.len(), 9, "two races and seven balls");
        let text = String::from_utf8(export_step(&mut ctx, &ids, "bearing_608", 0.0).unwrap()).unwrap();
        validate::check(&text).expect("structurally valid");
        let im = cadpole_import::step::import_unbounded(&text, &cadpole_import::step::Options::default()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(im.solids.len(), 9);
        let tol = ctx.tolerance;
        for (k, (id, back)) in ids.iter().zip(&im.solids).enumerate() {
            let name = ctx.body(*id).unwrap().name.clone();
            let solid = match &ctx.body(*id).unwrap().body {
                cadpole_core::context::Body::Brep(s) => s.clone(),
                _ => panic!("{name} is not a B-rep"),
            };
            assert_eq!(back.label, name, "solid {k} comes back under its own name, in file order");
            let (m0, t0) = cadpole_core::tessellate::tessellate(&solid, tol).unwrap();
            let (m1, t1) = cadpole_core::tessellate::tessellate(&back.solid, tol).unwrap();
            let rel = (m1.volume() - m0.volume()).abs() / m0.volume();
            eprintln!("{name}: {:.6} -> {:.6} mm3 ({rel:.1e}), {}", m0.volume(), m1.volume(), class_census(&t1));
            assert!(rel < 1e-6, "{name}: {} became {} ({rel:.1e})", m0.volume(), m1.volume());
            assert_eq!(class_census(&t1), class_census(&t0), "{name}");
            assert_eq!(back.faces.len(), solid.face_iter().count());
            assert!(back.faces.iter().enumerate().all(|(i, f)| f.index == i && f.piece == 0), "{name}: faces by file index");
            let census = class_census(&t1);
            if k < 2 {
                assert_eq!(census, "cylinder:12 plane:12", "{name}");
            } else {
                assert_eq!(census, "sphere:2", "{name}");
            }
        }
        assert_eq!(im.solids.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()[..3], ["inner_race", "outer_race", "ball"]);
    }
}
