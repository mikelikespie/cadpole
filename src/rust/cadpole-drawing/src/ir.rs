//! The drawing IR: a 2D scene that knows it will end up on paper.
//!
//! Two coordinate systems meet here and the split is the whole design.
//!
//! * **Geometry** is in model millimetres, the same numbers the solid is built
//!   from, so a dimension can state a true length and a click on a wall can
//!   find the wall.
//! * **Ink** — line weights, text heights, dash lengths, hatch spacing, arrow
//!   sizes — is in *paper* millimetres, so a drawing reads the same at 1:1 and
//!   at 1:48. ISO 128's width series (0.13 / 0.18 / 0.25 / 0.35 / 0.5 / 0.7 /
//!   1.0 mm) is what [`Layer::weight_mm`] holds.
//!
//! [`Drawing::flatten`] is where the two meet: it walks the item tree applying
//! each [`Group`]'s scale-and-translate, and hands the writers geometry
//! already in paper millimetres with the ink sizes untouched. A writer
//! therefore has no arithmetic to do and no way to disagree with its siblings
//! — SVG, PDF and DXF all consume the same [`Prim`] list.
//!
//! Arcs survive that walk. [`Group`] transforms are uniform scale plus
//! translation precisely so a circle stays a circle: an SVG `A` command and a
//! DXF bulge are exact, and a hole in an exported drawing re-imports as a real
//! arc.

use std::sync::Arc;

use cadpole_core::sketch::{Loop, Path, Segment, P2};

use crate::stdfont::StdFont;
use crate::units::UnitFormat;

/// Index into [`Drawing::layers`].
pub type LayerId = usize;

/// A colour, for writers that have one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    /// `#rrggbb`, which is what SVG and PDF both want.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
    /// Nearest AutoCAD Color Index, for DXF layers. A tiny table beats a
    /// full 256-entry palette: drawings use a handful of colours.
    pub fn aci(self) -> i16 {
        const TABLE: &[(Rgb, i16)] = &[
            (Rgb(255, 0, 0), 1),
            (Rgb(255, 255, 0), 2),
            (Rgb(0, 255, 0), 3),
            (Rgb(0, 255, 255), 4),
            (Rgb(0, 0, 255), 5),
            (Rgb(255, 0, 255), 6),
            (Rgb(255, 255, 255), 7),
            (Rgb(128, 128, 128), 8),
            (Rgb(192, 192, 192), 9),
            (Rgb(0, 0, 0), 7),
        ];
        let mut best = (i32::MAX, 7);
        for (c, i) in TABLE {
            let d = (c.0 as i32 - self.0 as i32).pow(2)
                + (c.1 as i32 - self.1 as i32).pow(2)
                + (c.2 as i32 - self.2 as i32).pow(2);
            if d < best.0 {
                best = (d, *i);
            }
        }
        best.1
    }
}

/// A line style, in paper millimetres. The names are the drafting ones.
#[derive(Clone, Debug, PartialEq)]
pub enum Dash {
    Solid,
    /// Long dashes: "above the cut", roof overhang, upper cabinets.
    Dashed,
    /// Short dashes: hidden edges in a mechanical view.
    Hidden,
    /// Long-short-long: centrelines and axes.
    Center,
    /// Long-short-short-long: phantom lines, adjacent parts.
    Phantom,
    Custom(Vec<f64>),
}

impl Dash {
    /// Dash/gap lengths in paper millimetres; empty means a solid line.
    pub fn pattern(&self) -> &[f64] {
        match self {
            Dash::Solid => &[],
            Dash::Dashed => &[4.0, 2.0],
            Dash::Hidden => &[2.0, 1.0],
            Dash::Center => &[8.0, 1.5, 1.5, 1.5],
            Dash::Phantom => &[10.0, 1.5, 1.5, 1.5, 1.5, 1.5],
            Dash::Custom(p) => p,
        }
    }
    pub fn is_solid(&self) -> bool {
        self.pattern().is_empty()
    }
    /// The DXF linetype name, so an architect's template picks it up.
    pub fn dxf_name(&self) -> &'static str {
        match self {
            Dash::Solid => "CONTINUOUS",
            Dash::Dashed => "DASHED",
            Dash::Hidden => "HIDDEN",
            Dash::Center => "CENTER",
            Dash::Phantom => "PHANTOM",
            Dash::Custom(_) => "DASHED",
        }
    }
}

/// A drawing layer. Names follow the AIA/NCS `<discipline>-<major>-<minor>`
/// convention (`A-WALL`, `A-ANNO-DIMS`) so a DXF opens into an architect's
/// own template unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Plotted width in paper millimetres, from the ISO 128 series.
    pub weight_mm: f64,
    pub color: Rgb,
    pub dash: Dash,
    /// False for construction geometry that exists but is never plotted.
    pub print: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, weight_mm: f64) -> Layer {
        Layer {
            name: name.into(),
            weight_mm,
            color: Rgb::BLACK,
            dash: Dash::Solid,
            print: true,
        }
    }
    pub fn with_dash(mut self, dash: Dash) -> Layer {
        self.dash = dash;
        self
    }
    pub fn with_color(mut self, color: Rgb) -> Layer {
        self.color = color;
        self
    }
}

/// The layer names the pipeline creates on its own. Mechanical views use the
/// first group, plans the `A-` group; both share the annotation layers.
pub mod layer {
    /// Visible edges of a projected view.
    pub const VISIBLE: &str = "M-VIEW";
    /// Hidden edges, dashed.
    pub const HIDDEN: &str = "M-VIEW-HIDD";
    /// The outline of a cut face.
    pub const SECTION: &str = "M-SECT";
    /// The hatch inside a cut face.
    pub const SECTION_HATCH: &str = "M-SECT-PATT";
    /// Centrelines and axes.
    pub const CENTER: &str = "M-ANNO-CNTR";
    /// Cut walls in a plan.
    pub const WALL: &str = "A-WALL";
    pub const DOOR: &str = "A-DOOR";
    pub const GLAZ: &str = "A-GLAZ";
    /// Dimension strings.
    pub const DIMS: &str = "A-ANNO-DIMS";
    /// Notes, labels, room names.
    pub const TEXT: &str = "A-ANNO-TEXT";
    /// Tags, north arrows, section marks.
    pub const SYMB: &str = "A-ANNO-SYMB";
    /// Sheet border and title block.
    pub const TITLE: &str = "G-ANNO-TTLB";
}

