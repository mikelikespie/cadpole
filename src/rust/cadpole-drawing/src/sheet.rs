//! Sheets: paper sizes, a border, a title block, and the three-view layout.
//!
//! A sheet is where the drawing stops being geometry and becomes a document.
//! Everything on it is in paper millimetres, and each view is a [`Group`] whose
//! transform carries the view scale — so the same dimension item states a model
//! length while its arrows stay 2.5 mm wide.
//!
//! The three-view layout is the one docs/views.md calls `sheet=three-view`:
//! front, the top directly above it, the right directly beside it, and an
//! isometric in the free corner. Third-angle projection (the US convention) by
//! default; first-angle swaps top with bottom and right with left, which is the
//! only difference between the two and the reason the projection symbol exists.

use cadpole_core::sketch::{Loop, Path, P2};
use cadpole_core::tessellate::{Mesh, Topology};

use crate::auto;
use crate::dim::DimStyle;
use crate::ir::{
    layer, Anchor, Drawing, Group, HAlign, Item, Scale, Stroke, Style, VAlign, Xform2,
};
use crate::units::UnitFormat;
use crate::view::{project, Hidden, ProjectOptions, ViewBasis};

/// A paper size, in millimetres, landscape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetSize {
    pub name: &'static str,
    pub w_mm: f64,
    pub h_mm: f64,
}

impl SheetSize {
    pub const A4: SheetSize = SheetSize {
        name: "A4",
        w_mm: 297.0,
        h_mm: 210.0,
    };
    pub const A3: SheetSize = SheetSize {
        name: "A3",
        w_mm: 420.0,
        h_mm: 297.0,
    };
    pub const A2: SheetSize = SheetSize {
        name: "A2",
        w_mm: 594.0,
        h_mm: 420.0,
    };
    pub const A1: SheetSize = SheetSize {
        name: "A1",
        w_mm: 841.0,
        h_mm: 594.0,
    };
    pub const A0: SheetSize = SheetSize {
        name: "A0",
        w_mm: 1189.0,
        h_mm: 841.0,
    };
    /// 9 × 12 in.
    pub const ARCH_A: SheetSize = SheetSize {
        name: "ARCH_A",
        w_mm: 304.8,
        h_mm: 228.6,
    };
    /// 12 × 18 in.
    pub const ARCH_B: SheetSize = SheetSize {
        name: "ARCH_B",
        w_mm: 457.2,
        h_mm: 304.8,
    };
    /// 18 × 24 in.
    pub const ARCH_C: SheetSize = SheetSize {
        name: "ARCH_C",
        w_mm: 609.6,
        h_mm: 457.2,
    };
    /// 24 × 36 in — the common residential sheet.
    pub const ARCH_D: SheetSize = SheetSize {
        name: "ARCH_D",
        w_mm: 914.4,
        h_mm: 609.6,
    };
    /// 36 × 48 in.
    pub const ARCH_E: SheetSize = SheetSize {
        name: "ARCH_E",
        w_mm: 1219.2,
        h_mm: 914.4,
    };
    /// 8.5 × 11 in.
    pub const ANSI_A: SheetSize = SheetSize {
        name: "ANSI_A",
        w_mm: 279.4,
        h_mm: 215.9,
    };
    /// 11 × 17 in.
    pub const ANSI_B: SheetSize = SheetSize {
        name: "ANSI_B",
        w_mm: 431.8,
        h_mm: 279.4,
    };

