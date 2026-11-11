//! Sheet sets: a numbered set of pages of one size, each with the
//! frame, a title column that carries the project, a stamp, the issue and
//! the sheet index, and whatever views and annotation the script placed in
//! paper millimetres.
//!
//! A page is a [`Drawing`] whose items are already in paper coordinates and
//! whose `sheet` says the page size; a view is a [`Group`] with a scale, a
//! placement and a viewport. This module owns the set and the title column
//! and the annotation vocabulary the script reaches for — text, notes,
//! tables, dimensions, leaders, north arrows, scale bars, legends, hatches
//! and polylines — so a sheet is composed out of the same IR the writers
//! consume and nothing is drawn twice.

use cadpole_core::sketch::{Loop, Path, P2};

use crate::dim::{Dim, DimStyle};
use crate::ir::{
    layer, Anchor, ClipRect, Dash, Drawing, FontRef, Group, HAlign, HatchItem, Item, Leader,
    Pattern, Rgb, Scale, Stroke, Style, TextItem, VAlign, Xform2,
};
use crate::sheet::{Sheet, SheetSize};
use crate::stdfont::StdFont;
use crate::units::{LengthUnit, UnitFormat};

/// What the title column says on every sheet of a set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleTemplate {
    pub project: String,
    pub address: String,
    pub block_lot: String,
    /// Lines in the boxed stamp ("CONCEPT STUDY", "NOT FOR CONSTRUCTION").
    pub stamp: Vec<String>,
    /// Small print under the stamp.
    pub notes: Vec<String>,
    pub issue: String,
    pub date: String,
    pub drawn: String,
    pub checked: String,
    /// More label/value rows under the standard ones.
    pub extra: Vec<(String, String)>,
    /// Width of the column, paper millimetres; 0 picks one for the sheet.
    pub column_mm: f64,
}

impl TitleTemplate {
    fn rows(&self) -> Vec<(String, String)> {
        let mut r = Vec::new();
        for (k, v) in [
            ("ISSUE", &self.issue),
            ("DATE", &self.date),
            ("DRAWN BY", &self.drawn),
            ("CHECKED BY", &self.checked),
        ] {
            if !v.is_empty() {
                r.push((k.to_owned(), v.clone()));
            }
        }
        r.extend(self.extra.iter().cloned());
        r
    }
}

/// One page of the set.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPage {
    pub number: String,
    pub name: String,
    /// The sheet's nominal scale, for the title column and as the default
    /// of every view placed on it.
    pub scale: Option<Scale>,
    /// Draw a north arrow in the title column.
    pub north: bool,
    /// The page's own items, in paper millimetres. The frame and the title
    /// column are added by [`SheetSet::page`], never stored here.
    pub drawing: Drawing,
    /// How many views have been placed, for naming.
    pub views: usize,
}

/// A set of sheets.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSet {
    pub name: String,
    pub size: SheetSize,
    pub margin_mm: f64,
    pub title: TitleTemplate,
    /// The face every string on the set is set in unless a call says
    /// otherwise: Helvetica by default, so the PDF's text is text.
    pub font: FontRef,
    pub unit_format: UnitFormat,
    pub default_scale: Option<Scale>,
    pub sheets: Vec<SheetPage>,
    /// Write every string as filled outlines in every format, for a file
    /// that must look the same on a machine with no fonts at all.
    pub text_outlines: bool,
    /// A degrees-clockwise rotation from the page's up to true north, for
    /// the north arrow.
    pub north_degrees: f64,
}

impl SheetSet {
    pub fn new(name: impl Into<String>, size: SheetSize) -> SheetSet {
        SheetSet {
            name: name.into(),
            size,
            margin_mm: 10.0,
            title: TitleTemplate::default(),
            font: FontRef::Standard(StdFont::Helvetica),
            unit_format: UnitFormat::mm(),
            default_scale: None,
            sheets: Vec::new(),
            text_outlines: false,
            north_degrees: 0.0,
        }
    }

    /// Width of the title column on this sheet size.
    pub fn column_mm(&self) -> f64 {
        if self.title.column_mm > 0.0 {
            self.title.column_mm
        } else {
            (self.size.w_mm * 0.11).clamp(55.0, 100.0)
        }
    }

    /// The rectangle a sheet's views and notes may occupy: inside the frame
    /// and clear of the title column, in paper millimetres.
    pub fn drawing_area(&self) -> ClipRect {
        let m = self.margin_mm;
        ClipRect::new(
            P2::new(m, m),
            P2::new(self.size.w_mm - m - self.column_mm() - 4.0, self.size.h_mm - m),
        )
    }

    /// Add a sheet and hand back its index.
    pub fn add_sheet(&mut self, number: impl Into<String>, name: impl Into<String>, scale: Option<Scale>) -> usize {
        let mut d = Drawing::new();
        d.unit_format = self.unit_format;
        d.ordered = true;
        d.sheet = Some(Sheet::new(self.size));
        if let Some(s) = d.sheet.as_mut() {
            s.margin_mm = self.margin_mm;
        }
        self.sheets.push(SheetPage {
            number: number.into(),
            name: name.into(),
            scale: scale.or(self.default_scale),
            north: false,
            drawing: d,
            views: 0,
        });
        self.sheets.len() - 1
    }

