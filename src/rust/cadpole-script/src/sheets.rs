//! Sheet sets in Starlark: `sheet_set(...)`, its `Sheet`s, the
//! views a sheet places and the annotation it carries, `export(set, path)`,
//! and the two things a cover sheet needs from the rest of the document —
//! `report()` and `sweep_table()`.
//!
//! A `SheetSet` value is a handle into the store's table, like a
//! `ConstrainedSketch`: placing a view on a sheet is stateful, and a value
//! that copied the whole set on every call would make a chain of calls
//! quadratic. Every method draws in **paper millimetres**, x
//! to the right and y up from the sheet's lower-left corner; a view's own
//! methods draw in the view's model frame and go through its transform.
//!
//! Nothing here touches the file system: `export(set, "x.pdf")` records the
//! request and the host writes the pages under `--run-exports`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use allocative::Allocative;
use cadpole_core::ParamValue;
use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::{Loop, P2, Path};
use cadpole_drawing::ir::{Anchor, ClipRect, Dash, FontRef, Group, HAlign, Item, Pattern, Rgb, VAlign, Xform2};
use cadpole_drawing::sheetset as ss;
use cadpole_drawing::views::{self, ViewBody, ViewOptions};
use cadpole_drawing::{Hidden, Scale, SheetSet, SheetSize, StdFont, Swatch, UnitFormat};
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::dict::{AllocDict, DictRef};
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::none::{NoneOr, NoneType};
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value, ValueLike};
use starlark_derive::starlark_value;

use crate::check::{VecArg, num, opt_num, p2};
use crate::store::{call_span, store};
use crate::values::{KAsset, KBody, KPlane, KSketch};

// ---------------------------------------------------------------------------
// What the host reads out of an evaluation
// ---------------------------------------------------------------------------

/// `export(set, path)` or `export(sheet, path)`: which set, which of its
/// sheets (all of them for a set), where, and in what format.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetExport {
    /// Index into [`crate::Evaluation::sheet_sets`].
    pub set: usize,
    /// One sheet of the set, for `export(sheet, path)`; `None` is the set.
    pub sheet: Option<usize>,
    pub path: String,
    /// `pdf`, `svg` or `dxf`, from the path's extension.
    pub format: String,
}

/// The files the script's `export(set, ...)` / `export(sheet, ...)` calls
/// ask for: one PDF of every page of a set, one SVG or DXF per
/// sheet named `<stem>_<number>.<ext>`, or one sheet in any of the three.
/// The host calls this under `--run-exports` and the `Export` rpc; an
/// ordinary evaluation records the request and writes nothing, so a
/// viewer's every keystroke does not deflate a set of ARCH D pages.
pub fn sheet_files(sets: &[SheetSet], exports: &[SheetExport]) -> Result<Vec<crate::OutputFile>, String> {
    let mut out = Vec::new();
    for e in exports {
        let set = sets.get(e.set).ok_or_else(|| format!("export: sheet set #{} does not exist", e.set))?;
        if set.sheets.is_empty() {
            return Err(format!(
                "export: sheet set {:?} has no sheets to write to {:?}; add them with set.sheet(number, name)",
                set.name, e.path
            ));
        }
        let files = cadpole_drawing_out::write_set(set, &e.path, &e.format, false, e.sheet)?;
        for (path, data) in files {
            out.push(crate::OutputFile { path, format: e.format.clone(), data });
        }
    }
    Ok(out)
}

/// `export(set_or_sheet, path)`: recorded when `target` is a
/// `SheetSet` or a `Sheet`, `None` when it is anything else, which
/// `export()` then treats as bodies. `given` names the body-only keywords
/// the call passed, which a sheet has no use for.
pub(crate) fn record_export<'v>(
    target: Value<'v>,
    path: &str,
    given: &[(&str, bool)],
    eval: &mut Evaluator<'v, '_, '_>,
) -> anyhow::Result<Option<NoneType>> {
    let (set, sheet) = if let Some(s) = target.downcast_ref::<KSheetSet>() {
        (s.index, None)
    } else if let Some(s) = target.downcast_ref::<KSheet>() {
        (s.set, Some(s.index))
    } else {
        return Ok(None);
    };
    if let Some((k, _)) = given.iter().find(|(_, on)| *on) {
        anyhow::bail!("export: {k}= is for bodies; a sheet set is written as it is drawn");
    }
    let format = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(format.as_str(), "pdf" | "svg" | "dxf") || !path.contains('.') {
        anyhow::bail!(
            "export: a sheet set is written as .pdf (every sheet, one page each), .svg or .dxf (one file per sheet); got {path:?}"
        );
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if file_name.rsplit_once('.').map(|(stem, _)| stem.is_empty()).unwrap_or(true) {
        anyhow::bail!("export: {path:?} has no file name in front of the extension");
    }
    let st = store(eval)?;
    if st.in_test.get() {
        anyhow::bail!("export: a test cannot ask for files");
    }
    if st.sheet_sets.borrow().get(set).is_none() {
        anyhow::bail!("export: sheet set #{set} does not exist");
    }
    st.sheet_exports.borrow_mut().push(SheetExport { set, sheet, path: path.to_owned(), format });
    Ok(Some(NoneType))
}

/// One `sweep_table(...)` call: the configurations to evaluate the document
/// at, each a set of parameter overrides, with a label for each.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepRequest {
    pub points: Vec<Vec<(String, ParamValue)>>,
    pub labels: Vec<String>,
}

/// One row a host hands back: the point, whether the model built there,
/// and every `report(name, value)` it made.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepRow {
    pub point: Vec<(String, ParamValue)>,
    pub label: String,
    pub ok: bool,
    pub error: String,
    pub values: Vec<(String, ParamValue)>,
}

/// The document key a `load(path)` written in the file `from` names: a
/// library label as it is, otherwise `path` joined onto `from`'s directory
/// with `.` and `..` folded. The entry file's directory is
/// the document root, so `load("lib.star")` from `main.star` is `lib.star`
/// and `load("helpers.star")` from `../lib/sheets.star` is
/// `../lib/helpers.star`. The CLI bundles files under the same keys.
pub fn resolve_load(from: &str, path: &str) -> String {
    if path.starts_with('@') {
        return path.to_owned();
    }
    let path = path.trim_start_matches("//");
    let base = match from.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    };
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A set of sheets: a handle into the store's table.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSheetSet {
    pub index: usize,
}
starlark_simple_value!(KSheetSet);

impl fmt::Display for KSheetSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sheet_set(#{})", self.index)
    }
}

/// One sheet of a set.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSheet {
    pub set: usize,
    pub index: usize,
}
starlark_simple_value!(KSheet);

impl fmt::Display for KSheet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sheet(#{}.{})", self.set, self.index)
    }
}

/// A view placed on a sheet: the group it draws into and the map from its
/// model frame to the paper.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSheetView {
    pub set: usize,
    pub sheet: usize,
    /// Index of the view's group in the sheet drawing's items.
    pub group: usize,
    pub scale: f64,
    pub tx: f64,
    pub ty: f64,
    pub clip: (f64, f64, f64, f64),
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// Model space to the view's frame, so a method may take a 3-D model
    /// point; `None` for a view with no projection behind it.
    #[allocative(skip)]
    pub frame: Option<cadpole_drawing::Frame>,
}
starlark_simple_value!(KSheetView);

impl fmt::Display for KSheetView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "view(#{}.{}, scale {})", self.set, self.sheet, self.scale)
    }
}

impl KSheetView {
    fn xform(&self) -> Xform2 {
        Xform2::new(self.scale, self.tx, self.ty)
    }
    fn to_paper(&self, p: P2) -> P2 {
        self.xform().apply(p)
    }
    /// A point of the view's frame from `(x, y)` (frame coordinates, as
    /// every view method takes) or `(x, y, z)` (a model point, projected
    /// the way the view projects the model: on an axonometric, the corner
    /// of a body lands on the drawn corner).
    fn frame_point(&self, v: &VecArg, what: &str) -> anyhow::Result<P2> {
        if v.items.len() != 3 {
            return p2(v, what);
        }
        let p = crate::check::vec3(v, what)?;
        match &self.frame {
            Some(f) => Ok(f.to_frame(p)),
            None => anyhow::bail!("{what}: a 3-D model point needs a view that projects the model; give (x, y) in the view's frame"),
        }
    }
    fn frame_points(&self, v: &UnpackList<VecArg>, what: &str) -> anyhow::Result<Vec<P2>> {
        v.items.iter().map(|p| self.frame_point(p, what)).collect()
    }
}

/// What `title_block(...)` makes: the column every sheet of a set carries.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KTitleBlock(#[allocative(skip)] pub cadpole_drawing::TitleTemplate);
starlark_simple_value!(KTitleBlock);

impl fmt::Display for KTitleBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "title_block({:?})", self.0.project)
    }
}

#[starlark_value(type = "TitleBlock")]
impl<'v> StarlarkValue<'v> for KTitleBlock {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let t = &self.0;
        Some(match attribute {
            "project" => heap.alloc(t.project.as_str()),
            "address" => heap.alloc(t.address.as_str()),
            "block_lot" => heap.alloc(t.block_lot.as_str()),
            "issue" => heap.alloc(t.issue.as_str()),
            "date" => heap.alloc(t.date.as_str()),
            "drawn" => heap.alloc(t.drawn.as_str()),
            "checked" => heap.alloc(t.checked.as_str()),
            "stamp" => crate::convert::alloc_str_list(heap, &t.stamp),
            "notes" => crate::convert::alloc_str_list(heap, &t.notes),
            "column" => heap.alloc(t.column_mm),
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["project", "address", "block_lot", "issue", "date", "drawn", "checked", "stamp", "notes", "column"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// The fields `title_block()` takes, for the dict form's diagnostic.
const TITLE_FIELDS: &str = "project, address, block_lot, stamp, notes, issue, date, drawn, checked, extra, column";

/// A string, or a list of strings, as lines.
fn lines_arg<'v>(v: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<String>> {
    if let Some(s) = v.unpack_str() {
        return Ok(s.split('\n').map(str::to_owned).collect());
    }
    let mut out = Vec::new();
    for item in v.iterate(heap).map_err(|_| anyhow::anyhow!("{what} must be a string or a list of strings"))? {
        out.push(item.unpack_str().map(str::to_owned).ok_or_else(|| anyhow::anyhow!("{what} must be a string or a list of strings"))?);
    }
    Ok(out)
}

/// `[(label, value), ...]`.
fn pairs_arg<'v>(v: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for item in v.iterate(heap).map_err(|_| anyhow::anyhow!("{what} is a list of (label, value)"))? {
        let pair: Vec<Value<'v>> = item.iterate(heap).map_err(|_| anyhow::anyhow!("{what} rows are (label, value)"))?.collect();
        let [k, val] = pair.as_slice() else { anyhow::bail!("{what} rows are (label, value)") };
        let text = |x: &Value<'v>| x.unpack_str().map(str::to_owned).unwrap_or_else(|| x.to_str());
        out.push((text(k), text(val)));
    }
    Ok(out)
}

/// A title block from a dict of `title_block()`'s fields.
fn title_from_dict<'v>(v: Value<'v>, heap: Heap<'v>) -> anyhow::Result<cadpole_drawing::TitleTemplate> {
    let d = DictRef::from_value(v)
        .ok_or_else(|| anyhow::anyhow!("sheet_set: title_block= is a title_block(...) or a dict of its fields, got {}", v.get_type()))?;
    let mut t = cadpole_drawing::TitleTemplate::default();
    for (k, val) in d.iter() {
        let Some(k) = k.unpack_str() else { anyhow::bail!("sheet_set: title_block keys are strings") };
        let what = format!("sheet_set: title_block[{k:?}]");
        let text = || val.unpack_str().map(str::to_owned).ok_or_else(|| anyhow::anyhow!("{what} must be a string"));
        match k {
            "project" => t.project = text()?,
            "address" => t.address = text()?,
            "block_lot" => t.block_lot = text()?,
            "stamp" => t.stamp = lines_arg(val, heap, &what)?,
            "notes" => t.notes = lines_arg(val, heap, &what)?,
            "issue" => t.issue = text()?,
            "date" => t.date = text()?,
            "drawn" => t.drawn = text()?,
            "checked" => t.checked = text()?,
            "extra" => t.extra = pairs_arg(val, heap, &what)?,
            "column" => {
                let n = UnpackFloat::unpack_value(val)
                    .map_err(|e| e.into_anyhow())?
                    .ok_or_else(|| anyhow::anyhow!("{what} is a width in mm"))?;
                t.column_mm = num(&n, &what)?;
            }
            other => anyhow::bail!("sheet_set: title_block has no field {other:?} ({TITLE_FIELDS})"),
        }
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Store access
// ---------------------------------------------------------------------------

fn with_set<R>(eval: &Evaluator<'_, '_, '_>, index: usize, f: impl FnOnce(&mut SheetSet) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let st = store(eval)?;
    let mut sets = st.sheet_sets.borrow_mut();
    let set = sets.get_mut(index).ok_or_else(|| anyhow::anyhow!("sheet set #{index} does not exist"))?;
    f(set)
}

fn with_sheet<R>(
    eval: &Evaluator<'_, '_, '_>,
    sheet: &KSheet,
    f: impl FnOnce(&SheetSet, &mut cadpole_drawing::Drawing) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let st = store(eval)?;
    let mut sets = st.sheet_sets.borrow_mut();
    let set = sets.get_mut(sheet.set).ok_or_else(|| anyhow::anyhow!("sheet set #{} does not exist", sheet.set))?;
    let mut page = std::mem::take(
        &mut set.sheets.get_mut(sheet.index).ok_or_else(|| anyhow::anyhow!("sheet #{} does not exist", sheet.index))?.drawing,
    );
    let r = f(set, &mut page);
    set.sheets[sheet.index].drawing = page;
    r
}

/// The group a view draws into, for a method on the view.
fn with_view<R>(
    eval: &Evaluator<'_, '_, '_>,
    v: &KSheetView,
    f: impl FnOnce(&SheetSet, &mut cadpole_drawing::Drawing, &mut Group) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let sheet = KSheet { set: v.set, index: v.sheet };
    with_sheet(eval, &sheet, |set, d| {
        let mut item = std::mem::replace(&mut d.items[v.group], Item::Group(Group::default()));
        let r = match &mut item {
            Item::Group(g) => f(set, d, g),
            _ => Err(anyhow::anyhow!("the view's group is gone")),
        };
        d.items[v.group] = item;
        r
    })
}

// ---------------------------------------------------------------------------
// Argument readers
// ---------------------------------------------------------------------------

fn points_arg(v: &UnpackList<VecArg>, what: &str) -> anyhow::Result<Vec<P2>> {
    v.items.iter().map(|p| p2(p, what)).collect()
}

/// `(x0, y0, x1, y1)`.
fn rect_arg(v: &VecArg, what: &str) -> anyhow::Result<ClipRect> {
    match v.items.as_slice() {
        [x0, y0, x1, y1] => {
            for (n, w) in [(x0, "x0"), (y0, "y0"), (x1, "x1"), (y1, "y1")] {
                crate::check::finite(n.0, &format!("{what}.{w}"))?;
            }
            Ok(ClipRect::new(P2::new(x0.0, y0.0), P2::new(x1.0, y1.0)))
        }
        other => anyhow::bail!("{what} must be (x0, y0, x1, y1), got {} numbers", other.len()),
    }
}

/// A colour: `"#rrggbb"`, a name, or `(r, g, b)` in 0..1.
fn color_arg(v: Option<Value<'_>>, what: &str) -> anyhow::Result<Option<Rgb>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(None) };
    if let Some(s) = v.unpack_str() {
        let named = match s.trim().to_ascii_lowercase().as_str() {
            "black" => Some(Rgb(0, 0, 0)),
            "white" => Some(Rgb(255, 255, 255)),
            "grey" | "gray" => Some(Rgb(128, 128, 128)),
            "lightgrey" | "lightgray" | "light" => Some(Rgb(200, 200, 200)),
            "darkgrey" | "darkgray" | "dark" => Some(Rgb(70, 70, 70)),
            "red" => Some(Rgb(200, 30, 30)),
            "blue" => Some(Rgb(30, 60, 200)),
            "green" => Some(Rgb(30, 140, 60)),
            _ => None,
        };
        if let Some(c) = named {
            return Ok(Some(c));
        }
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 {
            anyhow::bail!("{what}: a colour is \"#rrggbb\", a name (black, white, grey, lightgrey, darkgrey, red, blue, green) or (r, g, b) in 0..1, got {s:?}");
        }
        let c = u32::from_str_radix(h, 16).map_err(|_| anyhow::anyhow!("{what}: bad hex colour {s:?}"))?;
        return Ok(Some(Rgb(((c >> 16) & 255) as u8, ((c >> 8) & 255) as u8, (c & 255) as u8)));
    }
    let arg: VecArg = VecArg::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: a colour is \"#rrggbb\", a name or (r, g, b), got {}", v.get_type()))?;
    match arg.items.as_slice() {
        [r, g, b] => {
            let ch = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            Ok(Some(Rgb(ch(r.0), ch(g.0), ch(b.0))))
        }
        _ => anyhow::bail!("{what}: a colour tuple has 3 numbers"),
    }
}