    pub const ALL: &'static [SheetSize] = &[
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
        SheetSize::ARCH_A,
        SheetSize::ARCH_B,
        SheetSize::ARCH_C,
        SheetSize::ARCH_D,
        SheetSize::ARCH_E,
        SheetSize::ANSI_A,
        SheetSize::ANSI_B,
    ];

    /// The names a script writes, lower case, in the order `cadpole doc`
    /// lists them: every size in [`SheetSize::ALL`], then `letter` and
    /// `tabloid`, the US names of ANSI A and ANSI B. What the
    /// `sheet` vocabulary offers, and what [`SheetSize::parse`] accepts.
    pub const NAMES: &'static [&'static str] = &[
        "a4", "a3", "a2", "a1", "a0", "arch_a", "arch_b", "arch_c", "arch_d", "arch_e", "ansi_a", "ansi_b", "letter",
        "tabloid",
    ];

    /// Look a size up by name, case-insensitively (`a3`, `ARCH_D`, `arch-d`),
    /// `letter` and `tabloid` included.
    pub fn parse(name: &str) -> Option<SheetSize> {
        let want = name.trim().to_ascii_uppercase().replace('-', "_");
        match want.as_str() {
            "LETTER" => return Some(SheetSize::ANSI_A),
            "TABLOID" | "LEDGER" => return Some(SheetSize::ANSI_B),
            _ => {}
        }
        SheetSize::ALL.iter().copied().find(|s| s.name == want)
    }

    pub fn portrait(self) -> SheetSize {
        SheetSize {
            name: self.name,
            w_mm: self.h_mm,
            h_mm: self.w_mm,
        }
    }
}

/// The block in the corner that says what this sheet is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleBlock {
    pub project: String,
    pub title: String,
    pub number: String,
    pub scale: String,
    pub date: String,
    pub drawn_by: String,
    pub revision: String,
    /// Extra rows, drawn under the standard ones.
    pub extra: Vec<(String, String)>,
}

impl TitleBlock {
    pub fn titled(title: impl Into<String>) -> TitleBlock {
        TitleBlock {
            title: title.into(),
            ..TitleBlock::default()
        }
    }

    fn rows(&self) -> Vec<(&str, &str)> {
        let mut r = Vec::new();
        for (k, v) in [
            ("PROJECT", self.project.as_str()),
            ("TITLE", self.title.as_str()),
            ("SCALE", self.scale.as_str()),
            ("DATE", self.date.as_str()),
            ("DRAWN", self.drawn_by.as_str()),
            ("REV", self.revision.as_str()),
        ] {
            if !v.is_empty() {
                r.push((k, v));
            }
        }
        for (k, v) in &self.extra {
            r.push((k.as_str(), v.as_str()));
        }
        r
    }
}

/// A sheet of paper with a border, a title block and anything drawn directly
/// on it (sheet notes, the north arrow, the projection symbol).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub size: SheetSize,
    pub margin_mm: f64,
    pub title_block: TitleBlock,
    pub items: Vec<Item>,
}

impl Sheet {
    pub fn new(size: SheetSize) -> Sheet {
        Sheet {
            size,
            margin_mm: 10.0,
            title_block: TitleBlock::default(),
            items: Vec::new(),
        }
    }

    /// The rectangle views may occupy: inside the border and clear of the
    /// title block.
    ///
    /// The title block sits in the bottom-right corner, so strictly the strip
    /// to its left is usable too. Keeping the area a plain rectangle above it
    /// costs a little paper and buys a layout that is right by construction —
    /// nothing can ever land on the title block.
    pub fn drawing_area(&self) -> (P2, P2) {
        let m = self.margin_mm;
        let bottom = self.title_block_box().map(|b| b.1.y + 4.0).unwrap_or(m);
        (
            P2::new(m, bottom),
            P2::new(self.size.w_mm - m, self.size.h_mm - m),
        )
    }

    /// Where the title block sits, when there is one.
    pub fn title_block_box(&self) -> Option<(P2, P2)> {
        if self.title_block.rows().is_empty() {
            return None;
        }
        let m = self.margin_mm;
        let w = (self.size.w_mm * 0.32).clamp(70.0, 180.0);
        let rows = self.title_block.rows().len() as f64;
        let h = (rows * 7.0 + 4.0).max(24.0);
        Some((
            P2::new(self.size.w_mm - m - w, m),
            P2::new(self.size.w_mm - m, m + h),
        ))
    }
}