    /// The index of the sheet numbered `number`.
    pub fn find(&self, number: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.number.eq_ignore_ascii_case(number.trim()))
    }

    /// The scale a view on sheet `i` takes when it names none.
    pub fn scale_for(&self, i: usize) -> Scale {
        self.sheets
            .get(i)
            .and_then(|s| s.scale)
            .or(self.default_scale)
            .unwrap_or(Scale::ONE_TO_ONE)
    }

    /// The dimension style of the set: architectural ticks and feet-inches
    /// for an imperial document, filled arrows and millimetres otherwise,
    /// in the set's face.
    pub fn dim_style(&self) -> DimStyle {
        let base = if self.unit_format.unit.imperial() {
            DimStyle::architectural()
        } else {
            DimStyle::mechanical()
        };
        DimStyle {
            format: self.unit_format,
            ..base
        }
        .with_font(self.font.clone())
    }

    /// Sheet `i` as a finished drawing: its items, then the frame and the
    /// title column with the set's index on it.
    pub fn page(&self, i: usize) -> Option<Drawing> {
        let sheet = self.sheets.get(i)?;
        let mut d = sheet.drawing.clone();
        d.scale = sheet.scale.unwrap_or(Scale::ONE_TO_ONE);
        if self.text_outlines {
            outline_all(&mut d);
        }
        self.draw_frame(&mut d, i);
        Some(d)
    }

    /// Every sheet, in order, as finished drawings.
    pub fn pages(&self) -> Vec<Drawing> {
        (0..self.sheets.len()).filter_map(|i| self.page(i)).collect()
    }

    /// The frame and the title column: the project, the address and the
    /// block/lot, the stamp, the small print, the issue rows, the sheet
    /// index with this sheet marked, a north arrow when asked, the scale,
    /// the sheet name and the number large in the corner. The column is the
    /// right-hand one of an architectural set, scaled down with the page.
    fn draw_frame(&self, d: &mut Drawing, i: usize) {
        let sheet = &self.sheets[i];
        let title = d.layer(layer::TITLE);
        let text_layer = d.layer(layer::TEXT);
        let m = self.margin_mm;
        let (w, h) = (self.size.w_mm, self.size.h_mm);
        let font = self.font.clone();
        let bold = font.with_bold(true);
        let col = self.column_mm();
        // Type shrinks with the column: a 100 mm column reads at full size.
        let k = (col / 100.0).clamp(0.55, 1.0);
        let (x0, x1) = (w - m - col, w - m);
        let (y0, y1) = (m, h - m);
        let cx = (x0 + x1) / 2.0;

        // The border, drawn twice as architectural sets do.
        rect(d, title, P2::new(m * 0.55, m * 0.55), P2::new(w - m * 0.55, h - m * 0.55), 0.7);
        rect(d, title, P2::new(m, m), P2::new(w - m, h - m), 0.25);
        line(d, title, P2::new(x0, y0), P2::new(x0, y1), 0.7);

        let rule = |d: &mut Drawing, y: f64| line(d, title, P2::new(x0, y), P2::new(x1, y), 0.35);
        let center = |d: &mut Drawing, y: f64, s: &str, size: f64, f: &FontRef| {
            text(d, text_layer, P2::new(cx, y), s, size, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), f, None)
        };
        let left = |d: &mut Drawing, x: f64, y: f64, s: &str, size: f64, f: &FontRef| {
            text(d, text_layer, P2::new(x, y), s, size, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), f, None)
        };
        let t = &self.title;
        let mut y = y1 - 14.0 * k;

        // The project block, each line shrunk to the column if it would
        // run past it.
        let inside = col - 8.0 * k;
        if !t.project.is_empty() {
            center(d, y, &t.project, fit(&t.project, 5.2 * k, inside, &bold), &bold);
            y -= 7.0 * k;
        }
        if !t.address.is_empty() {
            center(d, y, &t.address, fit(&t.address, 2.8 * k, inside, &font), &font);
            y -= 5.0 * k;
        }
        if !t.block_lot.is_empty() {
            center(d, y, &t.block_lot, fit(&t.block_lot, 2.4 * k, inside, &font), &font);
            y -= 4.5 * k;
        }
        y -= 2.5 * k;
        rule(d, y);

        // The stamp.
        if !t.stamp.is_empty() {
            let box_h = (t.stamp.len() as f64) * 5.5 * k + 4.0 * k;
            y -= 6.0 * k;
            rect(d, title, P2::new(x0 + 8.0 * k, y - box_h), P2::new(x1 - 8.0 * k, y), 0.8);
            let mut yy = y - 5.0 * k;
            for s in &t.stamp {
                center(d, yy, s, fit(s, 3.4 * k, col - 20.0 * k, &bold), &bold);
                yy -= 5.5 * k;
            }
            y -= box_h + 3.5 * k;
            for s in &t.notes {
                for l in crate::text::wrap(s, col - 8.0 * k, 1.5 * k, &font) {
                    left(d, x0 + 4.0 * k, y, &l, 1.5 * k, &font);
                    y -= 2.4 * k;
                }
            }
            y -= 1.5 * k;
            rule(d, y);
        }

        // Issue, date, drawn, checked, and the extras.
        let rows = t.rows();
        if !rows.is_empty() {
            y -= 6.0 * k;
            for (kx, v) in &rows {
                left(d, x0 + 4.0 * k, y, &format!("{kx}:"), 2.0 * k, &bold);
                left(d, x0 + 30.0 * k, y, v, 2.0 * k, &font);
                y -= 4.5 * k;
            }
            y -= 1.0 * k;
            rule(d, y);
        }

        // The sheet index, this sheet marked.
        y -= 6.5 * k;
        left(d, x0 + 4.0 * k, y, "SHEET INDEX", 2.4 * k, &bold);
        y -= 4.5 * k;
        for (j, s) in self.sheets.iter().enumerate() {
            let f = if j == i { &bold } else { &font };
            left(d, x0 + 4.0 * k, y, &s.number, 1.9 * k, f);
            let name_lines = crate::text::wrap(&s.name, col - 22.0 * k, 1.7 * k, f);
            for (li, l) in name_lines.iter().enumerate() {
                left(d, x0 + 18.0 * k, y - (li as f64) * 2.6 * k, l, 1.7 * k, f);
            }
            let drop = (name_lines.len().max(1) as f64 - 1.0) * 2.6 * k;
            if j == i {
                line(d, title, P2::new(x0 + 2.0 * k, y - drop - 0.8 * k), P2::new(x1 - 2.0 * k, y - drop - 0.8 * k), 0.3);
            }
            y -= 3.8 * k + drop;
        }
        y -= 1.0 * k;
        rule(d, y);

        // The bottom block: north arrow, scale, sheet name, number.
        let base = y0;
        if sheet.north {
            north_arrow(d, P2::new(x0 + 18.0 * k, base + 62.0 * k), 8.0 * k, self.north_degrees, false, &font);
        }
        let sx = if sheet.north { x0 + 36.0 * k } else { x0 + 4.0 * k };
        left(d, sx, base + 66.0 * k, "SCALE:", 2.2 * k, &bold);
        let scale_text = match sheet.scale {
            Some(s) => s.label(&self.unit_format),
            None => "AS NOTED".to_owned(),
        };
        left(d, sx, base + 61.5 * k, &scale_text, 2.4 * k, &font);
        rule(d, base + 46.0 * k);
        let name_size = if sheet.name.len() < 34 { 2.9 * k } else { 2.3 * k };
        let name_lines = crate::text::wrap(&sheet.name, col - 8.0 * k, name_size, &bold);
        let mut ny = base + 39.5 * k + (name_lines.len() as f64 - 1.0) * name_size * 1.4 / 2.0;
        for l in &name_lines {
            text(d, text_layer, P2::new(x1 - 4.0 * k, ny), l, name_size, 0.0, Anchor::new(HAlign::Right, VAlign::Baseline), &bold, None);
            ny -= name_size * 1.4;
        }
        rule(d, base + 34.0 * k);
        text(d, text_layer, P2::new(x1 - 4.0 * k, base + 8.0 * k), &sheet.number, 16.0 * k, 0.0, Anchor::new(HAlign::Right, VAlign::Baseline), &bold, None);
        left(d, x0 + 4.0 * k, base + 8.0 * k, "SHEET", 2.2 * k, &bold);
        if !self.name.is_empty() {
            left(d, x0 + 4.0 * k, base + 3.0 * k, &self.name, 1.5 * k, &font);
        }
    }
}

