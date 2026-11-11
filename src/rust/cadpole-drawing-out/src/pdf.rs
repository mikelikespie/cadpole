//! PDF output, over `pdf-writer` (MIT OR Apache-2.0, verified against the
//! crate's own metadata; its only runtime dependencies are `bitflags`, `itoa`,
//! `memchr` and `ryu`, all already vendored).
//!
//! One page per drawing, vector throughout. PDF user space is points
//! (1/72 inch) with y **up**, which is the same handedness as the drawing IR,
//! so the only transform is the millimetre-to-point scale.
//!
//! **Text is text.** A string set in one of the standard faces
//! ([`cadpole_drawing::stdfont`]) is a `Type1` font resource naming
//! Helvetica or Courier with `WinAnsiEncoding`, no font program embedded,
//! which every reader is required to carry. A string in the bundled face
//! (Inter, regular or bold) is real text too: the writer shapes it,
//! and the glyphs the whole file uses are embedded once as a TrueType subset
//! instanced at each weight (`Type0` over a `CIDFontType2`, `Identity-H`,
//! with a `ToUnicode` map), so it is selectable, searchable, the face the
//! SVG names, and positioned glyph by glyph as the shaper kerned it. The
//! two weights share one code space and one `ToUnicode` map. A string with
//! a character the face or the encoding lacks falls back to filled
//! outlines, and `text_outlines` forces the outlines for every string, for
//! a file that has to look identical with no fonts at all.
//!
//! **Streams are deflated** (`miniz_oxide`, already a dependency of the 3MF
//! writer): a sheet of glyph outlines is 15 MB of plain text and a few
//! hundred kilobytes deflated. `compress: false` writes them plain, for
//! reading a file by eye.

use std::collections::HashMap;
use std::sync::Arc;

use cadpole_core::sketch::{Loop, Path, Segment, P2};
use cadpole_drawing::ir::{Anchor, Dash, Drawing, FontRef, HAlign, Image, Prim, Resolved, VAlign};
use cadpole_drawing::stdfont::{encode_winansi, StdFont};
use cadpole_drawing::text::embed;
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

/// Millimetres to PostScript points.
const MM_TO_PT: f64 = 72.0 / 25.4;

/// miniz level: 6 is the usual balance of size against time.
const DEFLATE_LEVEL: u8 = 6;

/// How the PDF is written.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// Margin around the content when the drawing has no sheet, in mm.
    pub margin_mm: f64,
    /// Chord tolerance when flattening curves, in mm. 0.05 mm is finer than
    /// any plotter.
    pub tolerance_mm: f64,
    pub title: Option<String>,
    /// Deflate every stream (the default). Off writes them plain.
    pub compress: bool,
    /// Fill every string as glyph outlines rather than writing text.
    pub text_outlines: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions {
            margin_mm: 5.0,
            tolerance_mm: 0.05,
            title: None,
            compress: true,
            text_outlines: false,
        }
    }
}

/// Write `d` as a one-page PDF.
pub fn write(d: &Drawing, opts: &PdfOptions) -> Vec<u8> {
    write_pages(&[d], opts)
}

/// Write `pages` as one PDF, one page per drawing in order: a sheet
/// set is one file.
pub fn write_pages(pages: &[&Drawing], opts: &PdfOptions) -> Vec<u8> {
    let mut alloc = Alloc::default();
    let catalog_id = alloc.next();
    let page_tree_id = alloc.next();
    let info_id = alloc.next();

    // Every page's content first, so the fonts and pictures it uses are
    // known before its dictionary is written.
    let mut built: Vec<BuiltPage> = Vec::new();
    // In the order first used, never a hash map's: the objects are written
    // in this order, and the same drawing must be the same bytes.
    let mut fonts: Vec<(StdFont, Ref, String)> = Vec::new();
    let mut embedded = Embedded::default();
    let mut images: Vec<(Arc<Image>, Ref, Option<Ref>, String)> = Vec::new();
    for d in pages {
        let page_id = alloc.next();
        let content_id = alloc.next();
        let page = build_page(d, opts, &mut alloc, &mut fonts, &mut embedded, &mut images);
        built.push(BuiltPage {
            page_id,
            content_id,
            page,
        });
    }

    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(built.iter().map(|b| b.page_id))
        .count(built.len() as i32);
    for b in &built {
        {
            let mut page = pdf.page(b.page_id);
            page.parent(page_tree_id);
            page.media_box(Rect::new(0.0, 0.0, b.page.w_pt, b.page.h_pt));
            page.contents(b.content_id);
            let mut res = page.resources();
            if !b.page.fonts.is_empty() {
                let mut fdict = res.fonts();
                for (name, id) in &b.page.fonts {
                    fdict.pair(Name(name.as_bytes()), *id);
                }
                fdict.finish();
            }
            if !b.page.images.is_empty() {
                let mut xdict = res.x_objects();
                for (name, id) in &b.page.images {
                    xdict.pair(Name(name.as_bytes()), *id);
                }
                xdict.finish();
            }
            res.finish();
        }
        write_stream(&mut pdf, b.content_id, &b.page.content, opts.compress);
    }
    for (font, id, _) in &fonts {
        pdf.type1_font(*id)
            .base_font(Name(font.base_font().as_bytes()))
            .encoding_predefined(Name(b"WinAnsiEncoding"));
    }
    embedded.write(&mut pdf, &mut alloc, opts.compress);
    for (image, id, mask_id, _) in &images {
        write_image(&mut pdf, image, *id, *mask_id, opts.compress);
    }
    {
        let title = opts.title.clone().or_else(|| {
            pages
                .first()
                .and_then(|d| d.sheet.as_ref())
                .map(|s| s.title_block.title.clone())
                .filter(|t| !t.is_empty())
        });
        let mut info = pdf.document_info(info_id);
        info.producer(TextStr("Cadpole"));
        if let Some(t) = &title {
            info.title(TextStr(t));
        }
        info.finish();
    }
    pdf.finish()
}

