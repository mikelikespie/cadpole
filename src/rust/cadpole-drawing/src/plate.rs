//! A one-page colour plate: one plan, section or elevation in the
//! colour look ([`crate::painted`]) laid out the way a design booklet's
//! plan pages are -- a title and a subtitle, a legend of swatches, a scale
//! bar, a north arrow on a plan, notes placed at model points, a footnote --
//! with every word real text. What `cadpole export --view top --section z=5.5
//! --format pdf` and the `Drawing` rpc's `look: "color"` write.
//!
//! No title block and no border: the page is the picture. A sheet set
//! places the same views with its own title column (`plan_view(...,
//! look="color")`).

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::{Loop, Path, P2};

use crate::ir::{layer, Anchor, ClipRect, Drawing, FontRef, Group, HAlign, Item, Rgb, Scale, Style, VAlign, Xform2};
use crate::sheet::{Sheet, SheetSize};
use crate::sheetset;
use crate::units::{format_length, LengthUnit, UnitFormat};
use crate::view::ViewBasis;
use crate::views::{self, Look, ViewBody, ViewOptions};

/// Which view a plate draws.
#[derive(Clone, Debug)]
pub enum PlateView {
    /// Seen from above: cut at `cut` (everything above removed) or, with
    /// none, the whole model -- a roof plan. The frame is the model's x, y.
    Plan { cut: Option<f64> },
    /// Cut by `plane`, looking against its normal at what lies behind it
    /// (up to `depth`), the frame the plane's own.
    Section { plane: Plane, depth: Option<f64> },
    /// The whole model from a direction, nothing cut.
    Elevation { basis: ViewBasis },
}

/// What a plate says besides the view.
#[derive(Clone, Debug, Default)]
pub struct PlateOptions {
    /// The paper; `None` is tabloid (11 x 17 in) turned to suit the view.
    pub sheet: Option<SheetSize>,
    /// `None` fits the view and snaps to a standard ratio.
    pub scale: Option<Scale>,
    pub title: String,
    pub subtitle: String,
    /// A footnote along the bottom.
    pub note: String,
    /// Swatches: a colour and what it means.
    pub legend: Vec<(Rgb, String)>,
    /// Notes at model points, each on a white ground.
    pub labels: Vec<(Vec3, String)>,
    /// Degrees clockwise from the page's up to north, for a plan's arrow;
    /// `None` draws no arrow.
    pub north: Option<f64>,
    pub scale_bar: bool,
    pub unit_format: UnitFormat,
    /// The part of the view to show, `(x0, y0, x1, y1)` in model mm of the
    /// view's frame (a plan's is the model's x and y): the page is fitted to
    /// it and nothing outside it is drawn.
    pub crop: Option<(P2, P2)>,
    /// The colour look (the default) or the drafting one: the cut hatched,
    /// what is beyond as hidden-line-removed strokes.
    pub look: Look,
}

const MARGIN: f64 = 10.0;
const INK: Rgb = Rgb(29, 39, 51);
const GREY: Rgb = Rgb(75, 85, 99);

