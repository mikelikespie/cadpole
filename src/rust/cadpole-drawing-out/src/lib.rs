//! The drawing writers that need a third-party library: PDF and DXF.
//!
//! `cadpole-drawing` owns the IR and the SVG writer, which needs nothing but
//! `format!`. This crate is the other half, kept separate so a PDF-writer
//! change does not rebuild the IR, the rasterizer or the playground wasm.
//!
//! Both writers consume [`cadpole_drawing::ir::Prim`], the same flattened list the
//! SVG writer sees, so the three formats cannot disagree about where a
//! dimension sits or how wide its arrowhead is.

pub mod dxf;
pub mod pdf;

#[cfg(test)]
mod roundtrip;

pub use dxf::{write as write_dxf, DxfOptions};
pub use pdf::{write as write_pdf, write_pages as write_pdf_pages, PdfOptions};

/// The files a sheet set writes in `format` at `path`: one PDF of
/// every page, or one SVG / DXF per sheet named `<stem>_<number>.<ext>`
/// beside where the PDF would go, because those formats have no pages.
/// `only` writes one sheet, by index, to `path` itself in any of the three.
/// `text_outlines` fills every string as glyph outlines instead of writing
/// text.
pub fn write_set(
    set: &cadpole_drawing::SheetSet,
    path: &str,
    format: &str,
    text_outlines: bool,
    only: Option<usize>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let format = format.trim().to_ascii_lowercase();
    if !FORMATS.contains(&format.as_str()) {
        return Err(format!("unknown sheet set format {format:?} (use pdf, svg or dxf)"));
    }
    let outlines = text_outlines || set.text_outlines;
    let indices: Vec<usize> = match only {
        Some(i) if i < set.sheets.len() => vec![i],
        Some(i) => return Err(format!("sheet #{i} is not in the set {:?}", set.name)),
        None => (0..set.sheets.len()).collect(),
    };
    let pages: Vec<(usize, cadpole_drawing::Drawing)> =
        indices.iter().filter_map(|i| set.page(*i).map(|p| (*i, p))).collect();
    let title = |i: usize| {
        let s = &set.sheets[i];
        format!("{} {}", s.number, s.name)
    };
    if format == "pdf" {
        let refs: Vec<&cadpole_drawing::Drawing> = pages.iter().map(|(_, p)| p).collect();
        let doc_title = match only {
            Some(i) => title(i),
            None => set.name.clone(),
        };
        let opts = PdfOptions {
            title: Some(doc_title).filter(|n| !n.is_empty()),
            text_outlines: outlines,
            ..PdfOptions::default()
        };
        return Ok(vec![(path.to_owned(), pdf::write_pages(&refs, &opts))]);
    }
    let (stem, ext) = match path.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), e.to_owned()),
        None => (path.to_owned(), format.clone()),
    };
    let mut out = Vec::new();
    for (i, page) in &pages {
        let sheet = &set.sheets[*i];
        let file = match only {
            Some(_) => path.to_owned(),
            None => format!("{stem}_{}.{ext}", sheet.number.replace(['/', '\\', ' '], "_")),
        };
        let data = if format == "svg" {
            cadpole_drawing::svg::write(
                page,
                &cadpole_drawing::svg::SvgOptions {
                    title: Some(title(*i)),
                    text_outlines: outlines,
                    ..Default::default()
                },
            )
            .into_bytes()
        } else {
            dxf::write(
                page,
                &DxfOptions {
                    text_outlines: outlines,
                    ..Default::default()
                },
            )?
        };
        out.push((file, data));
    }
    Ok(out)
}

/// Write `drawing` in `format`, which is `"svg"`, `"pdf"` or `"dxf"`.
///
/// One entry point so the CLI, the Starlark `export_*` builtins and the
/// `Drawing` rpc all pick a writer the same way: by name, with the same error
/// message when the name is wrong.
pub fn write(drawing: &cadpole_drawing::Drawing, format: &str) -> Result<Vec<u8>, String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "svg" => Ok(
            cadpole_drawing::svg::write(drawing, &cadpole_drawing::svg::SvgOptions::default())
                .into_bytes(),
        ),
        "pdf" => Ok(pdf::write(drawing, &PdfOptions::default())),
        "dxf" => dxf::write(drawing, &DxfOptions::default()),
        other => Err(format!(
            "unknown drawing format {other:?} (use svg, pdf or dxf)"
        )),
    }
}

/// The formats [`write`] accepts, for the "expected one of" diagnostic.
pub const FORMATS: &[&str] = &["svg", "pdf", "dxf"];

#[cfg(test)]
mod tests {
    use cadpole_core::sketch::{Loop, P2};
    use cadpole_drawing::{layer, Drawing};

    fn rect() -> Drawing {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(50.0, 25.0), 100.0, 50.0));
        d
    }

    #[test]
    fn every_format_produces_something_recognisable() {
        let d = rect();
        let svg = super::write(&d, "svg").unwrap();
        assert!(svg.starts_with(b"<?xml"));
        let pdf = super::write(&d, "pdf").unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let dxf = super::write(&d, "dxf").unwrap();
        assert!(String::from_utf8_lossy(&dxf).contains("LWPOLYLINE"));
    }

    #[test]
    fn a_set_writes_one_pdf_or_one_file_per_sheet() {
        let mut set = cadpole_drawing::SheetSet::new("set", cadpole_drawing::SheetSize::A3);
        set.add_sheet("G0.0", "COVER", None);
        set.add_sheet("A2.0", "PLAN", None);
        let pdf = super::write_set(&set, "out/set.pdf", "pdf", false, None).unwrap();
        assert_eq!(pdf.len(), 1);
        assert_eq!(pdf[0].0, "out/set.pdf");
        assert!(String::from_utf8_lossy(&pdf[0].1).contains("/Count 2"));
        let svgs = super::write_set(&set, "out/set.svg", "svg", false, None).unwrap();
        assert_eq!(svgs.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["out/set_G0.0.svg", "out/set_A2.0.svg"]);
        assert!(String::from_utf8_lossy(&svgs[1].1).contains("<title>A2.0 PLAN</title>"));
        let dxfs = super::write_set(&set, "set.dxf", "dxf", false, None).unwrap();
        assert_eq!(dxfs.len(), 2);
        assert!(super::write_set(&set, "set.png", "png", false, None).is_err());
        // One sheet goes to the path as given, in any format.
        let one = super::write_set(&set, "plan.svg", "svg", false, Some(1)).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].0, "plan.svg");
        let one = super::write_set(&set, "plan.pdf", "pdf", false, Some(1)).unwrap();
        assert!(String::from_utf8_lossy(&one[0].1).contains("/Count 1"));
        assert!(super::write_set(&set, "x.pdf", "pdf", false, Some(7)).is_err());
    }

    #[test]
    fn an_unknown_format_says_what_is_available() {
        let e = super::write(&rect(), "dwg").unwrap_err();
        assert!(e.contains("svg, pdf or dxf"), "{e}");
    }
}