/// Object ids, handed out in order.
#[derive(Default)]
struct Alloc(i32);

impl Alloc {
    fn next(&mut self) -> Ref {
        self.0 += 1;
        Ref::new(self.0)
    }
}

struct BuiltPage {
    page_id: Ref,
    content_id: Ref,
    page: PageContent,
}

struct PageContent {
    w_pt: f32,
    h_pt: f32,
    content: Vec<u8>,
    /// The fonts this page uses: resource name and font dictionary.
    fonts: Vec<(String, Ref)>,
    /// The pictures this page places: resource name and object.
    images: Vec<(String, Ref)>,
}

fn write_stream(pdf: &mut Pdf, id: Ref, data: &[u8], compress: bool) {
    if compress {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, DEFLATE_LEVEL);
        pdf.stream(id, &packed).filter(Filter::FlateDecode);
    } else {
        pdf.stream(id, data);
    }
}

/// An image XObject: RGB samples, and a separate gray soft mask when the
/// picture has transparency.
fn write_image(pdf: &mut Pdf, image: &Image, id: Ref, mask_id: Option<Ref>, compress: bool) {
    let n = (image.width as usize) * (image.height as usize);
    let mut rgb = Vec::with_capacity(n * 3);
    let mut alpha = Vec::with_capacity(n);
    for px in image.rgba.chunks_exact(4).take(n) {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3]);
    }
    let packed;
    let samples: &[u8] = if compress {
        packed = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, DEFLATE_LEVEL);
        &packed
    } else {
        &rgb
    };
    {
        let mut x = pdf.image_xobject(id, samples);
        x.width(image.width as i32);
        x.height(image.height as i32);
        x.color_space().device_rgb();
        x.bits_per_component(8);
        x.interpolate(true);
        if compress {
            x.filter(Filter::FlateDecode);
        }
        if let Some(m) = mask_id {
            x.s_mask(m);
        }
    }
    if let Some(m) = mask_id {
        let packed_a;
        let a: &[u8] = if compress {
            packed_a = miniz_oxide::deflate::compress_to_vec_zlib(&alpha, DEFLATE_LEVEL);
            &packed_a
        } else {
            &alpha
        };
        let mut x = pdf.image_xobject(m, a);
        x.width(image.width as i32);
        x.height(image.height as i32);
        x.color_space().device_gray();
        x.bits_per_component(8);
        if compress {
            x.filter(Filter::FlateDecode);
        }
    }
}