/// `size`, or smaller if `text` set at `size` would be wider than `width`.
fn fit(text: &str, size: f64, width: f64, font: &FontRef) -> f64 {
    let w = crate::text::width_in(text, size, font);
    if w > width && w > 0.0 { size * width / w } else { size }
}

/// Set every text item of `d` in the builtin face, which every writer
/// fills as outlines.
fn outline_all(d: &mut Drawing) {
    fn walk(items: &mut [Item]) {
        for it in items {
            match it {
                Item::Text(t) => t.font = FontRef::Builtin,
                Item::Dim(dm) => dm.dim.font = FontRef::Builtin,
                Item::Leader(l) => l.font = FontRef::Builtin,
                Item::Group(g) => walk(&mut g.items),
                _ => {}
            }
        }
    }
    walk(&mut d.items);
}

// ---------------------------------------------------------------------------
// The annotation vocabulary, in paper millimetres unless a group says
// otherwise. Every function pushes onto `d.items` and returns what a script
// wants to know next: a width, a bottom edge.
// ---------------------------------------------------------------------------

/// A stroke with an explicit weight, dash and colour.
pub fn stroke(
    d: &mut Drawing,
    layer_id: usize,
    path: Path,
    closed: bool,
    weight_mm: Option<f64>,
    dash: Option<Dash>,
    color: Option<Rgb>,
) {
    d.push(Item::Stroke(Stroke {
        style: Style {
            layer: layer_id,
            weight_mm,
            dash,
            color,
        },
        name: String::new(),
        path,
        closed,
    }));
}