/// The plate's page: the view painted, fitted between the header and the
/// footnote, and everything around it.
pub fn plate(bodies: &[ViewBody], view: &PlateView, opts: &PlateOptions) -> Result<Drawing, String> {
    let mut d = Drawing::new();
    // Paint order is the picture: a nearer surface after a farther one, a
    // label's white ground before its words, whatever layer each is on.
    d.ordered = true;
    let vopts = ViewOptions { look: opts.look, ..ViewOptions::default() };
    let (geometry, basis, shift) = match view {
        PlateView::Plan { cut: Some(z) } => {
            let plane = Plane::XY.offset(*z);
            let g = views::plan_view(&mut d, bodies, *z, &vopts);
            let basis = ViewBasis::looking_from(Vec3::Z, plane.y_dir());
            let shift = basis.project(plane.origin).0;
            (g, basis, shift)
        }
        PlateView::Plan { cut: None } => {
            let basis = ViewBasis::looking_from(Vec3::Z, Vec3::Y);
            (views::elevation_view(&mut d, bodies, &basis, &vopts), basis, P2::new(0.0, 0.0))
        }
        PlateView::Section { plane, depth } => {
            let n = plane.normal.normalized();
            let basis = ViewBasis::looking_from(n, plane.y_dir());
            let shift = basis.project(plane.origin).0;
            (views::section_view(&mut d, bodies, plane, *depth, true, &vopts), basis, shift)
        }
        PlateView::Elevation { basis } => (views::elevation_view(&mut d, bodies, basis, &vopts), *basis, P2::new(0.0, 0.0)),
    };
    let Some((lo, hi)) = geometry.bounds else {
        return Err("the view is empty: nothing of the model is on the drawn side of the cut".to_owned());
    };
    d.notes.extend(geometry.notes.iter().cloned());
    let (lo, hi) = match opts.crop {
        None => (lo, hi),
        Some((a, b)) => {
            let (c0, c1) = (P2::new(a.x.min(b.x), a.y.min(b.y)), P2::new(a.x.max(b.x), a.y.max(b.y)));
            let (l, h) = (P2::new(lo.x.max(c0.x), lo.y.max(c0.y)), P2::new(hi.x.min(c1.x), hi.y.min(c1.y)));
            if l.x >= h.x || l.y >= h.y {
                return Err(format!(
                    "crop ({:.1}, {:.1}, {:.1}, {:.1}) holds none of the view, which spans ({:.1}, {:.1}, {:.1}, {:.1}) in its frame",
                    c0.x, c0.y, c1.x, c1.y, lo.x, lo.y, hi.x, hi.y
                ));
            }
            (l, h)
        }
    };
    let (w, h) = ((hi.x - lo.x).max(1e-9), (hi.y - lo.y).max(1e-9));

    let font = FontRef::Builtin;
    let bold = FontRef::BuiltinBold;
    let header = header_height(opts);
    let footer = if opts.note.trim().is_empty() { 0.0 } else { 7.0 };
    let size = match opts.sheet {
        Some(s) => s,
        None => {
            // Tabloid, turned to whichever way gives the view more room.
            let land = SheetSize::ANSI_B;
            let port = land.portrait();
            let fit = |s: SheetSize| {
                let bw = s.w_mm - 2.0 * MARGIN;
                let bh = s.h_mm - 2.0 * MARGIN - header - footer;
                (bw / w).min(bh / h)
            };
            if fit(port) > fit(land) { port } else { land }
        }
    };
    let (pw, ph) = (size.w_mm, size.h_mm);
    let box_lo = P2::new(MARGIN, MARGIN + footer);
    let box_hi = P2::new(pw - MARGIN, ph - MARGIN - header);
    let fit = ((box_hi.x - box_lo.x) / w).min((box_hi.y - box_lo.y) / h);
    let scale = opts.scale.unwrap_or_else(|| {
        if opts.unit_format.unit.imperial() {
            // An architect's scale: the largest of 3" to 1/32" a foot
            // that fits.
            [4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0, 192.0, 384.0]
                .iter()
                .map(|den| Scale::new(1.0, *den))
                .find(|s| s.ratio() <= fit)
                .unwrap_or_else(|| crate::model::snap(fit))
        } else {
            crate::model::snap(fit)
        }
    });
    let s = scale.ratio();
    let tx = (box_lo.x + box_hi.x) / 2.0 - (lo.x + hi.x) / 2.0 * s;
    let ty = (box_lo.y + box_hi.y) / 2.0 - (lo.y + hi.y) / 2.0 * s;

    // The notes at model points: paper-sized text on a white ground, in the
    // view's frame so they land where the point does.
    let mut items = geometry.items;
    let text_layer = d.layer(layer::TEXT);
    let knock = d.layer(layer::SYMB);
    for (p, t) in &opts.labels {
        let (q, _) = basis.project(*p);
        let at = P2::new(q.x - shift.x, q.y - shift.y);
        let size_mm = 2.4;
        let tw = crate::text::width_in(t, size_mm, &font);
        let (pad_x, pad_y) = (1.0 / s, 0.9 / s);
        let half_w = tw / 2.0 / s + pad_x;
        let half_h = size_mm / 2.0 / s + pad_y;
        items.push(Item::Hatch(crate::ir::HatchItem {
            style: Style { layer: knock, weight_mm: None, dash: None, color: Some(Rgb(255, 255, 255)) },
            name: String::new(),
            outer: Loop::rect(at, 2.0 * half_w, 2.0 * half_h),
            holes: Vec::new(),
            pattern: crate::ir::Pattern::Solid,
            scale: 1.0,
        }));
        items.push(Item::Text(crate::ir::TextItem {
            style: Style { layer: text_layer, weight_mm: None, dash: None, color: Some(INK) },
            at,
            text: t.clone(),
            height_mm: size_mm,
            rotation_deg: 0.0,
            anchor: Anchor::CENTER,
            font: font.clone(),
            model_height: false,
        }));
    }
    d.items.push(Item::Group(Group {
        name: "view".to_owned(),
        source: None,
        transform: Xform2::new(s, tx, ty),
        items,
        clip: Some(match opts.crop {
            Some(_) => ClipRect::new(P2::new(lo.x * s + tx, lo.y * s + ty), P2::new(hi.x * s + tx, hi.y * s + ty)),
            None => ClipRect::new(P2::new(0.0, 0.0), P2::new(pw, ph)),
        }),
    }));

    // The header: title, subtitle, legend on the left; scale and north on
    // the right.
    let right_w = 78.0;
    let top = ph - MARGIN;
    let mut y = top;
    if !opts.title.trim().is_empty() {
        y -= 6.0;
        sheetset::text(&mut d, text_layer, P2::new(MARGIN, y), opts.title.trim(), 5.0, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &bold, Some(INK));
        y -= 2.5;
    }
    if !opts.subtitle.trim().is_empty() {
        let lines = crate::text::wrap(opts.subtitle.trim(), pw - 2.0 * MARGIN - right_w, 2.8, &font);
        for l in lines {
            y -= 4.6;
            sheetset::text(&mut d, text_layer, P2::new(MARGIN, y), &l, 2.8, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &font, Some(GREY));
        }
        y -= 1.2;
    }
    if !opts.legend.is_empty() {
        let fill_layer = d.layer(layer::SYMB);
        let mut x = MARGIN;
        y -= 4.6;
        let limit = pw - MARGIN - right_w;
        for (c, label) in &opts.legend {
            let tw = crate::text::width_in(label, 2.3, &font);
            if x + 4.5 + tw > limit && x > MARGIN {
                x = MARGIN;
                y -= 5.0;
            }
            sheetset::fill(&mut d, fill_layer, Loop::rect(P2::new(x + 1.6, y + 1.15), 3.2, 3.2), Vec::new(), Some(*c));
            sheetset::stroke(&mut d, fill_layer, rect_path(P2::new(x, y - 0.45), P2::new(x + 3.2, y + 2.75)), true, Some(0.12), None, Some(Rgb(154, 160, 166)));
            sheetset::text(&mut d, text_layer, P2::new(x + 4.4, y), label, 2.3, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &font, Some(GREY));
            x += 4.4 + tw + 6.0;
        }
    }
    let rx = pw - MARGIN - right_w;
    let mut ry = top - 4.0;
    sheetset::text(&mut d, text_layer, P2::new(pw - MARGIN, ry), &format!("SCALE {}", scale.label(&opts.unit_format)), 2.3, 0.0, Anchor::new(HAlign::Right, VAlign::Baseline), &bold, Some(INK));
    if let Some(north) = opts.north {
        sheetset::north_arrow(&mut d, P2::new(rx + 8.0, top - 10.0), 6.0, north, false, &font);
    }
    if opts.scale_bar {
        ry -= 9.0;
        scale_bar(&mut d, P2::new(rx + 22.0, ry), right_w - 22.0, s, &opts.unit_format, &font);
    }
    if footer > 0.0 {
        sheetset::text(&mut d, text_layer, P2::new(MARGIN, MARGIN + 1.0), opts.note.trim(), 2.4, 0.0, Anchor::new(HAlign::Left, VAlign::Baseline), &font, Some(GREY));
    }

    // The sheet is the paper and the PDF's title; it draws no border and
    // no title block of its own.
    let mut sheet = Sheet::new(size);
    sheet.title_block.title = opts.title.trim().to_owned();
    sheet.title_block.scale = scale.label(&opts.unit_format);
    d.sheet = Some(sheet);
    d.scale = scale;
    d.unit_format = opts.unit_format;
    Ok(d)
}