/// Weight and dash for a layer this pipeline creates, from the ISO 128 series
/// and standard residential practice.
fn default_layer(name: &str) -> Layer {
    match name {
        layer::VISIBLE => Layer::new(name, 0.5),
        layer::HIDDEN => Layer::new(name, 0.25)
            .with_dash(Dash::Hidden)
            .with_color(Rgb(90, 90, 90)),
        layer::SECTION => Layer::new(name, 0.7),
        layer::SECTION_HATCH => Layer::new(name, 0.13).with_color(Rgb(110, 110, 110)),
        layer::CENTER => Layer::new(name, 0.13)
            .with_dash(Dash::Center)
            .with_color(Rgb(120, 120, 170)),
        layer::WALL => Layer::new(name, 0.7),
        layer::DOOR | layer::GLAZ => Layer::new(name, 0.25),
        layer::DIMS => Layer::new(name, 0.18).with_color(Rgb(60, 60, 60)),
        layer::TEXT => Layer::new(name, 0.18),
        layer::SYMB => Layer::new(name, 0.25),
        layer::TITLE => Layer::new(name, 0.35),
        _ => Layer::new(name, 0.25),
    }
}

/// Per-item overrides on top of its layer. `None` everywhere is the normal
/// case: drawings are organised by layer, not by ad-hoc styling.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub layer: LayerId,
    pub weight_mm: Option<f64>,
    pub dash: Option<Dash>,
    pub color: Option<Rgb>,
}

impl Style {
    pub fn on(layer: LayerId) -> Style {
        Style {
            layer,
            ..Style::default()
        }
    }
    pub fn dashed(layer: LayerId, dash: Dash) -> Style {
        Style {
            layer,
            dash: Some(dash),
            ..Style::default()
        }
    }
}

/// How text sits relative to its anchor point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    /// The typographic baseline sits on the anchor.
    #[default]
    Baseline,
    /// The cap-height box is centred on the anchor.
    Middle,
    /// Cap height at the anchor, text below.
    Top,
    /// Descender-free bottom at the anchor.
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchor {
    pub h: HAlign,
    pub v: VAlign,
}

impl Anchor {
    pub const fn new(h: HAlign, v: VAlign) -> Anchor {
        Anchor { h, v }
    }
    pub const CENTER: Anchor = Anchor {
        h: HAlign::Center,
        v: VAlign::Middle,
    };
    pub const LEFT: Anchor = Anchor {
        h: HAlign::Left,
        v: VAlign::Middle,
    };
    pub const RIGHT: Anchor = Anchor {
        h: HAlign::Right,
        v: VAlign::Middle,
    };
}

/// Which face sets a string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontRef {
    /// The face bundled with Cadpole (Inter, OFL), at its regular weight. A PDF
    /// embeds the glyphs it uses as a TrueType subset and writes the string
    /// as real text; `text_outlines` fills them as outlines instead.
    Builtin,
    /// The bundled face at weight 700: the variable font instanced, so the
    /// PDF's subset is a true bold, not the regular stroked heavier.
    BuiltinBold,
    /// A font asset, by name. The writer resolves it; unknown names fall back
    /// to the builtin with a diagnostic rather than failing an export.
    Asset(String),
    /// One of the standard PDF faces: written as real text, measured
    /// from the Adobe metrics in [`crate::stdfont`], no font embedded.
    Standard(StdFont),
}

impl FontRef {
    /// The same face, bold or not.
    pub fn with_bold(&self, bold: bool) -> FontRef {
        match self {
            FontRef::Standard(f) => FontRef::Standard(f.with_bold(bold)),
            FontRef::Builtin | FontRef::BuiltinBold => {
                if bold {
                    FontRef::BuiltinBold
                } else {
                    FontRef::Builtin
                }
            }
            other => other.clone(),
        }
    }

    /// True for the bundled face at either weight.
    pub fn is_builtin(&self) -> bool {
        matches!(self, FontRef::Builtin | FontRef::BuiltinBold)
    }

    /// True for a bold face.
    pub fn bold(&self) -> bool {
        match self {
            FontRef::Standard(f) => f.bold(),
            FontRef::BuiltinBold => true,
            _ => false,
        }
    }
}

impl Default for FontRef {
    fn default() -> Self {
        FontRef::Builtin
    }
}

/// A run of text. `height_mm` is the **cap height** in paper millimetres,
/// which is what a drafter means by "2.5 mm text", not the em size.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub style: Style,
    pub at: P2,
    pub text: String,
    pub height_mm: f64,
    pub rotation_deg: f64,
    pub anchor: Anchor,
    pub font: FontRef,
    /// `height_mm` is in the item's own (model) units and scales with the
    /// groups it sits in, like geometry, rather than being paper
    /// millimetres: a room's name set in the model 300 mm tall reads 300 mm
    /// tall at any scale, as it does in a render.
    pub model_height: bool,
}

/// How a filled region is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// A solid fill (poché).
    Solid,
    /// Parallel lines. `angle_deg` is measured from +x, `spacing_mm` is in
    /// paper millimetres.
    Lines { angle_deg: f64, spacing_mm: f64 },
    /// Two line families at right angles.
    Cross { angle_deg: f64, spacing_mm: f64 },
    /// A dot screen: a filled dot of `radius_mm` on a square grid of
    /// `spacing_mm`, anchored to the origin. Reads as a grey on a plotter
    /// that has no greys.
    Dots { spacing_mm: f64, radius_mm: f64 },
}