/// A dash: a name from the drafting set, or a list of dash/gap lengths.
fn dash_arg<'v>(v: Option<Value<'v>>, heap: Heap<'v>, what: &str) -> anyhow::Result<Option<Dash>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(None) };
    if let Some(s) = v.unpack_str() {
        return Ok(Some(match s.trim().to_ascii_lowercase().as_str() {
            "solid" => Dash::Solid,
            "dashed" | "dash" => Dash::Dashed,
            "hidden" => Dash::Hidden,
            "center" | "centre" | "dashdot" => Dash::Center,
            "phantom" => Dash::Phantom,
            other => anyhow::bail!("{what}: dash must be solid, dashed, hidden, center or phantom, or a list of lengths, got {other:?}"),
        }));
    }
    let mut pattern = Vec::new();
    for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
        let n = UnpackFloat::unpack_value(item)
            .map_err(|e| e.into_anyhow())?
            .ok_or_else(|| anyhow::anyhow!("{what}: dash lengths are numbers"))?;
        pattern.push(num(&n, &format!("{what}: dash length"))?.abs());
    }
    Ok(Some(Dash::Custom(pattern)))
}

/// A hatch pattern by name, with its spacing and angle.
fn pattern_arg(name: Option<&str>, spacing: Option<f64>, angle: Option<f64>, what: &str) -> anyhow::Result<Pattern> {
    let name = name.unwrap_or("lines").trim().to_ascii_lowercase();
    Ok(match name.as_str() {
        "lines" | "line" => Pattern::Lines { angle_deg: angle.unwrap_or(45.0), spacing_mm: spacing.unwrap_or(2.5) },
        "cross" | "crosshatch" => Pattern::Cross { angle_deg: angle.unwrap_or(45.0), spacing_mm: spacing.unwrap_or(2.5) },
        "dots" | "dot" => Pattern::Dots { spacing_mm: spacing.unwrap_or(1.4), radius_mm: 0.18 },
        "solid" | "fill" => Pattern::Solid,
        "earth" => Pattern::Lines { angle_deg: angle.unwrap_or(-45.0), spacing_mm: spacing.unwrap_or(3.0) },
        "concrete" => Pattern::Cross { angle_deg: angle.unwrap_or(45.0), spacing_mm: spacing.unwrap_or(4.0) },
        "masonry" => Pattern::Lines { angle_deg: angle.unwrap_or(45.0), spacing_mm: spacing.unwrap_or(1.6) },
        other => anyhow::bail!("{what}: pattern must be lines, cross, dots, solid, earth, concrete or masonry, got {other:?}"),
    })
}

/// A region for a hatch or fill: a list of points, or a Sketch whose
/// loops nest into regions, in the caller's coordinates.
fn region_arg<'v>(v: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<(Loop, Vec<Loop>)>> {
    if let Some(s) = v.downcast_ref::<KSketch>() {
        let regions = s.sketch().regions().map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        return Ok(regions.into_iter().map(|r| (r.outer, r.holes)).collect());
    }
    let mut pts = Vec::new();
    for item in v.iterate(heap).map_err(|_| anyhow::anyhow!("{what}: a region is a list of (x, y) points or a Sketch"))? {
        let p: VecArg = VecArg::unpack_value(item)
            .map_err(|e| e.into_anyhow())?
            .ok_or_else(|| anyhow::anyhow!("{what}: a region's points are (x, y)"))?;
        pts.push(p2(&p, &format!("{what}: point"))?);
    }
    if pts.len() < 3 {
        anyhow::bail!("{what}: a region needs at least 3 points, got {}", pts.len());
    }
    Ok(vec![(Loop::polygon(&pts), Vec::new())])
}

fn anchor_arg(align: Option<&str>, valign: Option<&str>, what: &str) -> anyhow::Result<Anchor> {
    let h = match align.unwrap_or("left").trim().to_ascii_lowercase().as_str() {
        "left" => HAlign::Left,
        "center" | "centre" | "middle" => HAlign::Center,
        "right" => HAlign::Right,
        other => anyhow::bail!("{what}: align must be left, center or right, got {other:?}"),
    };
    let v = match valign.unwrap_or("baseline").trim().to_ascii_lowercase().as_str() {
        "baseline" | "alphabetic" => VAlign::Baseline,
        "middle" | "center" | "centre" => VAlign::Middle,
        "top" | "hanging" => VAlign::Top,
        "bottom" => VAlign::Bottom,
        other => anyhow::bail!("{what}: valign must be baseline, middle, top or bottom, got {other:?}"),
    };
    Ok(Anchor::new(h, v))
}

/// The face a call sets its text in: the set's, unless `font=` names one.
fn font_arg(set: &SheetSet, font: Option<&str>, bold: bool, italic: bool, what: &str) -> anyhow::Result<FontRef> {
    let base = match font.map(|f| f.trim().to_ascii_lowercase()) {
        None => set.font.clone(),
        Some(f) if f == "inter" || f == "builtin" => FontRef::Builtin,
        Some(f) => match StdFont::parse(&f, false, false) {
            Some(std) => FontRef::Standard(std),
            None => anyhow::bail!("{what}: font must be helvetica, courier or inter, got {f:?}"),
        },
    };
    Ok(match base {
        FontRef::Standard(f) => FontRef::Standard(StdFont::parse(f.family(), bold, italic).unwrap_or(f)),
        other => other,
    })
}

/// A scale: `"1:48"`, the architectural form, or a number of paper
/// millimetres per model millimetre.
fn scale_arg(v: Option<Value<'_>>, what: &str) -> anyhow::Result<Option<Scale>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(None) };
    if let Some(s) = v.unpack_str() {
        return Scale::parse(s).map(Some).ok_or_else(|| anyhow::anyhow!("{what}: scale {s:?} is not a ratio like \"1:48\" or 1/4\" = 1'-0\""));
    }
    let n = UnpackFloat::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: scale is a string like \"1:48\" or a number, got {}", v.get_type()))?;
    let r = num(&n, &format!("{what}: scale"))?;
    if r <= 0.0 {
        anyhow::bail!("{what}: scale must be positive, got {r}");
    }
    Ok(Some(if r < 1.0 { Scale::new(1.0, 1.0 / r) } else { Scale::new(r, 1.0) }))
}

fn hidden_arg(v: Option<&str>, default: Hidden, what: &str) -> anyhow::Result<Hidden> {
    match v {
        None => Ok(default),
        Some(s) => Hidden::parse(s).ok_or_else(|| anyhow::anyhow!("{what}: hidden must be \"dashed\", \"omit\" or \"solid\", got {s:?}")),
    }
}

/// The bodies a view draws, with their meshes, and each one's layer from
/// `layers=` (a dict of body name to layer, or one name for all).
fn view_bodies<'v>(bodies: Value<'v>, layers: Option<Value<'v>>, eval: &mut Evaluator<'v, '_, '_>, what: &str) -> anyhow::Result<Vec<ViewBody>> {
    let heap = eval.heap();
    let list: Vec<KBody> = crate::globals::collect_bodies(&[bodies], heap, what)?;
    if list.is_empty() {
        anyhow::bail!("{what}: no bodies to draw");
    }
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    let mut all: Option<String> = None;
    if let Some(v) = layers.filter(|v| !v.is_none()) {
        if let Some(s) = v.unpack_str() {
            all = Some(s.to_owned());
        } else if let Some(d) = DictRef::from_value(v) {
            for (k, val) in d.iter() {
                let (Some(k), Some(val)) = (k.unpack_str(), val.unpack_str()) else {
                    anyhow::bail!("{what}: layers= is a dict of body name to layer name");
                };
                by_name.insert(k.to_owned(), val.to_owned());
            }
        } else {
            anyhow::bail!("{what}: layers= is a layer name or a dict of body name to layer name, got {}", v.get_type());
        }
    }
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let mut out = Vec::new();
    for b in &list {
        let record = ctx.body(b.body_id())?;
        let name = record.name.clone();
        let feature = record.feature.0.clone();
        let (mesh, topo) = ctx.body_mesh(b.body_id()).map_err(|e| anyhow::anyhow!("{what}: {name}: {e}"))?.clone();
        let layer = by_name.get(&name).or_else(|| by_name.get(&feature)).cloned().or_else(|| all.clone());
        let (color, opacity, label) = cadpole_drawing::painted::paint_of(&ctx, ctx.body(b.body_id())?);
        out.push(ViewBody { name, mesh, topo, layer, color, opacity, label });
    }
    Ok(out)
}

/// Where a view lands: `at=` puts its model `origin=` at that paper point,
/// `box=` fits its bounds into that paper rectangle (and clips to it).
/// `crop=` is the part of the model the view shows, in its own frame.
struct Placement {
    at: Option<P2>,
    origin: P2,
    fit: Option<ClipRect>,
    clip: Option<ClipRect>,
    crop: Option<ClipRect>,
}

fn placement(
    at: Option<VecArg>,
    origin: Option<VecArg>,
    fit: Option<VecArg>,
    clip: Option<VecArg>,
    crop: Option<VecArg>,
    what: &str,
) -> anyhow::Result<Placement> {
    Ok(Placement {
        at: at.as_ref().map(|v| p2(v, &format!("{what}: at"))).transpose()?,
        origin: origin.as_ref().map(|v| p2(v, &format!("{what}: origin"))).transpose()?.unwrap_or(P2::new(0.0, 0.0)),
        fit: fit.as_ref().map(|v| rect_arg(v, &format!("{what}: box"))).transpose()?,
        clip: clip.as_ref().map(|v| rect_arg(v, &format!("{what}: clip"))).transpose()?,
        crop: crop.as_ref().map(|v| rect_arg(v, &format!("{what}: crop"))).transpose()?,
    })
}

/// The view's bounds inside its `crop=`, in the view's frame: what `box=`
/// centres and `bounds` reports once the model outside the crop is gone. A
/// crop that holds none of the view is an error naming both rectangles --
/// most often a crop written in feet over a model in millimetres.
fn cropped_bounds(bounds: Option<(P2, P2)>, crop: Option<ClipRect>, what: &str) -> anyhow::Result<Option<(P2, P2)>> {
    let (Some((lo, hi)), Some(c)) = (bounds, crop) else {
        return Ok(bounds);
    };
    let (a, b) = (P2::new(lo.x.max(c.lo.x), lo.y.max(c.lo.y)), P2::new(hi.x.min(c.hi.x), hi.y.min(c.hi.y)));
    if a.x > b.x || a.y > b.y {
        anyhow::bail!(
            "{what}: crop=({:.1}, {:.1}, {:.1}, {:.1}) holds none of the view, whose model spans ({:.1}, {:.1}, {:.1}, {:.1}) \
             in the view's frame, in millimetres",
            c.lo.x,
            c.lo.y,
            c.hi.x,
            c.hi.y,
            lo.x,
            lo.y,
            hi.x,
            hi.y
        );
    }
    Ok(Some((a, b)))
}

/// Put `geometry` on the sheet as a clipped group and hand back the view.
#[allow(clippy::too_many_arguments)]
fn place_view<'v>(
    sheet: &KSheet,
    geometry: views::ViewGeometry,
    scale: Option<Scale>,
    place: Placement,
    name: Option<&str>,
    eval: &mut Evaluator<'v, '_, '_>,
    what: &str,
) -> anyhow::Result<KSheetView> {
    let span = call_span(eval);
    let st = store(eval)?;
    for n in &geometry.notes {
        st.ctx.borrow_mut().diagnostics.push(cadpole_core::context::Diagnostic::warning(format!("{what}: {n}")).with_span(span.clone()));
    }
    let model_bounds = cropped_bounds(geometry.bounds, place.crop, what)?;
    let frame = geometry.frame;
    let (group, scale, tx, ty, clip, bounds) = with_sheet(eval, sheet, |set, d| {
        let area = set.drawing_area();
        let s = match scale {
            Some(s) => s,
            None => set.scale_for(sheet.index),
        };
        let mut ratio = s.ratio();
        let (tx, ty) = match (place.fit, place.at) {
            (Some(fit), _) => {
                let (lo, hi) = model_bounds.unwrap_or((P2::new(0.0, 0.0), P2::new(1.0, 1.0)));
                let (w, h) = ((hi.x - lo.x).max(1e-9), (hi.y - lo.y).max(1e-9));
                if scale.is_none() && set.default_scale.is_none() && set.sheets[sheet.index].scale.is_none() {
                    // No scale anywhere: fit the box and snap to a standard
                    // ratio, as the three-view sheet does.
                    let f = ((fit.hi.x - fit.lo.x) / w).min((fit.hi.y - fit.lo.y) / h);
                    ratio = snap(f).ratio();
                }
                let cx = (fit.lo.x + fit.hi.x) / 2.0 - (lo.x + hi.x) / 2.0 * ratio;
                let cy = (fit.lo.y + fit.hi.y) / 2.0 - (lo.y + hi.y) / 2.0 * ratio;
                (cx, cy)
            }
            (None, Some(at)) => (at.x - place.origin.x * ratio, at.y - place.origin.y * ratio),
            (None, None) => {
                // Neither: centre the bounds on the drawing area.
                let (lo, hi) = model_bounds.unwrap_or((P2::new(0.0, 0.0), P2::new(1.0, 1.0)));
                (
                    (area.lo.x + area.hi.x) / 2.0 - (lo.x + hi.x) / 2.0 * ratio,
                    (area.lo.y + area.hi.y) / 2.0 - (lo.y + hi.y) / 2.0 * ratio,
                )
            }
        };
        let clip = place.clip.or(place.fit).unwrap_or(area);
        let n = d.items.len();
        let bounds = model_bounds.map(|(lo, hi)| (lo.x * ratio + tx, lo.y * ratio + ty, hi.x * ratio + tx, hi.y * ratio + ty));
        // Two groups inside the view: what the script paints `under=True`
        // (contours under a roof, a knock-out), then the projection itself,
        // cut to `crop=` (a clip in the view's frame, the lines group's
        // parent); everything else a view method draws goes on top of both,
        // and only the viewport clips it, so a dimension may stand outside
        // the crop.
        d.items.push(Item::Group(Group {
            name: name.map(str::to_owned).unwrap_or_else(|| format!("view{}", n)),
            source: None,
            transform: Xform2::new(ratio, tx, ty),
            items: vec![
                Item::Group(Group { name: UNDER.to_owned(), ..Group::default() }),
                Item::Group(Group { name: "lines".to_owned(), items: geometry.items, clip: place.crop, ..Group::default() }),
            ],
            clip: Some(clip),
        }));
        Ok((n, ratio, tx, ty, (clip.lo.x, clip.lo.y, clip.hi.x, clip.hi.y), bounds))
    })?;
    with_set(eval, sheet.set, |set| {
        set.sheets[sheet.index].views += 1;
        Ok(())
    })?;
    Ok(KSheetView { set: sheet.set, sheet: sheet.index, group, scale, tx, ty, clip, bounds, frame })
}

/// The name of the group inside a view that `under=True` draws into.
const UNDER: &str = "under";