/// Draw the border and title block onto `d`'s sheet items.
pub fn draw_border(d: &mut Drawing) {
    let Some(sheet) = d.sheet.clone() else { return };
    let title = d.layer(layer::TITLE);
    let text_layer = d.layer(layer::TEXT);
    let m = sheet.margin_mm;
    let (w, h) = (sheet.size.w_mm, sheet.size.h_mm);
    let mut items = sheet.items.clone();

    items.push(rect_item(title, P2::new(m, m), P2::new(w - m, h - m)));

    if let Some((lo, hi)) = sheet.title_block_box() {
        items.push(rect_item(title, lo, hi));
        let rows = sheet.title_block.rows();
        let row_h = (hi.y - lo.y - 3.0) / rows.len() as f64;
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = hi.y - 1.5 - row_h * (i as f64 + 0.5);
            items.push(text_item(
                text_layer,
                P2::new(lo.x + 2.0, y),
                (*k).to_owned(),
                1.8,
                Anchor::new(HAlign::Left, VAlign::Middle),
            ));
            items.push(text_item(
                text_layer,
                P2::new(lo.x + 22.0, y),
                (*v).to_owned(),
                2.5,
                Anchor::new(HAlign::Left, VAlign::Middle),
            ));
            if i + 1 < rows.len() {
                items.push(line_item(
                    title,
                    P2::new(lo.x, y - row_h / 2.0),
                    P2::new(hi.x, y - row_h / 2.0),
                ));
            }
        }
    }
    if let Some(s) = d.sheet.as_mut() {
        s.items = items;
    }
}

fn rect_item(layer: usize, lo: P2, hi: P2) -> Item {
    let l = Loop::rect(
        P2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0),
        hi.x - lo.x,
        hi.y - lo.y,
    );
    let mut p = Path::new(l.segments[0].start());
    for s in &l.segments {
        p.segments.push(s.clone());
    }
    Item::Stroke(Stroke {
        style: Style::on(layer),
        name: String::new(),
        path: p,
        closed: true,
    })
}

fn line_item(layer: usize, a: P2, b: P2) -> Item {
    Item::Stroke(Stroke {
        style: Style::on(layer),
        name: String::new(),
        path: Path::new(a).line_to(b),
        closed: false,
    })
}

fn text_item(layer: usize, at: P2, text: String, height_mm: f64, anchor: Anchor) -> Item {
    Item::Text(crate::ir::TextItem {
        style: Style::on(layer),
        at,
        text,
        height_mm,
        rotation_deg: 0.0,
        anchor,
        font: crate::ir::FontRef::Builtin,
        model_height: false,
    })
}

/// Which way the views fall around the front view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProjectionAngle {
    /// US practice: the top view is above the front, the right view is to the
    /// right.
    #[default]
    Third,
    /// ISO practice: the top view is below, the right view is to the left.
    First,
}

/// How to build a three-view sheet.
#[derive(Clone, Debug)]
pub struct ThreeViewOptions {
    pub sheet: SheetSize,
    /// `None` fits the model to the sheet and snaps to a standard ratio.
    pub scale: Option<Scale>,
    pub dimensions: bool,
    pub hidden: Hidden,
    /// Include an isometric in the free corner.
    pub iso: bool,
    pub angle: ProjectionAngle,
    pub title: TitleBlock,
    pub unit_format: UnitFormat,
    pub dim_style: DimStyle,
    pub project: ProjectOptions,
}

impl Default for ThreeViewOptions {
    fn default() -> Self {
        ThreeViewOptions {
            sheet: SheetSize::A3,
            scale: None,
            dimensions: true,
            hidden: Hidden::Dashed,
            iso: true,
            angle: ProjectionAngle::Third,
            title: TitleBlock::default(),
            unit_format: UnitFormat::mm(),
            dim_style: DimStyle::mechanical(),
            project: ProjectOptions::default(),
        }
    }
}

/// Standard drawing ratios, largest first. Fitting snaps to one of these so a
/// sheet says "1:2" and not "1:2.137".
const NICE: &[f64] = &[
    10.0, 5.0, 4.0, 2.5, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.04, 0.025, 0.02, 0.01, 0.005,
    0.002, 0.001,
];