impl Pattern {
    /// `ANSI31`: 45°, the general-purpose section hatch.
    pub const ANSI31: Pattern = Pattern::Lines {
        angle_deg: 45.0,
        spacing_mm: 2.5,
    };
    /// Masonry: a tighter 45°.
    pub const MASONRY: Pattern = Pattern::Lines {
        angle_deg: 45.0,
        spacing_mm: 1.6,
    };
    /// Concrete, approximated as a coarse cross-hatch until a real dot/
    /// triangle pattern lands.
    pub const CONCRETE: Pattern = Pattern::Cross {
        angle_deg: 45.0,
        spacing_mm: 4.0,
    };
    /// Earth on a section.
    pub const EARTH: Pattern = Pattern::Lines {
        angle_deg: -45.0,
        spacing_mm: 3.0,
    };
}

/// A filled region: one outer loop and its holes, drawn with `pattern`.
#[derive(Clone, Debug, PartialEq)]
pub struct HatchItem {
    pub style: Style,
    /// A stable id, like [`Stroke::name`]. Empty when the region has none.
    pub name: String,
    pub outer: Loop,
    pub holes: Vec<Loop>,
    pub pattern: Pattern,
    /// Multiplies the pattern's spacing. 1.0 is the pattern as defined.
    pub scale: f64,
}

/// An open or closed run of ink.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub style: Style,
    /// A stable id, written as the SVG `id` and used to name the DXF entity's
    /// source. Empty when the stroke has no identity of its own. Imported and
    /// exported geometry keep the *same* names, so a round trip round-trips.
    pub name: String,
    pub path: Path,
    pub closed: bool,
}

/// A note with a line pointing at what it is about.
#[derive(Clone, Debug, PartialEq)]
pub struct Leader {
    pub style: Style,
    /// At least two points; the arrow is at `points[0]`.
    pub points: Vec<P2>,
    pub text: String,
    pub height_mm: f64,
    /// The face the note is set in: a sheet's labels are its own face, so
    /// they are text in the PDF like everything else on it.
    pub font: FontRef,
}

/// A raster picture: decoded pixels for the PDF, and the file it came from
/// for an SVG, which embeds the bytes as they were.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major, top row first, four bytes per pixel.
    pub rgba: Vec<u8>,
    /// The PNG the pixels were decoded from, when there was one.
    pub png: Option<Vec<u8>>,
}

impl Image {
    /// True when any pixel is not fully opaque, which decides whether the
    /// PDF needs a soft mask.
    pub fn has_alpha(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] != 255)
    }
}

/// A picture placed on the page: `at` is its lower-left corner and `w`, `h`
/// its size, all in the item's own coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageItem {
    pub style: Style,
    pub at: P2,
    pub w: f64,
    pub h: f64,
    pub image: Arc<Image>,
}

/// An axis-aligned rectangle, for a group's viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    pub lo: P2,
    pub hi: P2,
}

impl ClipRect {
    pub fn new(lo: P2, hi: P2) -> ClipRect {
        ClipRect {
            lo: P2::new(lo.x.min(hi.x), lo.y.min(hi.y)),
            hi: P2::new(lo.x.max(hi.x), lo.y.max(hi.y)),
        }
    }
    pub fn contains(&self, p: P2) -> bool {
        p.x >= self.lo.x - 1e-9 && p.x <= self.hi.x + 1e-9 && p.y >= self.lo.y - 1e-9 && p.y <= self.hi.y + 1e-9
    }
    fn map(&self, x: Xform2) -> ClipRect {
        ClipRect::new(x.apply(self.lo), x.apply(self.hi))
    }
}

/// A scale-and-translate placement. Uniform on purpose: a non-uniform scale
/// would turn every arc into an ellipse and cost the exactness the rest of
/// the pipeline works to keep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform2 {
    pub scale: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Xform2 {
    fn default() -> Self {
        Xform2::IDENTITY
    }
}

impl Xform2 {
    pub const IDENTITY: Xform2 = Xform2 {
        scale: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn new(scale: f64, tx: f64, ty: f64) -> Xform2 {
        Xform2 { scale, tx, ty }
    }

    pub fn apply(&self, p: P2) -> P2 {
        P2::new(p.x * self.scale + self.tx, p.y * self.scale + self.ty)
    }

    /// `self` applied first, then `outer`.
    pub fn then(&self, outer: &Xform2) -> Xform2 {
        Xform2 {
            scale: self.scale * outer.scale,
            tx: self.tx * outer.scale + outer.tx,
            ty: self.ty * outer.scale + outer.ty,
        }
    }
}

/// A named bundle of items that moves as one, carrying the model entity that
/// produced it so a click on the drawing can select the feature and a
/// dimension can be traced back to the two faces it measures.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Group {
    pub name: String,
    pub source: Option<String>,
    pub transform: Xform2,
    pub items: Vec<Item>,
    /// A viewport in the group's *parent* coordinates: whatever the group
    /// draws outside it is cut away when the drawing is flattened.
    /// That is what keeps a plan's neighbours off the title block.
    pub clip: Option<ClipRect>,
}

/// One thing in a drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Stroke(Stroke),
    Hatch(HatchItem),
    Text(TextItem),
    Dim(crate::dim::Dim),
    Leader(Leader),
    Group(Group),
    Image(ImageItem),
}

/// A drawing scale, written the way a title block writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// Paper units per `denominator` model units.
    pub numerator: f64,
    pub denominator: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Scale::ONE_TO_ONE
    }
}

impl Scale {
    pub const ONE_TO_ONE: Scale = Scale {
        numerator: 1.0,
        denominator: 1.0,
    };
    /// 1/4" = 1'-0", the residential plan scale.
    pub const QUARTER_INCH: Scale = Scale {
        numerator: 1.0,
        denominator: 48.0,
    };

    pub fn new(numerator: f64, denominator: f64) -> Scale {
        Scale {
            numerator,
            denominator,
        }
    }

    /// Paper millimetres per model millimetre.
    ///
    /// Never zero, negative or non-finite: [`Scale::new`] is public, and a
    /// drawing scaled by NaN is a corrupt file rather than a wrong one.
    pub fn ratio(&self) -> f64 {
        let r = self.numerator / self.denominator;
        if r.is_finite() && r > 0.0 {
            r
        } else {
            1.0
        }
    }