/// Where a view method's items go: on top of the view's line work, or
/// under it (`under=True`), in the order the calls are made either way.
fn target(g: &mut Group, under: bool) -> &mut Vec<Item> {
    let has_under = matches!(g.items.first(), Some(Item::Group(u)) if u.name == UNDER);
    if under && has_under {
        match g.items.first_mut() {
            Some(Item::Group(u)) => &mut u.items,
            _ => unreachable!("checked above"),
        }
    } else {
        &mut g.items
    }
}

/// Standard drawing ratios, largest first, for a fitted view.
fn snap(fit: f64) -> Scale {
    const NICE: &[f64] = &[
        10.0, 5.0, 4.0, 2.5, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.04, 0.025, 0.02, 0.01, 0.005, 0.002, 0.001,
    ];
    let s = NICE.iter().copied().find(|n| *n <= fit).unwrap_or(0.001);
    if s >= 1.0 { Scale::new(s, 1.0) } else { Scale::new(1.0, (1.0 / s).round()) }
}

/// `look=`: `"lines"` (the default) or `"color"`.
fn look_arg(look: Option<&str>, what: &str) -> anyhow::Result<cadpole_drawing::Look> {
    match look {
        None => Ok(cadpole_drawing::Look::Lines),
        Some(s) => cadpole_drawing::Look::parse(s).ok_or_else(|| anyhow::anyhow!("{what}: look must be \"lines\" or \"color\", got {s:?}")),
    }
}

fn view_options(hidden: Hidden, hatch: Option<&str>, hatch_spacing: Option<f64>, cut_layer: Option<&str>, what: &str) -> anyhow::Result<ViewOptions> {
    let hatch = match hatch.map(|h| h.trim().to_ascii_lowercase()) {
        None => Some(Pattern::ANSI31),
        Some(h) if h == "none" || h == "off" => None,
        Some(h) => Some(pattern_arg(Some(&h), hatch_spacing, None, what)?),
    };
    Ok(ViewOptions { hidden, hatch, cut_layer: cut_layer.map(str::to_owned), ..ViewOptions::default() })
}

/// `plane=`: a Plane, or `"x=3000"` / `"y=..."` / `"z=..."` in model
/// millimetres, drawn looking along the axis with the world's up axis up.
fn section_plane_arg<'v>(v: Value<'v>, what: &str) -> anyhow::Result<Plane> {
    if let Some(p) = v.downcast_ref::<KPlane>() {
        return Ok(p.0);
    }
    let Some(s) = v.unpack_str() else {
        anyhow::bail!("{what}: plane= is a Plane or \"x=<mm>\" / \"y=<mm>\" / \"z=<mm>\", got {}", v.get_type());
    };
    let (axis, value) = s.split_once('=').ok_or_else(|| anyhow::anyhow!("{what}: plane {s:?} is not \"x=<mm>\", \"y=<mm>\" or \"z=<mm>\""))?;
    let at = cadpole_drawing::parse_length(value).ok_or_else(|| anyhow::anyhow!("{what}: plane: {value:?} is not a length"))?;
    let (normal, x_dir) = match axis.trim().to_ascii_lowercase().as_str() {
        "x" => (Vec3::X, Vec3::Y),
        "y" => (Vec3::Y, Vec3::new(-1.0, 0.0, 0.0)),
        "z" => (Vec3::Z, Vec3::X),
        other => anyhow::bail!("{what}: plane axis must be x, y or z, got {other:?}"),
    };
    Plane::new(normal * at, normal, x_dir).map_err(|e| anyhow::anyhow!("{what}: {e}"))
}

fn direction_arg<'v>(v: Value<'v>, what: &str) -> anyhow::Result<cadpole_drawing::ViewBasis> {
    if let Some(s) = v.unpack_str() {
        return views::basis_for(s).ok_or_else(|| anyhow::anyhow!("{what}: direction {s:?} must be one of {}, or a vector", views::DIRECTIONS.join(", ")));
    }
    let arg: VecArg = VecArg::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: direction is a compass name or an (x, y, z) toward the eye, got {}", v.get_type()))?;
    let dir = crate::check::vec3(&arg, &format!("{what}: direction"))?;
    if dir.length() < 1e-12 {
        anyhow::bail!("{what}: direction must not be zero");
    }
    let up = if dir.normalized().z.abs() > 0.999 { Vec3::Y } else { Vec3::Z };
    Ok(cadpole_drawing::ViewBasis::looking_from(dir, up))
}

fn loop_path(l: &Loop) -> Path {
    let mut p = Path::new(l.segments[0].start());
    for s in &l.segments {
        p.segments.push(s.clone());
    }
    p
}

/// Draw a sketch's loops and paths into `items` in the sketch's own
/// coordinates, scaled and shifted.
fn sketch_items(sk: &KSketch, scale: f64, at: P2, weight: Option<f64>, dash: Option<Dash>, color: Option<Rgb>, fill: bool, layer: usize, items: &mut Vec<Item>) {
    let s = sk.sketch();
    let map = |p: P2| P2::new(p.x * scale + at.x, p.y * scale + at.y);
    let style = cadpole_drawing::Style { layer, weight_mm: weight, dash: dash.clone(), color };
    if fill {
        if let Ok(regions) = s.regions() {
            for r in regions {
                let xf = |l: &Loop| Loop { segments: l.segments.iter().map(|sg| cadpole_drawing::ir::xform_segment_with(sg, map)).collect() };
                items.push(Item::Hatch(cadpole_drawing::HatchItem {
                    style: style.clone(),
                    name: String::new(),
                    outer: xf(&r.outer),
                    holes: r.holes.iter().map(xf).collect(),
                    pattern: Pattern::Solid,
                    scale: 1.0,
                }));
            }
            return;
        }
    }
    for (i, l) in s.loops.iter().enumerate() {
        if l.segments.is_empty() {
            continue;
        }
        let p = Path { start: map(l.segments[0].start()), segments: l.segments.iter().map(|sg| cadpole_drawing::ir::xform_segment_with(sg, map)).collect() };
        items.push(Item::Stroke(cadpole_drawing::Stroke { style: style.clone(), name: s.loop_name(i).to_owned(), path: p, closed: true }));
    }
    for (i, p) in s.paths.iter().enumerate() {
        let p = Path { start: map(p.start), segments: p.segments.iter().map(|sg| cadpole_drawing::ir::xform_segment_with(sg, map)).collect() };
        items.push(Item::Stroke(cadpole_drawing::Stroke { style: style.clone(), name: s.path_name(i).to_owned(), path: p, closed: false }));
    }
}

/// A linear dimension between two model points in a view's frame.
fn dim_item(set: &SheetSet, a: P2, b: P2, offset: f64, units: Option<&str>, text: Option<&str>, size: Option<f64>, layer: usize) -> anyhow::Result<Item> {
    let mut style = set.dim_style();
    if let Some(u) = units {
        let f = crate::drawing::unit_format(u, "dimension")?;
        style = if f.unit.imperial() { cadpole_drawing::DimStyle::architectural() } else { cadpole_drawing::DimStyle::mechanical() }.with_font(set.font.clone());
        style.format = f;
    }
    if let Some(s) = size {
        style.text_height_mm = s;
    }
    let mut dm = cadpole_drawing::Dim::aligned(cadpole_drawing::Style::on(layer), a, b, offset, style);
    dm.text_override = text.map(str::to_owned);
    Ok(Item::Dim(dm))
}

fn text_item(at: P2, s: &str, size: f64, rotation: f64, anchor: Anchor, font: FontRef, color: Option<Rgb>, layer: usize) -> Item {
    Item::Text(cadpole_drawing::TextItem {
        style: cadpole_drawing::Style { layer, weight_mm: None, dash: None, color },
        at,
        text: s.to_owned(),
        height_mm: size,
        rotation_deg: rotation,
        anchor,
        font,
        model_height: false,
    })
}

/// Decode a PNG asset into an [`cadpole_drawing::Image`], keeping the file's
/// bytes for the SVG, which embeds them as they are.
fn decode_png(bytes: &[u8], what: &str) -> anyhow::Result<cadpole_drawing::Image> {
    let img = cadpole_core::render::Image::from_png(bytes).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
    Ok(cadpole_drawing::Image { width: img.width, height: img.height, rgba: img.rgba, png: Some(bytes.to_vec()) })
}

/// `(x0, y0, x1, y1)` as a Starlark tuple: the derive stops at three.
fn alloc_rect<'v>(heap: Heap<'v>, r: (f64, f64, f64, f64)) -> Value<'v> {
    heap.alloc(starlark::values::tuple::AllocTuple([r.0, r.1, r.2, r.3]))
}

fn param_value<'v>(v: Value<'v>, what: &str) -> anyhow::Result<ParamValue> {
    crate::modeltest::param_value_of(v).ok_or_else(|| anyhow::anyhow!("{what}: values are numbers, bools, strings or (x, y, z), got {}", v.get_type()))
}

fn alloc_param<'v>(heap: Heap<'v>, v: &ParamValue) -> Value<'v> {
    match v {
        ParamValue::Number(n) => heap.alloc(*n),
        ParamValue::Bool(b) => Value::new_bool(*b),
        ParamValue::Text(s) => heap.alloc(s.as_str()),
        ParamValue::Vector(v) => heap.alloc((v.x, v.y, v.z)),
    }
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

#[starlark_module]
pub(crate) fn sheet_globals(builder: &mut GlobalsBuilder) {
    /// Start a set of drawing sheets: one paper size, one title
    /// column, one sheet index, written as one multi-page PDF by
    /// `export(set, "set.pdf")` (or one SVG / DXF per sheet from a `.svg` /
    /// `.dxf` path) when the host runs the script's exports.
    ///
    /// `size` is a paper size -- `sheet.arch_d` (the default, 36 x 24 in),
    /// `sheet.arch_c`, `sheet.a1`, `sheet.a3`, `sheet.letter`, any member of
    /// the `sheet` vocabulary or its string -- or `(w, h)` in millimetres,
    /// landscape as given. `units` is how dimensions are written (`"mm"`,
    /// `"cm"`, `"m"`, `"in"`, `"ft"`, `"ft-in"`, or a `units` member; left
    /// out or `None`, the document's `units(...)` with its fraction, else
    /// millimetres); `scale` the default scale of every sheet (`"1:48"`,
    /// `1/4" = 1'-0"`, or a number); `font`
    /// `"helvetica"` (the default: the PDF's text is real, selectable
    /// text), `"courier"` or `"inter"` (the bundled face, filled as
    /// outlines); `margin` the frame's inset in mm (10); `north` the
    /// degrees clockwise from the page's up to true north, for every north
    /// arrow on the set. `title_block` is the column every sheet carries:
    /// a `title_block(...)` or a dict of the same fields. `text_outlines`
    /// fills every string as glyph outlines in every format, for a file
    /// that must look the same with no fonts at all. Then
    /// `set.sheet(number, name, scale=)` adds sheets.
    fn sheet_set<'v>(
        name: &str,
        #[starlark(require = named)] size: Option<Value<'v>>,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] font: Option<&str>,
        #[starlark(require = named)] margin: Option<UnpackFloat>,
        #[starlark(require = named)] north: Option<UnpackFloat>,
        #[starlark(require = named)] title_block: Option<Value<'v>>,
        #[starlark(require = named, default = false)] text_outlines: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheetSet> {
        let sheet_size = match size.filter(|v| !v.is_none()) {
            None => SheetSize::ARCH_D,
            Some(v) => match v.unpack_str() {
                Some(s) => SheetSize::parse(s).ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        crate::vocab::not_one_of(
                            format!("sheet_set: size {s:?} is not a paper size (or pass (w, h) in mm)"),
                            SheetSize::NAMES
                        )
                    )
                })?,
                None => {
                    let arg: VecArg = VecArg::unpack_value(v).map_err(|e| e.into_anyhow())?.ok_or_else(|| anyhow::anyhow!("sheet_set: size is a name or (w, h) in mm"))?;
                    let p = p2(&arg, "sheet_set: size")?;
                    if p.x <= 0.0 || p.y <= 0.0 {
                        anyhow::bail!("sheet_set: a custom size needs a positive width and height");
                    }
                    SheetSize { name: "CUSTOM", w_mm: p.x, h_mm: p.y }
                }
            },
        };
        // Its own `units=`, else the document's `units(...)`, else
        // millimetres -- the rule `drawing()` and the `export_*` calls keep.
        // `units=None` is the same as leaving it out.
        let unit_format = match units.into_option() {
            None => crate::units::document_format(&store(eval)?.ctx.borrow()).unwrap_or_else(UnitFormat::mm),
            Some(u) => crate::drawing::unit_format(u, "sheet_set")?,
        };
        let mut set = SheetSet::new(name, sheet_size);
        set.unit_format = unit_format;
        set.default_scale = scale_arg(scale, "sheet_set")?;
        set.font = match font.map(|f| f.trim().to_ascii_lowercase()) {
            None => FontRef::Standard(StdFont::Helvetica),
            Some(f) if f == "inter" || f == "builtin" => FontRef::Builtin,
            Some(f) => FontRef::Standard(StdFont::parse(&f, false, false).ok_or_else(|| anyhow::anyhow!("sheet_set: font must be helvetica, courier or inter, got {f:?}"))?),
        };
        if let Some(m) = margin {
            let m = num(&m, "sheet_set: margin")?;
            if m < 0.0 || m > sheet_size.h_mm / 3.0 {
                anyhow::bail!("sheet_set: margin must be between 0 and a third of the sheet, got {m}");
            }
            set.margin_mm = m;
        }
        set.north_degrees = opt_num(&north, "sheet_set: north")?.unwrap_or(0.0);
        set.text_outlines = text_outlines;
        if let Some(tb) = title_block.filter(|v| !v.is_none()) {
            set.title = match tb.downcast_ref::<KTitleBlock>() {
                Some(t) => t.0.clone(),
                None => title_from_dict(tb, eval.heap())?,
            };
        }
        let st = store(eval)?;
        let span = call_span(eval);
        {
            let mut ctx = st.ctx.borrow_mut();
            let mut args = BTreeMap::new();
            args.insert("size".to_owned(), sheet_size.name.to_owned());
            args.insert("units".to_owned(), set.unit_format.unit.name().to_owned());
            ctx.new_feature("sheet_set", Some(name.to_owned()), Vec::new(), args, span)?;
        }
        let mut sets = st.sheet_sets.borrow_mut();
        sets.push(set);
        Ok(KSheetSet { index: sets.len() - 1 })
    }

    /// The title column every sheet of a set carries, for
    /// `sheet_set(title_block=)`: the project's name and address, the
    /// block and lot, a boxed `stamp` ("CONCEPT STUDY", "NOT FOR
    /// CONSTRUCTION"; a list of lines or one string with newlines), `notes`
    /// in small print under it, the `issue`, `date`, `drawn` and `checked`
    /// rows, `extra` label/value rows under those, and the column's width
    /// in mm (`column`, 0 picks one for the paper). The set adds the sheet
    /// index -- every sheet's number and name, this one marked -- the
    /// scale, the sheet's name and its number, large, on every page.
    #[allow(clippy::too_many_arguments)]
    fn title_block<'v>(
        #[starlark(require = named, default = "")] project: &str,
        #[starlark(require = named, default = "")] address: &str,
        #[starlark(require = named, default = "")] block_lot: &str,
        #[starlark(require = named)] stamp: Option<Value<'v>>,
        #[starlark(require = named)] notes: Option<Value<'v>>,
        #[starlark(require = named, default = "")] issue: &str,
        #[starlark(require = named, default = "")] date: &str,
        #[starlark(require = named, default = "")] drawn: &str,
        #[starlark(require = named, default = "")] checked: &str,
        #[starlark(require = named)] extra: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(0.0))] column: UnpackFloat,
        heap: Heap<'v>,
    ) -> anyhow::Result<KTitleBlock> {
        let column = num(&column, "title_block: column")?;
        if column < 0.0 {
            anyhow::bail!("title_block: column must be 0 (pick one) or a width in mm, got {column}");
        }
        Ok(KTitleBlock(cadpole_drawing::TitleTemplate {
            project: project.to_owned(),
            address: address.to_owned(),
            block_lot: block_lot.to_owned(),
            stamp: match stamp.filter(|v| !v.is_none()) {
                Some(v) => lines_arg(v, heap, "title_block: stamp")?,
                None => Vec::new(),
            },
            notes: match notes.filter(|v| !v.is_none()) {
                Some(v) => lines_arg(v, heap, "title_block: notes")?,
                None => Vec::new(),
            },
            issue: issue.to_owned(),
            date: date.to_owned(),
            drawn: drawn.to_owned(),
            checked: checked.to_owned(),
            extra: match extra.filter(|v| !v.is_none()) {
                Some(v) => pairs_arg(v, heap, "title_block: extra")?,
                None => Vec::new(),
            },
            column_mm: column,
        }))
    }

    /// The advance width of `text` set at cap height `size` (mm) in `font`
    /// (`"helvetica"`, the default, `"courier"` or `"inter"`), bold or not:
    /// what a sheet needs to place the next thing after a label. Helvetica
    /// and Courier are measured from Adobe's own metrics, Inter by the
    /// shaper that draws it.
    fn text_width(
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named, default = UnpackFloat(2.5))] size: UnpackFloat,
        #[starlark(require = named)] font: Option<&str>,
        #[starlark(require = named, default = false)] bold: bool,
    ) -> anyhow::Result<f64> {
        let size = num(&size, "text_width: size")?;
        let f = match font.map(|f| f.trim().to_ascii_lowercase()) {
            None => FontRef::Standard(if bold { StdFont::HelveticaBold } else { StdFont::Helvetica }),
            Some(f) if f == "inter" || f == "builtin" => FontRef::Builtin,
            Some(f) => FontRef::Standard(StdFont::parse(&f, bold, false).ok_or_else(|| anyhow::anyhow!("text_width: font must be helvetica, courier or inter, got {f:?}"))?),
        };
        Ok(cadpole_drawing::text::width_in(text, size, &f))
    }

    /// Record a named value of this evaluation — a check's verdict, an
    /// area, a height — for `sweep_table()` to collect when the document
    /// is evaluated at other configurations. A value is a number, a bool,
    /// a string or an `(x, y, z)`. The last `report()` of a name wins.
    fn report<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = pos)] value: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let v = param_value(value, "report")?;
        let st = store(eval)?;
        let mut reports = st.reports.borrow_mut();
        if let Some(slot) = reports.iter_mut().find(|(n, _)| n == name) {
            slot.1 = v;
        } else {
            reports.push((name.to_owned(), v));
        }
        Ok(NoneType)
    }

    /// The document's `report()`ed values at other parameter values, for a
    /// cover sheet's table. `configs` is a list of dicts of
    /// parameter overrides; `labels` names each row. Returns one dict per
    /// configuration: `label`, `params`, `pending`, `ok`, `error` and
    /// `values` (a dict of every `report()` made at that configuration).
    ///
    /// A script cannot evaluate itself, so the rows are filled by the host:
    /// `cadpole export --run-exports` and the `Drawing` rpc evaluate the
    /// document once per configuration first — never inside this
    /// evaluation — and hand the rows to the run that writes the sheets.
    /// Anywhere else (the viewer, `cadpole test`) every row is `pending` with
    /// empty `values`, so read them with `.get()`. At most 64
    /// configurations per document.
    fn sweep_table<'v>(
        #[starlark(require = pos)] configs: Value<'v>,
        #[starlark(require = named)] labels: Option<UnpackList<String>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let mut points: Vec<Vec<(String, ParamValue)>> = Vec::new();
        for item in configs.iterate(heap).map_err(|_| anyhow::anyhow!("sweep_table: configs is a list of dicts of parameter overrides"))? {
            let d = DictRef::from_value(item).ok_or_else(|| anyhow::anyhow!("sweep_table: each configuration is a dict of parameter name to value, got {}", item.get_type()))?;
            let mut point = Vec::new();
            for (k, v) in d.iter() {
                let Some(k) = k.unpack_str() else { anyhow::bail!("sweep_table: parameter names are strings") };
                point.push((k.to_owned(), param_value(v, "sweep_table")?));
            }
            points.push(point);
        }
        if points.is_empty() {
            anyhow::bail!("sweep_table: no configurations");
        }
        let labels: Vec<String> = match labels {
            Some(l) => {
                if l.items.len() != points.len() {
                    anyhow::bail!("sweep_table: {} labels for {} configurations", l.items.len(), points.len());
                }
                l.items
            }
            None => points.iter().map(|p| crate::modeltest::point_label(p)).collect(),
        };
        let st = store(eval)?;
        let declared: Vec<String> = st.ctx.borrow().params.iter().map(|(d, _)| d.name.clone()).collect();
        for p in &points {
            for (k, _) in p {
                if !declared.contains(k) {
                    anyhow::bail!(
                        "sweep_table: {k:?} is not a param of this document (it declares {})",
                        if declared.is_empty() { "none".to_owned() } else { declared.join(", ") }
                    );
                }
            }
        }
        let total: usize = st.sweep_requests.borrow().iter().map(|r| r.points.len()).sum::<usize>() + points.len();
        if total > crate::limits::Limits::MAX_SWEEP_TABLE_POINTS {
            anyhow::bail!(
                "sweep_table: more than {} configurations in this document; every one is a whole evaluation",
                crate::limits::Limits::MAX_SWEEP_TABLE_POINTS
            );
        }
        let index = st.sweep_requests.borrow().len();
        st.sweep_requests.borrow_mut().push(SweepRequest { points: points.clone(), labels: labels.clone() });
        let results = st.sweep_results.as_ref().and_then(|r| r.get(index)).cloned();
        let mut rows: Vec<Value<'v>> = Vec::new();
        for (i, point) in points.iter().enumerate() {
            let params = AllocDict(point.iter().map(|(k, v)| (heap.alloc(k.as_str()), alloc_param(heap, v))).collect::<Vec<_>>());
            let row = results.as_ref().and_then(|rows| rows.get(i));
            let values = AllocDict(
                row.map(|r| r.values.iter().map(|(k, v)| (heap.alloc(k.as_str()), alloc_param(heap, v))).collect::<Vec<_>>()).unwrap_or_default(),
            );
            let entries: Vec<(Value<'v>, Value<'v>)> = vec![
                (heap.alloc("label"), heap.alloc(labels[i].as_str())),
                (heap.alloc("params"), heap.alloc(params)),
                (heap.alloc("pending"), Value::new_bool(row.is_none())),
                (heap.alloc("ok"), Value::new_bool(row.map(|r| r.ok).unwrap_or(false))),
                (heap.alloc("error"), heap.alloc(row.map(|r| r.error.as_str()).unwrap_or(""))),
                (heap.alloc("values"), heap.alloc(values)),
            ];
            rows.push(heap.alloc(AllocDict(entries)));
        }
        Ok(heap.alloc(rows))
    }
}

