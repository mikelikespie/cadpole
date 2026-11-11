//! SVG into sketch loops.
//!
//! Cadpole parses SVG itself, over `roxmltree` and `svgtypes` (usvg's own
//! tokenizer), rather than using `usvg`: usvg resolves everything beautifully
//! but hands back Move/Line/Quad/Cubic only, so a circle in the file is
//! already a pile of cubics by the time we see it. For CAD that matters — a
//! circle should extrude to a cylinder `faces(kind="cylinder")` can find — so
//! `A` with equal radii, `circle`, `rect`'s rounded corners and `ellipse`'s
//! circular case all stay [`Segment::Arc`].
//!
//! The file is **parsed, never rendered**:
//! DTDs are refused, so entity-expansion bombs die at the parser; `<script>`,
//! `<foreignObject>`, filters, masks and external `href`s are ignored;
//! `<use>` resolves only inside the document and only eight levels deep.

use std::collections::HashMap;

use cadpole_core::fill::{self, FillRule};
use cadpole_core::sketch::{Loop, P2, Path, Segment};
use svgtypes::{Align, AspectRatio, Length, LengthUnit, PathParser, PathSegment, TransformListParser, TransformListToken, ViewBox};

use crate::curve;
use crate::{Import, Note};

/// Parser caps. A hostile file costs at
/// most this much before it becomes a diagnostic.
const MAX_ELEMENTS: usize = 10_000;
const MAX_SEGMENTS: usize = 100_000;
const MAX_USE_DEPTH: usize = 8;
/// How deep elements may nest, counting both the file's own nesting and the
/// levels `<use>` expansion adds. Real drawings stay in the tens; the cap is
/// what keeps a hostile file from overflowing the stack, which `roxmltree`
/// (a recursive parser) would otherwise do before any other cap applied.
const MAX_NESTING: usize = 256;

/// How `import_svg` reads the file.
#[derive(Clone, Debug)]
pub struct Options {
    /// Element ids to import (a group brings its whole subtree). Empty means
    /// the whole drawing.
    pub ids: Vec<String>,
    /// An Inkscape layer label (`<g inkscape:groupmode="layer"
    /// inkscape:label="Cut">`).
    pub layer: Option<String>,
    /// `"auto"`, or a unit name for the file's user units.
    pub units: String,
    /// User units per inch when the file does not say (Illustrator: 72).
    pub dpi: f64,
    /// Multiplied on after units are resolved.
    pub scale: f64,
    /// Mirror about the viewBox height, because SVG's y points down.
    pub flip_y: bool,
    /// `"auto"` reads each element's `fill-rule`; `"nonzero"` / `"evenodd"`
    /// override it.
    pub fill_rule: String,
    /// Union shapes from different elements when they overlap, so `extrude`
    /// sees one region.
    pub merge: bool,
    /// Flattening tolerance in millimetres for what cannot stay exact.
    pub tolerance: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ids: Vec::new(),
            layer: None,
            units: "auto".into(),
            dpi: 96.0,
            scale: 1.0,
            flip_y: true,
            fill_rule: "auto".into(),
            merge: true,
            tolerance: 0.01,
        }
    }
}

/// A 2x3 affine transform, as SVG's `matrix(a b c d e f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Xf {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Xf {
    const ID: Xf = Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn apply(&self, p: P2) -> P2 {
        P2::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }

    /// `self` then `other` applied to the result: the SVG composition order
    /// for a child inside a transformed parent.
    fn then(&self, other: &Xf) -> Xf {
        Xf {
            a: other.a * self.a + other.c * self.b,
            b: other.b * self.a + other.d * self.b,
            c: other.a * self.c + other.c * self.d,
            d: other.b * self.c + other.d * self.d,
            e: other.a * self.e + other.c * self.f + other.e,
            f: other.b * self.e + other.d * self.f + other.f,
        }
    }

    fn translate(tx: f64, ty: f64) -> Xf {
        Xf { e: tx, f: ty, ..Xf::ID }
    }

    /// True when the transform scales x and y equally and does not shear, so
    /// a circle stays a circle under it.
    fn is_similarity(&self) -> bool {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        let dot = self.a * self.c + self.b * self.d;
        (sx - sy).abs() <= 1e-9 * sx.max(1.0) && dot.abs() <= 1e-9 * (sx * sy).max(1.0)
    }

    fn scale_factor(&self) -> f64 {
        ((self.a * self.d - self.b * self.c).abs()).sqrt()
    }
}

/// Everything one shape element contributed, before fill rules are applied.
struct Shape {
    name: String,
    fill_rule: FillRule,
    filled: bool,
    subpaths: Vec<Vec<Segment>>,
    open: Vec<Vec<Segment>>,
}

struct Ctx<'a, 'input> {
    by_id: HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    opts: &'a Options,
    notes: Vec<Note>,
    shapes: Vec<Shape>,
    elements: usize,
    segments: usize,
    ignored: HashMap<String, usize>,
    /// Ids the caller asked for that were actually found.
    seen_ids: Vec<String>,
    /// The nesting cap was hit; warn once, not once per subtree.
    nesting_warned: bool,
}

/// Read an SVG into loops and construction paths, in millimetres.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Import, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SVG is not valid UTF-8".to_string())?;
    if nesting_exceeds(text, MAX_NESTING) {
        return Err(format!("SVG elements are nested more than {MAX_NESTING} deep; refusing to parse it"));
    }
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions { allow_dtd: false, ..Default::default() },
    )
    .map_err(|e| format!("SVG could not be parsed: {e}"))?;

    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(format!("not an SVG: the root element is <{}>", root.tag_name().name()));
    }

    let mut by_id = HashMap::new();
    for n in doc.descendants().filter(roxmltree::Node::is_element) {
        if let Some(id) = n.attribute("id") {
            by_id.entry(id).or_insert(n);
        }
    }

    let (view_box, mm_per_user, unit_note) = resolve_units(&root, opts);
    let mut ctx = Ctx {
        by_id,
        opts,
        notes: unit_note.into_iter().collect(),
        shapes: Vec::new(),
        elements: 0,
        segments: 0,
        ignored: HashMap::new(),
        seen_ids: Vec::new(),
        nesting_warned: false,
    };

    walk(&mut ctx, root, Xf::ID, (view_box.w, view_box.h), &mut Vec::new(), 1, 0);

    if !opts.ids.is_empty() {
        let missing: Vec<&String> = opts.ids.iter().filter(|id| !ctx.seen_ids.contains(id)).collect();
        if !missing.is_empty() {
            let mut have: Vec<&str> = ctx.by_id.keys().copied().collect();
            have.sort_unstable();
            let shown: Vec<&str> = have.iter().take(24).copied().collect();
            return Err(format!(
                "SVG has no element with id {} (ids in the file: {}{})",
                missing.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", "),
                if shown.is_empty() { "none".to_owned() } else { shown.join(", ") },
                if have.len() > shown.len() { ", ..." } else { "" }
            ));
        }
    }

    let mut ignored: Vec<(String, usize)> = ctx.ignored.into_iter().collect();
    ignored.sort();
    for (kind, n) in ignored {
        ctx.notes.push(Note::info(format!("SVG <{kind}> ignored ({n})")));
    }

    // Fill rules per element, then across elements, then units.
    let mut import = assemble(ctx.shapes, opts, mm_per_user, &mut ctx.notes);
    import.notes.append(&mut ctx.notes);

    let k = mm_per_user * opts.scale;
    let flip_about = view_box.y + view_box.h;
    let (vx, vy) = (view_box.x, view_box.y);
    let flip = opts.flip_y;
    import.map_points(move |p| {
        let x = (p.x - vx) * k;
        let y = if flip { (flip_about - p.y) * k } else { (p.y - vy) * k };
        P2::new(x, y)
    });
    Ok(import)
}