    /// Parse `1:48`, `1/4"=1'-0"`, `2:1`, or a bare number (`0.5` = 1:2).
    ///
    /// Both halves must be finite and positive. That is not pedantry: Rust's
    /// float parser accepts `inf` and `nan`, and a scale of either produces a
    /// PDF whose content stream says `inf NaN m` — a file no reader can open —
    /// or an SVG in which every path has collapsed to a point. A negative
    /// scale silently mirrors the drawing. All of them are refused here, where
    /// the caller still has a diagnostic to put them in.
    pub fn parse(s: &str) -> Option<Scale> {
        let t = s.trim();
        let checked = |a: f64, b: f64| {
            (a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0).then(|| Scale::new(a, b))
        };
        if let Some((a, b)) = t.split_once('=') {
            return checked(
                crate::units::parse_length(a)?,
                crate::units::parse_length(b)?,
            );
        }
        if let Some((a, b)) = t.split_once(':') {
            return checked(a.trim().parse().ok()?, b.trim().parse().ok()?);
        }
        checked(t.parse().ok()?, 1.0)
    }

    /// How a title block writes it.
    pub fn label(&self, f: &UnitFormat) -> String {
        if f.unit.imperial() {
            // Imperial drawings are labelled as "1/4\" = 1'-0\"".
            let paper_in_per_foot = self.ratio() * 304.8 / 25.4;
            for (num, den, text) in [
                (3.0, 1.0, "3\" = 1'-0\""),
                (3.0, 2.0, "1 1/2\" = 1'-0\""),
                (1.0, 1.0, "1\" = 1'-0\""),
                (3.0, 4.0, "3/4\" = 1'-0\""),
                (1.0, 2.0, "1/2\" = 1'-0\""),
                (3.0, 8.0, "3/8\" = 1'-0\""),
                (1.0, 4.0, "1/4\" = 1'-0\""),
                (3.0, 16.0, "3/16\" = 1'-0\""),
                (1.0, 8.0, "1/8\" = 1'-0\""),
                (3.0, 32.0, "3/32\" = 1'-0\""),
                (1.0, 16.0, "1/16\" = 1'-0\""),
                (1.0, 32.0, "1/32\" = 1'-0\""),
            ] {
                if (paper_in_per_foot - num / den).abs() < 1e-6 {
                    return text.to_owned();
                }
            }
        }
        let r = self.ratio();
        if (r - 1.0).abs() < 1e-9 {
            "1:1".to_owned()
        } else if r < 1.0 {
            format!("1:{}", trim(1.0 / r))
        } else {
            format!("{}:1", trim(r))
        }
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

/// A 2D drawing: layers, items and an optional sheet to lay them out on.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub layers: Vec<Layer>,
    pub items: Vec<Item>,
    pub sheet: Option<crate::sheet::Sheet>,
    /// How lengths are written in dimensions and labels.
    pub unit_format: UnitFormat,
    /// The nominal scale, for the title block. Geometry inside the drawing is
    /// already in paper millimetres when a sheet is present; a bare drawing
    /// (one sketch, exported 1:1) carries `ONE_TO_ONE`.
    pub scale: Scale,
    /// Notes worth surfacing to the user: a dimension that would not fit, a
    /// font that was substituted.
    pub notes: Vec<String>,
    /// Paint the items in the order they were added rather than grouped by
    /// layer: a sheet's white knock-out under a label has to come
    /// after the hatch it covers, whatever layers the two are on.
    pub ordered: bool,
}

impl Default for Drawing {
    fn default() -> Self {
        Drawing::new()
    }
}

impl Drawing {
    pub fn new() -> Drawing {
        Drawing {
            layers: Vec::new(),
            items: Vec::new(),
            sheet: None,
            unit_format: UnitFormat::mm(),
            scale: Scale::ONE_TO_ONE,
            notes: Vec::new(),
            ordered: false,
        }
    }