/// One page's content stream, collecting the resources it needs.
fn build_page(
    d: &Drawing,
    opts: &PdfOptions,
    alloc: &mut Alloc,
    fonts: &mut Vec<(StdFont, Ref, String)>,
    embedded: &mut Embedded,
    images: &mut Vec<(Arc<Image>, Ref, Option<Ref>, String)>,
) -> PageContent {
    let (lo, hi) = cadpole_drawing::svg::page(d, opts.margin_mm);
    let (w, h) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
    // `pdf-writer` writes a non-finite float verbatim, so one NaN reaching
    // here produces `NaN NaN m` in the content stream and a file no reader can
    // open. Nothing upstream should hand us one; this is the last place that
    // can still say so cheaply.
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    let pt = |p: P2| {
        (
            finite((p.x - lo.x) * MM_TO_PT),
            finite((p.y - lo.y) * MM_TO_PT),
        )
    };

    let mut content = Content::new();
    content.set_line_cap(pdf_writer::types::LineCapStyle::RoundCap);
    content.set_line_join(pdf_writer::types::LineJoinStyle::RoundJoin);

    let mut page_fonts: Vec<(String, Ref)> = Vec::new();
    let mut page_images: Vec<(String, Ref)> = Vec::new();
    let mut state = State::default();
    for prim in d.flatten() {
        match &prim {
            Prim::Path {
                style,
                path,
                closed,
                ..
            } => {
                state.apply(&mut content, style, true);
                if emit_path(&mut content, path, *closed, &pt, opts.tolerance_mm) {
                    content.stroke();
                }
            }
            Prim::Fill {
                style,
                outer,
                holes,
                ..
            } => {
                state.apply(&mut content, style, false);
                let mut any = emit_loop(&mut content, outer, &pt, opts.tolerance_mm);
                for hole in holes {
                    any |= emit_loop(&mut content, hole, &pt, opts.tolerance_mm);
                }
                if any {
                    content.fill_even_odd();
                }
            }
            Prim::Text {
                style,
                at,
                text,
                height_mm,
                rotation_deg,
                anchor,
                font,
            } => {
                if text.is_empty() {
                    continue;
                }
                // Real text for a standard face whose every character the
                // encoding has; outlines otherwise.
                let real = match font {
                    FontRef::Standard(f) if !opts.text_outlines => {
                        encode_winansi(text).map(|bytes| (*f, bytes))
                    }
                    _ => None,
                };
                if font.is_builtin() && !opts.text_outlines {
                    if let Some(runs) = embedded.shape(text, font) {
                        let (name, id) = embedded.font_for(font, alloc);
                        if !page_fonts.iter().any(|(n, _)| *n == name) {
                            page_fonts.push((name.clone(), id));
                        }
                        state.apply(&mut content, style, false);
                        let em_mm = cadpole_drawing::text::em_for_cap_height_in(*height_mm, font);
                        show_runs(&mut content, &name, &runs, em_mm, *height_mm, *at, *rotation_deg, *anchor, &pt);
                        continue;
                    }
                }
                if let Some((f, bytes)) = real {
                    let (name, id) = match fonts.iter().find(|(g, ..)| *g == f) {
                        Some((_, id, name)) => (name.clone(), *id),
                        None => {
                            let name = format!("F{}", fonts.len() + 1);
                            let id = alloc.next();
                            fonts.push((f, id, name.clone()));
                            (name, id)
                        }
                    };
                    if !page_fonts.iter().any(|(n, _)| *n == name) {
                        page_fonts.push((name.clone(), id));
                    }
                    state.apply(&mut content, style, false);
                    let em_mm = cadpole_drawing::text::em_for_cap_height_in(*height_mm, font);
                    let size_pt = finite(em_mm * MM_TO_PT) as f32;
                    // Where the baseline's left end sits: the anchor moved
                    // back along the text by the alignment fraction of the
                    // width and down by the baseline shift, then rotated.
                    let width = cadpole_drawing::text::width_in(text, *height_mm, font);
                    let back = match anchor.h {
                        HAlign::Left => 0.0,
                        HAlign::Center => width / 2.0,
                        HAlign::Right => width,
                    };
                    let dy = cadpole_drawing::text::baseline_shift(*anchor, *height_mm);
                    let a = rotation_deg.to_radians();
                    let (c, s) = (a.cos(), a.sin());
                    let origin = P2::new(
                        at.x - back * c - dy * s,
                        at.y - back * s + dy * c,
                    );
                    let (ox, oy) = pt(origin);
                    content.begin_text();
                    content.set_font(Name(name.as_bytes()), size_pt);
                    content.set_text_matrix([
                        c as f32, s as f32, -s as f32, c as f32, ox as f32, oy as f32,
                    ]);
                    content.show(Str(&bytes));
                    content.end_text();
                    continue;
                }
                let loops = cadpole_drawing::text::outlines(
                    text,
                    *height_mm,
                    *at,
                    *rotation_deg,
                    *anchor,
                    font,
                );
                if loops.is_empty() {
                    continue;
                }
                state.apply(&mut content, style, false);
                let mut any = false;
                for l in &loops {
                    any |= emit_loop(&mut content, l, &pt, opts.tolerance_mm);
                }
                if any {
                    content.fill_nonzero();
                }
            }
            Prim::Image {
                at, w: iw, h: ih, image, ..
            } => {
                if image.width == 0 || image.height == 0 {
                    continue;
                }
                let name = match images.iter().find(|(im, ..)| Arc::ptr_eq(im, image)) {
                    Some((_, _, _, name)) => name.clone(),
                    None => {
                        let name = format!("Im{}", images.len() + 1);
                        let id = alloc.next();
                        let mask = image.has_alpha().then(|| alloc.next());
                        images.push((image.clone(), id, mask, name.clone()));
                        name
                    }
                };
                let id = images.iter().find(|(_, _, _, n)| *n == name).map(|(_, id, ..)| *id).unwrap();
                if !page_images.iter().any(|(n, _)| *n == name) {
                    page_images.push((name.clone(), id));
                }
                let (x, y) = pt(*at);
                content.save_state();
                content.transform([
                    finite(iw * MM_TO_PT) as f32,
                    0.0,
                    0.0,
                    finite(ih * MM_TO_PT) as f32,
                    x as f32,
                    y as f32,
                ]);
                content.x_object(Name(name.as_bytes()));
                content.restore_state();
            }
        }
    }
    PageContent {
        w_pt: finite(w * MM_TO_PT) as f32,
        h_pt: finite(h * MM_TO_PT) as f32,
        content: content.finish().to_vec(),
        fonts: page_fonts,
        images: page_images,
    }
}

/// One line of a string in the bundled face, shaped: each glyph's code in
/// the file's shared code space, its advance as shaped and as the embedded
/// font's widths will say, all in font units.
struct Run {
    codes: Vec<u16>,
    shaped: Vec<f64>,
    widths: Vec<u16>,
    units_per_em: f64,
}

impl Run {
    fn width_em(&self) -> f64 {
        self.shaped.iter().sum::<f64>() / self.units_per_em
    }
}

/// The bundled face as the file embeds it: one code space for both
/// weights, a code per (weight, glyph) in the order first used, and a
/// `Type0` font per weight whose glyph `c` is code `c`'s glyph at that
/// weight (empty for the other weight's codes). Deterministic: the same
/// pages give the same codes, the same subsets and the same bytes.
#[derive(Default)]
struct Embedded {
    /// `(weight, original glyph id)` of code `i + 1`.
    codes: Vec<(u16, u32)>,
    index: HashMap<(u16, u32), u16>,
    /// What each code reads back as.
    texts: Vec<String>,
    /// Per weight in first-use order: the font dictionary and its resource name.
    fonts: Vec<(u16, Ref, String)>,
}

impl Embedded {
    /// Shape `text` line by line, or `None` when any line has a character
    /// the face lacks or a glyph the shaper moved off its baseline: the
    /// caller then fills the string as outlines, as before.
    fn shape(&mut self, text: &str, font: &FontRef) -> Option<Vec<Run>> {
        let weight = cadpole_drawing::text::builtin_weight(font) as u16;
        let face = cadpole_drawing::text::builtin();
        let mut lines = Vec::new();
        for line in text.split('\n') {
            let shaped = embed::shape_line(face, line, weight as f64).ok()?;
            if shaped.glyphs.iter().any(|g| g.gid == 0 || g.x_offset != 0.0 || g.y_offset != 0.0) {
                return None;
            }
            lines.push((line, shaped));
        }
        if self.codes.len() + lines.iter().map(|(_, l)| l.glyphs.len()).sum::<usize>() >= u16::MAX as usize {
            return None;
        }
        let mut runs = Vec::new();
        for (line, shaped) in lines {
            let gids: Vec<u32> = shaped.glyphs.iter().map(|g| g.gid).collect();
            let widths = embed::advances(face, weight as f64, &gids).ok()?;
            let texts = shaped.glyph_texts(line);
            let mut codes = Vec::with_capacity(gids.len());
            for (gid, t) in gids.iter().zip(texts) {
                let code = match self.index.get(&(weight, *gid)) {
                    Some(c) => *c,
                    None => {
                        self.codes.push((weight, *gid));
                        self.texts.push(t);
                        let c = self.codes.len() as u16;
                        self.index.insert((weight, *gid), c);
                        c
                    }
                };
                // A glyph first met inside a cluster reads as nothing; give
                // it the text it stands for when it is met on its own.
                let slot = &mut self.texts[code as usize - 1];
                if slot.is_empty() {
                    *slot = shaped_text_for(line, &shaped, *gid);
                }
                codes.push(code);
            }
            runs.push(Run {
                codes,
                shaped: shaped.glyphs.iter().map(|g| g.advance).collect(),
                widths,
                units_per_em: shaped.units_per_em,
            });
        }
        Some(runs)
    }