fn line(d: &mut Drawing, layer_id: usize, a: P2, b: P2, weight_mm: f64) {
    stroke(d, layer_id, Path::new(a).line_to(b), false, Some(weight_mm), None, None);
}

fn rect(d: &mut Drawing, layer_id: usize, lo: P2, hi: P2, weight_mm: f64) {
    let l = Loop::rect(P2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0), hi.x - lo.x, hi.y - lo.y);
    let mut p = Path::new(l.segments[0].start());
    for s in &l.segments {
        p.segments.push(s.clone());
    }
    stroke(d, layer_id, p, true, Some(weight_mm), None, None);
}

/// A polyline through `pts`.
pub fn polyline(
    d: &mut Drawing,
    layer_id: usize,
    pts: &[P2],
    closed: bool,
    weight_mm: Option<f64>,
    dash: Option<Dash>,
    color: Option<Rgb>,
) {
    if pts.len() < 2 {
        return;
    }
    let mut p = Path::new(pts[0]);
    for q in &pts[1..] {
        p = p.line_to(*q);
    }
    stroke(d, layer_id, p, closed, weight_mm, dash, color);
}

/// A filled polygon, in `color` (white is the knock-out under a label).
pub fn fill(d: &mut Drawing, layer_id: usize, outer: Loop, holes: Vec<Loop>, color: Option<Rgb>) {
    d.push(Item::Hatch(HatchItem {
        style: Style {
            layer: layer_id,
            weight_mm: None,
            dash: None,
            color,
        },
        name: String::new(),
        outer,
        holes,
        pattern: Pattern::Solid,
        scale: 1.0,
    }));
}

/// A hatched region.
pub fn hatch(
    d: &mut Drawing,
    layer_id: usize,
    outer: Loop,
    holes: Vec<Loop>,
    pattern: Pattern,
    scale: f64,
    weight_mm: Option<f64>,
    color: Option<Rgb>,
) {
    d.push(Item::Hatch(HatchItem {
        style: Style {
            layer: layer_id,
            weight_mm,
            dash: None,
            color,
        },
        name: String::new(),
        outer,
        holes,
        pattern,
        scale,
    }));
}

/// A run of text; `size` is the cap height. Returns its measured width.
#[allow(clippy::too_many_arguments)]
pub fn text(
    d: &mut Drawing,
    layer_id: usize,
    at: P2,
    s: &str,
    size: f64,
    rotation_deg: f64,
    anchor: Anchor,
    font: &FontRef,
    color: Option<Rgb>,
) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    d.push(Item::Text(TextItem {
        style: Style {
            layer: layer_id,
            weight_mm: None,
            dash: None,
            color,
        },
        at,
        text: s.to_owned(),
        height_mm: size,
        rotation_deg,
        anchor,
        font: font.clone(),
        model_height: false,
    }));
    crate::text::width_in(s, size, font)
}

/// A note wrapped to `width` (or to its own newlines when `width` is
/// `None`), the first baseline at `at`. Returns the y of the next free
/// line under it.
#[allow(clippy::too_many_arguments)]
pub fn note(
    d: &mut Drawing,
    layer_id: usize,
    at: P2,
    s: &str,
    width: Option<f64>,
    size: f64,
    line_height: f64,
    h: HAlign,
    font: &FontRef,
    color: Option<Rgb>,
) -> f64 {
    let lines: Vec<String> = match width {
        Some(w) if w > 0.0 => crate::text::wrap(s, w, size, font),
        _ => s.split('\n').map(str::to_owned).collect(),
    };
    let mut y = at.y;
    for l in &lines {
        text(d, layer_id, P2::new(at.x, y), l, size, 0.0, Anchor::new(h, VAlign::Baseline), font, color);
        y -= line_height;
    }
    y
}