    /// The id of `name`, creating the layer with its standard weight and dash
    /// if this drawing has not used it yet.
    pub fn layer(&mut self, name: &str) -> LayerId {
        if let Some(i) = self.layers.iter().position(|l| l.name == name) {
            return i;
        }
        self.layers.push(default_layer(name));
        self.layers.len() - 1
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    /// Add an open or closed stroke on `layer`.
    pub fn stroke(&mut self, layer: LayerId, path: Path, closed: bool) {
        self.push(Item::Stroke(Stroke {
            style: Style::on(layer),
            name: String::new(),
            path,
            closed,
        }));
    }

    /// Add a stroke under a stable id.
    pub fn named_stroke(
        &mut self,
        layer: LayerId,
        path: Path,
        closed: bool,
        name: impl Into<String>,
    ) {
        self.push(Item::Stroke(Stroke {
            style: Style::on(layer),
            name: name.into(),
            path,
            closed,
        }));
    }

    /// Add a polyline through `points` on `layer`.
    pub fn polyline(&mut self, layer: LayerId, points: &[P2], closed: bool) {
        if points.len() < 2 {
            return;
        }
        let mut p = Path::new(points[0]);
        for q in &points[1..] {
            p = p.line_to(*q);
        }
        self.stroke(layer, p, closed);
    }

    /// Add a closed loop on `layer`.
    pub fn loop_stroke(&mut self, layer: LayerId, l: &Loop) {
        self.named_loop_stroke(layer, l, String::new());
    }

    /// Add a closed loop under a stable id.
    pub fn named_loop_stroke(&mut self, layer: LayerId, l: &Loop, name: impl Into<String>) {
        if l.segments.is_empty() {
            return;
        }
        let mut p = Path::new(l.segments[0].start());
        for s in &l.segments {
            p.segments.push(s.clone());
        }
        self.named_stroke(layer, p, true, name);
    }

    pub fn text(
        &mut self,
        layer: LayerId,
        at: P2,
        text: impl Into<String>,
        height_mm: f64,
        anchor: Anchor,
    ) {
        self.push(Item::Text(TextItem {
            style: Style::on(layer),
            at,
            text: text.into(),
            height_mm,
            rotation_deg: 0.0,
            anchor,
            font: FontRef::Builtin,
            model_height: false,
        }));
    }

    /// Set every string in the builtin face in `font` instead, text,
    /// dimensions and leaders alike, sheet items included: a
    /// standard face makes a PDF's text real, selectable text. Strings
    /// already in another face keep it.
    pub fn set_text_font(&mut self, font: &FontRef) {
        fn walk(items: &mut [Item], font: &FontRef) {
            for it in items {
                match it {
                    Item::Text(t) if t.font == FontRef::Builtin => t.font = font.clone(),
                    Item::Text(t) if t.font == FontRef::BuiltinBold => t.font = font.with_bold(true),
                    Item::Dim(d) if d.dim.font == FontRef::Builtin => d.dim.font = font.clone(),
                    Item::Leader(l) if l.font == FontRef::Builtin => l.font = font.clone(),
                    Item::Group(g) => walk(&mut g.items, font),
                    _ => {}
                }
            }
        }
        walk(&mut self.items, font);
        if let Some(s) = &mut self.sheet {
            walk(&mut s.items, font);
        }
    }

    /// Bounding box of everything, in whatever space the items are in.
    pub fn bounds(&self) -> Option<(P2, P2)> {
        let prims = self.flatten();
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        let mut any = false;
        let mut eat = |p: P2| {
            // `f64::min` ignores NaN, so without this a drawing of nothing but
            // non-finite points would report `lo = MAX, hi = MIN` — an
            // inverted box that every caller then divides by.
            if !(p.x.is_finite() && p.y.is_finite()) {
                return;
            }
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
            any = true;
        };
        for pr in &prims {
            match pr {
                Prim::Path { path, .. } => {
                    eat(path.start);
                    for p in sample_path(path, 0.05) {
                        eat(p);
                    }
                }
                Prim::Fill { outer, holes, .. } => {
                    for l in std::iter::once(outer).chain(holes) {
                        for p in l.sample_tolerance(0.05) {
                            eat(p);
                        }
                    }
                }
                Prim::Text {
                    at,
                    height_mm,
                    text,
                    anchor,
                    font,
                    ..
                } => {
                    let w = crate::text::width_in(text, *height_mm, font);
                    let x0 = match anchor.h {
                        HAlign::Left => at.x,
                        HAlign::Center => at.x - w / 2.0,
                        HAlign::Right => at.x - w,
                    };
                    eat(P2::new(x0, at.y - height_mm));
                    eat(P2::new(x0 + w, at.y + height_mm));
                }
                Prim::Image { at, w, h, .. } => {
                    eat(*at);
                    eat(P2::new(at.x + w, at.y + h));
                }
            }
        }
        if any {
            Some((lo, hi))
        } else {
            None
        }
    }

    /// Walk the item tree into flat primitives in paper millimetres.
    ///
    /// This is the one place group transforms are applied, dimensions are
    /// exploded into lines, arrows and text, and hatch patterns become line
    /// work. Every writer starts here, which is why SVG, PDF and DXF cannot
    /// drift apart.
    pub fn flatten(&self) -> Vec<Prim> {
        let mut out = Vec::new();
        let ctx = FlattenCtx {
            layers: &self.layers,
        };
        for item in &self.items {
            flatten_item(item, Xform2::IDENTITY, &ctx, &mut out);
        }
        if let Some(sheet) = &self.sheet {
            for item in &sheet.items {
                flatten_item(item, Xform2::IDENTITY, &ctx, &mut out);
            }
        }
        out
    }

    /// The layers that actually carry ink, in declaration order. Writers group
    /// their output by this so a DXF opens with the expected layer list.
    pub fn used_layers(&self) -> Vec<&Layer> {
        self.layers.iter().filter(|l| l.print).collect()
    }
}

/// A resolved style: no more `Option`s, no more layer lookups.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub layer: String,
    pub weight_mm: f64,
    pub color: Rgb,
    pub dash: Dash,
}

/// What a writer sees. Geometry in paper millimetres, ink sizes in paper
/// millimetres, arcs still arcs.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Path {
        style: Resolved,
        name: String,
        path: Path,
        closed: bool,
    },
    Fill {
        style: Resolved,
        name: String,
        outer: Loop,
        holes: Vec<Loop>,
    },
    Text {
        style: Resolved,
        at: P2,
        text: String,
        height_mm: f64,
        rotation_deg: f64,
        anchor: Anchor,
        font: FontRef,
    },
    /// A raster picture, lower-left corner at `at`, `w` by `h` paper mm.
    Image {
        style: Resolved,
        at: P2,
        w: f64,
        h: f64,
        image: Arc<Image>,
    },
}

impl Prim {
    /// The resolved style, whatever the primitive.
    pub fn style(&self) -> &Resolved {
        match self {
            Prim::Path { style, .. }
            | Prim::Fill { style, .. }
            | Prim::Text { style, .. }
            | Prim::Image { style, .. } => style,
        }
    }
}

pub(crate) struct FlattenCtx<'a> {
    pub layers: &'a [Layer],
}

impl FlattenCtx<'_> {
    pub(crate) fn resolve(&self, style: &Style) -> Resolved {
        let l = self.layers.get(style.layer);
        Resolved {
            layer: l.map(|l| l.name.clone()).unwrap_or_else(|| "0".to_owned()),
            weight_mm: style
                .weight_mm
                .or_else(|| l.map(|l| l.weight_mm))
                .unwrap_or(0.25),
            color: style
                .color
                .or_else(|| l.map(|l| l.color))
                .unwrap_or(Rgb::BLACK),
            dash: style
                .dash
                .clone()
                .or_else(|| l.map(|l| l.dash.clone()))
                .unwrap_or(Dash::Solid),
        }
    }
}