/// How tall the header is, from what it holds.
fn header_height(opts: &PlateOptions) -> f64 {
    let mut h: f64 = 4.0;
    if !opts.title.trim().is_empty() {
        h += 8.5;
    }
    if !opts.subtitle.trim().is_empty() {
        h += 5.8;
    }
    if !opts.legend.is_empty() {
        h += 5.6;
    }
    h.max(if opts.north.is_some() || opts.scale_bar { 22.0 } else { 10.0 })
}

fn rect_path(lo: P2, hi: P2) -> Path {
    Path::new(lo).line_to(P2::new(hi.x, lo.y)).line_to(hi).line_to(P2::new(lo.x, hi.y))
}

/// A bar of four round divisions of the drawing's own unit, at most
/// `max_len` paper mm, labelled; `ratio` is paper mm per model mm.
fn scale_bar(d: &mut Drawing, at: P2, max_len: f64, ratio: f64, unit_format: &UnitFormat, font: &FontRef) {
    let symb = d.layer(layer::SYMB);
    let text_layer = d.layer(layer::TEXT);
    // Feet-and-inches draws feet; any other unit draws itself.
    let unit = match unit_format.unit {
        LengthUnit::FtIn | LengthUnit::Ft => LengthUnit::Ft,
        u => u,
    };
    let unit_paper = unit.mm() * ratio;
    let want = max_len / 4.0 / unit_paper.max(1e-12);
    let mut div = 1e-6;
    let mut k = 1e-6_f64;
    while k <= 1e9 {
        for m in if unit.imperial() { &[1.0, 2.0, 5.0][..] } else { &[1.0, 2.0, 2.5, 5.0][..] } {
            if m * k <= want {
                div = m * k;
            }
        }
        k *= 10.0;
    }
    let step = div * unit_paper;
    let h = 1.6;
    let mut x = at.x;
    // As many decimals as the division needs, and the unit on the last.
    let decimals = (0..4).find(|k| ((div * 10f64.powi(*k as i32)).round() - div * 10f64.powi(*k as i32)).abs() < 1e-6).unwrap_or(3);
    let fmt = UnitFormat { unit, decimals, suffix: false, ..UnitFormat::mm() };
    for i in 0..4 {
        let x1 = x + step;
        let l = Loop::rect(P2::new((x + x1) / 2.0, at.y + h / 2.0), x1 - x, h);
        if i % 2 == 0 {
            sheetset::fill(d, symb, l, Vec::new(), Some(INK));
        } else {
            sheetset::stroke(d, symb, rect_path(P2::new(x, at.y), P2::new(x1, at.y + h)), true, Some(0.2), None, Some(INK));
        }
        let label = format_length(div * i as f64 * unit.mm(), &fmt);
        sheetset::text(d, text_layer, P2::new(x, at.y - 3.0), &label, 1.9, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), font, Some(GREY));
        x = x1;
    }
    let last = if unit.imperial() { format_length(div * 4.0 * unit.mm(), &fmt) } else { format!("{} {}", format_length(div * 4.0 * unit.mm(), &fmt), unit.name()) };
    sheetset::text(d, text_layer, P2::new(x, at.y - 3.0), &last, 1.9, 0.0, Anchor::new(HAlign::Center, VAlign::Baseline), font, Some(GREY));
}