    /// The resource name and font dictionary for `font`'s weight.
    fn font_for(&mut self, font: &FontRef, alloc: &mut Alloc) -> (String, Ref) {
        let weight = cadpole_drawing::text::builtin_weight(font) as u16;
        if let Some((_, id, name)) = self.fonts.iter().find(|(w, ..)| *w == weight) {
            return (name.clone(), *id);
        }
        let name = format!("K{}", self.fonts.len() + 1);
        let id = alloc.next();
        self.fonts.push((weight, id, name.clone()));
        (name, id)
    }

    /// Write every weight's font: the `Type0` dictionary, its CID font, the
    /// descriptor, the subset program and the shared `ToUnicode` map.
    fn write(&self, pdf: &mut Pdf, alloc: &mut Alloc, compress: bool) {
        if self.fonts.is_empty() {
            return;
        }
        let cmap_id = alloc.next();
        write_stream(pdf, cmap_id, &to_unicode(&self.texts), compress);
        for (weight, type0_id, _) in &self.fonts {
            let slots: Vec<Option<u32>> =
                self.codes.iter().map(|(w, g)| (w == weight).then_some(*g)).collect();
            let Ok(f) = embed::instance_subset(cadpole_drawing::text::builtin(), *weight as f64, &slots) else {
                continue;
            };
            let (cid_id, desc_id, file_id) = (alloc.next(), alloc.next(), alloc.next());
            let base = format!("{}+{}", subset_tag(*weight, &slots), f.ps_name);
            let k = 1000.0 / f.units_per_em as f32;
            pdf.type0_font(*type0_id)
                .base_font(Name(base.as_bytes()))
                .encoding_predefined(Name(b"Identity-H"))
                .descendant_font(cid_id)
                .to_unicode(cmap_id);
            {
                let mut cid = pdf.cid_font(cid_id);
                cid.subtype(CidFontType::Type2);
                cid.base_font(Name(base.as_bytes()));
                cid.system_info(SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 });
                cid.font_descriptor(desc_id);
                cid.default_width(0.0);
                cid.widths().consecutive(0, f.advances.iter().map(|a| *a as f32 * k));
                cid.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            pdf.font_descriptor(desc_id)
                .name(Name(base.as_bytes()))
                .flags(FontFlags::SYMBOLIC)
                .bbox(Rect::new(
                    f.bbox[0] as f32 * k,
                    f.bbox[1] as f32 * k,
                    f.bbox[2] as f32 * k,
                    f.bbox[3] as f32 * k,
                ))
                .italic_angle(0.0)
                .ascent(f.ascent as f32 * k)
                .descent(f.descent as f32 * k)
                .cap_height(f.cap_height as f32 * k)
                .stem_v(if f.weight >= 600 { 140.0 } else { 80.0 })
                .font_file2(file_id);
            if compress {
                let packed = miniz_oxide::deflate::compress_to_vec_zlib(&f.ttf, DEFLATE_LEVEL);
                pdf.stream(file_id, &packed)
                    .filter(Filter::FlateDecode)
                    .pair(Name(b"Length1"), f.ttf.len() as i32);
            } else {
                pdf.stream(file_id, &f.ttf).pair(Name(b"Length1"), f.ttf.len() as i32);
            }
        }
    }
}

/// The characters glyph `gid` stands for in `line` where it first starts a
/// cluster of its own, or nothing.
fn shaped_text_for(line: &str, shaped: &embed::ShapedLine, gid: u32) -> String {
    let texts = shaped.glyph_texts(line);
    shaped
        .glyphs
        .iter()
        .zip(texts)
        .find(|(g, t)| g.gid == gid && !t.is_empty())
        .map(|(_, t)| t)
        .unwrap_or_default()
}

/// Six capitals naming a subset, from what is in it (ISO 32000 9.6.4): the
/// same glyphs give the same tag.
fn subset_tag(weight: u16, slots: &[Option<u32>]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    eat(weight as u64);
    for s in slots {
        eat(s.map(|g| g as u64 + 1).unwrap_or(0));
    }
    (0..6)
        .map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char)
        .collect()
}