/// A grid table whose top-left corner is `at`. `rows` are rows of cells;
/// the first is the header when `header` is set (bold, under a heavier
/// rule). A cell wider than its column wraps and the row grows. Returns
/// the bottom y.
#[allow(clippy::too_many_arguments)]
pub fn table(
    d: &mut Drawing,
    line_layer: usize,
    text_layer: usize,
    at: P2,
    widths: &[f64],
    rows: &[Vec<String>],
    size: f64,
    row_h: f64,
    align: &[HAlign],
    bold_rows: &[usize],
    header: bool,
    font: &FontRef,
) -> f64 {
    if widths.is_empty() || rows.is_empty() {
        return at.y;
    }
    let bold = font.with_bold(true);
    let total_w: f64 = widths.iter().sum();
    let pad = size * 0.6;
    let line_h = size * 1.35;
    // Wrap every cell first, so the row heights are known before a line
    // is drawn.
    let mut cells: Vec<Vec<Vec<String>>> = Vec::new();
    let mut heights: Vec<f64> = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let f = if (header && ri == 0) || bold_rows.contains(&ri) {
            &bold
        } else {
            font
        };
        let mut lines_in_row = 1usize;
        let mut wrapped = Vec::new();
        for (ci, cell) in row.iter().enumerate() {
            let w = widths.get(ci).copied().unwrap_or(0.0) - 2.0 * pad;
            let ls = crate::text::wrap(cell, w.max(size), size, f);
            lines_in_row = lines_in_row.max(ls.len());
            wrapped.push(ls);
        }
        cells.push(wrapped);
        heights.push(row_h + (lines_in_row as f64 - 1.0) * line_h);
    }
    let total_h: f64 = heights.iter().sum();
    let (x0, y0) = (at.x, at.y);
    rect(d, line_layer, P2::new(x0, y0 - total_h), P2::new(x0 + total_w, y0), 0.25);
    let mut y = y0;
    for (ri, hgt) in heights.iter().enumerate() {
        y -= hgt;
        if ri + 1 < heights.len() {
            let w = if header && ri == 0 { 0.5 } else { 0.18 };
            line(d, line_layer, P2::new(x0, y), P2::new(x0 + total_w, y), w);
        }
    }
    let mut x = x0;
    for w in &widths[..widths.len() - 1] {
        x += w;
        line(d, line_layer, P2::new(x, y0), P2::new(x, y0 - total_h), 0.18);
    }
    let mut y = y0;
    for (ri, row) in cells.iter().enumerate() {
        let f = if (header && ri == 0) || bold_rows.contains(&ri) {
            &bold
        } else {
            font
        };
        let mut x = x0;
        for (ci, ls) in row.iter().enumerate() {
            let w = widths.get(ci).copied().unwrap_or(0.0);
            let h = align.get(ci).copied().unwrap_or(HAlign::Left);
            let tx = match h {
                HAlign::Left => x + pad,
                HAlign::Center => x + w / 2.0,
                HAlign::Right => x + w - pad,
            };
            for (li, l) in ls.iter().enumerate() {
                let ty = y - row_h * 0.68 - (li as f64) * line_h;
                text(d, text_layer, P2::new(tx, ty), l, size, 0.0, Anchor::new(h, VAlign::Baseline), f, None);
            }
            x += w;
        }
        y -= heights[ri];
    }
    y0 - total_h
}

/// A north arrow: a ring, a needle turned `true_north_deg` clockwise from
/// the page's up, and an N. `label` adds "TRUE NORTH" under it.
pub fn north_arrow(d: &mut Drawing, c: P2, r: f64, true_north_deg: f64, label: bool, font: &FontRef) {
    let symb = d.layer(layer::SYMB);
    let text_layer = d.layer(layer::TEXT);
    let mut ring = Path::new(P2::new(c.x + r, c.y));
    let circle = Loop::circle(c, r);
    for s in &circle.segments {
        ring.segments.push(s.clone());
    }
    stroke(d, symb, ring, true, Some(0.35), None, None);
    let a = (90.0 - true_north_deg).to_radians();
    let (ux, uy) = (a.cos(), a.sin());
    let (nx, ny) = (-uy, ux);
    let tip = P2::new(c.x + ux * r * 0.95, c.y + uy * r * 0.95);
    let tail = P2::new(c.x - ux * r * 0.6, c.y - uy * r * 0.6);
    fill(
        d,
        symb,
        Loop::polygon(&[tip, P2::new(c.x + nx * r * 0.22, c.y + ny * r * 0.22), tail]),
        Vec::new(),
        None,
    );
    polyline(
        d,
        symb,
        &[tip, P2::new(c.x - nx * r * 0.22, c.y - ny * r * 0.22), tail],
        true,
        Some(0.3),
        None,
        None,
    );
    let bold = font.with_bold(true);
    text(
        d,
        text_layer,
        P2::new(c.x + ux * (r + 3.5), c.y + uy * (r + 3.5) - 1.2),
        "N",
        3.0,
        0.0,
        Anchor::new(HAlign::Center, VAlign::Baseline),
        &bold,
        None,
    );
    if label {
        text(d, text_layer, P2::new(c.x, c.y - r - 4.5), "TRUE NORTH", 1.9, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), font, None);
    }
}