impl PlateView {
    /// The view a request names: `section` is the cut as `cadpole render
    /// --section` writes it (`z=1700`, `x=26`, `-x=26`, `y=...`, any written
    /// length), empty for none; `view` the direction to look from when
    /// nothing is cut (`top`, the default, or an elevation's direction).
    pub fn parse(view: &str, section: &str) -> Result<PlateView, String> {
        let section = section.trim();
        let view = view.trim().to_ascii_lowercase();
        if section.is_empty() {
            return match view.as_str() {
                "" | "top" | "plan" => Ok(PlateView::Plan { cut: None }),
                v => views::basis_for(v).map(|basis| PlateView::Elevation { basis }).ok_or_else(|| {
                    format!("view {v:?} is not a direction to look from (one of {})", views::DIRECTIONS.join(", "))
                }),
            };
        }
        let (axis, value) = section
            .split_once('=')
            .ok_or_else(|| format!("section {section:?} is not \"<axis>=<value>\" (z=1700, x=26, -x=26)"))?;
        let at = crate::units::parse_length(value).ok_or_else(|| format!("section: {value:?} is not a length"))?;
        let axis = axis.trim().to_ascii_lowercase();
        let (flip, axis) = match axis.strip_prefix('-') {
            Some(a) => (true, a.trim().to_owned()),
            None => (false, axis),
        };
        // The normal points at the eye, away from what is kept; x_dir is
        // chosen so the plane's y is up the page (the model's z for a
        // vertical cut, its y for a plan).
        let (normal, x_dir) = match (axis.as_str(), flip) {
            ("z", false) => (Vec3::Z, Vec3::X),
            ("z", true) => (Vec3::new(0.0, 0.0, -1.0), Vec3::X),
            ("x", false) => (Vec3::X, Vec3::Y),
            ("x", true) => (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)),
            ("y", false) => (Vec3::Y, Vec3::new(-1.0, 0.0, 0.0)),
            ("y", true) => (Vec3::new(0.0, -1.0, 0.0), Vec3::X),
            (other, _) => return Err(format!("section: axis {other:?} must be x, y or z")),
        };
        if axis == "z" && !flip {
            if !matches!(view.as_str(), "" | "top" | "plan") {
                return Err(format!("section {section:?} is a plan, seen from the top; view {view:?} does not look down"));
            }
            return Ok(PlateView::Plan { cut: Some(at) });
        }
        let origin = Vec3::new(normal.x.abs() * at, normal.y.abs() * at, normal.z.abs() * at);
        let plane = Plane::new(origin, normal, x_dir).map_err(|e| format!("section plane: {e}"))?;
        Ok(PlateView::Section { plane, depth: None })
    }

    /// The same view of a model scaled by `k` about the origin.
    pub fn scaled(self, k: f64) -> PlateView {
        match self {
            PlateView::Plan { cut } => PlateView::Plan { cut: cut.map(|z| z * k) },
            PlateView::Section { mut plane, depth } => {
                plane.origin = plane.origin * k;
                PlateView::Section { plane, depth: depth.map(|d| d * k) }
            }
            e => e,
        }
    }

    /// True for a view seen from above, which carries a north arrow.
    pub fn is_plan(&self) -> bool {
        matches!(self, PlateView::Plan { .. })
    }
}