/// The viewBox, millimetres per user unit, and the warning that goes with a
/// file that did not say what its units were.
fn resolve_units(root: &roxmltree::Node<'_, '_>, opts: &Options) -> (ViewBox, f64, Option<Note>) {
    let vb = root
        .attribute("viewBox")
        .and_then(|s| s.parse::<ViewBox>().ok())
        .unwrap_or_else(|| {
            let w = root.attribute("width").and_then(parse_length).map(|l| l.number).unwrap_or(100.0);
            let h = root.attribute("height").and_then(parse_length).map(|l| l.number).unwrap_or(100.0);
            ViewBox::new(0.0, 0.0, w, h)
        });

    // An explicit `units=` always wins: the user is telling us what a user
    // unit means, and no amount of file metadata overrides that.
    if opts.units != "auto" {
        if let Some(k) = crate::unit_scale(&opts.units) {
            return (vb, k, None);
        }
        return (
            vb,
            1.0,
            Some(Note::warn(format!(
                "units {:?} is not one of {}; treating user units as mm",
                opts.units,
                crate::UNIT_NAMES.join(", ")
            ))),
        );
    }

    // `width="120mm"` plus a viewBox is an exact statement of scale.
    let w = root.attribute("width").and_then(parse_length);
    let h = root.attribute("height").and_then(parse_length);
    if let Some(w) = w {
        if let Some(mm) = length_mm(w, opts.dpi) {
            if vb.w > 0.0 && !matches!(w.unit, LengthUnit::None | LengthUnit::Px | LengthUnit::Percent) {
                return (vb, mm / vb.w, None);
            }
        }
    }
    if let Some(h) = h {
        if let Some(mm) = length_mm(h, opts.dpi) {
            if vb.h > 0.0 && !matches!(h.unit, LengthUnit::None | LengthUnit::Px | LengthUnit::Percent) {
                return (vb, mm / vb.h, None);
            }
        }
    }
    // No units anywhere: CSS pixels at `dpi`.
    let note = Note::info(format!(
        "SVG gives no physical size; reading user units as CSS pixels at {} px/in \
         (Illustrator exports at 72 — pass dpi=72, or units= to override)",
        opts.dpi
    ));
    (vb, 25.4 / opts.dpi, Some(note))
}

fn parse_length(s: &str) -> Option<Length> {
    s.parse::<Length>().ok()
}

fn length_mm(l: Length, dpi: f64) -> Option<f64> {
    Some(match l.unit {
        LengthUnit::Mm => l.number,
        LengthUnit::Cm => l.number * 10.0,
        LengthUnit::In => l.number * 25.4,
        LengthUnit::Pt => l.number * 25.4 / 72.0,
        LengthUnit::Pc => l.number * 25.4 / 6.0,
        LengthUnit::None | LengthUnit::Px => l.number * 25.4 / dpi,
        LengthUnit::Em | LengthUnit::Ex | LengthUnit::Percent => return None,
    })
}

/// True when `text` opens elements more than `cap` deep.
///
/// A deliberately small scan over the raw text, run before the real parse:
/// `roxmltree` recurses once per level, so a file of a few hundred kilobytes
/// of `<g>` overflows the stack inside the parser itself. Comments, CDATA,
/// processing instructions and declarations are skipped, and quotes inside a
/// start tag are honoured, so a `>` in an attribute value does not end it.
/// Malformed text is left for the parser to reject.
fn nesting_exceeds(text: &str, cap: usize) -> bool {
    fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
        b.get(from..)?.windows(pat.len()).position(|w| w == pat).map(|i| from + i)
    }
    let b = text.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    while let Some(off) = b[i..].iter().position(|&c| c == b'<') {
        i += off;
        let rest = &b[i..];
        let skip_to = |pat: &[u8]| find(b, i, pat).map(|j| j + pat.len());
        let next = if rest.starts_with(b"<!--") {
            skip_to(b"-->")
        } else if rest.starts_with(b"<![CDATA[") {
            skip_to(b"]]>")
        } else if rest.starts_with(b"<?") {
            skip_to(b"?>")
        } else if rest.starts_with(b"<!") {
            skip_to(b">")
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            skip_to(b">")
        } else {
            let mut j = i + 1;
            let mut quote: Option<u8> = None;
            while j < b.len() {
                let c = b[j];
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None if c == b'"' || c == b'\'' => quote = Some(c),
                    None if c == b'>' => break,
                    None => {}
                }
                j += 1;
            }
            if j >= b.len() {
                return false;
            }
            if b[j - 1] != b'/' {
                depth += 1;
                if depth > cap {
                    return true;
                }
            }
            Some(j + 1)
        };
        match next {
            Some(n) => i = n,
            None => return false,
        }
    }
    false
}