/// A graphic scale bar at `scale`: alternating solid and open boxes over
/// round lengths of the document's unit (feet or metres), labelled.
pub fn scale_bar(d: &mut Drawing, at: P2, scale: Scale, unit_format: &UnitFormat, length_mm: f64, font: &FontRef) {
    let symb = d.layer(layer::SYMB);
    let text_layer = d.layer(layer::TEXT);
    let imperial = unit_format.unit.imperial();
    let unit = if imperial { LengthUnit::Ft } else { LengthUnit::M };
    let unit_mm = unit.mm() * scale.ratio(); // paper mm per unit
    // A round division: 1, 2, 4, 5, 10, 20, 40, 50 … units, four of them
    // fitting the length.
    let nice = [1.0, 2.0, 4.0, 5.0, 10.0, 20.0, 40.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let want = length_mm / 4.0 / unit_mm.max(1e-9);
    let div = nice.iter().copied().rev().find(|n| *n <= want).unwrap_or(1.0);
    let h = 2.2;
    let mut x = at.x;
    for i in 0..4 {
        let x1 = x + div * unit_mm;
        if i % 2 == 0 {
            fill(d, symb, Loop::rect(P2::new((x + x1) / 2.0, at.y + h / 2.0), x1 - x, h), Vec::new(), None);
        } else {
            rect(d, symb, P2::new(x, at.y), P2::new(x1, at.y + h), 0.25);
        }
        let label = crate::units::format_length(div * i as f64 * unit.mm(), &UnitFormat { unit, decimals: 0, ..UnitFormat::mm() });
        text(d, text_layer, P2::new(x, at.y - 2.8), &label, 1.9, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), font, None);
        x = x1;
    }
    let last = crate::units::format_length(div * 4.0 * unit.mm(), &UnitFormat { unit, decimals: 0, ..UnitFormat::mm() });
    text(d, text_layer, P2::new(x, at.y - 2.8), &last, 1.9, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), font, None);
    let title = if imperial { "GRAPHIC SCALE (FEET)" } else { "GRAPHIC SCALE (METRES)" };
    text(d, text_layer, P2::new(at.x, at.y + h + 1.2), title, 1.9, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), font, None);
}

/// A swatch kind for a legend row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Line,
    Heavy,
    Dashed,
    Center,
    Hatch,
    Cross,
    Dots,
    Fill,
    Box,
    Ring,
}

impl Swatch {
    pub const ALL: &'static [&'static str] =
        &["line", "heavy", "dashed", "center", "hatch", "cross", "dots", "fill", "box", "ring"];
    pub fn parse(s: &str) -> Option<Swatch> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "line" => Swatch::Line,
            "heavy" => Swatch::Heavy,
            "dashed" => Swatch::Dashed,
            "center" | "dashdot" => Swatch::Center,
            "hatch" => Swatch::Hatch,
            "cross" | "crosshatch" => Swatch::Cross,
            "dots" => Swatch::Dots,
            "fill" | "solid" => Swatch::Fill,
            "box" => Swatch::Box,
            "ring" | "circle" => Swatch::Ring,
            _ => return None,
        })
    }
}

/// A legend: a title, then one swatch and label per item. Returns the
/// bottom y.
pub fn legend(d: &mut Drawing, at: P2, items: &[(Swatch, String)], size: f64, title: &str, font: &FontRef) -> f64 {
    let symb = d.layer(layer::SYMB);
    let text_layer = d.layer(layer::TEXT);
    let bold = font.with_bold(true);
    let mut y = at.y;
    if !title.is_empty() {
        text(d, text_layer, P2::new(at.x, y), title, size + 0.4, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &bold, None);
        y -= 5.0;
    }
    let (bw, bh) = (9.0, 5.0);
    for (kind, label) in items {
        let (x0, y0, x1, y1) = (at.x, y - bh + 1.0, at.x + bw, y + 1.0);
        let mid = (y0 + y1) / 2.0;
        let sq = Loop::rect(P2::new((x0 + x1) / 2.0, mid), bw, bh);
        match kind {
            Swatch::Line => line(d, symb, P2::new(x0, mid), P2::new(x1, mid), 0.35),
            Swatch::Heavy => line(d, symb, P2::new(x0, mid), P2::new(x1, mid), 0.8),
            Swatch::Dashed => polyline(d, symb, &[P2::new(x0, mid), P2::new(x1, mid)], false, Some(0.3), Some(Dash::Dashed), None),
            Swatch::Center => polyline(d, symb, &[P2::new(x0, mid), P2::new(x1, mid)], false, Some(0.35), Some(Dash::Center), None),
            Swatch::Hatch => {
                rect(d, symb, P2::new(x0, y0), P2::new(x1, y1), 0.2);
                hatch(d, symb, sq, Vec::new(), Pattern::Lines { angle_deg: 45.0, spacing_mm: 1.6 }, 1.0, Some(0.15), None);
            }
            Swatch::Cross => {
                rect(d, symb, P2::new(x0, y0), P2::new(x1, y1), 0.2);
                hatch(d, symb, sq, Vec::new(), Pattern::Cross { angle_deg: 45.0, spacing_mm: 1.6 }, 1.0, Some(0.15), None);
            }
            Swatch::Dots => {
                rect(d, symb, P2::new(x0, y0), P2::new(x1, y1), 0.2);
                hatch(d, symb, sq, Vec::new(), Pattern::Dots { spacing_mm: 1.0, radius_mm: 0.18 }, 1.0, None, None);
            }
            Swatch::Fill => fill(d, symb, sq, Vec::new(), None),
            Swatch::Box => rect(d, symb, P2::new(x0, y0), P2::new(x1, y1), 0.3),
            Swatch::Ring => {
                let c = Loop::circle(P2::new((x0 + x1) / 2.0, mid), bh / 2.0 - 0.5);
                let mut p = Path::new(c.segments[0].start());
                for s in &c.segments {
                    p.segments.push(s.clone());
                }
                stroke(d, symb, p, true, Some(0.3), None, None);
            }
        }
        text(d, text_layer, P2::new(x1 + 3.0, y - bh / 2.0 + 0.3), label, size, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), font, None);
        y -= bh + 1.5;
    }
    y
}