pub(crate) fn flatten_item(item: &Item, x: Xform2, ctx: &FlattenCtx<'_>, out: &mut Vec<Prim>) {
    // `print: false` means construction geometry: it exists, it is selectable,
    // it is never plotted. Enforcing it here rather than in each writer is
    // what makes that true of all three of them.
    let plotted = |style: &Style| ctx.layers.get(style.layer).map(|l| l.print).unwrap_or(true);
    match item {
        Item::Stroke(s) if !plotted(&s.style) => {}
        Item::Hatch(h) if !plotted(&h.style) => {}
        Item::Text(t) if !plotted(&t.style) => {}
        Item::Dim(d) if !plotted(&d.style) => {}
        Item::Leader(l) if !plotted(&l.style) => {}
        Item::Image(i) if !plotted(&i.style) => {}
        Item::Image(i) => out.push(Prim::Image {
            style: ctx.resolve(&i.style),
            at: x.apply(i.at),
            w: i.w * x.scale,
            h: i.h * x.scale,
            image: i.image.clone(),
        }),
        Item::Stroke(s) => out.push(Prim::Path {
            style: ctx.resolve(&s.style),
            name: s.name.clone(),
            path: xform_path(&s.path, x),
            closed: s.closed,
        }),
        Item::Hatch(h) => {
            let style = ctx.resolve(&h.style);
            let outer = xform_loop(&h.outer, x);
            let holes: Vec<Loop> = h.holes.iter().map(|l| xform_loop(l, x)).collect();
            match &h.pattern {
                Pattern::Solid => out.push(Prim::Fill {
                    style,
                    name: h.name.clone(),
                    outer,
                    holes,
                }),
                Pattern::Dots { spacing_mm, radius_mm } => {
                    for c in crate::hatch::dots(&outer, &holes, *spacing_mm, h.scale) {
                        out.push(Prim::Fill {
                            style: style.clone(),
                            name: String::new(),
                            outer: Loop::circle(c, radius_mm.max(0.01)),
                            holes: Vec::new(),
                        });
                    }
                }
                p => {
                    for seg in crate::hatch::lines(&outer, &holes, p, h.scale) {
                        out.push(Prim::Path {
                            style: style.clone(),
                            name: String::new(),
                            path: Path::new(seg.0).line_to(seg.1),
                            closed: false,
                        });
                    }
                }
            }
        }
        Item::Text(t) => out.push(Prim::Text {
            style: ctx.resolve(&t.style),
            at: x.apply(t.at),
            text: t.text.clone(),
            height_mm: if t.model_height { t.height_mm * x.scale } else { t.height_mm },
            rotation_deg: t.rotation_deg,
            anchor: t.anchor,
            font: t.font.clone(),
        }),
        Item::Dim(d) => d.flatten(x, ctx, out),
        Item::Leader(l) => {
            if l.points.len() < 2 {
                return;
            }
            let style = ctx.resolve(&l.style);
            let pts: Vec<P2> = l.points.iter().map(|p| x.apply(*p)).collect();
            let mut path = Path::new(pts[0]);
            for p in &pts[1..] {
                path = path.line_to(*p);
            }
            out.push(Prim::Path {
                style: style.clone(),
                name: String::new(),
                path,
                closed: false,
            });
            let tip = pts[0];
            let dir = P2::new(pts[1].x - tip.x, pts[1].y - tip.y);
            for tri in
                crate::dim::arrow_head(tip, dir, l.height_mm * 0.8, &crate::dim::Arrow::Filled)
            {
                out.push(tri.into_prim(style.clone()));
            }
            let last = *pts.last().unwrap();
            let prev = pts[pts.len() - 2];
            let anchor = if last.x >= prev.x {
                Anchor::LEFT
            } else {
                Anchor::RIGHT
            };
            let pad = if last.x >= prev.x {
                l.height_mm * 0.5
            } else {
                -l.height_mm * 0.5
            };
            out.push(Prim::Text {
                style,
                at: P2::new(last.x + pad, last.y),
                text: l.text.clone(),
                height_mm: l.height_mm,
                rotation_deg: 0.0,
                anchor,
                font: l.font.clone(),
            });
        }
        Item::Group(g) => {
            let inner = g.transform.then(&x);
            match &g.clip {
                None => {
                    for it in &g.items {
                        flatten_item(it, inner, ctx, out);
                    }
                }
                Some(clip) => {
                    // The viewport is in the group's parent space; `x` maps
                    // that space to the paper, and a rectangle stays one.
                    let rect = clip.map(x);
                    let mut mine = Vec::new();
                    for it in &g.items {
                        flatten_item(it, inner, ctx, &mut mine);
                    }
                    for p in mine {
                        clip_prim(p, &rect, out);
                    }
                }
            }
        }
    }
}

/// Chord tolerance used when a clipped path has to be flattened. A path that
/// lies wholly inside its viewport is kept exactly, arcs and all.
const CLIP_TOLERANCE: f64 = 0.02;