/// Walk the element tree, composing transforms and collecting shapes.
///
/// `path` is the name path built from ancestors: an element
/// contributes its id, or `kind[n]` when it has none, so everything in the
/// file is addressable whether the author named it or not. It is one stack,
/// pushed on the way down and popped on the way up, so a deep file costs
/// memory in proportion to its depth rather than to depth squared. The path
/// is empty exactly when the walk is outside every selected subtree.
///
/// `vp` is the width and height of the current viewport in the parent's
/// user units, which percentages resolve against. `level` counts nesting
/// including `<use>` expansion (the root `<svg>` is 1); `use_depth` counts
/// `<use>` expansions alone.
#[allow(clippy::too_many_arguments)]
fn walk<'a, 'i>(
    ctx: &mut Ctx<'a, 'i>,
    node: roxmltree::Node<'a, 'i>,
    parent_xf: Xf,
    vp: (f64, f64),
    path: &mut Vec<String>,
    level: usize,
    use_depth: usize,
) {
    if ctx.elements >= MAX_ELEMENTS || ctx.segments >= MAX_SEGMENTS {
        return;
    }
    if level > MAX_NESTING {
        if !ctx.nesting_warned {
            ctx.nesting_warned = true;
            ctx.notes.push(Note::warn(format!(
                "SVG elements (counting <use> expansion) are nested deeper than {MAX_NESTING}; \
                 the deeper ones are ignored"
            )));
        }
        return;
    }
    let mut index: HashMap<&str, usize> = HashMap::new();
    for child in node.children().filter(roxmltree::Node::is_element) {
        let tag = child.tag_name().name();
        let n = {
            let e = index.entry(tag).or_insert(0);
            let v = *e;
            *e += 1;
            v
        };
        ctx.elements += 1;
        if ctx.elements >= MAX_ELEMENTS {
            ctx.notes.push(Note::warn(format!("SVG has more than {MAX_ELEMENTS} elements; the rest is ignored")));
            return;
        }
        if hidden(&child) {
            continue;
        }
        let id = child.attribute("id");
        let selected = id.map(|i| ctx.opts.ids.iter().any(|w| w == i)).unwrap_or(false)
            || layer_matches(ctx, &child);
        if selected {
            if let Some(i) = id {
                if !ctx.seen_ids.iter().any(|s| s == i) {
                    ctx.seen_ids.push(i.to_owned());
                }
            }
        }
        // Nothing is imported until we are inside a selected subtree, unless
        // no selection was asked for at all.
        let wanted = ctx.opts.ids.is_empty() && ctx.opts.layer.is_none();
        let inside = wanted || selected || !path.is_empty();
        let own = match id {
            Some(i) => i.to_owned(),
            None => format!("{tag}[{n}]"),
        };
        let xf = local_transform(&child).then(&parent_xf);

        match tag {
            "g" | "svg" | "a" | "switch" => {
                // A nested <svg> opens a new viewport: its x/y/width/height
                // and viewBox map its content into the parent's user space.
                let (xf, vp) = if tag == "svg" {
                    // A zero (or negative) width or height disables the
                    // element and everything in it.
                    let Some((inner, vp)) = viewport(&child, None, vp) else { continue };
                    (inner.then(&xf), vp)
                } else {
                    (xf, vp)
                };
                if inside {
                    path.push(own);
                    walk(ctx, child, xf, vp, path, level + 1, use_depth);
                    path.pop();
                } else {
                    walk(ctx, child, xf, vp, path, level + 1, use_depth);
                }
            }
            "defs" | "symbol" | "clipPath" | "mask" | "marker" | "pattern" | "linearGradient"
            | "radialGradient" | "filter" | "style" => {
                // Definitions are only reached through <use>.
                if tag == "style" {
                    *ctx.ignored.entry("style".into()).or_insert(0) += 1;
                }
            }
            "use" => {
                if !inside {
                    continue;
                }
                if use_depth >= MAX_USE_DEPTH {
                    ctx.notes.push(Note::warn(format!("SVG <use> nested deeper than {MAX_USE_DEPTH}; stopped")));
                    continue;
                }
                let href = child
                    .attribute("href")
                    .or_else(|| child.attribute(("http://www.w3.org/1999/xlink", "href")));
                let Some(href) = href else { continue };
                let Some(target_id) = href.strip_prefix('#') else {
                    ctx.notes.push(Note::warn(format!(
                        "SVG <use href={href:?}> points outside the file; ignored"
                    )));
                    continue;
                };
                let Some(target) = ctx.by_id.get(target_id).copied() else {
                    ctx.notes.push(Note::warn(format!("SVG <use> target {target_id:?} does not exist")));
                    continue;
                };
                let x = user_length(&child, "x", vp.0).unwrap_or(0.0);
                let y = user_length(&child, "y", vp.1).unwrap_or(0.0);
                let xf = Xf::translate(x, y).then(&xf);
                // A <use> of a container expands its children; a <use> of a
                // shape draws that shape. Either way the referenced element
                // is drawn as if it stood here, so its own `transform`
                // applies; a <symbol> or <svg> also brings its viewport,
                // sized by the <use>'s width/height when it has them.
                let tt = target.tag_name().name();
                path.push(own);
                match tt {
                    "symbol" | "svg" => {
                        // As for a nested <svg>: an empty viewport draws nothing.
                        if let Some((inner, inner_vp)) = viewport(&target, Some(&child), vp) {
                            let xf = inner.then(&local_transform(&target)).then(&xf);
                            walk(ctx, target, xf, inner_vp, path, level + 1, use_depth + 1);
                        }
                    }
                    "g" => {
                        let xf = local_transform(&target).then(&xf);
                        walk(ctx, target, xf, vp, path, level + 1, use_depth + 1);
                    }
                    _ => {
                        let xf = local_transform(&target).then(&xf);
                        path.push(tt.to_owned());
                        shape(ctx, target, xf, path);
                        path.pop();
                    }
                }
                path.pop();
            }
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                if inside {
                    path.push(own);
                    shape(ctx, child, xf, path);
                    path.pop();
                }
            }
            "title" | "desc" | "metadata" => {}
            other => {
                *ctx.ignored.entry(other.to_owned()).or_insert(0) += 1;
            }
        }
    }
}

/// A length attribute in user units: plain numbers and `px` as they are,
/// absolute units through CSS's 96 px per inch, percentages of `reference`.
fn user_length(node: &roxmltree::Node<'_, '_>, name: &str, reference: f64) -> Option<f64> {
    let l = parse_length(node.attribute(name)?)?;
    Some(match l.unit {
        LengthUnit::Percent => l.number / 100.0 * reference,
        LengthUnit::Em | LengthUnit::Ex => l.number,
        _ => length_mm(l, 96.0)? * 96.0 / 25.4,
    })
}

/// The transform a nested `<svg>` or a used `<symbol>` applies to its
/// content, and the size of the viewport it establishes (SVG 2 §8.2, the
/// viewBox-to-viewport mapping with `preserveAspectRatio`).
///
/// `node` supplies x, y, width, height, viewBox and preserveAspectRatio; a
/// referencing `<use>` overrides width and height when it sets them. Width
/// and height default to 100 % of the parent viewport `vp`. `None` when the
/// width or height is zero, negative or not a number: SVG disables rendering
/// of such an element, and a scale of 0 would collapse its content into
/// zero-area loops.
fn viewport(node: &roxmltree::Node<'_, '_>, by_use: Option<&roxmltree::Node<'_, '_>>, vp: (f64, f64)) -> Option<(Xf, (f64, f64))> {
    let x = user_length(node, "x", vp.0).unwrap_or(0.0);
    let y = user_length(node, "y", vp.1).unwrap_or(0.0);
    let size = |name: &str, reference: f64| {
        by_use
            .and_then(|u| user_length(u, name, reference))
            .or_else(|| user_length(node, name, reference))
            .unwrap_or(reference)
    };
    let (w, h) = (size("width", vp.0), size("height", vp.1));
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let vb = node.attribute("viewBox").and_then(|s| s.parse::<ViewBox>().ok()).filter(|v| v.w > 0.0 && v.h > 0.0);
    let Some(vb) = vb else {
        return Some((Xf::translate(x, y), (w, h)));
    };
    let ar = node
        .attribute("preserveAspectRatio")
        .and_then(|s| s.parse::<AspectRatio>().ok())
        .unwrap_or(AspectRatio { defer: false, align: Align::XMidYMid, slice: false });
    let (mut sx, mut sy) = (w / vb.w, h / vb.h);
    if ar.align != Align::None {
        let k = if ar.slice { sx.max(sy) } else { sx.min(sy) };
        sx = k;
        sy = k;
    }
    // Where the scaled viewBox sits inside the viewport: 0 = min, 0.5 = mid,
    // 1 = max, per axis.
    let (fx, fy) = match ar.align {
        Align::None | Align::XMinYMin => (0.0, 0.0),
        Align::XMidYMin => (0.5, 0.0),
        Align::XMaxYMin => (1.0, 0.0),
        Align::XMinYMid => (0.0, 0.5),
        Align::XMidYMid => (0.5, 0.5),
        Align::XMaxYMid => (1.0, 0.5),
        Align::XMinYMax => (0.0, 1.0),
        Align::XMidYMax => (0.5, 1.0),
        Align::XMaxYMax => (1.0, 1.0),
    };
    let tx = x - vb.x * sx + fx * (w - vb.w * sx);
    let ty = y - vb.y * sy + fy * (h - vb.h * sy);
    Some((Xf { a: sx, b: 0.0, c: 0.0, d: sy, e: tx, f: ty }, (vb.w, vb.h)))
}

fn layer_matches(ctx: &Ctx<'_, '_>, node: &roxmltree::Node<'_, '_>) -> bool {
    let Some(want) = &ctx.opts.layer else { return false };
    let label = node
        .attribute(("http://www.inkscape.org/namespaces/inkscape", "label"))
        .or_else(|| node.attribute("inkscape:label"));
    label == Some(want.as_str())
}

fn hidden(node: &roxmltree::Node<'_, '_>) -> bool {
    if node.attribute("display") == Some("none") || node.attribute("visibility") == Some("hidden") {
        return true;
    }
    match style_property(node, "display") {
        Some(v) if v == "none" => return true,
        _ => {}
    }
    matches!(style_property(node, "visibility").as_deref(), Some("hidden") | Some("collapse"))
}

/// A presentation property, preferring inline `style=` over the attribute of
/// the same name (which is what CSS specificity says).
fn presentation<'n>(node: &roxmltree::Node<'n, '_>, name: &str) -> Option<String> {
    style_property(node, name).or_else(|| node.attribute(name).map(str::to_owned))
}

fn style_property(node: &roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    let style = node.attribute("style")?;
    for decl in style.split(';') {
        let (k, v) = decl.split_once(':')?;
        if k.trim() == name {
            return Some(v.trim().to_owned());
        }
    }
    None
}