/// A leader from `points[0]` (the arrow) to the last point, the text past
/// it, set in `font`.
pub fn label(d: &mut Drawing, points: &[P2], s: &str, size: f64, font: &FontRef) {
    if points.len() < 2 {
        return;
    }
    let l = d.layer(layer::TEXT);
    d.push(Item::Leader(Leader {
        style: Style::on(l),
        points: points.to_vec(),
        text: s.to_owned(),
        height_mm: size,
        font: font.clone(),
    }));
}

/// A linear dimension between two paper points on a sheet drawn at
/// `scale`: the value it states is the model distance, `text` replaces it.
#[allow(clippy::too_many_arguments)]
pub fn dimension(
    d: &mut Drawing,
    a: P2,
    b: P2,
    offset_mm: f64,
    scale: Scale,
    style: &DimStyle,
    text_override: Option<String>,
) {
    let dims = d.layer(layer::DIMS);
    let s = scale.ratio();
    let mut dm = Dim::aligned(Style::on(dims), P2::new(a.x / s, a.y / s), P2::new(b.x / s, b.y / s), offset_mm, style.clone());
    dm.text_override = text_override;
    d.push(Item::Group(Group {
        name: String::new(),
        source: None,
        transform: Xform2::new(s, 0.0, 0.0),
        items: vec![Item::Dim(dm)],
        clip: None,
    }));
}