/// Keep the part of `p` inside `r`, in paper millimetres.
fn clip_prim(p: Prim, r: &ClipRect, out: &mut Vec<Prim>) {
    match p {
        Prim::Path {
            style,
            name,
            path,
            closed,
        } => {
            let mut pts = sample_path(&path, CLIP_TOLERANCE);
            if pts.iter().all(|q| r.contains(*q)) {
                out.push(Prim::Path {
                    style,
                    name,
                    path,
                    closed,
                });
                return;
            }
            if closed && pts.len() > 1 {
                pts.push(pts[0]);
            }
            let mut run: Vec<P2> = Vec::new();
            let flush = |run: &mut Vec<P2>, out: &mut Vec<Prim>| {
                if run.len() >= 2 {
                    let mut path = Path::new(run[0]);
                    for q in &run[1..] {
                        path = path.line_to(*q);
                    }
                    out.push(Prim::Path {
                        style: style.clone(),
                        name: name.clone(),
                        path,
                        closed: false,
                    });
                }
                run.clear();
            };
            for w in pts.windows(2) {
                match clip_segment(w[0], w[1], r) {
                    Some((a, b)) => {
                        let joins = run
                            .last()
                            .map(|l| (l.x - a.x).abs() < 1e-9 && (l.y - a.y).abs() < 1e-9)
                            .unwrap_or(false);
                        if !joins {
                            flush(&mut run, out);
                            run.push(a);
                        }
                        run.push(b);
                    }
                    None => flush(&mut run, out),
                }
            }
            flush(&mut run, out);
        }
        Prim::Fill {
            style,
            name,
            outer,
            holes,
        } => {
            let inside = |l: &Loop| l.sample_tolerance(CLIP_TOLERANCE).iter().all(|q| r.contains(*q));
            if inside(&outer) && holes.iter().all(inside) {
                out.push(Prim::Fill {
                    style,
                    name,
                    outer,
                    holes,
                });
                return;
            }
            // A hole that crosses the viewport edge cannot be expressed as a
            // hole of the clipped outer in general; each loop is clipped on
            // its own and the even-odd fill sorts out the rest.
            let clipped: Vec<Loop> = std::iter::once(&outer)
                .chain(&holes)
                .filter_map(|l| {
                    let poly = clip_polygon(&l.sample_tolerance(CLIP_TOLERANCE), r);
                    (poly.len() >= 3).then(|| Loop::polygon(&poly))
                })
                .collect();
            if let Some((first, rest)) = clipped.split_first() {
                out.push(Prim::Fill {
                    style,
                    name,
                    outer: first.clone(),
                    holes: rest.to_vec(),
                });
            }
        }
        Prim::Text { ref at, .. } => {
            if r.contains(*at) {
                out.push(p);
            }
        }
        Prim::Image { at, w, h, .. } => {
            if r.contains(P2::new(at.x + w / 2.0, at.y + h / 2.0)) {
                out.push(p);
            }
        }
    }
}

/// Liang–Barsky: the part of `a`–`b` inside `r`, or `None`.
pub fn clip_segment(a: P2, b: P2, r: &ClipRect) -> Option<(P2, P2)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.x - r.lo.x),
        (dx, r.hi.x - a.x),
        (-dy, a.y - r.lo.y),
        (dy, r.hi.y - a.y),
    ] {
        if p.abs() < 1e-15 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                if t > t0 {
                    t0 = t;
                }
            } else {
                if t < t0 {
                    return None;
                }
                if t < t1 {
                    t1 = t;
                }
            }
        }
    }
    if t1 - t0 < 1e-12 {
        return None;
    }
    Some((
        P2::new(a.x + dx * t0, a.y + dy * t0),
        P2::new(a.x + dx * t1, a.y + dy * t1),
    ))
}