/// The note for an explicit `scale` at which a drawing `need_w` x `need_h`
/// model mm does not fit the `area_w` x `area_h` paper mm drawing area of
/// `sheet`, naming the largest standard scale that does (`fit` is the
/// ratio that exactly fills the room left for the views). `None` when it
/// fits. The views are still drawn at the scale asked for: the note is how
/// a caller learns they run off the paper.
pub(crate) fn overflow_note(
    scale: Scale,
    fit: f64,
    (need_w, need_h): (f64, f64),
    (area_w, area_h): (f64, f64),
    sheet: &SheetSize,
    unit_format: &UnitFormat,
) -> Option<String> {
    let s = scale.ratio();
    let (w, h) = (need_w * s, need_h * s);
    // Against the room the layout really has (the views are placed inside
    // the drawing area's padding), the same room `fit` was computed from.
    if s <= fit * (1.0 + 1e-9) {
        return None;
    }
    Some(format!(
        "scale {} does not fit the {} sheet: the views need {:.0} x {:.0} mm of paper and the room for them is \
         {:.0} x {:.0} mm, so they run off the page; the largest standard scale that fits is {}.",
        scale.label(unit_format),
        sheet.name,
        w,
        h,
        area_w,
        area_h,
        snap_scale(fit).label(unit_format),
    ))
}

pub(crate) fn snap_scale(fit: f64) -> Scale {
    let s = NICE.iter().copied().find(|n| *n <= fit).unwrap_or(0.001);
    if s >= 1.0 {
        Scale::new(s, 1.0)
    } else {
        Scale::new(1.0, (1.0 / s).round())
    }
}

struct View {
    name: &'static str,
    basis: ViewBasis,
    lo: P2,
    hi: P2,
    polys_visible: Vec<Vec<P2>>,
    polys_hidden: Vec<Vec<P2>>,
}

fn make_view(name: &'static str, mesh: &Mesh, topo: &Topology, opts: &ProjectOptions) -> View {
    let basis = ViewBasis::preset(name);
    let p = project(mesh, topo, &basis, opts);
    let (lo, hi) = p.bounds().unwrap_or((P2::new(0.0, 0.0), P2::new(1.0, 1.0)));
    View {
        name,
        basis,
        lo,
        hi,
        polys_visible: p.visible,
        polys_hidden: p.hidden,
    }
}

impl View {
    fn w(&self) -> f64 {
        (self.hi.x - self.lo.x).max(1e-6)
    }
    fn h(&self) -> f64 {
        (self.hi.y - self.lo.y).max(1e-6)
    }

    /// The view's strokes as a group, placed so its bottom-left corner lands
    /// on `at` in paper millimetres at `scale`.
    fn group(&self, d: &mut Drawing, at: P2, scale: f64, hidden: Hidden) -> Group {
        let vis = d.layer(layer::VISIBLE);
        let hid = d.layer(layer::HIDDEN);
        let mut items = Vec::new();
        for poly in &self.polys_visible {
            items.push(polyline_item(vis, poly));
        }
        if hidden != Hidden::Omit {
            for poly in &self.polys_hidden {
                items.push(polyline_item(hid, poly));
            }
        }
        Group {
            name: self.name.to_owned(),
            source: None,
            // Place the projected bbox min at `at`: translate in paper units
            // after scaling.
            transform: Xform2::new(scale, at.x - self.lo.x * scale, at.y - self.lo.y * scale),
            items,
            clip: None,
        }
    }
}

fn polyline_item(layer: usize, poly: &[P2]) -> Item {
    let mut p = Path::new(poly[0]);
    for q in &poly[1..] {
        p = p.line_to(*q);
    }
    Item::Stroke(Stroke {
        style: Style::on(layer),
        name: String::new(),
        path: p,
        closed: false,
    })
}