/// A drawing title in the manner of an architectural set: a numbered ring, the
/// title in capitals, the scale under it, a rule.
pub fn view_title(d: &mut Drawing, at: P2, number: &str, title: &str, scale_text: &str, width: f64, font: &FontRef) {
    let symb = d.layer(layer::SYMB);
    let text_layer = d.layer(layer::TEXT);
    let bold = font.with_bold(true);
    let c = P2::new(at.x + 4.0, at.y + 2.0);
    let ring = Loop::circle(c, 4.0);
    let mut p = Path::new(ring.segments[0].start());
    for s in &ring.segments {
        p.segments.push(s.clone());
    }
    stroke(d, symb, p, true, Some(0.3), None, None);
    text(d, text_layer, P2::new(c.x, c.y - 1.1), number, 3.0, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), &bold, None);
    text(d, text_layer, P2::new(at.x + 11.0, at.y + 2.4), title, 3.4, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &bold, None);
    if !scale_text.is_empty() {
        text(d, text_layer, P2::new(at.x + 11.0, at.y - 1.8), &format!("Scale: {scale_text}"), 2.2, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), font, None);
    }
    line(d, symb, P2::new(at.x, at.y - 3.2), P2::new(at.x + width, at.y - 3.2), 0.5);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Prim;

    fn texts(d: &Drawing) -> Vec<String> {
        d.flatten()
            .into_iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_page_carries_the_frame_the_title_column_and_the_index() {
        let mut set = SheetSet::new("house", SheetSize::ARCH_D);
        set.title = TitleTemplate {
            project: "12 EXAMPLE ST".into(),
            address: "SOMEWHERE".into(),
            stamp: vec!["CONCEPT STUDY".into(), "NOT FOR CONSTRUCTION".into()],
            issue: "2026-09-27".into(),
            drawn: "cadpole".into(),
            ..Default::default()
        };
        set.unit_format = UnitFormat::ft_in();
        let a = set.add_sheet("G0.0", "COVER SHEET", None);
        let b = set.add_sheet("A2.0", "SITE PLAN", Some(Scale::QUARTER_INCH));
        set.sheets[b].north = true;
        assert_eq!(set.find("a2.0"), Some(b));
        let cover = set.page(a).unwrap();
        let t = texts(&cover);
        for want in ["12 EXAMPLE ST", "CONCEPT STUDY", "SHEET INDEX", "G0.0", "A2.0", "COVER SHEET", "SITE PLAN", "AS NOTED"] {
            assert!(t.iter().any(|s| s == want), "{want} missing from {t:?}");
        }
        let plan = set.page(b).unwrap();
        let t = texts(&plan);
        assert!(t.iter().any(|s| s == "1/4\" = 1'-0\""), "{t:?}");
        assert!(t.iter().any(|s| s == "N"), "the north arrow's N");
        // Everything stays on the page.
        let (lo, hi) = plan.bounds().unwrap();
        assert!(lo.x >= 0.0 && lo.y >= 0.0 && hi.x <= SheetSize::ARCH_D.w_mm && hi.y <= SheetSize::ARCH_D.h_mm, "{lo:?} {hi:?}");
        // The drawing area is left of the column and inside the margin.
        let area = set.drawing_area();
        assert!(area.hi.x < SheetSize::ARCH_D.w_mm - set.column_mm());
        assert!((area.lo.x - 10.0).abs() < 1e-9);
        // The set's face is a standard one, so the PDF's text will be text.
        assert!(matches!(set.font, FontRef::Standard(StdFont::Helvetica)));
        assert!(set.pages().len() == 2);
    }

    /// A project name too long for the column is set smaller, never past
    /// the column's edge.
    #[test]
    fn a_long_project_name_is_shrunk_to_the_column() {
        let mut set = SheetSet::new("s", SheetSize::ARCH_D);
        set.title.project = "A VERY LONG PROJECT NAME THAT WOULD NEVER FIT".into();
        set.add_sheet("A1", "PLAN", None);
        let page = set.page(0).unwrap();
        let (x0, x1) = (SheetSize::ARCH_D.w_mm - 10.0 - set.column_mm(), SheetSize::ARCH_D.w_mm - 10.0);
        for p in page.flatten() {
            if let Prim::Text { text, at, height_mm, font, anchor, .. } = &p {
                if text.starts_with("A VERY LONG") {
                    let w = crate::text::width_in(text, *height_mm, font);
                    assert_eq!(anchor.h, HAlign::Center);
                    assert!(at.x - w / 2.0 >= x0 && at.x + w / 2.0 <= x1, "{} .. {} outside {x0} .. {x1}", at.x - w / 2.0, at.x + w / 2.0);
                }
            }
        }
    }

    #[test]
    fn a_small_sheet_scales_its_column_down() {
        let set = SheetSet::new("s", SheetSize::A4);
        assert!((set.column_mm() - 55.0).abs() < 1e-9);
        let big = SheetSet::new("s", SheetSize::ARCH_D);
        assert!((big.column_mm() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_annotation_vocabulary_draws_what_it_says() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        let font = FontRef::Standard(StdFont::Helvetica);
        let w = text(&mut d, l, P2::new(10.0, 10.0), "Hello", 2.5, 0.0, Anchor::CENTER, &font, None);
        assert!(w > 5.0 && w < 12.0, "{w}");
        let bottom = note(&mut d, l, P2::new(10.0, 40.0), "one two three four five six seven", Some(20.0), 2.0, 3.0, HAlign::Left, &font, None);
        assert!(bottom < 40.0 - 3.0, "wrapped onto several lines: {bottom}");
        let rows = vec![
            vec!["NO.".to_string(), "SHEET".to_string()],
            vec!["A2.0".to_string(), "SITE PLAN".to_string()],
        ];
        let b2 = table(&mut d, l, l, P2::new(50.0, 100.0), &[16.0, 60.0], &rows, 2.0, 4.6, &[HAlign::Left, HAlign::Left], &[], true, &font);
        assert!((b2 - (100.0 - 9.2)).abs() < 1e-9, "{b2}");
        let b3 = legend(&mut d, P2::new(150.0, 100.0), &[(Swatch::Hatch, "cut".into()), (Swatch::Dashed, "beyond".into())], 2.0, "LEGEND", &font);
        assert!(b3 < 100.0 - 10.0);
        north_arrow(&mut d, P2::new(200.0, 50.0), 8.0, 30.0, true, &font);
        scale_bar(&mut d, P2::new(10.0, 5.0), Scale::QUARTER_INCH, &UnitFormat::ft_in(), 60.0, &font);
        label(&mut d, &[P2::new(0.0, 0.0), P2::new(5.0, 5.0)], "here", 2.0, &font);
        dimension(&mut d, P2::new(0.0, 0.0), P2::new(63.5, 0.0), 8.0, Scale::QUARTER_INCH, &DimStyle::architectural().with_font(font.clone()), None);
        view_title(&mut d, P2::new(10.0, 200.0), "1", "SITE PLAN", "1/4\" = 1'-0\"", 120.0, &font);
        let t = texts(&d);
        // 63.5 paper mm at 1:48 is 3048 mm, ten feet.
        assert!(t.iter().any(|s| s == "10'-0\""), "{t:?}");
        for want in ["SITE PLAN", "LEGEND", "TRUE NORTH", "GRAPHIC SCALE (FEET)", "here", "Hello"] {
            assert!(t.iter().any(|s| s == want), "{want} missing from {t:?}");
        }
        assert!(matches!(Swatch::parse("crosshatch"), Some(Swatch::Cross)));
        assert!(Swatch::parse("zigzag").is_none());
    }
}