fn local_transform(node: &roxmltree::Node<'_, '_>) -> Xf {
    let Some(s) = node.attribute("transform") else { return Xf::ID };
    let mut acc = Xf::ID;
    for token in TransformListParser::from(s).flatten() {
        let t = match token {
            TransformListToken::Matrix { a, b, c, d, e, f } => Xf { a, b, c, d, e, f },
            TransformListToken::Translate { tx, ty } => Xf::translate(tx, ty),
            TransformListToken::Scale { sx, sy } => Xf { a: sx, d: sy, ..Xf::ID },
            TransformListToken::Rotate { angle } => {
                let r = angle.to_radians();
                Xf { a: r.cos(), b: r.sin(), c: -r.sin(), d: r.cos(), ..Xf::ID }
            }
            TransformListToken::SkewX { angle } => Xf { c: angle.to_radians().tan(), ..Xf::ID },
            TransformListToken::SkewY { angle } => Xf { b: angle.to_radians().tan(), ..Xf::ID },
        };
        acc = t.then(&acc);
    }
    acc
}

fn number(node: &roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    let v = node.attribute(name)?;
    parse_length(v).map(|l| l.number).or_else(|| v.trim().parse().ok())
}

fn points_attr(node: &roxmltree::Node<'_, '_>) -> Vec<P2> {
    let Some(s) = node.attribute("points") else { return Vec::new() };
    svgtypes::PointsParser::from(s).map(|(x, y)| P2::new(x, y)).collect()
}

/// Turn one shape element into subpaths in the *file's* user units, with the
/// element's transform already applied.
fn shape<'a, 'i>(ctx: &mut Ctx<'a, 'i>, node: roxmltree::Node<'a, 'i>, xf: Xf, name: &[String]) {
    let tol_user = (ctx.opts.tolerance / xf.scale_factor().max(1e-9)).max(1e-9);
    let mut subpaths: Vec<Vec<Segment>> = Vec::new();
    let mut open: Vec<Vec<Segment>> = Vec::new();
    match node.tag_name().name() {
        "path" => {
            let Some(d) = node.attribute("d") else { return };
            let (closed, unclosed) = path_data(d, tol_user, &mut ctx.segments);
            subpaths = closed;
            open = unclosed;
        }
        "rect" => {
            let (x, y) = (number(&node, "x").unwrap_or(0.0), number(&node, "y").unwrap_or(0.0));
            let (w, h) = (number(&node, "width").unwrap_or(0.0), number(&node, "height").unwrap_or(0.0));
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let rx = number(&node, "rx").or_else(|| number(&node, "ry")).unwrap_or(0.0);
            let ry = number(&node, "ry").or_else(|| number(&node, "rx")).unwrap_or(0.0);
            let r = rx.min(ry).min(w / 2.0).min(h / 2.0).max(0.0);
            let l = Loop::rounded_rect(P2::new(x + w / 2.0, y + h / 2.0), w, h, r);
            subpaths.push(l.segments);
        }
        "circle" => {
            let c = P2::new(number(&node, "cx").unwrap_or(0.0), number(&node, "cy").unwrap_or(0.0));
            let r = number(&node, "r").unwrap_or(0.0);
            if r <= 0.0 {
                return;
            }
            subpaths.push(curve::circle_loop(c, r).segments);
        }
        "ellipse" => {
            let c = P2::new(number(&node, "cx").unwrap_or(0.0), number(&node, "cy").unwrap_or(0.0));
            let rx = number(&node, "rx").unwrap_or(0.0);
            let ry = number(&node, "ry").unwrap_or(0.0);
            if rx <= 0.0 || ry <= 0.0 {
                return;
            }
            if (rx - ry).abs() <= 1e-9 * rx {
                subpaths.push(curve::circle_loop(c, rx).segments);
            } else {
                subpaths.push(curve::ellipse_cubics(c, rx, ry, 0.0, 0.0, std::f64::consts::TAU, tol_user));
            }
        }
        "line" => {
            let a = P2::new(number(&node, "x1").unwrap_or(0.0), number(&node, "y1").unwrap_or(0.0));
            let b = P2::new(number(&node, "x2").unwrap_or(0.0), number(&node, "y2").unwrap_or(0.0));
            open.push(vec![Segment::Line { a, b }]);
        }
        "polyline" | "polygon" => {
            let pts = points_attr(&node);
            if pts.len() < 2 {
                return;
            }
            let closed = node.tag_name().name() == "polygon";
            let n = if closed { pts.len() } else { pts.len() - 1 };
            let segs: Vec<Segment> =
                (0..n).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect();
            if closed {
                subpaths.push(segs);
            } else {
                open.push(segs);
            }
        }
        _ => return,
    }

    // Circular arcs only survive a similarity transform; under a shear or a
    // non-uniform scale they become cubics, which is what the design promises.
    let similarity = xf.is_similarity();
    let mut degrade = false;
    for segs in subpaths.iter_mut().chain(open.iter_mut()) {
        if !similarity {
            let mut out = Vec::with_capacity(segs.len());
            for s in segs.iter() {
                if let Segment::Arc { a, b, via } = s {
                    degrade = true;
                    out.extend(arc_to_cubics(*a, *b, *via, tol_user));
                } else {
                    out.push(s.clone());
                }
            }
            *segs = out;
        }
        for s in segs.iter_mut() {
            curve::map_segment(s, |p| xf.apply(p));
        }
    }
    if degrade {
        ctx.notes.push(Note::info(format!(
            "{} is under a non-uniform transform; its arcs became Béziers within tolerance",
            name.join("/")
        )));
    }

    ctx.segments += subpaths.iter().chain(open.iter()).map(Vec::len).sum::<usize>();
    if ctx.segments >= MAX_SEGMENTS {
        ctx.notes.push(Note::warn(format!("SVG has more than {MAX_SEGMENTS} path segments; the rest is ignored")));
    }

    let fill = presentation(&node, "fill").unwrap_or_else(|| "black".into());
    let filled = fill != "none";
    let rule = match ctx.opts.fill_rule.as_str() {
        "nonzero" => FillRule::NonZero,
        "evenodd" => FillRule::EvenOdd,
        _ => presentation(&node, "fill-rule")
            .as_deref()
            .and_then(FillRule::from_name)
            .unwrap_or(FillRule::NonZero),
    };
    if !subpaths.is_empty() || !open.is_empty() {
        ctx.shapes.push(Shape { name: name.join("/"), fill_rule: rule, filled, subpaths, open });
    }
}

fn arc_to_cubics(a: P2, b: P2, via: P2, tol: f64) -> Vec<Segment> {
    match cadpole_core::sketch::arc_params(a, b, via) {
        Some((c, r, t0, t1)) => curve::ellipse_cubics(c, r, r, 0.0, t0, t1 - t0, tol),
        None => vec![Segment::Line { a, b }],
    }
}