/// Sutherland–Hodgman against the four edges of `r`.
pub fn clip_polygon(pts: &[P2], r: &ClipRect) -> Vec<P2> {
    let mut poly: Vec<P2> = pts.to_vec();
    for edge in 0..4 {
        if poly.is_empty() {
            break;
        }
        let inside = |p: P2| match edge {
            0 => p.x >= r.lo.x,
            1 => p.x <= r.hi.x,
            2 => p.y >= r.lo.y,
            _ => p.y <= r.hi.y,
        };
        let cross = |p: P2, q: P2| -> P2 {
            match edge {
                0 | 1 => {
                    let x = if edge == 0 { r.lo.x } else { r.hi.x };
                    let t = (x - p.x) / (q.x - p.x);
                    P2::new(x, p.y + (q.y - p.y) * t)
                }
                _ => {
                    let y = if edge == 2 { r.lo.y } else { r.hi.y };
                    let t = (y - p.y) / (q.y - p.y);
                    P2::new(p.x + (q.x - p.x) * t, y)
                }
            }
        };
        let mut next = Vec::with_capacity(poly.len() + 4);
        for i in 0..poly.len() {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            match (inside(p), inside(q)) {
                (true, true) => next.push(q),
                (true, false) => next.push(cross(p, q)),
                (false, true) => {
                    next.push(cross(p, q));
                    next.push(q);
                }
                (false, false) => {}
            }
        }
        poly = next;
    }
    poly.dedup_by(|a, b| (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12);
    poly
}

pub(crate) fn xform_path(p: &Path, x: Xform2) -> Path {
    Path {
        start: x.apply(p.start),
        segments: p.segments.iter().map(|s| xform_segment(s, x)).collect(),
    }
}

pub(crate) fn xform_loop(l: &Loop, x: Xform2) -> Loop {
    Loop {
        segments: l.segments.iter().map(|s| xform_segment(s, x)).collect(),
    }
}

pub(crate) fn xform_segment(s: &Segment, x: Xform2) -> Segment {
    xform_segment_with(s, |p| x.apply(p))
}

/// Map every point of a segment. Arcs stay arcs under any similarity, which
/// includes the mirror the SVG writer applies for y-down paper coordinates.
pub fn xform_segment_with(s: &Segment, f: impl Fn(P2) -> P2) -> Segment {
    match s {
        Segment::Line { a, b } => Segment::Line { a: f(*a), b: f(*b) },
        Segment::Arc { a, b, via } => Segment::Arc {
            a: f(*a),
            b: f(*b),
            via: f(*via),
        },
        Segment::Bezier { a, b, ctrl } => Segment::Bezier {
            a: f(*a),
            b: f(*b),
            ctrl: ctrl.iter().map(|c| f(*c)).collect(),
        },
    }
}

/// Sample an open path as a polyline, arcs and Béziers subdivided.
pub fn sample_path(p: &Path, tolerance: f64) -> Vec<P2> {
    if p.segments.is_empty() {
        return vec![p.start];
    }
    let l = Loop {
        segments: p.segments.clone(),
    };
    let mut pts = l.sample_tolerance(tolerance);
    // `Loop::sample_tolerance` samples each segment from its start and lets the
    // ring close itself, so an open path is missing its far end — which for a
    // two-point line is *half the geometry*, and a bounding box computed from
    // it is wrong rather than merely coarse.
    let end = p.segments.last().map(|s| s.end()).unwrap_or(p.start);
    match pts.last() {
        Some(q) if (q.x - end.x).abs() < 1e-12 && (q.y - end.y).abs() < 1e-12 => {}
        _ => pts.push(end),
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_created_once_with_their_standard_weight() {
        let mut d = Drawing::new();
        let a = d.layer(layer::WALL);
        let b = d.layer(layer::WALL);
        assert_eq!(a, b);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].weight_mm, 0.7);
        let h = d.layer(layer::HIDDEN);
        assert!(!d.layers[h].dash.is_solid());
    }

    #[test]
    fn group_transforms_compose_and_keep_arcs_circular() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        let mut inner = Group {
            transform: Xform2::new(2.0, 1.0, 0.0),
            ..Group::default()
        };
        inner.items.push(Item::Stroke(Stroke {
            style: Style::on(l),
            name: String::new(),
            path: Path::new(P2::new(0.0, 0.0)).arc_to(P2::new(2.0, 0.0), P2::new(1.0, 1.0)),
            closed: false,
        }));
        let outer = Group {
            transform: Xform2::new(3.0, 0.0, 5.0),
            items: vec![Item::Group(inner)],
            ..Group::default()
        };
        d.push(Item::Group(outer));
        let prims = d.flatten();
        assert_eq!(prims.len(), 1);
        let Prim::Path { path, .. } = &prims[0] else {
            panic!()
        };
        // 2 then 3 is scale 6, and the inner translate is scaled by the outer.
        assert_eq!(path.start, P2::new(3.0, 5.0));
        let Segment::Arc { b, via, .. } = &path.segments[0] else {
            panic!("arc stayed an arc")
        };
        assert_eq!(*b, P2::new(15.0, 5.0));
        assert_eq!(*via, P2::new(9.0, 11.0));
    }

    #[test]
    fn scale_parses_and_labels_both_conventions() {
        assert_eq!(Scale::parse("1:48").unwrap().ratio(), 1.0 / 48.0);
        assert_eq!(Scale::parse("2:1").unwrap().ratio(), 2.0);
        assert!(Scale::parse("1:0").is_none());
        assert_eq!(
            Scale::QUARTER_INCH.label(&UnitFormat::ft_in()),
            "1/4\" = 1'-0\""
        );
        assert_eq!(Scale::QUARTER_INCH.label(&UnitFormat::mm()), "1:48");
        assert_eq!(Scale::new(1.0, 2.0).label(&UnitFormat::mm()), "1:2");
        // Every architectural scale a set uses reads the architect's way.
        for text in ["3/16\" = 1'-0\"", "3/32\" = 1'-0\"", "1 1/2\" = 1'-0\"", "1/32\" = 1'-0\""] {
            let s = Scale::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(s.label(&UnitFormat::ft_in()), text);
        }
        // 1/4" = 1'-0" written out longhand is the same scale.
        let long = Scale::parse("1/4\" = 1'-0\"").unwrap();
        assert!((long.ratio() - Scale::QUARTER_INCH.ratio()).abs() < 1e-12);
    }

    #[test]
    fn a_scale_must_be_a_real_ratio() {
        // Rust's float parser takes all of these; a drawing must not.
        for bad in [
            "inf", "-inf", "nan", "0", "-1", "1:nan", "nan:1", "1:0", "0:1", "-1:2", "inf:1",
        ] {
            assert!(Scale::parse(bad).is_none(), "{bad:?} is not a scale");
        }
        for good in ["1:48", "2:1", "0.5", "1:2.5"] {
            let s = Scale::parse(good).unwrap_or_else(|| panic!("{good}"));
            assert!(s.ratio().is_finite() && s.ratio() > 0.0);
        }
        // And a hand-built one cannot poison the arithmetic either.
        assert_eq!(Scale::new(1.0, 0.0).ratio(), 1.0);
        assert_eq!(Scale::new(f64::NAN, 1.0).ratio(), 1.0);
        assert_eq!(Scale::new(-2.0, 1.0).ratio(), 1.0);
    }

    #[test]
    fn a_layer_that_is_not_plotted_reaches_no_writer() {
        let mut d = Drawing::new();
        let vis = d.layer(layer::VISIBLE);
        let hidden = d.layer("X-CONSTRUCTION");
        d.layers[hidden].print = false;
        d.polyline(vis, &[P2::new(0.0, 0.0), P2::new(10.0, 0.0)], false);
        d.polyline(hidden, &[P2::new(0.0, 5.0), P2::new(10.0, 5.0)], false);
        d.text(hidden, P2::new(0.0, 9.0), "note", 2.5, Anchor::LEFT);
        let prims = d.flatten();
        assert_eq!(prims.len(), 1, "only the plotted layer");
        assert_eq!(style_layer(&prims[0]), layer::VISIBLE);
    }

    fn style_layer(p: &Prim) -> &str {
        p.style().layer.as_str()
    }

    #[test]
    fn an_open_path_samples_its_far_end() {
        let p = Path::new(P2::new(0.0, 0.0)).line_to(P2::new(10.0, 4.0));
        let pts = sample_path(&p, 0.05);
        assert_eq!(pts.first().copied(), Some(P2::new(0.0, 0.0)));
        assert_eq!(pts.last().copied(), Some(P2::new(10.0, 4.0)));
    }

    #[test]
    fn bounds_ignores_points_that_are_not_finite() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.polyline(
            l,
            &[P2::new(f64::NAN, 0.0), P2::new(f64::INFINITY, 1.0)],
            false,
        );
        assert!(d.bounds().is_none(), "nothing real was drawn");
        d.polyline(l, &[P2::new(0.0, 0.0), P2::new(10.0, 4.0)], false);
        let (lo, hi) = d.bounds().unwrap();
        assert_eq!((lo.x, lo.y, hi.x, hi.y), (0.0, 0.0, 10.0, 4.0));
    }

    #[test]
    fn colours_map_to_the_nearest_autocad_index() {
        assert_eq!(Rgb(255, 0, 0).aci(), 1);
        assert_eq!(Rgb::BLACK.aci(), 7);
        assert_eq!(Rgb(250, 250, 10).hex(), "#fafa0a");
    }
}