/// A `ToUnicode` CMap: code `i + 1` reads as `texts[i]`.
fn to_unicode(texts: &[String]) -> Vec<u8> {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(usize, &String)> = texts.iter().enumerate().filter(|(_, t)| !t.is_empty()).collect();
    for chunk in pairs.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (i, t) in chunk {
            let hex: String = t.encode_utf16().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{:04X}> <{hex}>\n", i + 1));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMapResource defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// Show shaped `runs` (one per line) as text: each line's baseline placed
/// as the outlines would be -- aligned by its own width, the block shifted
/// for the anchor's vertical alignment, rotated about `at` -- and each
/// glyph moved by what its shaped advance adds to the font's width (the
/// kerning), in a `TJ` array.
#[allow(clippy::too_many_arguments)]
fn show_runs(
    content: &mut Content,
    name: &str,
    runs: &[Run],
    em_mm: f64,
    cap_mm: f64,
    at: P2,
    rotation_deg: f64,
    anchor: Anchor,
    pt: &impl Fn(P2) -> (f64, f64),
) {
    let line_step = 1.2 * em_mm;
    let n = runs.len() as f64;
    let shift = match anchor.v {
        VAlign::Baseline | VAlign::Bottom => 0.0,
        VAlign::Middle => (-cap_mm + (n - 1.0) * line_step) / 2.0,
        VAlign::Top => -cap_mm,
    };
    let a = rotation_deg.to_radians();
    let (c, s) = (a.cos(), a.sin());
    let size_pt = (em_mm * MM_TO_PT) as f32;
    for (i, run) in runs.iter().enumerate() {
        if run.codes.is_empty() {
            continue;
        }
        let w = run.width_em() * em_mm;
        let dx = match anchor.h {
            HAlign::Left => 0.0,
            HAlign::Center => -w / 2.0,
            HAlign::Right => -w,
        };
        let dy = shift - i as f64 * line_step;
        let origin = P2::new(at.x + dx * c - dy * s, at.y + dx * s + dy * c);
        let (ox, oy) = pt(origin);
        content.begin_text();
        content.set_font(Name(name.as_bytes()), size_pt);
        content.set_text_matrix([c as f32, s as f32, -s as f32, c as f32, ox as f32, oy as f32]);
        {
            let mut show = content.show_positioned();
            let mut items = show.items();
            let mut bytes: Vec<u8> = Vec::new();
            for (k, code) in run.codes.iter().enumerate() {
                bytes.extend_from_slice(&code.to_be_bytes());
                let adj = (run.widths[k] as f64 - run.shaped[k]) * 1000.0 / run.units_per_em;
                if adj.abs() > 0.05 && k + 1 < run.codes.len() {
                    items.show(Str(&bytes));
                    bytes.clear();
                    items.adjust(adj as f32);
                }
            }
            if !bytes.is_empty() {
                items.show(Str(&bytes));
            }
        }
        content.end_text();
    }
}

/// Tracks the graphics state so the content stream does not repeat itself.
#[derive(Default)]
struct State {
    stroke: Option<(f32, f32, f32)>,
    fill: Option<(f32, f32, f32)>,
    width: Option<f32>,
    dash: Option<Dash>,
}

impl State {
    fn apply(&mut self, content: &mut Content, style: &Resolved, stroking: bool) {
        let c = (
            style.color.0 as f32 / 255.0,
            style.color.1 as f32 / 255.0,
            style.color.2 as f32 / 255.0,
        );
        if stroking {
            if self.stroke != Some(c) {
                content.set_stroke_rgb(c.0, c.1, c.2);
                self.stroke = Some(c);
            }
            let w = (style.weight_mm * MM_TO_PT) as f32;
            if self.width != Some(w) {
                content.set_line_width(w.max(0.05));
                self.width = Some(w);
            }
            if self.dash.as_ref() != Some(&style.dash) {
                let p: Vec<f32> = style
                    .dash
                    .pattern()
                    .iter()
                    .map(|v| (v * MM_TO_PT) as f32)
                    .collect();
                content.set_dash_pattern(p, 0.0);
                self.dash = Some(style.dash.clone());
            }
        } else if self.fill != Some(c) {
            content.set_fill_rgb(c.0, c.1, c.2);
            self.fill = Some(c);
        }
    }
}

fn emit_path(
    content: &mut Content,
    path: &Path,
    closed: bool,
    pt: &impl Fn(P2) -> (f64, f64),
    tolerance: f64,
) -> bool {
    if path.segments.is_empty() {
        return false;
    }
    let (x, y) = pt(path.start);
    content.move_to(x as f32, y as f32);
    for s in &path.segments {
        emit_segment(content, s, pt, tolerance);
    }
    if closed {
        content.close_path();
    }
    true
}

fn emit_loop(
    content: &mut Content,
    l: &Loop,
    pt: &impl Fn(P2) -> (f64, f64),
    tolerance: f64,
) -> bool {
    if l.segments.is_empty() {
        return false;
    }
    let (x, y) = pt(l.segments[0].start());
    content.move_to(x as f32, y as f32);
    for s in &l.segments {
        emit_segment(content, s, pt, tolerance);
    }
    content.close_path();
    true
}

fn emit_segment(
    content: &mut Content,
    s: &Segment,
    pt: &impl Fn(P2) -> (f64, f64),
    tolerance: f64,
) {
    match s {
        Segment::Line { b, .. } => {
            let (x, y) = pt(*b);
            content.line_to(x as f32, y as f32);
        }
        Segment::Arc { a, b, via } => {
            // PDF has no arc operator; a chord-bounded polyline at 0.05 mm is
            // finer than any plotter resolves, and it keeps the writer honest
            // about what the file contains.
            for p in sample_arc(*a, *b, *via, tolerance) {
                let (x, y) = pt(p);
                content.line_to(x as f32, y as f32);
            }
        }
        Segment::Bezier { b, ctrl, .. } => match ctrl.len() {
            1 => {
                // A quadratic raised to a cubic: exact, not approximated.
                let (c, e) = (ctrl[0], *b);
                let a = s.start();
                let c1 = P2::new(a.x + 2.0 / 3.0 * (c.x - a.x), a.y + 2.0 / 3.0 * (c.y - a.y));
                let c2 = P2::new(e.x + 2.0 / 3.0 * (c.x - e.x), e.y + 2.0 / 3.0 * (c.y - e.y));
                let (p1, p2, p3) = (pt(c1), pt(c2), pt(e));
                content.cubic_to(
                    p1.0 as f32,
                    p1.1 as f32,
                    p2.0 as f32,
                    p2.1 as f32,
                    p3.0 as f32,
                    p3.1 as f32,
                );
            }
            n if n >= 2 => {
                let (p1, p2, p3) = (pt(ctrl[0]), pt(ctrl[1]), pt(*b));
                content.cubic_to(
                    p1.0 as f32,
                    p1.1 as f32,
                    p2.0 as f32,
                    p2.1 as f32,
                    p3.0 as f32,
                    p3.1 as f32,
                );
            }
            _ => {
                let (x, y) = pt(*b);
                content.line_to(x as f32, y as f32);
            }
        },
    }
}

/// Points along an arc, excluding the start, with chord error under
/// `tolerance`.
pub(crate) fn sample_arc(a: P2, b: P2, via: P2, tolerance: f64) -> Vec<P2> {
    let Some((c, r, t0, t1)) = cadpole_core::sketch::arc_params(a, b, via) else {
        return vec![b];
    };
    if !(r.is_finite() && r > 0.0) {
        return vec![b];
    }
    let sweep = (t1 - t0).abs();
    // Chord error of a segment spanning angle d is r(1 - cos(d/2)).
    let max_step = if tolerance >= r {
        std::f64::consts::PI
    } else {
        2.0 * (1.0 - tolerance / r).clamp(-1.0, 1.0).acos()
    };
    let n = ((sweep / max_step.max(1e-6)).ceil() as usize).clamp(1, 512);
    (1..=n)
        .map(|i| {
            let t = t0 + (t1 - t0) * (i as f64 / n as f64);
            P2::new(c.x + t.cos() * r, c.y + t.sin() * r)
        })
        .collect()
}

/// The text a PDF shows, read back out of the file the way `pdftotext`
/// would: every `(...) Tj` string of every content stream, deflated or not,
/// decoded from WinAnsi. Test support, and the proof that "selectable" is a
/// property of the bytes rather than of a viewer.
pub fn extract_text(pdf: &[u8]) -> Vec<String> {
    // The streams, inflated, first: the `ToUnicode` maps are read out of
    // them before any content is decoded.
    let mut streams: Vec<Vec<u8>> = Vec::new();
    let mut at = 0;
    while let Some(i) = find(pdf, at, b"stream\n") {
        let start = i + b"stream\n".len();
        let Some(end) = find(pdf, start, b"endstream") else {
            break;
        };
        let dict_start = pdf[..i].iter().rposition(|b| *b == b'<').unwrap_or(0);
        let dict = &pdf[dict_start.saturating_sub(64)..i];
        let raw = &pdf[start..end];
        let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
        streams.push(if find(dict, 0, b"FlateDecode").is_some() {
            miniz_oxide::inflate::decompress_to_vec_zlib(raw).unwrap_or_default()
        } else {
            raw.to_vec()
        });
        at = end + b"endstream".len();
    }
    let mut cmap: HashMap<u16, String> = HashMap::new();
    for s in &streams {
        if find(s, 0, b"beginbfchar").is_some() {
            read_bfchar(s, &mut cmap);
        }
    }
    let mut out = Vec::new();
    for s in &streams {
        if find(s, 0, b"beginbfchar").is_none() {
            out.extend(shown_strings(s, &cmap));
        }
    }
    out
}

/// `<code> <utf16>` pairs of a `ToUnicode` map.
fn read_bfchar(s: &[u8], cmap: &mut HashMap<u16, String>) {
    let text = String::from_utf8_lossy(s);
    for line in text.lines() {
        let parts: Vec<&str> = line.split(|c| c == '<' || c == '>').filter(|p| !p.trim().is_empty()).collect();
        if parts.len() != 2 || parts[0].len() != 4 {
            continue;
        }
        let Ok(code) = u16::from_str_radix(parts[0], 16) else { continue };
        let units: Vec<u16> = (0..parts[1].len() / 4)
            .filter_map(|k| u16::from_str_radix(&parts[1][k * 4..k * 4 + 4], 16).ok())
            .collect();
        cmap.insert(code, String::from_utf16_lossy(&units));
    }
}

/// The strings a content stream shows: `(...) Tj` in a standard face,
/// decoded from WinAnsi, and `[...] TJ` in the embedded face, its two-byte
/// codes read through `cmap`, one string per operator.
fn shown_strings(content: &[u8], cmap: &HashMap<u16, String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'[' {
            // A TJ array: literal strings and numbers up to `]`.
            let mut j = i + 1;
            let mut codes: Vec<u8> = Vec::new();
            while j < content.len() && content[j] != b']' {
                if content[j] == b'(' {
                    let (bytes, next) = literal(content, j);
                    codes.extend(bytes);
                    j = next;
                } else {
                    j += 1;
                }
            }
            let rest = &content[(j + 1).min(content.len())..];
            if rest.iter().skip_while(|b| b.is_ascii_whitespace()).take(2).copied().collect::<Vec<u8>>() == b"TJ" {
                out.push(codes.chunks(2).filter(|c| c.len() == 2).map(|c| cmap.get(&u16::from_be_bytes([c[0], c[1]])).cloned().unwrap_or_default()).collect());
            }
            i = j + 1;
            continue;
        }
        if content[i] != b'(' {
            i += 1;
            continue;
        }
        let (bytes, j) = literal(content, i);
        let rest = &content[j..];
        let is_tj = rest
            .iter()
            .skip_while(|b| b.is_ascii_whitespace())
            .take(2)
            .copied()
            .collect::<Vec<u8>>()
            == b"Tj";
        if is_tj {
            out.push(bytes.iter().map(|b| winansi_char(*b)).collect());
        }
        i = j;
    }
    out
}

/// The bytes of the literal string starting at `content[i] == '('`,
/// unescaped, and the index after its closing parenthesis.
fn literal(content: &[u8], i: usize) -> (Vec<u8>, usize) {
    let mut j = i + 1;
    let mut depth = 1;
    let mut bytes = Vec::new();
    while j < content.len() && depth > 0 {
        match content[j] {
            b'\\' if j + 1 < content.len() => {
                let c = content[j + 1];
                if c.is_ascii_digit() {
                    // An octal escape: up to three digits.
                    let mut v = 0u32;
                    let mut k = j + 1;
                    while k < content.len() && k < j + 4 && content[k].is_ascii_digit() {
                        v = v * 8 + (content[k] - b'0') as u32;
                        k += 1;
                    }
                    bytes.push(v as u8);
                    j = k;
                    continue;
                }
                bytes.push(match c {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 8,
                    b'f' => 12,
                    other => other,
                });
                j += 2;
                continue;
            }
            b'(' => {
                depth += 1;
                bytes.push(b'(');
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    bytes.push(b')');
                }
            }
            other => bytes.push(other),
        }
        j += 1;
    }
    (bytes, j)
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The character a WinAnsi byte stands for, enough for reading a drawing
/// back: Latin-1 straight through, the typographic row by table.
fn winansi_char(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        other => other as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_drawing::{layer, Anchor, SheetSize};

    fn rect() -> Drawing {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(50.0, 25.0), 100.0, 50.0));
        d
    }

    fn plain() -> PdfOptions {
        PdfOptions {
            compress: false,
            ..Default::default()
        }
    }

    #[test]
    fn the_page_is_the_drawing_in_points() {
        let bytes = write(
            &rect(),
            &PdfOptions {
                margin_mm: 0.0,
                ..plain()
            },
        );
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("%PDF-"));
        // 100 mm x 50 mm is 283.46 x 141.73 pt.
        assert!(
            s.contains("/MediaBox [0 0 283.46"),
            "{}",
            &s[..600.min(s.len())]
        );
        assert!(s.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn output_is_byte_identical_across_runs() {
        let d = rect();
        assert_eq!(
            write(&d, &PdfOptions::default()),
            write(&d, &PdfOptions::default())
        );
        // Several standard faces: their objects are written in the order the
        // page first used them, not in a hash map's order, which differs
        // from one process to the next.
        let mut d = rect();
        let l = d.layer(layer::TEXT);
        for (i, f) in [StdFont::Helvetica, StdFont::CourierBold, StdFont::HelveticaBold, StdFont::Courier].into_iter().enumerate() {
            d.push(cadpole_drawing::Item::Text(cadpole_drawing::TextItem {
                style: cadpole_drawing::Style::on(l),
                at: P2::new(10.0, 10.0 + i as f64 * 5.0),
                text: format!("face {i}"),
                height_mm: 2.5,
                rotation_deg: 0.0,
                anchor: Anchor::LEFT,
                font: FontRef::Standard(f),
                model_height: false,
            }));
        }
        let a = write(&d, &plain());
        assert_eq!(a, write(&d, &plain()));
        let s = String::from_utf8_lossy(&a);
        let order: Vec<usize> = ["/Helvetica\n", "/Courier-Bold", "/Helvetica-Bold", "/Courier\n"]
            .iter()
            .map(|n| s.find(&format!("/BaseFont {n}")).unwrap_or_else(|| panic!("{n} in {s}")))
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "fonts in first-use order: {order:?}");
    }

    fn builtin_page(lines: &[(&str, FontRef)]) -> Drawing {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        for (i, (t, f)) in lines.iter().enumerate() {
            d.push(cadpole_drawing::Item::Text(cadpole_drawing::TextItem {
                style: cadpole_drawing::Style::on(l),
                at: P2::new(10.0, 10.0 + 8.0 * i as f64),
                text: (*t).into(),
                height_mm: 5.0,
                rotation_deg: 0.0,
                anchor: Anchor::CENTER,
                font: f.clone(),
                model_height: false,
            }));
        }
        d
    }

    /// The bundled face is embedded and its strings are text.
    #[test]
    fn builtin_text_is_embedded_text_that_reads_back() {
        let d = builtin_page(&[("CADPOLE", FontRef::Builtin), ("Family room 297 sf", FontRef::Builtin)]);
        let bytes = write(&d, &plain());
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Subtype /Type0"), "{s}");
        assert!(s.contains("/CIDFontType2") && s.contains("/Identity-H") && s.contains("/ToUnicode"));
        assert!(s.contains("/FontFile2") && s.contains("/Length1"), "the font program is embedded");
        assert!(s.contains("+Inter-Regular"), "a subset, tagged: {s}");
        assert!(s.contains("TJ"), "shown with a TJ array");
        assert_eq!(extract_text(&bytes), vec!["CADPOLE".to_string(), "Family room 297 sf".to_string()]);
        // Deflated, the same.
        let packed = write(&d, &PdfOptions::default());
        assert_eq!(extract_text(&packed), extract_text(&bytes));
        assert_eq!(packed, write(&d, &PdfOptions::default()), "byte-identical across runs");
        // Outlines on request: fills, no font.
        let outlined = write(&d, &PdfOptions { text_outlines: true, ..plain() });
        let o = String::from_utf8_lossy(&outlined);
        assert!(!o.contains("/Font"));
        assert!(o.contains("\nf\n") || o.contains(" f\n"), "the nonzero fill");
        assert!(extract_text(&outlined).is_empty());
    }

    #[test]
    fn bold_and_regular_share_one_code_space_and_each_has_its_own_instance() {
        let d = builtin_page(&[("Plan", FontRef::Builtin), ("PLAN TITLE", FontRef::BuiltinBold), ("Plan", FontRef::BuiltinBold)]);
        let bytes = write(&d, &plain());
        let s = String::from_utf8_lossy(&bytes);
        assert_eq!(s.matches("/Subtype /Type0").count(), 2, "one font per weight");
        assert!(s.contains("+Inter-Bold") && s.contains("+Inter-Regular"));
        assert_eq!(s.matches("/CMapName").count(), 1, "one ToUnicode map, shared");
        assert_eq!(extract_text(&bytes), vec!["Plan", "PLAN TITLE", "Plan"]);
    }

    #[test]
    fn a_character_the_face_lacks_falls_back_to_outlines_for_that_string_only() {
        let d = builtin_page(&[("ok", FontRef::Builtin), ("\u{10FFFD}", FontRef::Builtin)]);
        let bytes = write(&d, &plain());
        assert_eq!(extract_text(&bytes), vec!["ok".to_string()]);
    }

    #[test]
    fn multiline_builtin_text_is_one_run_a_line() {
        let d = builtin_page(&[("one\ntwo", FontRef::Builtin)]);
        assert_eq!(extract_text(&write(&d, &plain())), vec!["one".to_string(), "two".to_string()]);
    }

    fn helvetica_page(text: &str) -> Drawing {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        d.push(cadpole_drawing::Item::Text(cadpole_drawing::TextItem {
            style: cadpole_drawing::Style::on(l),
            at: P2::new(40.0, 20.0),
            text: text.into(),
            height_mm: 3.0,
            rotation_deg: 0.0,
            anchor: Anchor::CENTER,
            font: FontRef::Standard(StdFont::Helvetica),
            model_height: false,
        }));
        d
    }

    #[test]
    fn standard_face_text_is_written_as_text_and_reads_back() {
        let d = helvetica_page("SITE PLAN 12'-6\"");
        let bytes = write(&d, &PdfOptions::default());
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Type1"), "a Type1 font resource: {s}");
        assert!(s.contains("/BaseFont /Helvetica"));
        assert!(s.contains("/WinAnsiEncoding"));
        assert!(s.contains("/FlateDecode"), "the content stream is deflated");
        assert_eq!(extract_text(&bytes), vec!["SITE PLAN 12'-6\"".to_string()]);
        // Plain streams read back the same way.
        assert_eq!(extract_text(&write(&d, &plain())), vec!["SITE PLAN 12'-6\"".to_string()]);
        // And the outline option turns it into fills, no font at all.
        let outlined = write(&d, &PdfOptions { text_outlines: true, ..Default::default() });
        assert!(!String::from_utf8_lossy(&outlined).contains("/Font"));
        assert!(extract_text(&outlined).is_empty());
    }

    #[test]
    fn a_character_the_encoding_lacks_falls_back_to_outlines() {
        let d = helvetica_page("\u{4e2d}");
        let bytes = write(&d, &plain());
        assert!(!String::from_utf8_lossy(&bytes).contains("/Font"));
    }

    #[test]
    fn compression_shrinks_a_page_of_outlines() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        for i in 0..20 {
            d.text(l, P2::new(10.0, 10.0 + i as f64 * 6.0), "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG", 3.0, Anchor::LEFT);
        }
        let packed = write(&d, &PdfOptions::default());
        let loose = write(&d, &plain());
        assert!(packed.len() * 2 < loose.len(), "{} vs {}", packed.len(), loose.len());
    }

    #[test]
    fn a_set_is_one_file_with_a_page_per_sheet_at_the_sheet_size() {
        let mut a = helvetica_page("COVER");
        a.sheet = Some(cadpole_drawing::Sheet::new(SheetSize::ARCH_D));
        let mut b = helvetica_page("SITE PLAN");
        b.sheet = Some(cadpole_drawing::Sheet::new(SheetSize::ARCH_D));
        let bytes = write_pages(&[&a, &b], &PdfOptions::default());
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Count 2"), "{s}");
        // ARCH D is 36 x 24 in = 2592 x 1728 pt.
        assert_eq!(s.matches("/MediaBox [0 0 2592 1728]").count(), 2, "{s}");
        assert_eq!(extract_text(&bytes), vec!["COVER".to_string(), "SITE PLAN".to_string()]);
        // One font object shared by both pages.
        assert_eq!(s.matches("/BaseFont /Helvetica").count(), 1);
    }

    #[test]
    fn a_picture_becomes_an_image_xobject_with_a_soft_mask_when_it_has_alpha() {
        let image = Arc::new(Image {
            width: 2,
            height: 2,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 0],
            png: None,
        });
        let mut d = Drawing::new();
        let l = d.layer(layer::SYMB);
        d.push(cadpole_drawing::Item::Image(cadpole_drawing::ImageItem {
            style: cadpole_drawing::Style::on(l),
            at: P2::new(10.0, 10.0),
            w: 40.0,
            h: 40.0,
            image,
        }));
        let bytes = write(&d, &plain());
        let s = String::from_utf8_lossy(&bytes);
        assert_eq!(s.matches("/Subtype /Image").count(), 2, "the picture and its mask: {s}");
        assert!(s.contains("/SMask"));
        assert!(s.contains("/Im1 Do"));
    }

    #[test]
    fn arcs_are_sampled_within_tolerance() {
        let a = P2::new(10.0, 0.0);
        let b = P2::new(-10.0, 0.0);
        let via = P2::new(0.0, 10.0);
        let pts = sample_arc(a, b, via, 0.05);
        assert!(pts.len() > 8, "{} points", pts.len());
        for p in &pts {
            let r = (p.x * p.x + p.y * p.y).sqrt();
            assert!((r - 10.0).abs() < 1e-9, "on the circle: {r}");
        }
        assert!((pts.last().unwrap().x - b.x).abs() < 1e-9);
    }
}