/// A three-view sheet of `mesh`: front, top, right and an isometric, with
/// overall dimensions.
pub fn three_view(mesh: &Mesh, topo: &Topology, opts: &ThreeViewOptions) -> Drawing {
    let mut d = Drawing::new();
    d.unit_format = opts.unit_format;
    let mut sheet = Sheet::new(opts.sheet);
    sheet.title_block = opts.title.clone();
    d.sheet = Some(sheet);

    let po = ProjectOptions {
        hidden: opts.hidden,
        ..opts.project.clone()
    };
    let front = make_view("front", mesh, topo, &po);
    let top = make_view("top", mesh, topo, &po);
    let right = make_view("right", mesh, topo, &po);
    // The isometric is there to be looked at, not measured, so it gets no
    // hidden lines: standard practice, and it keeps the corner readable.
    let iso = opts.iso.then(|| {
        make_view(
            "iso",
            mesh,
            topo,
            &ProjectOptions {
                hidden: Hidden::Omit,
                ..po.clone()
            },
        )
    });

    let (area_lo, area_hi) = d.sheet.as_ref().unwrap().drawing_area();
    // Leave room for dimension strings around the outside of the block.
    let dim_pad = if opts.dimensions {
        opts.dim_style.text_height_mm * 7.0
    } else {
        4.0
    };
    let gap = 14.0f64;
    let avail_w = (area_hi.x - area_lo.x - 2.0 * dim_pad - gap).max(10.0);
    let avail_h = (area_hi.y - area_lo.y - 2.0 * dim_pad - gap).max(10.0);

    let need_w = front.w() + right.w();
    let need_h = front.h() + top.h();
    let fit = (avail_w / need_w).min(avail_h / need_h);
    let scale = opts.scale.unwrap_or_else(|| snap_scale(fit));
    let s = scale.ratio();
    d.scale = scale;
    if d.sheet
        .as_ref()
        .map(|sh| sh.title_block.scale.is_empty())
        .unwrap_or(false)
    {
        if let Some(sh) = d.sheet.as_mut() {
            sh.title_block.scale = scale.label(&opts.unit_format);
        }
    }

    // Block origin. Third angle grows up and to the right of the front view;
    // first angle grows down and to the left. Measuring how far the block
    // reaches on each side of the front view — rather than assuming it only
    // grows — is what keeps the first-angle layout on the paper instead of
    // over the title block.
    let block_w = (front.w() + right.w()) * s + gap;
    let block_h = (front.h() + top.h()) * s + gap;
    let (grow_left, grow_down) = match opts.angle {
        ProjectionAngle::Third => (0.0, 0.0),
        ProjectionAngle::First => (right.w() * s + gap, top.h() * s + gap),
    };
    if opts.scale.is_some() {
        d.notes.extend(overflow_note(
            scale,
            fit,
            (block_w / s, block_h / s),
            (avail_w, avail_h),
            &opts.sheet,
            &opts.unit_format,
        ));
    }
    let ox = area_lo.x + dim_pad + grow_left + (avail_w - block_w).max(0.0) / 2.0;
    let oy = area_lo.y + dim_pad + grow_down + (avail_h - block_h).max(0.0) / 2.0;

    let front_at = P2::new(ox, oy);
    let (top_at, right_at, iso_at) = match opts.angle {
        ProjectionAngle::Third => (
            P2::new(ox, oy + front.h() * s + gap),
            P2::new(ox + front.w() * s + gap, oy),
            P2::new(ox + front.w() * s + gap, oy + front.h() * s + gap),
        ),
        ProjectionAngle::First => (
            P2::new(ox, oy - top.h() * s - gap),
            P2::new(ox - right.w() * s - gap, oy),
            // First angle grows down and to the left, so the free corner — and
            // the one that stays inside the block the origin was sized for —
            // is down-left, not up-right.
            P2::new(ox - right.w() * s - gap, oy - top.h() * s - gap),
        ),
    };

    let g_front = front.group(&mut d, front_at, s, opts.hidden);
    let g_top = top.group(&mut d, top_at, s, opts.hidden);
    let g_right = right.group(&mut d, right_at, s, opts.hidden);
    d.push(Item::Group(g_front));
    d.push(Item::Group(g_top));
    d.push(Item::Group(g_right));
    if let Some(iso) = &iso {
        // The isometric is a picture, not a measurement: fit it into the free
        // corner at whatever scale it takes.
        let box_w = right.w() * s;
        let box_h = top.h() * s;
        let is = (box_w / iso.w()).min(box_h / iso.h()).max(1e-9);
        let at = P2::new(
            iso_at.x + (box_w - iso.w() * is) / 2.0,
            iso_at.y + (box_h - iso.h() * is) / 2.0,
        );
        let g = iso.group(&mut d, at, is, Hidden::Omit);
        d.push(Item::Group(g));
    }

    if opts.dimensions {
        let dims = d.layer(layer::DIMS);
        let off = opts.dim_style.text_height_mm * 2.4;
        // Overall width and height on the front view, depth on the right one.
        // Each view's dimensions belong to that view's group, so they scale
        // with it and state the model's size rather than the paper's — and
        // they are placed *per view*, because two views sitting side by side on
        // the sheet share no space and must not push each other around.
        let mut front_dims =
            auto::extents_dims(Style::on(dims), front.lo, front.hi, &opts.dim_style, off);
        let front_xf = Xform2::new(s, front_at.x - front.lo.x * s, front_at.y - front.lo.y * s);
        d.notes.extend(auto::place(&mut front_dims, front_xf));

        let mut right_dims =
            auto::extents_dims(Style::on(dims), right.lo, right.hi, &opts.dim_style, off)
                .into_iter()
                .filter(|dm| {
                    // Only the horizontal one: the height is already
                    // dimensioned on the front view, and repeating a dimension
                    // is a drafting error, not a courtesy.
                    matches!(&dm.kind, crate::dim::DimKind::Linear { dir, .. } if dir.x.abs() > 0.5)
                })
                .collect::<Vec<_>>();
        let right_xf = Xform2::new(s, right_at.x - right.lo.x * s, right_at.y - right.lo.y * s);
        d.notes.extend(auto::place(&mut right_dims, right_xf));

        d.push(Item::Group(Group {
            name: "front/dims".into(),
            source: None,
            transform: front_xf,
            items: front_dims.into_iter().map(Item::Dim).collect(),
            clip: None,
        }));
        d.push(Item::Group(Group {
            name: "right/dims".into(),
            source: None,
            transform: right_xf,
            items: right_dims.into_iter().map(Item::Dim).collect(),
            clip: None,
        }));
    }

    // View labels, below each view and clear of the dimension string that runs
    // under it.
    let text_layer = d.layer(layer::TEXT);
    // Only the views with a dimension string under them need the label pushed
    // clear of it; the top view has nothing below it but the gap.
    let dimmed_drop = if opts.dimensions {
        opts.dim_style.text_height_mm * 5.4
    } else {
        5.0
    };
    for (v, at, drop) in [
        (&front, front_at, dimmed_drop),
        (&top, top_at, 5.0),
        (&right, right_at, dimmed_drop),
    ] {
        let label = v.name.to_uppercase();
        let items = vec![text_item(
            text_layer,
            P2::new(at.x + v.w() * s / 2.0, at.y - drop),
            label,
            2.5,
            Anchor::new(HAlign::Center, VAlign::Top),
        )];
        d.push(Item::Group(Group {
            name: format!("{}/label", v.name),
            source: None,
            transform: Xform2::IDENTITY,
            items,
            clip: None,
        }));
    }
    let _ = front.basis;

    draw_border(&mut d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_sizes_parse_and_flip() {
        assert_eq!(SheetSize::parse("a3"), Some(SheetSize::A3));
        assert_eq!(SheetSize::parse("arch-d"), Some(SheetSize::ARCH_D));
        assert_eq!(SheetSize::parse("letter"), Some(SheetSize::ANSI_A));
        assert_eq!(SheetSize::parse("legal"), None);
        assert_eq!(SheetSize::A3.portrait().w_mm, 297.0);
        // ARCH D is 24 x 36 inches.
        assert!((SheetSize::ARCH_D.w_mm / 25.4 - 36.0).abs() < 1e-9);
    }

    #[test]
    fn fitting_snaps_to_a_standard_ratio() {
        assert_eq!(snap_scale(0.9).label(&UnitFormat::mm()), "1:2");
        assert_eq!(snap_scale(1.0).label(&UnitFormat::mm()), "1:1");
        assert_eq!(snap_scale(3.0).label(&UnitFormat::mm()), "2.5:1");
        assert_eq!(snap_scale(0.3).label(&UnitFormat::mm()), "1:4");
    }

    #[test]
    fn the_title_block_stays_inside_the_border() {
        let mut s = Sheet::new(SheetSize::A3);
        s.title_block = TitleBlock::titled("Bracket");
        let (lo, hi) = s.title_block_box().unwrap();
        assert!(lo.x >= s.margin_mm && hi.x <= s.size.w_mm - s.margin_mm + 1e-9);
        assert!(lo.y >= s.margin_mm);
        let (alo, ahi) = s.drawing_area();
        assert!(alo.y >= hi.y, "views start above the title block");
        assert!(alo.x >= s.margin_mm && ahi.y <= s.size.h_mm - s.margin_mm + 1e-9);
        assert!(
            ahi.y - alo.y > 100.0,
            "and there is still a sheet to draw on"
        );
    }

    #[test]
    fn both_projection_angles_stay_on_the_paper() {
        use cadpole_core::geom::Plane;
        use cadpole_core::sketch::{Loop, Sketch};
        let sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 80.0, 40.0));
        let solids = cadpole_core::kernel::extrude(&sk, 25.0).expect("extrude");
        let (mesh, topo) = cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess");
        for angle in [ProjectionAngle::Third, ProjectionAngle::First] {
            let opts = ThreeViewOptions {
                sheet: SheetSize::A3,
                angle,
                title: TitleBlock::titled("Block"),
                ..Default::default()
            };
            let d = three_view(&mesh, &topo, &opts);
            let (lo, hi) = d.bounds().expect("bounds");
            let sheet = d.sheet.as_ref().unwrap();
            assert!(
                lo.x >= -1.0 && lo.y >= -1.0,
                "{angle:?} runs off the sheet at {lo:?}"
            );
            assert!(
                hi.x <= sheet.size.w_mm + 1.0 && hi.y <= sheet.size.h_mm + 1.0,
                "{angle:?} runs off the sheet at {hi:?}"
            );
            // And clear of the title block, which lives in the bottom-right.
            let (tlo, _) = sheet.title_block_box().unwrap();
            let views: Vec<&Group> = d
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Group(g) if g.name == "front" || g.name == "top" || g.name == "right" => {
                        Some(g)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(views.len(), 3);
            for g in views {
                assert!(
                    g.transform.ty >= tlo.y,
                    "{} sits on the title block",
                    g.name
                );
            }
        }
    }

    #[test]
    fn an_empty_title_block_leaves_the_whole_sheet() {
        let s = Sheet::new(SheetSize::A4);
        assert!(s.title_block_box().is_none());
        let (lo, hi) = s.drawing_area();
        assert!((hi.y - (s.size.h_mm - s.margin_mm)).abs() < 1e-9);
        assert!((lo.y - s.margin_mm).abs() < 1e-9);
        assert!((lo.x - s.margin_mm).abs() < 1e-9);
    }

    /// Every name a script can write parses, and every size has one.
    #[test]
    fn every_sheet_name_parses_and_every_size_is_named() {
        for n in SheetSize::NAMES {
            assert!(SheetSize::parse(n).is_some(), "{n}");
        }
        for s in SheetSize::ALL {
            assert!(SheetSize::NAMES.contains(&s.name.to_ascii_lowercase().as_str()), "{}", s.name);
        }
        assert_eq!(SheetSize::parse("Letter"), Some(SheetSize::ANSI_A));
        assert_eq!(SheetSize::parse("tabloid"), Some(SheetSize::ANSI_B));
        assert_eq!(SheetSize::parse("arch-d"), Some(SheetSize::ARCH_D));
    }
}