/// A crop, `"x0,y0,x1,y1"` in model mm or written lengths.
pub fn parse_crop(s: &str) -> Result<(P2, P2), String> {
    let v: Vec<f64> = s
        .split(',')
        .map(|c| crate::units::parse_length(c.trim()))
        .collect::<Option<Vec<f64>>>()
        .filter(|v| v.len() == 4)
        .ok_or_else(|| format!("crop {s:?} is not four numbers x0,y0,x1,y1"))?;
    Ok((P2::new(v[0], v[1]), P2::new(v[2], v[3])))
}

/// A legend entry, `"#rrggbb=label"`.
pub fn parse_legend(s: &str) -> Result<(Rgb, String), String> {
    let (c, label) = s.split_once('=').ok_or_else(|| format!("legend {s:?} is not \"#rrggbb=label\""))?;
    let color = parse_hex(c).ok_or_else(|| format!("legend {s:?}: {c:?} is not a colour like #e39b72"))?;
    Ok((color, label.trim().to_owned()))
}

/// A note at a model point, `"x,y,z=text"`, each coordinate in mm or any
/// written length.
pub fn parse_label(s: &str) -> Result<(Vec3, String), String> {
    let (p, text) = s.split_once('=').ok_or_else(|| format!("label {s:?} is not \"x,y,z=text\""))?;
    let v: Vec<f64> = p
        .split(',')
        .map(|c| crate::units::parse_length(c.trim()))
        .collect::<Option<Vec<f64>>>()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| format!("label {s:?}: {p:?} is not three coordinates x,y,z"))?;
    Ok((Vec3::new(v[0], v[1], v[2]), text.to_owned()))
}