// ---------------------------------------------------------------------------
// SheetSet
// ---------------------------------------------------------------------------

#[starlark_value(type = "SheetSet")]
impl<'v> StarlarkValue<'v> for KSheetSet {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("sheet_set_methods", sheet_set_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        // Attributes read the store through the evaluator, which a plain
        // attribute has no handle on; the counts and names a script wants
        // are methods instead (`set.sheets()`, `set.area()`).
        match attribute {
            "id" => Some(heap.alloc(self.index as i32)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        vec!["id".to_owned()]
    }
}

#[starlark_module]
fn sheet_set_methods(builder: &mut MethodsBuilder) {
    /// Add a sheet: its number (`"A2.0"`), its name (`"SITE PLAN"`), its
    /// scale (`"1:48"`, the architectural form, or a number; the set's
    /// default when left out) and whether its title column carries a north
    /// arrow. Sheets are numbered in the index in the order they are added.
    fn sheet<'v>(
        this: &KSheetSet,
        #[starlark(require = pos)] number: &str,
        #[starlark(require = pos)] name: &str,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named, default = false)] north: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheet> {
        if number.trim().is_empty() {
            anyhow::bail!("sheet: the number is empty");
        }
        let scale = scale_arg(scale, "sheet")?;
        let index = with_set(eval, this.index, |set| {
            if set.find(number).is_some() {
                anyhow::bail!("sheet: {number:?} is already in the set");
            }
            let i = set.add_sheet(number, name, scale);
            set.sheets[i].north = north;
            Ok(i)
        })?;
        Ok(KSheet { set: this.index, index })
    }

    /// The sheet numbers of the set, in order.
    fn sheets<'v>(this: &KSheetSet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let numbers = with_set(eval, this.index, |set| Ok(set.sheets.iter().map(|s| s.number.clone()).collect::<Vec<_>>()))?;
        Ok(crate::convert::alloc_str_list(heap, &numbers))
    }

    /// The rectangle a sheet's views and notes may occupy, `(x0, y0, x1,
    /// y1)` in paper millimetres: inside the frame, clear of the title
    /// column.
    fn area<'v>(this: &KSheetSet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let r = with_set(eval, this.index, |set| Ok(set.drawing_area()))?;
        Ok(alloc_rect(heap, (r.lo.x, r.lo.y, r.hi.x, r.hi.y)))
    }

    /// The paper size, `(width, height)` in millimetres.
    fn size<'v>(this: &KSheetSet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let (w, h) = with_set(eval, this.index, |set| Ok((set.size.w_mm, set.size.h_mm)))?;
        Ok(heap.alloc((w, h)))
    }
}

// ---------------------------------------------------------------------------
// Sheet
// ---------------------------------------------------------------------------

#[starlark_value(type = "Sheet")]
impl<'v> StarlarkValue<'v> for KSheet {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("sheet_methods", sheet_methods);
        Some(RES.methods())
    }
}