/// Parse a `d` attribute into closed and open subpaths.
///
/// Every command is handled, in both absolute and relative form; `S`/`T`
/// reflect the previous control point, `Z` closes back to the subpath's start
/// and a command after `Z` restarts there, as the spec says.
fn path_data(d: &str, tolerance: f64, budget: &mut usize) -> (Vec<Vec<Segment>>, Vec<Vec<Segment>>) {
    let mut closed = Vec::new();
    let mut open = Vec::new();
    let mut cur = P2::new(0.0, 0.0);
    let mut start = cur;
    let mut segs: Vec<Segment> = Vec::new();
    let mut is_closed = false;
    // Reflection state for S / T.
    let mut last_cubic_ctrl: Option<P2> = None;
    let mut last_quad_ctrl: Option<P2> = None;

    let flush = |segs: &mut Vec<Segment>, is_closed: &mut bool, closed: &mut Vec<Vec<Segment>>, open: &mut Vec<Vec<Segment>>| {
        if segs.is_empty() {
            *is_closed = false;
            return;
        }
        if *is_closed { closed.push(std::mem::take(segs)) } else { open.push(std::mem::take(segs)) }
        *is_closed = false;
    };

    for seg in PathParser::from(d).flatten() {
        if *budget >= MAX_SEGMENTS {
            break;
        }
        *budget += 1;
        let abs_point = |abs: bool, x: f64, y: f64, cur: P2| {
            if abs { P2::new(x, y) } else { P2::new(cur.x + x, cur.y + y) }
        };
        match seg {
            PathSegment::MoveTo { abs, x, y } => {
                flush(&mut segs, &mut is_closed, &mut closed, &mut open);
                cur = abs_point(abs, x, y, cur);
                start = cur;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            PathSegment::LineTo { abs, x, y } => {
                let p = abs_point(abs, x, y, cur);
                segs.push(Segment::Line { a: cur, b: p });
                cur = p;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                let p = P2::new(if abs { x } else { cur.x + x }, cur.y);
                segs.push(Segment::Line { a: cur, b: p });
                cur = p;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            PathSegment::VerticalLineTo { abs, y } => {
                let p = P2::new(cur.x, if abs { y } else { cur.y + y });
                segs.push(Segment::Line { a: cur, b: p });
                cur = p;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            PathSegment::CurveTo { abs, x1, y1, x2, y2, x, y } => {
                let c1 = abs_point(abs, x1, y1, cur);
                let c2 = abs_point(abs, x2, y2, cur);
                let p = abs_point(abs, x, y, cur);
                segs.push(Segment::Bezier { a: cur, b: p, ctrl: vec![c1, c2] });
                cur = p;
                last_cubic_ctrl = Some(c2);
                last_quad_ctrl = None;
            }
            PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                let c1 = match last_cubic_ctrl {
                    Some(c) => P2::new(2.0 * cur.x - c.x, 2.0 * cur.y - c.y),
                    None => cur,
                };
                let c2 = abs_point(abs, x2, y2, cur);
                let p = abs_point(abs, x, y, cur);
                segs.push(Segment::Bezier { a: cur, b: p, ctrl: vec![c1, c2] });
                cur = p;
                last_cubic_ctrl = Some(c2);
                last_quad_ctrl = None;
            }
            PathSegment::Quadratic { abs, x1, y1, x, y } => {
                let c = abs_point(abs, x1, y1, cur);
                let p = abs_point(abs, x, y, cur);
                segs.push(Segment::Bezier { a: cur, b: p, ctrl: vec![c] });
                cur = p;
                last_quad_ctrl = Some(c);
                last_cubic_ctrl = None;
            }
            PathSegment::SmoothQuadratic { abs, x, y } => {
                let c = match last_quad_ctrl {
                    Some(q) => P2::new(2.0 * cur.x - q.x, 2.0 * cur.y - q.y),
                    None => cur,
                };
                let p = abs_point(abs, x, y, cur);
                segs.push(Segment::Bezier { a: cur, b: p, ctrl: vec![c] });
                cur = p;
                last_quad_ctrl = Some(c);
                last_cubic_ctrl = None;
            }
            PathSegment::EllipticalArc { abs, rx, ry, x_axis_rotation, large_arc, sweep, x, y } => {
                let p = abs_point(abs, x, y, cur);
                segs.extend(curve::svg_arc(cur, rx, ry, x_axis_rotation, large_arc, sweep, p, tolerance));
                cur = p;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            PathSegment::ClosePath { .. } => {
                if !segs.is_empty() {
                    let end = segs.last().unwrap().end();
                    if (end.x - start.x).abs() > curve::EPS || (end.y - start.y).abs() > curve::EPS {
                        segs.push(Segment::Line { a: end, b: start });
                    }
                    is_closed = true;
                    flush(&mut segs, &mut is_closed, &mut closed, &mut open);
                }
                cur = start;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
        }
    }
    flush(&mut segs, &mut is_closed, &mut closed, &mut open);
    (closed, open)
}

/// Apply fill rules and assemble the final loops and paths.
///
/// Per element: if the element's own subpaths cross, its rule decides, and
/// `i_overlay` rebuilds them (which flattens curves, so we only do it when
/// they really cross — both rules agree with containment otherwise). Across
/// elements: `merge` unions filled shapes that overlap, so `extrude` sees one
/// region instead of two bodies fighting over the same area.
///
/// "Overlap" means the outlines cross. A filled shape wholly inside another
/// element's region is not unioned: it nests, and containment makes it a
/// hole (or an island inside a hole), exactly as it would with no other
/// shape in the file. The union is done per group of elements whose
/// outlines cross, so one crossing never changes how unrelated shapes nest.
fn assemble(shapes: Vec<Shape>, opts: &Options, mm_per_user: f64, notes: &mut Vec<Note>) -> Import {
    // Tolerance is in millimetres; the geometry is still in user units here.
    let tol = (opts.tolerance / (mm_per_user * opts.scale).max(1e-12)).max(1e-9);
    let mut out = Import::default();
    let mut filled: Vec<(String, Loop)> = Vec::new();
    // Which element each filled loop came from, and the rule its holes
    // follow: the element's own, or parity once its loops were rebuilt
    // (rebuilt loops nest cleanly, so parity reads them right).
    let mut element: Vec<usize> = Vec::new();
    let mut rules: Vec<FillRule> = Vec::new();
    for (e, shape) in shapes.into_iter().enumerate() {
        for (i, segs) in shape.open.iter().enumerate() {
            if segs.is_empty() {
                continue;
            }
            out.paths.push((
                format!("{}/path[{i}]", shape.name),
                Path { start: segs[0].start(), segments: segs.clone() },
            ));
        }
        if shape.subpaths.is_empty() {
            continue;
        }
        let mut loops: Vec<Loop> = shape.subpaths.into_iter().map(|segments| Loop { segments }).collect();
        if !shape.filled {
            // `fill="none"`: the outline is construction geometry, not a face.
            for (i, l) in loops.into_iter().enumerate() {
                let start = l.segments[0].start();
                out.paths.push((format!("{}/outline[{i}]", shape.name), Path { start, segments: l.segments }));
            }
            continue;
        }
        let mut rule = shape.fill_rule;
        if loops.len() > 1 && fill::loops_cross(&loops, tol) {
            notes.push(Note::info(format!(
                "{}: its subpaths cross, so the {} rule was applied and the loops rebuilt \
                 (segment indices no longer match the file)",
                shape.name,
                if shape.fill_rule == FillRule::EvenOdd { "even-odd" } else { "nonzero" }
            )));
            loops = fill::rebuild(&loops, shape.fill_rule, tol);
            rule = FillRule::EvenOdd;
        }
        for (i, l) in loops.into_iter().enumerate() {
            filled.push((format!("{}/loop[{i}]", shape.name), l));
            element.push(e);
        }
        while rules.len() <= e {
            rules.push(rule);
        }
        rules[e] = rule;
    }

    if opts.merge && filled.len() > 1 {
        filled = merge_crossing_elements(filled, &element, &rules, tol, notes);
    }
    out.loops = filled;
    out
}

/// Union the elements whose outlines cross, group by group.
///
/// Elements are joined into groups through any pair of their loops that
/// cross. A group of one is left exactly as it was, arcs and names intact.
/// A larger group is rebuilt under nonzero after each element's loops are
/// wound so that its own region has winding number one and everything else
/// zero (outer loops counter-clockwise, its holes clockwise, by nesting
/// depth within the element). The nonzero sum is then exactly the union of
/// the elements' regions: a hole one element punched in itself survives
/// unless another element of the group covers it.
fn merge_crossing_elements(
    filled: Vec<(String, Loop)>,
    element: &[usize],
    rules: &[FillRule],
    tol: f64,
    notes: &mut Vec<Note>,
) -> Vec<(String, Loop)> {
    let n = filled.len();
    // Flatten once; the crossing test then works on cheap polygons.
    let polys: Vec<Loop> = filled.iter().map(|(_, l)| Loop::polygon(&l.sample_tolerance(tol))).collect();
    let boxes: Vec<(f64, f64, f64, f64)> = polys
        .iter()
        .map(|l| {
            l.segments.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, s| {
                let p = s.start();
                (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y))
            })
        })
        .collect();

    let elements = element.iter().copied().max().map_or(0, |m| m + 1);
    let mut parent: Vec<usize> = (0..elements).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (boxes[i], boxes[j]);
            if a.0 > b.2 || b.0 > a.2 || a.1 > b.3 || b.1 > a.3 {
                continue;
            }
            let (ri, rj) = (find(&mut parent, element[i]), find(&mut parent, element[j]));
            if ri == rj {
                continue;
            }
            if fill::loops_cross(&[polys[i].clone(), polys[j].clone()], tol) {
                parent[ri] = rj;
            }
        }
    }

    // Members of each group, by loop index, in file order.
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, element[i]);
        groups.entry(r).or_default().push(i);
    }
    let crossing: Vec<usize> = groups
        .iter()
        .filter(|(_, members)| members.iter().any(|&i| element[i] != element[members[0]]))
        .map(|(&r, _)| r)
        .collect();
    if crossing.is_empty() {
        return filled;
    }
    notes.push(Note::info(
        "filled shapes from different elements overlap; they were unioned into one region \
         (pass merge=False to keep them separate)"
            .to_string(),
    ));

    let mut out = Vec::with_capacity(n);
    let mut merged = 0usize;
    let mut done: Vec<bool> = vec![false; n];
    for i in 0..n {
        if done[i] {
            continue;
        }
        let r = find(&mut parent, element[i]);
        if !crossing.contains(&r) {
            done[i] = true;
            out.push(filled[i].clone());
            continue;
        }
        // The whole group goes out here, where its first loop stood.
        let members = &groups[&r];
        // Each loop wound as its own element's rule makes it: outward when
        // the element fills just inside it and not just outside, inward for
        // the reverse, and left out when it separates nothing (a same-wound
        // square inside a nonzero square is filled on both sides).
        let mut wound: Vec<Loop> = Vec::with_capacity(members.len());
        for &k in members {
            done[k] = true;
            let probe = filled[k].1.segments[0].start();
            let around = members.iter().filter(|&&m| m != k && element[m] == element[k] && filled[m].1.contains_point(probe));
            let l = &filled[k].1;
            let sign = |l: &Loop| if l.signed_area() > 0.0 { 1i64 } else { -1 };
            let (inside, outside) = match rules.get(element[k]).copied().unwrap_or(FillRule::EvenOdd) {
                FillRule::NonZero => {
                    let w: i64 = around.map(|&m| sign(&filled[m].1)).sum();
                    (w + sign(l) != 0, w != 0)
                }
                FillRule::EvenOdd => {
                    let depth = around.count();
                    (depth % 2 == 0, depth % 2 == 1)
                }
            };
            if inside == outside {
                continue;
            }
            wound.push(if (l.signed_area() > 0.0) == inside { l.clone() } else { l.reversed() });
        }
        for l in fill::rebuild(&wound, FillRule::NonZero, tol) {
            out.push((format!("merged/loop[{merged}]"), l));
            merged += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(svg: &str) -> Import {
        import(svg.as_bytes(), &Options { tolerance: 0.001, ..Default::default() }).unwrap()
    }

    const EVERY_COMMAND: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <path id="all" d="M 10 10 H 40 V 20 L 50 30 C 60 30 70 20 70 10 S 80 0 90 10 Q 95 20 90 30 T 80 50 A 10 10 0 0 1 60 50 Z"/>
</svg>"#;

    #[test]
    fn every_path_command_is_parsed_and_the_arc_stays_an_arc() {
        let im = run(EVERY_COMMAND);
        assert_eq!(im.loops.len(), 1, "one closed subpath");
        let (name, l) = &im.loops[0];
        assert_eq!(name, "all/loop[0]");
        let kinds: Vec<&str> = l
            .segments
            .iter()
            .map(|s| match s {
                Segment::Line { .. } => "line",
                Segment::Arc { .. } => "arc",
                Segment::Bezier { ctrl, .. } if ctrl.len() == 1 => "quad",
                Segment::Bezier { .. } => "cubic",
            })
            .collect();
        // H V L, C S, Q T, A, then Z's closing line.
        assert_eq!(kinds, vec!["line", "line", "line", "cubic", "cubic", "quad", "quad", "arc", "line"], "{kinds:?}");
        assert!(l.is_closed(1e-9));
    }

    #[test]
    fn relative_commands_reach_the_same_place_as_absolute_ones() {
        let abs = run(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><path d="M 10 10 L 30 10 L 30 30 Z"/></svg>"#);
        let rel = run(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><path d="m 10 10 l 20 0 l 0 20 z"/></svg>"#);
        let a: Vec<P2> = abs.loops[0].1.segments.iter().map(|s| s.start()).collect();
        let b: Vec<P2> = rel.loops[0].1.segments.iter().map(|s| s.start()).collect();
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(b.iter()) {
            assert!((p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9, "{p:?} vs {q:?}");
        }
    }

    #[test]
    fn a_nested_subpath_becomes_a_hole() {
        // Outer square 0..40, inner square 10..30, one <path>, two subpaths.
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40">
  <path id="plate" d="M0 0 H40 V40 H0 Z M10 10 H30 V30 H10 Z"/>
</svg>"#);
        assert_eq!(im.loops.len(), 2);
        let s = cadpole_core::Sketch::new(cadpole_core::Plane::XY);
        let s = im.loops.iter().fold(s, |s, (n, l)| s.with_named_loop(l.clone(), n.clone()));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1, "the inner subpath is a hole");
        assert!((regions[0].outer.signed_area() - 1600.0).abs() < 1e-6);
        assert!((regions[0].holes[0].signed_area() + 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_circle_stays_two_arcs_and_is_the_right_size_in_mm() {
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><circle id="hole" cx="50" cy="50" r="10"/></svg>"#);
        assert_eq!(im.loops.len(), 1);
        let l = &im.loops[0].1;
        assert_eq!(l.segments.len(), 2);
        // Two real arcs of radius 10 mm, not a pile of cubics: this is what
        // makes the extruded hole a cylinder rather than a spline surface.
        for s in &l.segments {
            let Segment::Arc { a, b, via } = s else { panic!("expected arcs, got {s:?}") };
            let (_, r, _, _) = cadpole_core::sketch::arc_params(*a, *b, *via).unwrap();
            assert!((r - 10.0).abs() < 1e-9, "radius {r}");
        }
        let (lo, hi) = im.bbox().unwrap();
        assert!((hi.x - lo.x - 20.0).abs() < 0.01 && (hi.y - lo.y - 20.0).abs() < 0.01, "{lo:?} {hi:?}");
    }

    #[test]
    fn groups_use_and_transforms_compose() {
        let im = run(r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100mm" height="100mm" viewBox="0 0 100 100">
  <defs><rect id="tile" x="0" y="0" width="10" height="10"/></defs>
  <g id="row" transform="translate(20,20)">
    <use xlink:href="#tile"/>
    <use xlink:href="#tile" x="20"/>
    <g transform="scale(2)"><use href="#tile" x="20"/></g>
  </g>
</svg>"##);
        assert_eq!(im.loops.len(), 3);
        let names: Vec<&str> = im.loops.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names[0], "row/use[0]/rect/loop[0]");
        // The scaled tile is 20 mm on a side, the others 10.
        let areas: Vec<f64> = im.loops.iter().map(|(_, l)| l.signed_area().abs()).collect();
        assert!((areas[0] - 100.0).abs() < 1e-6, "{areas:?}");
        assert!((areas[2] - 400.0).abs() < 1e-6, "the scale(2) group doubles it: {areas:?}");
    }

    #[test]
    fn ids_select_a_subtree_and_a_bad_id_lists_what_exists() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <g id="badge"><rect x="0" y="0" width="10" height="10"/></g>
  <g id="frame"><rect x="50" y="0" width="10" height="10"/></g>
</svg>"#;
        let im = import(svg.as_bytes(), &Options { ids: vec!["badge".into()], ..Default::default() }).unwrap();
        assert_eq!(im.loops.len(), 1);
        assert!(im.loops[0].0.starts_with("badge/"), "{}", im.loops[0].0);
        let err = import(svg.as_bytes(), &Options { ids: vec!["nope".into()], ..Default::default() }).unwrap_err();
        assert!(err.contains("\"nope\""), "{err}");
        assert!(err.contains("badge") && err.contains("frame"), "the error lists the ids: {err}");
    }

    #[test]
    fn y_is_flipped_by_default_and_units_come_from_the_root() {
        // A 10 mm square at the top of a 40 mm tall drawing lands at the top
        // in sketch coordinates too, because y is flipped.
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><rect x="0" y="0" width="10" height="10"/></svg>"#);
        let (lo, hi) = im.bbox().unwrap();
        assert!((lo.y - 30.0).abs() < 1e-6 && (hi.y - 40.0).abs() < 1e-6, "{lo:?} {hi:?}");
        let flat = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><rect x="0" y="0" width="10" height="10"/></svg>"#.as_bytes(),
            &Options { flip_y: false, ..Default::default() },
        )
        .unwrap();
        let (lo, _) = flat.bbox().unwrap();
        assert!(lo.y.abs() < 1e-6);
    }

    #[test]
    fn a_unitless_file_is_css_pixels_and_says_so() {
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect x="0" y="0" width="96" height="96"/></svg>"#);
        let (lo, hi) = im.bbox().unwrap();
        assert!((hi.x - lo.x - 25.4).abs() < 1e-6, "96 px at 96 dpi is one inch: {}", hi.x - lo.x);
        assert!(im.notes.iter().any(|n| n.message.contains("CSS pixels")), "{:?}", im.notes);
    }

    #[test]
    fn crossing_subpaths_are_rebuilt_under_the_elements_fill_rule() {
        // Two overlapping squares in one path, even-odd: the overlap is a hole.
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40">
  <path id="x" fill-rule="evenodd" d="M0 0 H20 V20 H0 Z M10 10 H30 V30 H10 Z"/>
</svg>"#);
        assert!(im.notes.iter().any(|n| n.message.contains("even-odd")), "{:?}", im.notes);
        let net: f64 = im.loops.iter().map(|(_, l)| l.signed_area()).sum::<f64>().abs();
        assert!((net - (400.0 + 400.0 - 2.0 * 100.0)).abs() < 1e-3, "{net}");
    }

    fn regions_of(svg: &str) -> Vec<cadpole_core::sketch::Region> {
        let im = run(svg);
        let s = im
            .loops
            .iter()
            .fold(cadpole_core::Sketch::new(cadpole_core::Plane::XY), |s, (n, l)| s.with_named_loop(l.clone(), n.clone()));
        s.regions().unwrap()
    }

    #[test]
    fn an_unrelated_crossing_does_not_fill_a_nested_shape() {
        // A disc with a smaller disc inside it (a separate element), and a
        // tab across the big disc's edge far from the small one. The tab
        // unions with the disc; the small disc stays what it is without the
        // tab, a hole. It used to vanish: once any outline crossed, every
        // loop in the file was rebuilt under nonzero.
        let regions = regions_of(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="30"/>
  <circle cx="50" cy="50" r="9"/>
  <rect x="75" y="45" width="20" height="10"/>
</svg>"#,
        );
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1, "the inner disc is still a hole");
        let hole = regions[0].holes[0].signed_area().abs();
        // `signed_area` samples an arc at 16 steps, ~0.6 % under the exact
        // area of a circle; the hole is still the r = 9 circle.
        assert!((hole - std::f64::consts::PI * 81.0).abs() < 0.01 * std::f64::consts::PI * 81.0, "hole area {hole}");
        assert_eq!(regions[0].holes[0].segments.len(), 2, "the hole kept its arcs");
    }

    #[test]
    fn crossing_shapes_union_and_what_sits_inside_the_union_nests() {
        // A logo: a head with a tail across its outline, an eye inside the
        // head and a pupil inside the eye. Head and tail are one region; the
        // eye is a hole in it and the pupil an island, as with no tail.
        let regions = regions_of(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <circle id="head" cx="40" cy="50" r="25"/>
  <rect id="tail" x="60" y="45" width="30" height="10"/>
  <circle id="eye" fill="white" cx="35" cy="45" r="8"/>
  <circle id="pupil" cx="35" cy="45" r="3"/>
</svg>"#,
        );
        assert_eq!(regions.len(), 2, "the head with its tail, and the pupil");
        let head = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
        assert_eq!(head.holes.len(), 1, "the eye");
        assert!(regions.iter().any(|r| r.holes.is_empty() && (r.outer.signed_area() - std::f64::consts::PI * 9.0).abs() < 0.5));
    }

    #[test]
    fn a_hole_survives_a_merge_with_another_element_under_the_elements_rule() {
        // A second element crossing the outer square's edge is unioned with
        // it; the inner square stays what the path's own fill rule makes it.
        let merged = |path: &str| {
            regions_of(&format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="60mm" height="60mm" viewBox="0 0 60 60">
  {path}
  <rect x="35" y="15" width="20" height="10"/>
</svg>"#
            ))
        };
        let with_window = 1600.0 - 400.0 + 15.0 * 10.0;
        // evenodd, and nonzero with the inner square wound the other way:
        // the window is a hole.
        for path in [
            r#"<path fill-rule="evenodd" d="M0 0 H40 V40 H0 Z M10 10 H30 V30 H10 Z"/>"#,
            r#"<path d="M0 0 H40 V40 H0 Z M10 10 V30 H30 V10 Z"/>"#,
        ] {
            let regions = merged(path);
            assert_eq!(regions.len(), 1, "{path}");
            assert_eq!(regions[0].holes.len(), 1, "the window is still a hole: {path}");
            let net = regions[0].outer.signed_area().abs() - regions[0].holes[0].signed_area().abs();
            assert!((net - with_window).abs() < 1e-6, "{path}: net area {net}");
        }
        // nonzero with both squares wound the same way: SVG fills the inner
        // square, so there is no window.
        let regions = merged(r#"<path d="M0 0 H40 V40 H0 Z M10 10 H30 V30 H10 Z"/>"#);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].holes.is_empty(), "a same-wound nonzero square is filled");
        assert!((regions[0].outer.signed_area().abs() - (1600.0 + 15.0 * 10.0)).abs() < 1e-6);
    }

    #[test]
    fn an_unfilled_path_becomes_a_construction_path() {
        let im = run(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40">
  <path id="guide" fill="none" stroke="black" d="M0 0 L10 10"/>
  <polyline id="wire" points="0,20 10,20 10,30"/>
</svg>"#);
        assert!(im.loops.is_empty());
        assert_eq!(im.paths.len(), 2, "{:?}", im.paths.iter().map(|(n, _)| n).collect::<Vec<_>>());
    }

    fn nested_groups(n: usize) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{}<rect width="5" height="5"/>{}</svg>"#,
            "<g>".repeat(n),
            "</g>".repeat(n)
        )
    }

    #[test]
    fn absurdly_deep_nesting_is_a_clean_error_not_a_stack_overflow() {
        // 100 000 nested groups used to overflow the stack inside the XML
        // parser and abort the whole process.
        let err = import(nested_groups(100_000).as_bytes(), &Options::default()).unwrap_err();
        assert!(err.contains("nested more than"), "{err}");
        // Just past the cap is refused the same way, before any parsing.
        let err = import(nested_groups(MAX_NESTING).as_bytes(), &Options::default()).unwrap_err();
        assert!(err.contains("nested more than"), "{err}");
    }

    #[test]
    fn nesting_up_to_the_cap_imports_with_the_full_name_path() {
        // The root <svg> is one level, so this many groups nest exactly
        // MAX_NESTING deep: the pre-parse scan lets the file through, and
        // the walk must then not drop the rect inside the innermost group.
        let n = MAX_NESTING - 1;
        let im = import(nested_groups(n).as_bytes(), &Options::default()).unwrap();
        assert_eq!(im.loops.len(), 1);
        let want = format!("{}rect[0]/loop[0]", "g[0]/".repeat(n));
        assert_eq!(im.loops[0].0, want);
    }

    #[test]
    fn deep_use_chains_stop_at_the_nesting_cap_with_a_warning() {
        // Each <use> expands a group that is itself a deep stack; the
        // expansion depth counts toward the same nesting cap as plain groups.
        let depth = MAX_NESTING / 4;
        let mut defs = String::new();
        for k in 0..6 {
            let inner = if k == 0 {
                r#"<rect width="1" height="1"/>"#.to_string()
            } else {
                format!(r##"<use href="#s{}"/>"##, k - 1)
            };
            defs.push_str(&format!(
                r#"<g id="s{k}">{}{inner}{}</g>"#,
                "<g>".repeat(depth),
                "</g>".repeat(depth)
            ));
        }
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><defs>{defs}</defs><use href="#s5"/></svg>"##
        );
        let im = import(svg.as_bytes(), &Options::default()).unwrap();
        assert!(im.loops.is_empty(), "the rect sits past the cap");
        assert!(im.notes.iter().any(|n| n.message.contains("nested deeper than")), "{:?}", im.notes);
    }

    /// Bounding box of an import into a 100 mm root whose user unit is 1 mm,
    /// with y left pointing down so the numbers read like the file.
    fn bbox_in_mm_root(body: &str) -> (P2, P2) {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"#
        );
        let im = import(svg.as_bytes(), &Options { flip_y: false, ..Default::default() }).unwrap();
        im.bbox().unwrap()
    }

    fn assert_box(got: (P2, P2), lo: (f64, f64), hi: (f64, f64)) {
        let ok = (got.0.x - lo.0).abs() < 1e-6
            && (got.0.y - lo.1).abs() < 1e-6
            && (got.1.x - hi.0).abs() < 1e-6
            && (got.1.y - hi.1).abs() < 1e-6;
        assert!(ok, "got {:?}..{:?}, want {lo:?}..{hi:?}", got.0, got.1);
    }

    #[test]
    fn a_nested_svg_is_placed_at_its_x_and_y() {
        let b = bbox_in_mm_root(r#"<svg x="10" y="10"><rect width="2" height="2"/></svg>"#);
        assert_box(b, (10.0, 10.0), (12.0, 12.0));
    }

    #[test]
    fn a_nested_svg_maps_its_view_box_onto_its_viewport() {
        let b = bbox_in_mm_root(
            r#"<svg x="10" y="10" width="50" height="50" viewBox="0 0 10 10"><rect width="2" height="2"/></svg>"#,
        );
        assert_box(b, (10.0, 10.0), (20.0, 20.0));
    }

    #[test]
    fn preserve_aspect_ratio_meets_centred_by_default_and_none_stretches() {
        // A 10 x 20 viewBox in a 50 x 50 viewport: meet scales by 2.5 and
        // centres the 25 mm wide result.
        let meet = bbox_in_mm_root(
            r#"<svg width="50" height="50" viewBox="0 0 10 20"><rect width="10" height="20"/></svg>"#,
        );
        assert_box(meet, (12.5, 0.0), (37.5, 50.0));
        let slice = bbox_in_mm_root(
            r#"<svg width="50" height="50" viewBox="0 0 10 20" preserveAspectRatio="xMinYMin slice"><rect width="10" height="20"/></svg>"#,
        );
        assert_box(slice, (0.0, 0.0), (50.0, 100.0));
        let none = bbox_in_mm_root(
            r#"<svg width="50" height="50" viewBox="0 0 10 20" preserveAspectRatio="none"><rect width="10" height="20"/></svg>"#,
        );
        assert_box(none, (0.0, 0.0), (50.0, 50.0));
    }

    /// A zero width or height disables the element: a scale of 0 used to
    /// import its content as collapsed zero-area loops.
    #[test]
    fn a_zero_sized_viewport_imports_nothing() {
        for doc in [
            r#"<svg width="0" height="10" viewBox="0 0 1 1" preserveAspectRatio="none"><circle cx="0.5" cy="0.5" r="0.4"/></svg>"#,
            r##"<defs><symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10"/></symbol></defs>
                <use href="#s" x="5" y="5" width="0" height="20"/>"##,
        ] {
            let text = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{doc}</svg>"#);
            let im = import(text.as_bytes(), &Options::default()).unwrap();
            assert!(im.loops.is_empty() && im.paths.is_empty(), "{doc}: {:?}", im.loops);
        }
    }

    #[test]
    fn a_symbol_view_box_scales_to_the_use_width_and_height() {
        let b = bbox_in_mm_root(
            r##"<defs><symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10"/></symbol></defs>
                <use href="#s" x="5" y="5" width="20" height="20"/>"##,
        );
        assert_box(b, (5.0, 5.0), (25.0, 25.0));
    }

    #[test]
    fn a_use_of_a_nested_svg_keeps_its_viewport() {
        let b = bbox_in_mm_root(
            r##"<defs><svg id="v" viewBox="0 0 10 10" width="50" height="50"><rect width="2" height="2"/></svg></defs>
                <use xlink:href="#v" x="10" y="10"/>"##,
        );
        assert_box(b, (10.0, 10.0), (20.0, 20.0));
        // The <use>'s own width and height override the svg's.
        let b = bbox_in_mm_root(
            r##"<defs><svg id="v" viewBox="0 0 10 10" width="50" height="50"><rect width="2" height="2"/></svg></defs>
                <use href="#v" x="10" y="10" width="20" height="20"/>"##,
        );
        assert_box(b, (10.0, 10.0), (14.0, 14.0));
    }

    #[test]
    fn a_use_draws_its_target_with_the_targets_own_transform() {
        let b = bbox_in_mm_root(
            r##"<defs><g id="t" transform="scale(2)"><rect width="2" height="2"/></g>
                      <rect id="r" transform="translate(1,1)" width="2" height="2"/></defs>
                <use href="#t" x="10" y="10"/><use href="#r" x="30" y="30"/>"##,
        );
        assert_box(b, (10.0, 10.0), (33.0, 33.0));
    }

    #[test]
    fn percentages_on_a_nested_svg_resolve_against_the_parent_viewport() {
        let b = bbox_in_mm_root(
            r#"<svg x="50%" width="50%" height="50%" viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"#,
        );
        assert_box(b, (50.0, 0.0), (100.0, 50.0));
    }

    #[test]
    fn a_dtd_is_refused_rather_than_expanded() {
        let bomb = r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa">]><svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L1 1 Z"/></svg>"#;
        let err = import(bomb.as_bytes(), &Options::default()).unwrap_err();
        assert!(err.contains("parsed"), "{err}");
    }
}