/// `#rrggbb`, the `#` optional.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(Rgb(b(0)?, b(2)?, b(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Prim;
    use cadpole_core::sketch::Sketch;

    fn block(name: &str, x: f64, y: f64, z0: f64, w: f64, d: f64, h: f64, color: Rgb) -> ViewBody {
        let s = Sketch::new(Plane::XY.offset(z0)).with_loop(Loop::rect(P2::new(x + w / 2.0, y + d / 2.0), w, d));
        let solids = cadpole_core::kernel::extrude(&s, h).expect("extrude");
        let (mesh, topo) = cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess");
        ViewBody { color: Some(color), ..ViewBody::new(name, mesh, topo) }
    }

    /// A room: a slab, two walls, a table, and the room's name lying on
    /// the floor as a glyph body.
    fn room() -> Vec<ViewBody> {
        let mut name = block("glyphs", 1800.0, 1400.0, 0.0, 600.0, 200.0, 20.0, Rgb(29, 39, 51));
        name.label = Some(crate::painted::BodyLabel {
            group: "room".into(),
            text: "Living".into(),
            cap_height: 200.0,
            anchor: Anchor::CENTER,
            bold: false,
            plane: Plane::XY,
            at: P2::new(2100.0, 1500.0),
        });
        vec![
            block("slab", 0.0, 0.0, -150.0, 4000.0, 3000.0, 150.0, Rgb(217, 205, 184)),
            block("south", 0.0, 0.0, 0.0, 4000.0, 200.0, 2700.0, Rgb(201, 119, 79)),
            block("west", 0.0, 0.0, 0.0, 200.0, 3000.0, 2700.0, Rgb(201, 119, 79)),
            block("table", 2600.0, 600.0, 0.0, 900.0, 600.0, 740.0, Rgb(168, 134, 95)),
            name,
        ]
    }

    fn opts() -> PlateOptions {
        PlateOptions {
            title: "LIVING ROOM".into(),
            subtitle: "a test plate".into(),
            note: "north up".into(),
            legend: vec![(Rgb(201, 119, 79), "walls".into())],
            labels: vec![(Vec3::new(3050.0, 900.0, 740.0), "table".into())],
            north: Some(0.0),
            scale_bar: true,
            unit_format: UnitFormat::mm(),
            look: Look::Color,
            ..Default::default()
        }
    }

    #[test]
    fn views_and_options_parse_the_way_render_writes_them() {
        assert!(matches!(PlateView::parse("top", "z=1700").unwrap(), PlateView::Plan { cut: Some(c) } if c == 1700.0));
        assert!(matches!(PlateView::parse("", "z=5'-6\"").unwrap(), PlateView::Plan { cut: Some(c) } if (c - 1676.4).abs() < 1e-9));
        assert!(matches!(PlateView::parse("", "").unwrap(), PlateView::Plan { cut: None }));
        let PlateView::Section { plane, .. } = PlateView::parse("", "x=26").unwrap() else { panic!() };
        assert_eq!(plane.normal, Vec3::X, "the eye at +x, looking toward -x");
        assert!((plane.y_dir().z - 1.0).abs() < 1e-12, "up the page is up");
        let PlateView::Section { plane, .. } = PlateView::parse("", "-x=26").unwrap() else { panic!() };
        assert_eq!(plane.normal, Vec3::new(-1.0, 0.0, 0.0));
        assert!((plane.origin.x - 26.0).abs() < 1e-12 && (plane.y_dir().z - 1.0).abs() < 1e-12);
        let PlateView::Section { plane, .. } = PlateView::parse("", "y=100").unwrap() else { panic!() };
        assert!((plane.y_dir().z - 1.0).abs() < 1e-12);
        assert!(matches!(PlateView::parse("north", "").unwrap(), PlateView::Elevation { .. }));
        assert!(PlateView::parse("front", "z=10").unwrap_err().contains("plan"));
        assert!(PlateView::parse("", "w=10").unwrap_err().contains("x, y or z"));
        assert!(PlateView::parse("sideways", "").unwrap_err().contains("direction"));
        assert_eq!(parse_legend("#e39b72=exterior walls").unwrap(), (Rgb(0xe3, 0x9b, 0x72), "exterior walls".into()));
        assert!(parse_legend("orange=walls").is_err());
        assert_eq!(parse_label("26,40.5,-3=chef kitchen 7'-5\" clear").unwrap(), (Vec3::new(26.0, 40.5, -3.0), "chef kitchen 7'-5\" clear".into()));
        assert!(parse_label("1,2=x").is_err());
        assert_eq!(parse_crop("-9,-3,54,72").unwrap(), (P2::new(-9.0, -3.0), P2::new(54.0, 72.0)));
        assert!(parse_crop("1,2,3").is_err());
    }

    #[test]
    fn a_plate_is_a_page_of_the_view_and_its_words() {
        let d = plate(&room(), &PlateView::Plan { cut: Some(1200.0) }, &opts()).unwrap();
        let sheet = d.sheet.as_ref().expect("a page size");
        assert_eq!((sheet.size.w_mm, sheet.size.h_mm), (431.8, 279.4), "tabloid, landscape for a wide room");
        let words: Vec<String> = d.flatten().into_iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text) } else { None }).collect();
        for w in ["LIVING ROOM", "a test plate", "walls", "table", "north up", "Living", "N", "SCALE 1:20"] {
            assert!(words.iter().any(|t| t == w), "{w:?} in {words:?}");
        }
        // The room's name is model-sized: 200 mm at 1:20 is 10 mm of cap.
        let living = d.flatten().into_iter().find_map(|p| match p {
            Prim::Text { text, height_mm, .. } if text == "Living" => Some(height_mm),
            _ => None,
        });
        assert!((living.unwrap() - 10.0).abs() < 1e-9);
        // A section has no north arrow.
        let s = plate(&room(), &PlateView::parse("", "y=1500").unwrap(), &PlateOptions { north: None, ..opts() }).unwrap();
        assert!(!s.flatten().iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "N")));
        // A crop that misses the view says so.
        let e = plate(&room(), &PlateView::Plan { cut: Some(1200.0) }, &PlateOptions { crop: Some((P2::new(9e3, 9e3), P2::new(1e4, 1e4))), ..opts() }).unwrap_err();
        assert!(e.contains("holds none of the view"), "{e}");
        // A crop fits the page to it: a quarter of the room is drawn larger.
        let c = plate(&room(), &PlateView::Plan { cut: Some(1200.0) }, &PlateOptions { crop: Some((P2::new(0.0, 0.0), P2::new(2000.0, 1500.0))), ..opts() }).unwrap();
        assert!(c.scale.ratio() > d.scale.ratio(), "{:?} vs {:?}", c.scale, d.scale);
    }

    /// The colour look's SVG, byte for byte: the paint order, the colours,
    /// the shading, the edges, the page around it. A change here is a
    /// change in what every plate looks like, so it is reviewed as a diff.
    #[test]
    fn a_painted_plate_matches_the_golden_svg() {
        let golden = include_str!("../testdata/painted_plate.svg");
        let d = plate(&room(), &PlateView::Plan { cut: Some(1200.0) }, &opts()).unwrap();
        let actual = crate::svg::write(&d, &Default::default());
        if actual.trim() != golden.trim() {
            if let Ok(dir) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
                let _ = std::fs::write(format!("{dir}/painted_plate.svg"), &actual);
            }
            panic!(
                "src/rust/cadpole-drawing/testdata/painted_plate.svg is out of date: the new one is in \
                 bazel-testlogs/src/rust/cadpole-drawing/cadpole_drawing_test/test.outputs/ -- review it and copy it over"
            );
        }
    }
}