#[starlark_module]
fn sheet_methods(builder: &mut MethodsBuilder) {
    /// This sheet's number.
    fn number<'v>(this: &KSheet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<String> {
        with_set(eval, this.set, |set| Ok(set.sheets[this.index].number.clone()))
    }

    /// This sheet's name.
    fn name<'v>(this: &KSheet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<String> {
        with_set(eval, this.set, |set| Ok(set.sheets[this.index].name.clone()))
    }

    /// This sheet's scale, written the way its title column writes it.
    fn scale<'v>(this: &KSheet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<String> {
        with_set(eval, this.set, |set| {
            Ok(match set.sheets[this.index].scale {
                Some(s) => s.label(&set.unit_format),
                None => "AS NOTED".to_owned(),
            })
        })
    }

    /// The rectangle this sheet's views and notes may occupy, `(x0, y0,
    /// x1, y1)` in paper millimetres.
    fn area<'v>(this: &KSheet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let r = with_set(eval, this.set, |set| Ok(set.drawing_area()))?;
        Ok(alloc_rect(heap, (r.lo.x, r.lo.y, r.hi.x, r.hi.y)))
    }

    /// How much ink the sheet holds so far: strokes, fills and text runs
    /// after views and dimensions are expanded.
    fn items<'v>(this: &KSheet, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<i32> {
        with_set(eval, this.set, |set| Ok(set.sheets[this.index].drawing.flatten().len() as i32))
    }

    /// A plan: `bodies` cut at height `cut` (model mm) and seen from above,
    /// what is below the cut projected with its hidden lines removed, the
    /// cut walls outlined heavily and hatched (`hatch=` `"lines"`, the
    /// default, `"cross"`, `"dots"`, `"solid"` or `"none"`; `hatch_spacing`
    /// in paper mm). Placed with `at=` (the paper point the model's
    /// `origin=` lands on) or `box=` (a paper rectangle the plan is fitted
    /// and clipped to); `scale=` overrides the sheet's; `clip=` a paper
    /// rectangle, the drawing area by default. `crop=(x0, y0, x1, y1)` is
    /// the part of the model the view shows, in model mm of its frame: the
    /// projection is cut to it (a driveway stops where the plan's interest
    /// does) and `box=` centres what is left; what the script draws on the
    /// view is clipped by the viewport alone. `layers=` names the layer
    /// each body draws on (a dict of body name to layer, or one name);
    /// `hidden=` `"omit"` (the default), `"dashed"` or `"solid"`.
    /// `look="color"` paints the plan the way a section render
    /// shows it instead: every surface below the cut filled in its body's
    /// colour, nearer over farther, edges as fine lines, the cut filled in
    /// the colour of what it cuts, and labels made with
    /// `extrude(text(...))` written as text the size they are in the model.
    /// Returns the view, whose own methods draw in model coordinates.
    #[allow(clippy::too_many_arguments)]
    fn plan_view<'v>(
        this: &KSheet,
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named)] cut: UnpackFloat,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] r#box: Option<VecArg>,
        #[starlark(require = named)] clip: Option<VecArg>,
        #[starlark(require = named)] crop: Option<VecArg>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        #[starlark(require = named)] hidden: Option<&str>,
        #[starlark(require = named)] hatch: Option<&str>,
        #[starlark(require = named)] hatch_spacing: Option<UnpackFloat>,
        #[starlark(require = named)] cut_layer: Option<&str>,
        #[starlark(require = named)] look: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheetView> {
        let cut = num(&cut, "plan_view: cut")?;
        let bodies = view_bodies(bodies, layers, eval, "plan_view")?;
        let mut opts = view_options(hidden_arg(hidden, Hidden::Omit, "plan_view")?, hatch, opt_num(&hatch_spacing, "plan_view: hatch_spacing")?, cut_layer, "plan_view")?;
        opts.look = look_arg(look, "plan_view")?;
        let scale = scale_arg(scale, "plan_view")?;
        let place = placement(at, origin, r#box, clip, crop, "plan_view")?;
        let geometry = with_sheet(eval, this, |_, d| Ok(views::plan_view(d, &bodies, cut, &opts)))?;
        place_view(this, geometry, scale, place, name, eval, "plan_view")
    }

    /// A section: `bodies` cut by `plane` (a Plane, or `"x=<mm>"`,
    /// `"y=<mm>"`, `"z=<mm>"`), the cut faces hatched and outlined, and
    /// what lies beyond the plane — up to `depth` mm of it — projected
    /// behind them (`beyond=False` draws the cut alone). The view's frame
    /// is the plane's own: x along its x axis, y up it. Placement, scale,
    /// layers, hidden lines, hatch and `look=` as `plan_view`.
    #[allow(clippy::too_many_arguments)]
    fn section_view<'v>(
        this: &KSheet,
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named)] plane: Value<'v>,
        #[starlark(require = named)] depth: Option<UnpackFloat>,
        #[starlark(require = named, default = true)] beyond: bool,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] r#box: Option<VecArg>,
        #[starlark(require = named)] clip: Option<VecArg>,
        #[starlark(require = named)] crop: Option<VecArg>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        #[starlark(require = named)] hidden: Option<&str>,
        #[starlark(require = named)] hatch: Option<&str>,
        #[starlark(require = named)] hatch_spacing: Option<UnpackFloat>,
        #[starlark(require = named)] cut_layer: Option<&str>,
        #[starlark(require = named)] look: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheetView> {
        let plane = section_plane_arg(plane, "section_view")?;
        let depth = opt_num(&depth, "section_view: depth")?;
        if depth.is_some_and(|d| d <= 0.0) {
            anyhow::bail!("section_view: depth must be positive");
        }
        let bodies = view_bodies(bodies, layers, eval, "section_view")?;
        let mut opts = view_options(hidden_arg(hidden, Hidden::Omit, "section_view")?, hatch, opt_num(&hatch_spacing, "section_view: hatch_spacing")?, cut_layer, "section_view")?;
        opts.look = look_arg(look, "section_view")?;
        let scale = scale_arg(scale, "section_view")?;
        let place = placement(at, origin, r#box, clip, crop, "section_view")?;
        let geometry = with_sheet(eval, this, |_, d| Ok(views::section_view(d, &bodies, &plane, depth, beyond, &opts)))?;
        place_view(this, geometry, scale, place, name, eval, "section_view")
    }

    /// An elevation: `bodies` projected from `direction` — `"north"`,
    /// `"south"`, `"east"`, `"west"` (the face seen from that side, +Y
    /// being north), one of `drawing()`'s view names, or an `(x, y, z)`
    /// toward the eye — with hidden lines removed (`hidden="dashed"` keeps
    /// them dashed). The frame is the view's screen: for a south elevation
    /// x is the model's x and y its z. Placement, scale and `look=` as `plan_view`.
    #[allow(clippy::too_many_arguments)]
    fn elevation_view<'v>(
        this: &KSheet,
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named)] direction: Value<'v>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] r#box: Option<VecArg>,
        #[starlark(require = named)] clip: Option<VecArg>,
        #[starlark(require = named)] crop: Option<VecArg>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        #[starlark(require = named)] hidden: Option<&str>,
        #[starlark(require = named)] look: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheetView> {
        let basis = direction_arg(direction, "elevation_view")?;
        let bodies = view_bodies(bodies, layers, eval, "elevation_view")?;
        let opts = ViewOptions {
            hidden: hidden_arg(hidden, Hidden::Omit, "elevation_view")?,
            look: look_arg(look, "elevation_view")?,
            ..ViewOptions::default()
        };
        let scale = scale_arg(scale, "elevation_view")?;
        let place = placement(at, origin, r#box, clip, crop, "elevation_view")?;
        let geometry = with_sheet(eval, this, |_, d| Ok(views::elevation_view(d, &bodies, &basis, &opts)))?;
        place_view(this, geometry, scale, place, name, eval, "elevation_view")
    }

    /// An axonometric: `bodies` projected from azimuth `az` (degrees from
    /// the south view turning toward +X) and elevation `el` (degrees above
    /// the ground), the renderer's own convention, hidden lines removed.
    /// A picture, so `box=` with no scale fits it; placement as
    /// `plan_view`.
    #[allow(clippy::too_many_arguments)]
    fn axon_view<'v>(
        this: &KSheet,
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named, default = UnpackFloat(45.0))] az: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(30.0))] el: UnpackFloat,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] r#box: Option<VecArg>,
        #[starlark(require = named)] clip: Option<VecArg>,
        #[starlark(require = named)] crop: Option<VecArg>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        #[starlark(require = named)] hidden: Option<&str>,
        #[starlark(require = named)] look: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSheetView> {
        let (az, el) = (num(&az, "axon_view: az")?, num(&el, "axon_view: el")?);
        let bodies = view_bodies(bodies, layers, eval, "axon_view")?;
        let opts = ViewOptions {
            hidden: hidden_arg(hidden, Hidden::Omit, "axon_view")?,
            look: look_arg(look, "axon_view")?,
            ..ViewOptions::default()
        };
        let scale = scale_arg(scale, "axon_view")?;
        let place = placement(at, origin, r#box, clip, crop, "axon_view")?;
        let geometry = with_sheet(eval, this, |_, d| Ok(views::axon_view(d, &bodies, az, el, &opts)))?;
        place_view(this, geometry, scale, place, name, eval, "axon_view")
    }

    /// A picture on the sheet: a PNG asset (`asset("render.png")`), its
    /// lower-left corner at `at`, `width` and/or `height` in paper mm (one
    /// keeps the picture's proportions). The PDF and the SVG carry it; a
    /// DXF cannot and leaves it out.
    fn image_view<'v>(
        this: &KSheet,
        #[starlark(require = pos)] image: Value<'v>,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] width: Option<UnpackFloat>,
        #[starlark(require = named)] height: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = p2(&at, "image_view: at")?;
        let (w, h) = (opt_num(&width, "image_view: width")?, opt_num(&height, "image_view: height")?);
        let Some(a) = image.downcast_ref::<KAsset>() else {
            anyhow::bail!("image_view: expected a PNG asset (asset(\"picture.png\")), got {}", image.get_type());
        };
        let st = store(eval)?;
        let Some((_, bytes)) = st.assets.get(&a.name) else {
            anyhow::bail!("image_view: the document has no bytes for asset {:?}", a.name);
        };
        let img = Arc::new(decode_png(&bytes, "image_view")?);
        let aspect = img.width as f64 / img.height.max(1) as f64;
        let (w, h) = match (w, h) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w / aspect),
            (None, Some(h)) => (h * aspect, h),
            (None, None) => anyhow::bail!("image_view: give width= and/or height= in paper mm"),
        };
        if w <= 0.0 || h <= 0.0 {
            anyhow::bail!("image_view: width and height must be positive");
        }
        with_sheet(eval, this, |_, d| {
            let l = d.layer(cadpole_drawing::layer::SYMB);
            d.push(Item::Image(cadpole_drawing::ImageItem { style: cadpole_drawing::Style::on(l), at, w, h, image: img }));
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A run of text at `at` (paper mm); `size` is the cap height in mm;
    /// `align` `"left"`, `"center"` or `"right"` and `valign`
    /// `"baseline"`, `"middle"`, `"top"` or `"bottom"` say how the text
    /// sits on the point; `rotation` in degrees counter-clockwise; `bold`,
    /// `italic`, `font` (`"helvetica"`, `"courier"`, `"inter"`) and `color`
    /// (`"#rrggbb"`, a name, or `(r, g, b)`) as they say. Returns the
    /// text's measured width.
    #[allow(clippy::too_many_arguments)]
    fn text<'v>(
        this: &KSheet,
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named, default = UnpackFloat(2.5))] size: UnpackFloat,
        #[starlark(require = named, default = false)] bold: bool,
        #[starlark(require = named, default = false)] italic: bool,
        #[starlark(require = named, default = UnpackFloat(0.0))] rotation: UnpackFloat,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named)] valign: Option<&str>,
        #[starlark(require = named)] font: Option<&str>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] layer: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let at = p2(&at, "text: at")?;
        let size = num(&size, "text: size")?;
        if size <= 0.0 {
            anyhow::bail!("text: size must be positive");
        }
        let rotation = num(&rotation, "text: rotation")?;
        let anchor = anchor_arg(align, valign, "text")?;
        let color = color_arg(color, "text")?;
        with_sheet(eval, this, |set, d| {
            let f = font_arg(set, font, bold, italic, "text")?;
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::TEXT));
            Ok(ss::text(d, l, at, text, size, rotation, anchor, &f, color))
        })
    }

    /// A note: `text` wrapped to `width` paper mm (or broken only at its
    /// own newlines when `width` is left out), the first baseline at `at`,
    /// lines `line_height` apart (1.45 × size by default). Returns the y of
    /// the next free line.
    #[allow(clippy::too_many_arguments)]
    fn note<'v>(
        this: &KSheet,
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] width: Option<UnpackFloat>,
        #[starlark(require = named, default = UnpackFloat(2.2))] size: UnpackFloat,
        #[starlark(require = named)] line_height: Option<UnpackFloat>,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named, default = false)] bold: bool,
        #[starlark(require = named)] font: Option<&str>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let at = p2(&at, "note: at")?;
        let size = num(&size, "note: size")?;
        let width = opt_num(&width, "note: width")?;
        let lh = opt_num(&line_height, "note: line_height")?.unwrap_or(size * 1.45);
        let h = anchor_arg(align, None, "note")?.h;
        let color = color_arg(color, "note")?;
        with_sheet(eval, this, |set, d| {
            let f = font_arg(set, font, bold, false, "note")?;
            let l = d.layer(cadpole_drawing::layer::TEXT);
            Ok(ss::note(d, l, at, text, width, size, lh, h, &f, color))
        })
    }

    /// A table whose top-left corner is `at`: `rows` are lists of cells
    /// (strings, or numbers written as `str()` writes them -- `format(x,
    /// ".1f")` for a fixed number of decimals), `col_widths` the column
    /// widths in mm;
    /// the first row is a bold header under a heavier rule unless
    /// `header=False`; `align` one of `"left"`, `"center"`, `"right"` per
    /// column; `bold_rows` the indices of rows set bold; a cell wider than
    /// its column wraps and the row grows. Returns the bottom y.
    #[allow(clippy::too_many_arguments)]
    fn table<'v>(
        this: &KSheet,
        #[starlark(require = pos)] rows: Value<'v>,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] col_widths: UnpackList<UnpackFloat>,
        #[starlark(require = named, default = UnpackFloat(2.2))] size: UnpackFloat,
        #[starlark(require = named)] row_height: Option<UnpackFloat>,
        #[starlark(require = named)] align: Option<UnpackList<String>>,
        #[starlark(require = named)] bold_rows: Option<UnpackList<i32>>,
        #[starlark(require = named, default = true)] header: bool,
        #[starlark(require = named)] font: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let heap = eval.heap();
        let at = p2(&at, "table: at")?;
        let size = num(&size, "table: size")?;
        let row_h = opt_num(&row_height, "table: row_height")?.unwrap_or(size * 2.1);
        let widths: Vec<f64> = col_widths.items.iter().map(|w| num(w, "table: col_widths")).collect::<anyhow::Result<_>>()?;
        if widths.is_empty() || widths.iter().any(|w| *w <= 0.0) {
            anyhow::bail!("table: col_widths are positive column widths in mm");
        }
        let mut cells: Vec<Vec<String>> = Vec::new();
        for row in rows.iterate(heap).map_err(|_| anyhow::anyhow!("table: rows is a list of rows"))? {
            let mut r = Vec::new();
            for cell in row.iterate(heap).map_err(|_| anyhow::anyhow!("table: each row is a list of cells"))? {
                r.push(match cell.unpack_str() {
                    Some(s) => s.to_owned(),
                    None => cell.to_str(),
                });
            }
            cells.push(r);
        }
        let align: Vec<HAlign> = match align {
            None => Vec::new(),
            Some(a) => a.items.iter().map(|s| anchor_arg(Some(s), None, "table").map(|a| a.h)).collect::<anyhow::Result<_>>()?,
        };
        let bold: Vec<usize> = bold_rows.map(|b| b.items.iter().map(|i| (*i).max(0) as usize).collect()).unwrap_or_default();
        with_sheet(eval, this, |set, d| {
            let f = font_arg(set, font, false, false, "table")?;
            let line = d.layer(cadpole_drawing::layer::SYMB);
            let text = d.layer(cadpole_drawing::layer::TEXT);
            Ok(ss::table(d, line, text, at, &widths, &cells, size, row_h, &align, &bold, header, &f))
        })
    }

    /// A linear dimension between two paper points at this sheet's scale
    /// (or `scale=`): the number is the model distance, in the set's units
    /// or `units=`, with ticks for feet-inches and arrows otherwise; the
    /// dimension line sits `offset` mm to the left of `p1 -> p2` (negative:
    /// right); `text=` replaces the number; `size` is the text height.
    #[allow(clippy::too_many_arguments)]
    fn dimension<'v>(
        this: &KSheet,
        #[starlark(require = pos)] p1: VecArg,
        #[starlark(require = pos)] p2: VecArg,
        #[starlark(require = named, default = UnpackFloat(8.0))] offset: UnpackFloat,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named)] text: Option<&str>,
        #[starlark(require = named)] size: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let a = crate::check::p2(&p1, "dimension: p1")?;
        let b = crate::check::p2(&p2, "dimension: p2")?;
        let offset = num(&offset, "dimension: offset")?;
        let size = opt_num(&size, "dimension: size")?;
        let scale = scale_arg(scale, "dimension")?;
        with_sheet(eval, this, |set, d| {
            let s = scale.unwrap_or_else(|| set.scale_for(this.index));
            let r = s.ratio();
            let l = d.layer(cadpole_drawing::layer::DIMS);
            let item = dim_item(set, P2::new(a.x / r, a.y / r), P2::new(b.x / r, b.y / r), offset, units.into_option(), text, size, l)?;
            d.push(Item::Group(Group { name: String::new(), source: None, transform: Xform2::new(r, 0.0, 0.0), items: vec![item], clip: None }));
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A north arrow centred at `at` with radius `r` mm, the needle turned
    /// to the set's `north=` (or `angle=`, degrees clockwise from the
    /// page's up); `label` writes "TRUE NORTH" under it.
    fn north_arrow<'v>(
        this: &KSheet,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named, default = UnpackFloat(9.0))] r: UnpackFloat,
        #[starlark(require = named)] angle: Option<UnpackFloat>,
        #[starlark(require = named, default = true)] label: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = p2(&at, "north_arrow: at")?;
        let r = num(&r, "north_arrow: r")?;
        let angle = opt_num(&angle, "north_arrow: angle")?;
        with_sheet(eval, this, |set, d| {
            ss::north_arrow(d, at, r, angle.unwrap_or(set.north_degrees), label, &set.font);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A graphic scale bar with its left end at `at`, about `length` mm
    /// long, divided into round feet (imperial sets) or metres at this
    /// sheet's scale or `scale=`.
    fn scale_bar<'v>(
        this: &KSheet,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] scale: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(60.0))] length: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = p2(&at, "scale_bar: at")?;
        let length = num(&length, "scale_bar: length")?;
        let scale = scale_arg(scale, "scale_bar")?;
        with_sheet(eval, this, |set, d| {
            let s = scale.unwrap_or_else(|| set.scale_for(this.index));
            ss::scale_bar(d, at, s, &set.unit_format, length, &set.font);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A legend at `at`: `items` are `(swatch, label)` pairs with the swatch
    /// one of `"line"`, `"heavy"`, `"dashed"`, `"center"`, `"hatch"`,
    /// `"cross"`, `"dots"`, `"fill"`, `"box"`, `"ring"`. Returns the
    /// bottom y.
    fn legend<'v>(
        this: &KSheet,
        #[starlark(require = pos)] items: Value<'v>,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named, default = UnpackFloat(2.0))] size: UnpackFloat,
        #[starlark(require = named)] title: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let heap = eval.heap();
        let at = p2(&at, "legend: at")?;
        let size = num(&size, "legend: size")?;
        let mut rows: Vec<(Swatch, String)> = Vec::new();
        for item in items.iterate(heap).map_err(|_| anyhow::anyhow!("legend: items is a list of (swatch, label)"))? {
            let pair: Vec<Value<'v>> = item.iterate(heap).map_err(|_| anyhow::anyhow!("legend: each item is (swatch, label)"))?.collect();
            let [k, l] = pair.as_slice() else { anyhow::bail!("legend: each item is (swatch, label)") };
            let (Some(k), Some(l)) = (k.unpack_str(), l.unpack_str()) else { anyhow::bail!("legend: swatch and label are strings") };
            let sw = Swatch::parse(k).ok_or_else(|| anyhow::anyhow!("legend: swatch {k:?} must be one of {}", Swatch::ALL.join(", ")))?;
            rows.push((sw, l.to_owned()));
        }
        with_sheet(eval, this, |set, d| Ok(ss::legend(d, at, &rows, size, title.unwrap_or("LEGEND"), &set.font)))
    }

    /// A hatched region: `region` is a list of paper points or a Sketch
    /// (in paper mm); `pattern` `"lines"`, `"cross"`, `"dots"`, `"solid"`,
    /// `"earth"`, `"concrete"` or `"masonry"`; `spacing` and `angle` in
    /// mm and degrees; `weight` the line weight; `color` the ink.
    #[allow(clippy::too_many_arguments)]
    fn hatch<'v>(
        this: &KSheet,
        #[starlark(require = pos)] region: Value<'v>,
        #[starlark(require = named)] pattern: Option<&str>,
        #[starlark(require = named)] spacing: Option<UnpackFloat>,
        #[starlark(require = named)] angle: Option<UnpackFloat>,
        #[starlark(require = named)] weight: Option<UnpackFloat>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] layer: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let regions = region_arg(region, heap, "hatch")?;
        let p = pattern_arg(pattern, opt_num(&spacing, "hatch: spacing")?, opt_num(&angle, "hatch: angle")?, "hatch")?;
        let weight = opt_num(&weight, "hatch: weight")?;
        let color = color_arg(color, "hatch")?;
        with_sheet(eval, this, |_, d| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SECTION_HATCH));
            for (outer, holes) in regions {
                ss::hatch(d, l, outer, holes, p.clone(), 1.0, weight, color);
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A filled region in `color` (black by default; `"white"` knocks out
    /// what is under it): a list of paper points or a Sketch.
    fn fill<'v>(
        this: &KSheet,
        #[starlark(require = pos)] region: Value<'v>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] layer: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let regions = region_arg(region, heap, "fill")?;
        let color = color_arg(color, "fill")?;
        with_sheet(eval, this, |_, d| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SYMB));
            for (outer, holes) in regions {
                ss::fill(d, l, outer, holes, color);
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A polyline through paper points with a line `weight` (mm), a `dash`
    /// (`"solid"`, `"dashed"`, `"hidden"`, `"center"`, `"phantom"`, or a
    /// list of dash and gap lengths), a `color` and a `layer`.
    #[allow(clippy::too_many_arguments)]
    fn polyline<'v>(
        this: &KSheet,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] dash: Option<Value<'v>>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] closed: bool,
        #[starlark(require = named)] layer: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let pts = points_arg(&points, "polyline: point")?;
        if pts.len() < 2 {
            anyhow::bail!("polyline: needs at least 2 points");
        }
        let weight = num(&weight, "polyline: weight")?;
        let dash = dash_arg(dash, heap, "polyline")?;
        let color = color_arg(color, "polyline")?;
        with_sheet(eval, this, |_, d| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SYMB));
            ss::polyline(d, l, &pts, closed, Some(weight), dash, color);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A grade line: a heavy polyline through paper points (the ground in a
    /// section) with earth hatch under it to `hatch_depth` mm when given.
    fn grade_line<'v>(
        this: &KSheet,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.7))] weight: UnpackFloat,
        #[starlark(require = named)] hatch_depth: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let pts = points_arg(&points, "grade_line: point")?;
        if pts.len() < 2 {
            anyhow::bail!("grade_line: needs at least 2 points");
        }
        let weight = num(&weight, "grade_line: weight")?;
        let depth = opt_num(&hatch_depth, "grade_line: hatch_depth")?;
        with_sheet(eval, this, |_, d| {
            if let Some(depth) = depth.filter(|v| *v > 0.0) {
                let mut poly = pts.clone();
                let lowest = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min) - depth;
                poly.push(P2::new(pts[pts.len() - 1].x, lowest));
                poly.push(P2::new(pts[0].x, lowest));
                let l = d.layer(cadpole_drawing::layer::SECTION_HATCH);
                ss::hatch(d, l, Loop::polygon(&poly), Vec::new(), Pattern::EARTH, 1.0, Some(0.13), None);
            }
            let l = d.layer(cadpole_drawing::layer::SECTION);
            ss::polyline(d, l, &pts, false, Some(weight), None, None);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A rectangle from corner `lo` to corner `hi` (paper mm), stroked at
    /// `weight`; `fill=` a colour fills it instead.
    fn rect<'v>(
        this: &KSheet,
        #[starlark(require = pos)] lo: VecArg,
        #[starlark(require = pos)] hi: VecArg,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] fill: Option<Value<'v>>,
        #[starlark(require = named)] dash: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let (lo, hi) = (p2(&lo, "rect: lo")?, p2(&hi, "rect: hi")?);
        let weight = num(&weight, "rect: weight")?;
        let fill = color_arg(fill, "rect")?;
        let dash = dash_arg(dash, heap, "rect")?;
        let l = Loop::rect(P2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0), (hi.x - lo.x).abs().max(1e-9), (hi.y - lo.y).abs().max(1e-9));
        with_sheet(eval, this, |_, d| {
            let layer = d.layer(cadpole_drawing::layer::SYMB);
            match fill {
                Some(c) => ss::fill(d, layer, l, Vec::new(), Some(c)),
                None => ss::stroke(d, layer, loop_path(&l), true, Some(weight), dash, None),
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A circle of radius `r` at `center` (paper mm), stroked at `weight`
    /// or filled with `fill=`.
    fn circle<'v>(
        this: &KSheet,
        #[starlark(require = named)] center: VecArg,
        #[starlark(require = named)] r: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] fill: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let c = p2(&center, "circle: center")?;
        let r = num(&r, "circle: r")?;
        if r <= 0.0 {
            anyhow::bail!("circle: r must be positive");
        }
        let weight = num(&weight, "circle: weight")?;
        let fill = color_arg(fill, "circle")?;
        let l = Loop::circle(c, r);
        with_sheet(eval, this, |_, d| {
            let layer = d.layer(cadpole_drawing::layer::SYMB);
            match fill {
                Some(col) => ss::fill(d, layer, l, Vec::new(), Some(col)),
                None => ss::stroke(d, layer, loop_path(&l), true, Some(weight), None, None),
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A label with a leader: the text sits at `at` and the arrow points at
    /// `to` (paper mm), through `via` if given.
    fn label<'v>(
        this: &KSheet,
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] to: VecArg,
        #[starlark(require = named)] via: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(2.0))] size: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = p2(&at, "label: at")?;
        let to = p2(&to, "label: to")?;
        let via = via.as_ref().map(|v| p2(v, "label: via")).transpose()?;
        let size = num(&size, "label: size")?;
        let mut pts = vec![to];
        if let Some(v) = via {
            pts.push(v);
        }
        pts.push(at);
        with_sheet(eval, this, |set, d| {
            ss::label(d, &pts, text, size, &set.font);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A drawing title at `at`: a numbered
    /// ring, the title, the scale under it, a rule `width` mm long.
    fn view_title<'v>(
        this: &KSheet,
        #[starlark(require = pos)] number: &str,
        #[starlark(require = pos)] title: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] scale: Option<&str>,
        #[starlark(require = named, default = UnpackFloat(140.0))] width: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = p2(&at, "view_title: at")?;
        let width = num(&width, "view_title: width")?;
        with_sheet(eval, this, |set, d| {
            let scale_text = match scale {
                Some(s) => s.to_owned(),
                None => match set.sheets[this.index].scale {
                    Some(s) => s.label(&set.unit_format),
                    None => String::new(),
                },
            };
            ss::view_title(d, at, number, title, &scale_text, width, &set.font);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// Draw a Sketch's loops and paths on the sheet: its own 2D
    /// coordinates times `scale` (paper mm per sketch unit, 1 by default)
    /// plus `at`; `fill=True` fills its regions instead of stroking.
    #[allow(clippy::too_many_arguments)]
    fn sketch<'v>(
        this: &KSheet,
        #[starlark(require = pos)] sketch: &KSketch,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(1.0))] scale: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] dash: Option<Value<'v>>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] fill: bool,
        #[starlark(require = named)] layer: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let at = at.as_ref().map(|v| p2(v, "sketch: at")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let scale = num(&scale, "sketch: scale")?;
        let weight = num(&weight, "sketch: weight")?;
        let dash = dash_arg(dash, heap, "sketch")?;
        let color = color_arg(color, "sketch")?;
        with_sheet(eval, this, |_, d| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SYMB));
            let mut items = Vec::new();
            sketch_items(sketch, scale, at, Some(weight), dash, color, fill, l, &mut items);
            d.items.extend(items);
            Ok(())
        })?;
        Ok(NoneType)
    }
}

// ---------------------------------------------------------------------------
// SheetView
// ---------------------------------------------------------------------------

#[starlark_value(type = "SheetView")]
impl<'v> StarlarkValue<'v> for KSheetView {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("sheet_view_methods", sheet_view_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "scale" => Some(heap.alloc(self.scale)),
            "at" => Some(heap.alloc((self.tx, self.ty))),
            "clip" => Some(alloc_rect(heap, self.clip)),
            "bounds" => Some(match self.bounds {
                Some(b) => alloc_rect(heap, b),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["scale", "at", "clip", "bounds"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn sheet_view_methods(builder: &mut MethodsBuilder) {
    /// The paper point a point lands on: `(x, y)` in this view's frame, or
    /// `(x, y, z)`, a model point projected as the view projects the model
    /// (a corner of a body on an axonometric lands where the corner is
    /// drawn), so a callout can be placed on the geometry.
    fn point<'v>(this: &KSheetView, #[starlark(require = pos)] p: VecArg, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let q = this.to_paper(this.frame_point(&p, "point")?);
        Ok(heap.alloc((q.x, q.y)))
    }

    /// A linear dimension between two model points of this view's frame:
    /// the number is their true distance in the set's units or `units=`;
    /// the dimension line sits `offset` paper mm to the left of `p1 ->
    /// p2`; `text=` replaces the number.
    fn dimension<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] p1: VecArg,
        #[starlark(require = pos)] p2: VecArg,
        #[starlark(require = named, default = UnpackFloat(8.0))] offset: UnpackFloat,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named)] text: Option<&str>,
        #[starlark(require = named)] size: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let a = crate::check::p2(&p1, "dimension: p1")?;
        let b = crate::check::p2(&p2, "dimension: p2")?;
        let offset = num(&offset, "dimension: offset")?;
        let size = opt_num(&size, "dimension: size")?;
        with_view(eval, this, |set, d, g| {
            let l = d.layer(cadpole_drawing::layer::DIMS);
            g.items.push(dim_item(set, a, b, offset, units.into_option(), text, size, l)?);
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A polyline through points of this view's frame (or 3-D model points,
    /// projected as the view projects the model); weight and
    /// dash in paper mm. `under=True` paints it beneath the view's own line
    /// work.
    #[allow(clippy::too_many_arguments)]
    fn polyline<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] dash: Option<Value<'v>>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] closed: bool,
        #[starlark(require = named)] layer: Option<&str>,
        #[starlark(require = named, default = false)] under: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let pts = this.frame_points(&points, "polyline: point")?;
        if pts.len() < 2 {
            anyhow::bail!("polyline: needs at least 2 points");
        }
        let weight = num(&weight, "polyline: weight")?;
        let dash = dash_arg(dash, heap, "polyline")?;
        let color = color_arg(color, "polyline")?;
        with_view(eval, this, |_, d, g| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SYMB));
            let mut path = Path::new(pts[0]);
            for q in &pts[1..] {
                path = path.line_to(*q);
            }
            target(g, under).push(Item::Stroke(cadpole_drawing::Stroke { style: cadpole_drawing::Style { layer: l, weight_mm: Some(weight), dash, color }, name: String::new(), path, closed }));
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// Text at a point of this view's frame, or at a 3-D model point
    /// projected as the view projects the model; `size` in paper mm, the
    /// rest as `Sheet.text`. Returns the measured width in paper mm.
    #[allow(clippy::too_many_arguments)]
    fn text<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named, default = UnpackFloat(2.5))] size: UnpackFloat,
        #[starlark(require = named, default = false)] bold: bool,
        #[starlark(require = named, default = UnpackFloat(0.0))] rotation: UnpackFloat,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named)] valign: Option<&str>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<f64> {
        let at = this.frame_point(&at, "text: at")?;
        let size = num(&size, "text: size")?;
        let rotation = num(&rotation, "text: rotation")?;
        let anchor = anchor_arg(align, valign, "text")?;
        let color = color_arg(color, "text")?;
        with_view(eval, this, |set, d, g| {
            let f = set.font.with_bold(bold);
            let l = d.layer(cadpole_drawing::layer::TEXT);
            g.items.push(text_item(at, text, size, rotation, anchor, f.clone(), color, l));
            Ok(cadpole_drawing::text::width_in(text, size, &f))
        })
    }

    /// A label with a leader, both ends points of this view's frame or 3-D
    /// model points (projected as the view projects the model, so the
    /// arrow can sit on a body's corner in an axonometric): the text at
    /// `at`, the arrow at `to`.
    fn label<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] text: &str,
        #[starlark(require = named)] at: VecArg,
        #[starlark(require = named)] to: VecArg,
        #[starlark(require = named, default = UnpackFloat(2.0))] size: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let at = this.frame_point(&at, "label: at")?;
        let to = this.frame_point(&to, "label: to")?;
        let size = num(&size, "label: size")?;
        with_view(eval, this, |set, d, g| {
            let l = d.layer(cadpole_drawing::layer::TEXT);
            g.items.push(Item::Leader(cadpole_drawing::ir::Leader {
                style: cadpole_drawing::Style::on(l),
                points: vec![to, at],
                text: text.to_owned(),
                height_mm: size,
                font: set.font.clone(),
            }));
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A hatched region in model coordinates of this view's frame; the
    /// pattern's spacing stays in paper mm. `under=True` paints it beneath
    /// the view's own line work.
    #[allow(clippy::too_many_arguments)]
    fn hatch<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] region: Value<'v>,
        #[starlark(require = named)] pattern: Option<&str>,
        #[starlark(require = named)] spacing: Option<UnpackFloat>,
        #[starlark(require = named)] angle: Option<UnpackFloat>,
        #[starlark(require = named)] weight: Option<UnpackFloat>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] under: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let regions = region_arg(region, heap, "hatch")?;
        let p = pattern_arg(pattern, opt_num(&spacing, "hatch: spacing")?, opt_num(&angle, "hatch: angle")?, "hatch")?;
        let weight = opt_num(&weight, "hatch: weight")?;
        let color = color_arg(color, "hatch")?;
        with_view(eval, this, |_, d, g| {
            let l = d.layer(cadpole_drawing::layer::SECTION_HATCH);
            for (outer, holes) in regions {
                target(g, under).push(Item::Hatch(cadpole_drawing::HatchItem { style: cadpole_drawing::Style { layer: l, weight_mm: weight, dash: None, color }, name: String::new(), outer, holes, pattern: p.clone(), scale: 1.0 }));
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// A filled region in model coordinates of this view's frame, black by
    /// default; `"white"` knocks out what is painted before it.
    /// `under=True` paints it beneath the view's own line work -- a white
    /// roof outline under the projection hides the contours drawn under it.
    fn fill<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] region: Value<'v>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] under: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let regions = region_arg(region, heap, "fill")?;
        let color = color_arg(color, "fill")?;
        with_view(eval, this, |_, d, g| {
            let l = d.layer(cadpole_drawing::layer::SYMB);
            for (outer, holes) in regions {
                target(g, under).push(Item::Hatch(cadpole_drawing::HatchItem { style: cadpole_drawing::Style { layer: l, weight_mm: None, dash: None, color }, name: String::new(), outer, holes, pattern: Pattern::Solid, scale: 1.0 }));
            }
            Ok(())
        })?;
        Ok(NoneType)
    }

    /// Draw a Sketch in this view's frame (its 2D coordinates are the
    /// frame's model coordinates): a site plan's contours on a plan.
    /// `under=True` paints it beneath the view's own line work.
    #[allow(clippy::too_many_arguments)]
    fn sketch<'v>(
        this: &KSheetView,
        #[starlark(require = pos)] sketch: &KSketch,
        #[starlark(require = named, default = UnpackFloat(0.25))] weight: UnpackFloat,
        #[starlark(require = named)] dash: Option<Value<'v>>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named, default = false)] fill: bool,
        #[starlark(require = named)] layer: Option<&str>,
        #[starlark(require = named, default = false)] under: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let weight = num(&weight, "sketch: weight")?;
        let dash = dash_arg(dash, heap, "sketch")?;
        let color = color_arg(color, "sketch")?;
        with_view(eval, this, |_, d, g| {
            let l = d.layer(layer.unwrap_or(cadpole_drawing::layer::SYMB));
            sketch_items(sketch, 1.0, P2::new(0.0, 0.0), Some(weight), dash, color, fill, l, target(g, under));
            Ok(())
        })?;
        Ok(NoneType)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::sketch::Segment;

    #[test]
    fn load_paths_resolve_from_the_loading_file() {
        assert_eq!(resolve_load("main.star", "lib.star"), "lib.star");
        assert_eq!(resolve_load("main.star", "./lib.star"), "lib.star");
        assert_eq!(resolve_load("main.star", "../lib/sheets.star"), "../lib/sheets.star");
        assert_eq!(resolve_load("../lib/sheets.star", "helpers.star"), "../lib/helpers.star");
        assert_eq!(resolve_load("sheets/a.star", "../model.star"), "model.star");
        assert_eq!(resolve_load("sheets/a.star", "b.star"), "sheets/b.star");
        assert_eq!(resolve_load("main.star", "@cadpole/site.star"), "@cadpole/site.star");
        assert_eq!(resolve_load("", "//lib.star"), "lib.star");
    }

    #[test]
    fn a_loop_becomes_a_closed_path() {
        let l = Loop::rect(P2::new(0.0, 0.0), 2.0, 2.0);
        let p = loop_path(&l);
        assert_eq!(p.segments.len(), 4);
        assert!(matches!(p.segments[0], Segment::Line { .. }));
    }

    fn scripts(files: &[(&str, &str)]) -> crate::EvalInput {
        crate::EvalInput {
            scripts: files.iter().map(|(p, s)| crate::ScriptFile { path: (*p).into(), source: (*s).into() }).collect(),
            ..Default::default()
        }
    }

    fn eval(src: &str) -> crate::Evaluation {
        crate::evaluate_full(scripts(&[("main.star", src)]))
    }

    fn errors(ev: &crate::Evaluation) -> Vec<String> {
        ev.ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Error)
            .map(|d| d.message.clone())
            .collect()
    }

    const HOUSE: &str = "house = box(9000, 6000, 5000, name=\"house\")\n";

    /// A 3-D model point lands where the view draws it: on a south
    /// elevation (x, y, z) is the frame's (x, z) whatever y is, and on an
    /// axonometric the eight corners of a box span exactly the paper
    /// bounds of the drawn box. Labels, text and polylines take the same
    /// points.
    #[test]
    fn a_view_takes_three_d_model_points() {
        let src = format!(
            "{HOUSE}\
             s = sheet_set(\"demo\", size=sheet.arch_d, scale=\"1:48\")\n\
             b = s.sheet(\"A1.0\", \"VIEWS\")\n\
             e = b.elevation_view(house, direction=\"south\", at=(400, 100))\n\
             assert_eq(e.point((1000, 0, 2500)), e.point((1000, 2500)))\n\
             assert_eq(e.point((1000, 4000, 2500)), e.point((1000, 2500)))\n\
             for az, el in [(45, 30), (30, 20), (-60, 45)]:\n\
             \x20   ax = b.axon_view(house, az=az, el=el, box=(600, 300, 780, 500), look=\"color\" if az == 30 else \"lines\")\n\
             \x20   ps = [ax.point((x, y, z)) for x in (0, 9000) for y in (0, 6000) for z in (0, 5000)]\n\
             \x20   x0, y0, x1, y1 = ax.bounds\n\
             \x20   assert_true(abs(min([p[0] for p in ps]) - x0) < 1e-6 and abs(max([p[0] for p in ps]) - x1) < 1e-6, msg=str((ps, ax.bounds)))\n\
             \x20   assert_true(abs(min([p[1] for p in ps]) - y0) < 1e-6 and abs(max([p[1] for p in ps]) - y1) < 1e-6, msg=str((ps, ax.bounds)))\n\
             \x20   ax.label(\"ROOF\", at=(9000, 6000, 7000), to=(9000, 6000, 5000))\n\
             \x20   ax.text(\"CORNER\", at=(0, 0, 5000))\n\
             \x20   ax.polyline([(0, 0, 0), (9000, 0, 0), (9000, 0, 5000)])\n\
             x = b.section_view(house, plane=\"y=3000\", at=(400, 300))\n\
             assert_eq(x.point((1000, 3000, 2500)), x.point((1000, 5000, 2500)))\n\
             export(s, \"views.pdf\")\n"
        );
        let ev = eval(&src);
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let src = format!(
            "{HOUSE}s = sheet_set(\"demo\", size=sheet.arch_d, scale=\"1:48\")\nb = s.sheet(\"A1.0\", \"V\")\n\
             ax = b.axon_view(house, box=(600, 300, 780, 500))\nax.point((1, 2, 3, 4))\n"
        );
        assert!(!errors(&eval(&src)).is_empty(), "four numbers is not a point");
    }

    /// The whole vocabulary on two sheets, one export, and the file the
    /// host writes from it: a two-page PDF whose text reads back.
    #[test]
    fn a_set_is_drawn_and_written_as_one_pdf() {
        let src = format!(
            "{HOUSE}\
             s = sheet_set(\"demo\", size=sheet.arch_d, units=\"ft-in\", scale=\"1:48\", title_block=title_block(project=\"DEMO\", stamp=\"CONCEPT\"))\n\
             a = s.sheet(\"G0.0\", \"COVER SHEET\")\n\
             b = s.sheet(\"A1.0\", \"FLOOR PLAN\")\n\
             w = a.text(\"HELLO\", at=(50, 50), size=5, rotation=30, bold=True, color=\"grey\")\n\
             assert_true(w > 10 and w < 30)\n\
             y = a.note(\"one two three four five six seven eight nine ten\", at=(50, 100), width=30)\n\
             assert_true(y < 100 - 6)\n\
             t = a.table([[\"A\", \"B\"], [\"1\", 2.5]], at=(100, 300), col_widths=[20, 30], align=[\"left\", \"right\"])\n\
             assert_true(t < 300)\n\
             a.legend([(\"hatch\", \"CUT\"), (\"dashed\", \"ABOVE\")], at=(200, 300))\n\
             a.north_arrow(at=(300, 300))\n\
             a.scale_bar(at=(300, 100))\n\
             a.hatch([(0, 0), (10, 0), (10, 10)], pattern=\"cross\", spacing=1.5, angle=30)\n\
             a.polyline([(0, 20), (40, 20), (40, 40)], weight=0.7, dash=[4, 1, 1, 1], color=(0.2, 0.2, 0.2))\n\
             a.fill([(60, 60), (70, 60), (70, 70)], color=\"white\")\n\
             a.rect((10, 400), (40, 420), weight=0.5)\n\
             a.circle(center=(80, 410), r=5, fill=\"black\")\n\
             a.label(\"NOTE\", at=(120, 420), to=(100, 400))\n\
             a.view_title(\"1\", \"PLAN\", at=(10, 500))\n\
             a.dimension((10, 200), (73.5, 200), offset=5)\n\
             a.sketch(sketch(XY).circle(5), at=(400, 400))\n\
             v = b.plan_view(house, cut=1200, at=(100, 100), hatch=\"solid\")\n\
             v.dimension((0, 0), (9000, 0), offset=-10)\n\
             v.text(\"ROOM\", at=(4500, 3000), align=\"center\", valign=\"middle\")\n\
             v.label(\"WALL\", at=(10000, 7000), to=(9000, 6000))\n\
             v.hatch([(0, 0), (1000, 0), (1000, 1000)], pattern=\"dots\")\n\
             v.polyline([(0, -500), (9000, -500)], dash=\"center\")\n\
             assert_eq(v.scale, 1.0 / 48)\n\
             assert_eq(v.point((0, 0)), (100.0, 100.0))\n\
             assert_eq(len(v.bounds), 4)\n\
             e = b.elevation_view(house, direction=\"south\", at=(400, 100))\n\
             x = b.section_view(house, plane=\"y=3000\", at=(400, 300), beyond=False)\n\
             ax = b.axon_view(house, box=(600, 300, 780, 500))\n\
             assert_eq(s.sheets(), [\"G0.0\", \"A1.0\"])\n\
             assert_eq(b.scale(), \"1/4\\\" = 1'-0\\\"\")\n\
             assert_true(b.items() > 20)\n\
             export(s, \"demo.pdf\")\n\
             export(b, \"plan.svg\")\n"
        );
        let ev = eval(&src);
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        assert_eq!(ev.sheet_sets.len(), 1);
        assert_eq!(ev.sheet_exports.len(), 2);
        let files = sheet_files(&ev.sheet_sets, &ev.sheet_exports).unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["demo.pdf", "plan.svg"]);
        let pdf = &files[0].data;
        let s = String::from_utf8_lossy(pdf);
        assert_eq!(s.matches("/Type /Page\n").count(), 2, "two pages");
        assert_eq!(s.matches("/MediaBox [0 0 2592 1728]").count(), 2, "ARCH D");
        let text = cadpole_drawing_out::pdf::extract_text(pdf);
        for want in ["COVER SHEET", "FLOOR PLAN", "DEMO", "CONCEPT", "SHEET INDEX", "ROOM", "HELLO", "NOTE", "WALL"] {
            assert!(text.iter().any(|t| t == want), "{want} is not text in {text:?}");
        }
        // 9000 mm is 29'-6 5/16" in feet and inches.
        assert!(text.iter().any(|t| t == "29'-6 5/16\""), "{text:?}");
        let svg = String::from_utf8_lossy(&files[1].data);
        assert!(svg.contains("<title>A1.0 FLOOR PLAN</title>"), "{svg}");
    }

    #[test]
    fn export_says_what_a_sheet_set_can_be_written_as() {
        for (call, want) in [
            ("export(s, \"x.stl\")", ".pdf (every sheet"),
            ("export(s, \"x.pdf\", tolerance=1)", "tolerance= is for bodies"),
            ("export(s, \".pdf\")", "no file name"),
        ] {
            let ev = eval(&format!("s = sheet_set(\"s\")\ns.sheet(\"A1\", \"PLAN\")\n{call}\n"));
            let e = errors(&ev);
            assert!(e.iter().any(|m| m.contains(want)), "{call}: {e:?}");
        }
        // A set with no sheets is refused when it is written, by name.
        let ev = eval("s = sheet_set(\"empty\")\nexport(s, \"e.pdf\")\n");
        assert!(errors(&ev).is_empty());
        let e = sheet_files(&ev.sheet_sets, &ev.sheet_exports).unwrap_err();
        assert!(e.contains("no sheets"), "{e}");
    }

    /// The rows are pending in an ordinary evaluation, and filled
    /// by `evaluate_with_sweeps` from one evaluation per configuration;
    /// the configuration runs never see rows of their own.
    #[test]
    fn a_sweep_table_is_pending_until_a_host_fills_it() {
        let src = "w = param(\"w\", 10)\n\
                   report(\"area\", w * 2)\n\
                   report(\"area\", w * 3)\n\
                   report(\"tag\", \"w%d\" % w)\n\
                   rows = sweep_table([{\"w\": 1}, {\"w\": 2}], labels=[\"one\", \"two\"])\n\
                   ROWS = rows\n\
                   def check():\n\
                   \x20   assert_eq(len(ROWS), 2)\n\
                   test(\"rows\", check)\n";
        let plain = eval(src);
        assert!(errors(&plain).is_empty(), "{:?}", errors(&plain));
        assert_eq!(plain.reports, vec![("area".to_owned(), ParamValue::Number(30.0)), ("tag".to_owned(), ParamValue::Text("w10".into()))]);
        assert_eq!(plain.sweep_requests.len(), 1);
        // Every evaluation but the host's last sees the rows pending, as the
        // viewer does, so a script reads them with a guard (or `.get()`).
        let filled = crate::evaluate_with_sweeps(scripts(&[("main.star", &format!("{src}\n\
            if not rows[0][\"pending\"]:\n\
            \x20   assert_eq([r[\"pending\"] for r in rows], [False, False])\n\
            \x20   assert_eq([r[\"values\"][\"area\"] for r in rows], [3.0, 6.0])\n\
            \x20   assert_eq([r[\"values\"][\"tag\"] for r in rows], [\"w1\", \"w2\"])\n\
            \x20   assert_eq([r[\"label\"] for r in rows], [\"one\", \"two\"])\n\
            \x20   assert_eq(rows[0][\"params\"], {{\"w\": 1}})\n\
            \x20   assert_true(rows[0][\"ok\"])\n\
            \x20   report(\"checked\", True)\n"))]));
        assert!(errors(&filled).is_empty(), "{:?}", errors(&filled));
        assert!(filled.reports.contains(&("checked".to_owned(), ParamValue::Bool(true))), "the filled rows were read: {:?}", filled.reports);
        // The same script evaluated plainly sees every row pending.
        let pending = eval(&format!("{src}\nassert_eq([r[\"pending\"] for r in rows], [True, True])\nassert_eq(rows[0][\"values\"], {{}})\n"));
        assert!(errors(&pending).is_empty(), "{:?}", errors(&pending));
        // A configuration that fails is a row that says so.
        let failing = crate::evaluate_with_sweeps(scripts(&[(
            "main.star",
            "w = param(\"w\", 10)\nif w < 5:\n    fail(\"too narrow\")\nrows = sweep_table([{\"w\": 1}])\n\
             if not rows[0][\"pending\"]:\n    assert_eq(rows[0][\"ok\"], False)\n    assert_true(\"too narrow\" in rows[0][\"error\"])\n    report(\"checked\", True)\n",
        )]));
        assert!(errors(&failing).is_empty(), "{:?}", errors(&failing));
        assert_eq!(failing.reports, vec![("checked".to_owned(), ParamValue::Bool(true))]);
        // And the table names what it cannot do.
        let e = errors(&eval("w = param(\"w\", 1)\nsweep_table([{\"x\": 1}])\n"));
        assert!(e.iter().any(|m| m.contains("\"x\" is not a param")), "{e:?}");
        let e = errors(&eval("w = param(\"w\", 1)\nsweep_table([{\"w\": i} for i in range(65)])\n"));
        assert!(e.iter().any(|m| m.contains("more than 64")), "{e:?}");
    }

    /// A set that names no `units=` -- or says `units=None` -- writes the
    /// document's `units(...)`, its fraction included, as
    /// `drawing()` does; its own `units=` (a string or a member of the
    /// vocabulary) wins, and so does a dimension's.
    #[test]
    fn a_set_takes_the_documents_units() {
        let ev = eval(
            "units(units.ft_in, fraction=8)\n\
             s = sheet_set(\"s\")\n\
             n = sheet_set(\"n\", units=None)\n\
             t = sheet_set(\"t\", units=\"mm\")\n\
             m = sheet_set(\"m\", units=units.m)\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let formats: Vec<(&str, u32)> =
            ev.sheet_sets.iter().map(|s| (s.unit_format.unit.name(), s.unit_format.denominator)).collect();
        assert_eq!(formats, [("ft-in", 8), ("ft-in", 8), ("mm", 0), ("m", 0)]);
        let plain = eval("s = sheet_set(\"s\")\nn = sheet_set(\"n\", units=None)\n");
        assert!(errors(&plain).is_empty(), "{:?}", errors(&plain));
        assert!(plain.sheet_sets.iter().all(|s| s.unit_format == UnitFormat::mm()), "millimetres without a declaration");

        // On the page: 81.28 mm is 3.2", 3 3/16" in sixteenths and 3 1/4" in
        // the document's eighths.
        // A view's dimension and a sheet's take the set's format when they
        // name none (or None); `units="ft-in"` asks for sixteenths again.
        let ev = eval(
            "units(units.ft_in, fraction=8)\n\
             b = box(81.28, 50, 10, name=\"b\")\n\
             s = sheet_set(\"s\", units=None, scale=\"1:1\")\n\
             p = s.sheet(\"A1\", \"PLAN\")\n\
             v = p.plan_view(b, cut=5, at=(100, 100), hatch=\"none\")\n\
             v.dimension((0, 0), (81.28, 0), offset=-10, units=None)\n\
             v.dimension((0, 50), (81.28, 50), offset=10, units=\"ft-in\")\n\
             p.dimension((300, 300), (381.28, 300), offset=5, units=None)\n\
             export(s, \"s.pdf\")\n\
             d = drawing(b, view=\"top\", units=None)\n\
             export_svg(d, \"d.svg\", units=None)\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let files = sheet_files(&ev.sheet_sets, &ev.sheet_exports).unwrap();
        let text = cadpole_drawing_out::pdf::extract_text(&files[0].data);
        assert_eq!(text.iter().filter(|t| *t == "3 1/4\"").count(), 2, "the view's and the sheet's: {text:?}");
        assert_eq!(text.iter().filter(|t| *t == "3 3/16\"").count(), 1, "units= ft-in: {text:?}");
        let svg = String::from_utf8_lossy(&ev.files[0].data);
        assert!(svg.contains("3 1/4&quot;"), "drawing(units=None) is the document's eighths");
    }

    #[test]
    fn a_title_block_is_a_value_or_a_dict_and_a_bad_field_is_named() {
        let ev = eval(
            "t = title_block(project=\"P\", stamp=[\"A\", \"B\"], extra=[(\"ZONE\", \"R-1\")])\n\
             assert_eq(t.project, \"P\")\n\
             assert_eq(t.stamp, [\"A\", \"B\"])\n\
             a = sheet_set(\"a\", title_block=t)\n\
             b = sheet_set(name=\"b\", size=(500, 300), title_block={\"project\": \"Q\", \"notes\": \"one\\ntwo\"})\n\
             assert_eq(b.size(), (500.0, 300.0))\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        assert_eq!(ev.sheet_sets[0].title.extra, vec![("ZONE".to_owned(), "R-1".to_owned())]);
        assert_eq!(ev.sheet_sets[1].title.notes, vec!["one".to_owned(), "two".to_owned()]);
        let e = errors(&eval("sheet_set(\"a\", title_block={\"projekt\": \"P\"})\n"));
        assert!(e.iter().any(|m| m.contains("no field \"projekt\"") && m.contains("block_lot")), "{e:?}");
        let e = errors(&eval("sheet_set(\"a\", size=\"legal\")\n"));
        assert!(e.iter().any(|m| m.contains("arch_d")), "{e:?}");
    }

    /// `under=True` goes beneath the projection, everything else on top,
    /// each in the order it was drawn.
    #[test]
    fn under_paints_beneath_a_views_line_work() {
        let ev = eval(&format!(
            "{HOUSE}s = sheet_set(\"s\", scale=\"1:100\")\n\
             v = s.sheet(\"A1\", \"PLAN\").plan_view(house, cut=10000, at=(100, 100), hatch=\"none\")\n\
             v.fill([(0, 0), (9000, 0), (9000, 6000)], color=\"white\", under=True)\n\
             v.polyline([(0, -100), (9000, -100)], color=\"red\")\n\
             v.polyline([(0, -200), (9000, -200)], color=\"blue\", under=True)\n"
        ));
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let prims = ev.sheet_sets[0].sheets[0].drawing.flatten();
        let colour = |p: &cadpole_drawing::Prim| p.style().color;
        let at = |c: Rgb| prims.iter().position(|p| colour(p) == c).unwrap();
        let (white, blue, red) = (at(Rgb(255, 255, 255)), at(Rgb(30, 60, 200)), at(Rgb(200, 30, 30)));
        let first_line = prims.iter().position(|p| p.style().layer == cadpole_drawing::layer::VISIBLE).unwrap();
        assert!(white < blue && blue < first_line && first_line < red, "{white} {blue} {first_line} {red}");
    }

    /// `crop=` is the part of the model a view shows, in the view's frame:
    /// the projection is cut to it and `box=` centres what is left, while
    /// what the script draws on the view is clipped by the viewport alone.
    /// A house and a driveway running 20 m toward the street, drawn whole
    /// and cropped 3 m in front of the house.
    #[test]
    fn a_crop_cuts_the_projection_and_centres_what_is_left() {
        let src = "house = box(9000, 6000, 5000, name=\"house\")\n\
                   drive = box(3000, 20000, 100, at=(3000, -20000, 0), name=\"drive\")\n\
                   s = sheet_set(\"s\", scale=\"1:100\")\n\
                   w = s.sheet(\"A1\", \"WHOLE\").plan_view([house, drive], cut=1000, box=(100, 100, 300, 400))\n\
                   v = s.sheet(\"A2\", \"CROPPED\").plan_view([house, drive], cut=1000, box=(100, 100, 300, 400), crop=(-1000, -3000, 10000, 7000))\n\
                   v.text(\"STREET\", at=(4500, -8000), align=\"center\")\n\
                   for got, want in zip(v.bounds, (155, 205, 245, 295)):\n\
                   \x20   assert_close(got, want, tol=1e-6)\n\
                   assert_close(w.bounds[1], 120, tol=1e-6)\n";
        let ev = eval(src);
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        // The lowest point of the line work and fills on a sheet, paper mm
        // (a plan of boxes is straight lines, so the ends are enough).
        let lowest_line = |sheet: usize| {
            let mut low = f64::MAX;
            for p in ev.sheet_sets[0].sheets[sheet].drawing.flatten() {
                let pts: Vec<P2> = match &p {
                    cadpole_drawing::Prim::Path { path, .. } => {
                        std::iter::once(path.start).chain(path.segments.iter().map(|s| s.end())).collect()
                    }
                    cadpole_drawing::Prim::Fill { outer, .. } => outer.sample(),
                    _ => continue,
                };
                low = pts.iter().fold(low, |m, q| m.min(q.y));
            }
            low
        };
        assert!((lowest_line(0) - 120.0).abs() < 0.01, "the whole drive: {}", lowest_line(0));
        assert!((lowest_line(1) - 205.0).abs() < 0.01, "cut at the crop: {}", lowest_line(1));
        let street = ev.sheet_sets[0].sheets[1].drawing.flatten().into_iter().find_map(|p| match p {
            cadpole_drawing::Prim::Text { text, at, .. } if text == "STREET" => Some(at),
            _ => None,
        });
        assert!(street.is_some_and(|at| (at.y - 155.0).abs() < 1e-6), "the label outside the crop is drawn: {street:?}");
        let e = errors(&eval(&format!("{HOUSE}sheet_set(\"s\").sheet(\"A1\", \"P\").plan_view(house, cut=1000, crop=(20000, 0, 30000, 100))\n")));
        assert!(e.iter().any(|m| m.contains("crop=(20000.0, 0.0, 30000.0, 100.0) holds none of the view")), "{e:?}");
    }

    /// A loaded file builds, but its exports are its own
    /// document's; its own `load()` is taken from its directory.
    #[test]
    fn a_loaded_file_keeps_its_exports_and_finds_its_siblings() {
        let ev = crate::evaluate_full(scripts(&[
            ("main.star", "load(\"lib/model.star\", \"part\")\nexport(part, \"mine.stl\")\n"),
            (
                "lib/model.star",
                "load(\"dims.star\", \"W\")\npart = box(W, 10, 10, name=\"part\")\nexport(part, \"model.step\")\n\
                 export_svg(sketch(XY).rect(W, 10), \"model.svg\")\ns = sheet_set(\"m\")\ns.sheet(\"A\", \"A\")\nexport(s, \"m.pdf\")\n",
            ),
            ("lib/dims.star", "W = 42\n"),
            // A same-named file at the root must not be the one found.
            ("dims.star", "W = 1\n"),
        ]));
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        assert_eq!(ev.ctx.exports.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["mine.stl"]);
        assert!(ev.files.is_empty() && ev.sheet_exports.is_empty());
        let infos: Vec<&str> = ev.ctx.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(infos.iter().any(|m| m.contains("lib/model.star asked for 3 exports")), "{infos:?}");
        let w = crate::evaluate_full(scripts(&[
            ("main.star", "load(\"lib/model.star\", \"part\")\nassert_eq(part.bbox()[1][0], 42.0)\n"),
            ("lib/model.star", "load(\"dims.star\", \"W\")\npart = box(W, 10, 10, name=\"part\")\n"),
            ("lib/dims.star", "W = 42\n"),
            ("dims.star", "W = 1\n"),
        ]));
        assert!(errors(&w).is_empty(), "{:?}", errors(&w));
    }

    /// A chain of sketch values shares one log, so N polygons cost
    /// N pushes; a branch copies once and the two branches do not see
    /// each other.
    #[test]
    fn a_sketch_chain_shares_one_log_and_a_branch_does_not_leak() {
        let ev = eval(
            "s = sketch(XY)\n\
             for i in range(2000):\n\
             \x20   s = s.polygon([(i * 3, 0), (i * 3 + 1, 0), (i * 3, 1)])\n\
             assert_eq(s.loops, 2000)\n\
             base = sketch(XY).rect(10, 10)\n\
             a = base.circle(1, center=(-2, 0))\n\
             b = base.circle(1, center=(2, 0))\n\
             assert_eq((base.loops, a.loops, b.loops), (1, 2, 2))\n\
             assert_eq(extrude(a, 1).volume() > 0, True)\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let s0 = crate::values::KSketch::new("s".into(), cadpole_core::sketch::Sketch::new(Plane::XY));
        let s1 = s0.appended_for_test(Loop::rect(P2::new(0.0, 0.0), 1.0, 1.0));
        let s2 = s1.appended_for_test(Loop::rect(P2::new(5.0, 0.0), 1.0, 1.0));
        assert!(s1.shares_log_with(&s2), "a linear chain is one log");
        let branch = s1.appended_for_test(Loop::rect(P2::new(9.0, 0.0), 1.0, 1.0));
        assert!(!branch.shares_log_with(&s2), "a branch copies its prefix");
        assert_eq!((s2.loop_count(), branch.loop_count()), (2, 2));
        assert_eq!(branch.sketch().loops[1].segments[0].start().x, 8.5);
    }

    /// An open polyline is a path, bounds nothing, and is written
    /// as an open stroke.
    #[test]
    fn an_open_polyline_is_a_path_and_exports_open() {
        let ev = eval(
            "s = sketch(XY, name=\"grade\").polyline([(0, 0), (10, 2), (20, 1)])\n\
             s = s.add(path((0, 5)).line((5, 5)).arc((10, 5), via=(7.5, 7)), close=False)\n\
             assert_eq((s.loops, s.paths), (0, 2))\n\
             c = sketch(XY).polyline([(0, 0), (10, 0), (10, 10)], closed=True)\n\
             assert_eq((c.loops, c.paths), (1, 0))\n\
             export_svg(s, \"grade.svg\")\n\
             export_pdf(s, \"grade.pdf\")\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let svg = String::from_utf8_lossy(&ev.files[0].data);
        assert_eq!(svg.matches("<path").count(), 2, "{svg}");
        assert!(!svg.contains(" Z"), "open paths are not closed: {svg}");
        assert!(svg.contains(" A "), "the arc stays an arc: {svg}");
        assert!(errors(&eval("sketch(XY).polyline([(0, 0)])\n")).iter().any(|m| m.contains("at least 2")));
    }

    #[test]
    fn a_picture_is_placed_and_a_non_png_is_refused() {
        let mut img = cadpole_core::render::Image::new(4, 2, [1.0, 0.0, 0.0]);
        img.put(0, 0, [0, 0, 255, 128]);
        let mut store = cadpole_assets::MemoryAssetStore::new();
        store.insert("render.png", img.to_png());
        store.insert("notes.txt", b"not a picture".to_vec());
        let mut input = scripts(&[(
            "main.star",
            "s = sheet_set(\"s\")\na = s.sheet(\"A1\", \"RENDER\")\na.image_view(asset(\"render.png\"), at=(10, 10), width=80)\nexport(s, \"r.pdf\")\n",
        )]);
        input.assets = Some(std::sync::Arc::new(store.clone()));
        let ev = crate::evaluate_full(input);
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let prims = ev.sheet_sets[0].sheets[0].drawing.flatten();
        let placed = prims.iter().find_map(|p| match p {
            cadpole_drawing::Prim::Image { w, h, image, .. } => Some((*w, *h, image.width)),
            _ => None,
        });
        assert_eq!(placed, Some((80.0, 40.0, 4)), "80 mm wide keeps the 2:1 picture's proportions");
        let pdf = &sheet_files(&ev.sheet_sets, &ev.sheet_exports).unwrap()[0].data;
        assert!(String::from_utf8_lossy(pdf).contains("/SMask"), "the half-transparent pixel keeps its alpha");
        let mut input = scripts(&[("main.star", "s = sheet_set(\"s\")\ns.sheet(\"A1\", \"X\").image_view(asset(\"notes.txt\"), at=(0, 0), width=10)\n")]);
        input.assets = Some(std::sync::Arc::new(store));
        let e = errors(&crate::evaluate_full(input));
        assert!(e.iter().any(|m| m.contains("not a PNG")), "{e:?}");
    }

    /// `text()` records its string and frame; a body extruded
    /// straight from it is a label of the colour look, a body moved after
    /// is not; and a sheet's colour plan writes the label as text in the
    /// embedded face.
    #[test]
    fn a_text_label_on_a_colour_plan_is_text_in_the_pdf() {
        let ev = eval(
            "floor = box(6000, 4000, 100, at=(0, 0, -100), name=\"floor\")\n\
             wall = box(6000, 200, 2700, name=\"wall\")\n\
             wall.material(color=\"#c9774f\")\n\
             t = text(\"Kitchen\", size=300, align=\"center\", baseline=\"middle\", plane=XY.offset(5), at=(3000, 2000), name=\"k_text\")\n\
             glyphs = extrude(t, 20, name=\"k\")\n\
             moved = translate(extrude(text(\"Moved\", size=300), 20, name=\"m\")[0], (0, 500, 0))\n\
             s = sheet_set(\"s\", size=sheet.arch_b)\n\
             v = s.sheet(\"A1\", \"PLAN\").plan_view([floor, wall] + glyphs, cut=1200, look=\"color\")\n\
             export(s, \"s.pdf\")\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let ctx = &ev.ctx;
        let label = ctx.text_labels.get("k_text").expect("text() recorded its label");
        assert_eq!(label.text, "Kitchen");
        assert_eq!((label.align.as_str(), label.baseline.as_str()), ("center", "middle"));
        assert!((label.plane.origin.z - 5.0).abs() < 1e-12 && (label.at.x - 3000.0).abs() < 1e-12);
        let glyph = ctx.bodies.iter().find(|b| b.feature.0 == "k").expect("a glyph body");
        assert_eq!(ctx.text_label_of(glyph).map(|(f, l)| (f.0.as_str(), l.text.as_str())), Some(("k", "Kitchen")));
        let moved = ctx.bodies.iter().filter(|b| b.feature.0 != "m" && b.name.contains("translate")).count()
            + ctx.bodies.iter().filter(|b| ctx.feature(&b.feature).map(|f| f.kind == "translate").unwrap_or(false)).count();
        assert!(moved > 0, "the moved copy exists");
        for b in ctx.bodies.iter().filter(|b| ctx.feature(&b.feature).map(|f| f.kind == "translate").unwrap_or(false)) {
            assert!(ctx.text_label_of(b).is_none(), "a moved glyph is geometry again");
        }
        let pdf = &sheet_files(&ev.sheet_sets, &ev.sheet_exports).unwrap()[0].data;
        let text = cadpole_drawing_out::pdf::extract_text(pdf);
        assert!(text.iter().any(|t| t == "Kitchen"), "{text:?}");
        assert!(String::from_utf8_lossy(pdf).contains("/FontFile2"), "the face is embedded");
        let e = errors(&eval("s = sheet_set(\"s\")\ns.sheet(\"A1\", \"X\").plan_view(box(1, 1, 1), cut=0.5, look=\"pastel\")\n"));
        assert!(e.iter().any(|m| m.contains("look must be")), "{e:?}");
    }

    /// `export_pdf` deflates, and a drawing set in a standard face
    /// is text a reader can select; the bundled face is text too, embedded.
    #[test]
    fn a_drawing_in_a_standard_face_exports_selectable_text() {
        let ev = eval(
            "b = box(60, 30, 6, name=\"b\")\n\
             export_pdf(drawing(b, view=\"top\", sheet=sheet.a4, title=\"Plate\", font=\"helvetica\"), \"real.pdf\")\n\
             export_pdf(drawing(b, view=\"top\", sheet=sheet.a4, title=\"Plate\"), \"outlines.pdf\")\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
        let real = cadpole_drawing_out::pdf::extract_text(&ev.files[0].data);
        assert!(real.iter().any(|t| t == "Plate"), "{real:?}");
        assert!(real.len() > 3, "the dimensions are text too: {real:?}");
        let embedded = cadpole_drawing_out::pdf::extract_text(&ev.files[1].data);
        assert!(embedded.iter().any(|t| t == "Plate"), "{embedded:?}");
        assert!(String::from_utf8_lossy(&ev.files[1].data).contains("/FontFile2"));
        for f in &ev.files {
            assert!(String::from_utf8_lossy(&f.data).contains("/FlateDecode"), "{} is deflated", f.path);
        }
        let e = errors(&eval("drawing(box(1, 1, 1), font=\"comic\")\n"));
        assert!(e.iter().any(|m| m.contains("helvetica, courier or inter")), "{e:?}");
    }

    #[test]
    fn text_width_measures_the_face_it_is_set_in() {
        let ev = eval(
            "h = text_width(\"Hello\", size=2.5)\n\
             assert_true(abs(h - 2.278 * 2.5 / 0.718) < 1e-9)\n\
             assert_true(text_width(\"Hello\", size=2.5, bold=True) > h)\n\
             assert_true(text_width(\"iiii\", size=2.5, font=\"courier\") > text_width(\"iiii\", size=2.5))\n\
             assert_true(text_width(\"Hello\", size=2.5, font=\"inter\") > 0)\n",
        );
        assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
    }
}
