//! Dimension primitives: the geometry, the arrowheads and the number.
//!
//! A dimension is stored as *intent* — these two points, this direction, this
//! offset — and only becomes line work in [`Dim::flatten`]. That matters for
//! three reasons. The value is computed from the model-space points, so it
//! states a true length no matter what scale the view is placed at. The witness
//! lines, arrows and text are sized in paper millimetres, so they look the same
//! on every sheet. And the placement pass (`crate::auto::place`) can ask a
//! dimension where its text *would* go, move it, and ask again, without ever
//! touching geometry.
//!
//! "Do not scale off dimensions" is the
//! reason all of this exists: the figured number is the deliverable, so it has
//! to be right and it has to be legible.

use cadpole_core::sketch::{Loop, Path, Segment, P2};

use crate::ir::{Anchor, FlattenCtx, FontRef, HAlign, Prim, Resolved, Style, VAlign, Xform2};
use crate::units::{format_length, UnitFormat};

/// How a dimension line terminates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Arrow {
    /// The solid mechanical arrowhead.
    #[default]
    Filled,
    /// Two strokes, no fill.
    Open,
    /// The architectural slash. Standard on plan dimension strings.
    Tick,
    /// A filled dot, for a dimension with no room for anything else.
    Dot,
    None,
}

/// Everything about how a dimension is drawn, in paper millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct DimStyle {
    pub text_height_mm: f64,
    pub arrow: Arrow,
    pub arrow_size_mm: f64,
    /// How far the witness line runs past the dimension line.
    pub extension_beyond_mm: f64,
    /// Gap between the measured geometry and the start of the witness line.
    pub extension_gap_mm: f64,
    /// Gap between the dimension line and its text.
    pub text_gap_mm: f64,
    pub format: UnitFormat,
    /// The face the value is set in, which decides how wide it measures.
    pub font: FontRef,
}

impl Default for DimStyle {
    fn default() -> Self {
        DimStyle::mechanical()
    }
}

impl DimStyle {
    /// Filled arrows, millimetres: a part drawing.
    pub fn mechanical() -> DimStyle {
        DimStyle {
            text_height_mm: 2.5,
            arrow: Arrow::Filled,
            arrow_size_mm: 2.5,
            extension_beyond_mm: 1.5,
            extension_gap_mm: 1.0,
            text_gap_mm: 0.7,
            format: UnitFormat::mm(),
            font: FontRef::Builtin,
        }
    }

    /// Ticks, feet and inches: a floor plan.
    pub fn architectural() -> DimStyle {
        DimStyle {
            text_height_mm: 2.5,
            arrow: Arrow::Tick,
            arrow_size_mm: 2.0,
            extension_beyond_mm: 2.0,
            extension_gap_mm: 1.5,
            text_gap_mm: 0.8,
            format: UnitFormat::ft_in(),
            font: FontRef::Builtin,
        }
    }

    /// The same style set in `font`.
    pub fn with_font(mut self, font: FontRef) -> DimStyle {
        self.font = font;
        self
    }
}

/// What is being measured.
#[derive(Clone, Debug, PartialEq)]
pub enum DimKind {
    /// The distance between `a` and `b` measured along `dir`. The dimension
    /// line sits `offset` paper millimetres from `a` along `dir`'s normal.
    Linear { a: P2, b: P2, dir: P2, offset: f64 },
    /// The true distance between `a` and `b`, measured along the line joining
    /// them.
    Aligned { a: P2, b: P2, offset: f64 },
    /// `R25`: a leader to a point on an arc.
    Radial {
        center: P2,
        radius: f64,
        angle_deg: f64,
        leader_mm: f64,
    },
    /// `Ø50`: a line across a circle.
    Diameter {
        center: P2,
        radius: f64,
        angle_deg: f64,
    },
    /// The angle at `center` between the rays to `a` and `b`, drawn at
    /// `radius` model units.
    Angular {
        center: P2,
        a: P2,
        b: P2,
        radius: f64,
    },
}

/// One dimension.
#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    pub style: Style,
    pub kind: DimKind,
    pub dim: DimStyle,
    /// Replaces the computed value (`TYP`, `2 PLACES`, a tolerance).
    pub text_override: Option<String>,
    /// The model entities measured, so an edit re-dimensions instead of
    /// drifting and a stale reference can be flagged.
    pub refs: Vec<String>,
    /// Nudge applied by the placement pass, along the dimension line's normal,
    /// in paper millimetres.
    pub text_shift: f64,
    /// Placement decided the text does not fit between the witness lines.
    pub text_outside: bool,
}

impl Dim {
    pub fn new(style: Style, kind: DimKind, dim: DimStyle) -> Dim {
        Dim {
            style,
            kind,
            dim,
            text_override: None,
            refs: Vec::new(),
            text_shift: 0.0,
            text_outside: false,
        }
    }

    pub fn linear(style: Style, a: P2, b: P2, dir: P2, offset: f64, dim: DimStyle) -> Dim {
        Dim::new(
            style,
            DimKind::Linear {
                a,
                b,
                dir: unit(dir),
                offset,
            },
            dim,
        )
    }

    pub fn aligned(style: Style, a: P2, b: P2, offset: f64, dim: DimStyle) -> Dim {
        Dim::new(style, DimKind::Aligned { a, b, offset }, dim)
    }

    pub fn with_refs(mut self, refs: Vec<String>) -> Dim {
        self.refs = refs;
        self
    }

    /// The measured value in model units: a length, or degrees for an angular
    /// dimension.
    pub fn value(&self) -> f64 {
        match &self.kind {
            DimKind::Linear { a, b, dir, .. } => {
                let d = unit(*dir);
                ((b.x - a.x) * d.x + (b.y - a.y) * d.y).abs()
            }
            DimKind::Aligned { a, b, .. } => dist(*a, *b),
            DimKind::Radial { radius, .. } => *radius,
            DimKind::Diameter { radius, .. } => radius * 2.0,
            DimKind::Angular { center, a, b, .. } => {
                let mut d = angle_of(*center, *b) - angle_of(*center, *a);
                while d <= -std::f64::consts::PI {
                    d += std::f64::consts::TAU;
                }
                while d > std::f64::consts::PI {
                    d -= std::f64::consts::TAU;
                }
                d.abs().to_degrees()
            }
        }
    }

    /// What the dimension text reads.
    ///
    /// ```
    /// # use cadpole_drawing::dim::{Dim, DimStyle};
    /// # use cadpole_drawing::ir::Style;
    /// # use cadpole_core::sketch::P2;
    /// let mut s = DimStyle::architectural();
    /// let d = Dim::aligned(Style::default(), P2::new(0.0, 0.0), P2::new(3810.0, 0.0), 10.0, s.clone());
    /// assert_eq!(d.text(), "12'-6\"");
    /// s.format = cadpole_drawing::units::UnitFormat::mm();
    /// let d = Dim::aligned(Style::default(), P2::new(0.0, 0.0), P2::new(3810.0, 0.0), 10.0, s);
    /// assert_eq!(d.text(), "3810");
    /// ```
    pub fn text(&self) -> String {
        if let Some(t) = &self.text_override {
            return t.clone();
        }
        let v = self.value();
        match &self.kind {
            DimKind::Radial { .. } => format!("R{}", format_length(v, &self.dim.format)),
            DimKind::Diameter { .. } => format!("\u{d8}{}", format_length(v, &self.dim.format)),
            DimKind::Angular { .. } => {
                let s = format!("{v:.1}");
                format!("{}\u{b0}", s.trim_end_matches('0').trim_end_matches('.'))
            }
            _ => format_length(v, &self.dim.format),
        }
    }

    /// Where the text will land, and how wide it is, in the space `x` maps to.
    /// The placement pass uses this to find collisions.
    pub fn text_box(&self, x: Xform2) -> Option<Rect> {
        let l = self.layout(x)?;
        Some(Rect::for_text(
            l.text_at,
            l.text_width,
            self.dim.text_height_mm,
            l.text_rotation,
            l.text_anchor,
        ))
    }

    /// The direction the text moves when the placement pass nudges it.
    pub fn nudge_dir(&self, x: Xform2) -> P2 {
        self.layout(x)
            .map(|l| l.normal)
            .unwrap_or(P2::new(0.0, 1.0))
    }

    pub(crate) fn flatten(&self, x: Xform2, ctx: &FlattenCtx<'_>, out: &mut Vec<Prim>) {
        let Some(l) = self.layout(x) else { return };
        let style = ctx.resolve(&self.style);
        for (a, b) in &l.lines {
            out.push(Prim::Path {
                style: style.clone(),
                name: String::new(),
                path: Path::new(*a).line_to(*b),
                closed: false,
            });
        }
        for (center, radius, t0, t1) in &l.arcs {
            if let Some(p) = arc_path(*center, *radius, *t0, *t1) {
                out.push(Prim::Path {
                    style: style.clone(),
                    name: String::new(),
                    path: p,
                    closed: false,
                });
            }
        }
        for (tip, dir) in &l.arrows {
            for g in arrow_head(*tip, *dir, self.dim.arrow_size_mm, &self.dim.arrow) {
                out.push(g.into_prim(style.clone()));
            }
        }
        out.push(Prim::Text {
            style,
            at: l.text_at,
            text: l.text,
            height_mm: self.dim.text_height_mm,
            rotation_deg: l.text_rotation,
            anchor: l.text_anchor,
            font: self.dim.font.clone(),
        });
    }

    /// Resolve the dimension into concrete geometry, once, so drawing it and
    /// measuring it can never disagree.
    fn layout(&self, x: Xform2) -> Option<Layout> {
        let text = self.text();
        let th = self.dim.text_height_mm;
        let tw = crate::text::width_in(&text, th, &self.dim.font);
        let mut lines: Vec<(P2, P2)> = Vec::new();
        let mut arcs: Vec<(P2, f64, f64, f64)> = Vec::new();
        let mut arrows: Vec<(P2, P2)> = Vec::new();
        match &self.kind {
            DimKind::Linear { a, b, dir, offset } => {
                let d = unit(*dir);
                self.linear_layout(x, *a, *b, d, *offset, text, tw, th, &mut lines, &mut arrows)
            }
            DimKind::Aligned { a, b, offset } => {
                let d = unit(P2::new(b.x - a.x, b.y - a.y));
                self.linear_layout(x, *a, *b, d, *offset, text, tw, th, &mut lines, &mut arrows)
            }
            DimKind::Radial {
                center,
                radius,
                angle_deg,
                leader_mm,
            } => {
                let c = x.apply(*center);
                let r = radius * x.scale;
                let t = angle_deg.to_radians();
                let u = P2::new(t.cos(), t.sin());
                let on_arc = P2::new(c.x + u.x * r, c.y + u.y * r);
                let out_to = P2::new(on_arc.x + u.x * leader_mm, on_arc.y + u.y * leader_mm);
                let sign = if u.x >= 0.0 { 1.0 } else { -1.0 };
                let shoulder = P2::new(out_to.x + sign * tw.max(th), out_to.y);
                lines.push((P2::new(c.x + u.x * r * 0.35, c.y + u.y * r * 0.35), on_arc));
                lines.push((on_arc, out_to));
                lines.push((out_to, shoulder));
                arrows.push((on_arc, P2::new(-u.x, -u.y)));
                let anchor = if sign > 0.0 {
                    Anchor::RIGHT
                } else {
                    Anchor::LEFT
                };
                let at = P2::new(
                    shoulder.x,
                    shoulder.y + self.dim.text_gap_mm + th * 0.5 + self.text_shift,
                );
                Some(Layout {
                    lines,
                    arcs,
                    arrows,
                    text,
                    text_at: at,
                    text_width: tw,
                    text_rotation: 0.0,
                    text_anchor: anchor,
                    normal: P2::new(0.0, 1.0),
                })
            }
            DimKind::Diameter {
                center,
                radius,
                angle_deg,
            } => {
                let c = x.apply(*center);
                let r = radius * x.scale;
                let t = angle_deg.to_radians();
                let u = P2::new(t.cos(), t.sin());
                let p0 = P2::new(c.x - u.x * r, c.y - u.y * r);
                let p1 = P2::new(c.x + u.x * r, c.y + u.y * r);
                lines.push((p0, p1));
                arrows.push((p0, u));
                arrows.push((p1, P2::new(-u.x, -u.y)));
                let (d_read, rot) = readable(u);
                let n = P2::new(-d_read.y, d_read.x);
                let gap = self.dim.text_gap_mm + th * 0.5 + self.text_shift;
                let at = P2::new(c.x + n.x * gap, c.y + n.y * gap);
                Some(Layout {
                    lines,
                    arcs,
                    arrows,
                    text,
                    text_at: at,
                    text_width: tw,
                    text_rotation: rot,
                    text_anchor: Anchor::CENTER,
                    normal: n,
                })
            }
            DimKind::Angular {
                center,
                a,
                b,
                radius,
            } => {
                let c = x.apply(*center);
                let (pa, pb) = (x.apply(*a), x.apply(*b));
                let r = radius * x.scale;
                let (t0, t1) = sweep(angle_of(c, pa), angle_of(c, pb));
                arcs.push((c, r, t0, t1));
                for (p, t) in [(pa, t0), (pb, t1)] {
                    let on = P2::new(c.x + t.cos() * r, c.y + t.sin() * r);
                    let far = P2::new(
                        c.x + t.cos() * (r + self.dim.extension_beyond_mm),
                        c.y + t.sin() * (r + self.dim.extension_beyond_mm),
                    );
                    if dist(c, p) < r {
                        lines.push((p, far));
                    } else {
                        lines.push((on, far));
                    }
                }
                // Tangent arrows at the arc ends, pointing along the sweep.
                arrows.push((
                    P2::new(c.x + t0.cos() * r, c.y + t0.sin() * r),
                    P2::new(-t0.sin(), t0.cos()),
                ));
                arrows.push((
                    P2::new(c.x + t1.cos() * r, c.y + t1.sin() * r),
                    P2::new(t1.sin(), -t1.cos()),
                ));
                let tm = (t0 + t1) / 2.0;
                let rr = r + self.dim.text_gap_mm + th * 0.5 + self.text_shift;
                let at = P2::new(c.x + tm.cos() * rr, c.y + tm.sin() * rr);
                Some(Layout {
                    lines,
                    arcs,
                    arrows,
                    text,
                    text_at: at,
                    text_width: tw,
                    text_rotation: 0.0,
                    text_anchor: Anchor::CENTER,
                    normal: P2::new(tm.cos(), tm.sin()),
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn linear_layout(
        &self,
        x: Xform2,
        a: P2,
        b: P2,
        d: P2,
        offset: f64,
        text: String,
        tw: f64,
        th: f64,
        lines: &mut Vec<(P2, P2)>,
        arrows: &mut Vec<(P2, P2)>,
    ) -> Option<Layout> {
        if d.x == 0.0 && d.y == 0.0 {
            return None;
        }
        let (pa, pb) = (x.apply(a), x.apply(b));
        let n = P2::new(-d.y, d.x);
        let line_n = dot(n, pa) + offset;
        let proj = |p: P2| {
            P2::new(
                d.x * dot(d, p) + n.x * line_n,
                d.y * dot(d, p) + n.y * line_n,
            )
        };
        let (qa, qb) = (proj(pa), proj(pb));
        let span = dist(qa, qb);
        if span < 1e-9 {
            return None;
        }
        // Witness lines, each running from just off the measured point to a
        // little past the dimension line.
        for (p, q) in [(pa, qa), (pb, qb)] {
            let reach = dot(n, q) - dot(n, p);
            if reach.abs() > self.dim.extension_gap_mm * 1.5 {
                let s = reach.signum();
                let start = P2::new(
                    p.x + n.x * s * self.dim.extension_gap_mm,
                    p.y + n.y * s * self.dim.extension_gap_mm,
                );
                let end = P2::new(
                    q.x + n.x * s * self.dim.extension_beyond_mm,
                    q.y + n.y * s * self.dim.extension_beyond_mm,
                );
                lines.push((start, end));
            }
        }
        let along = unit(P2::new(qb.x - qa.x, qb.y - qa.y));
        arrows.push((qa, along));
        arrows.push((qb, P2::new(-along.x, -along.y)));

        let room = span - 2.0 * self.dim.arrow_size_mm;
        let fits = tw + 2.0 * self.dim.text_gap_mm <= room && !self.text_outside;
        let (d_read, rot) = readable(d);
        let n_read = P2::new(-d_read.y, d_read.x);
        let lift = self.dim.text_gap_mm + th * 0.5;
        if fits {
            lines.push((qa, qb));
            let mid = P2::new((qa.x + qb.x) / 2.0, (qa.y + qb.y) / 2.0);
            let off = lift + self.text_shift;
            let at = P2::new(mid.x + n_read.x * off, mid.y + n_read.y * off);
            Some(Layout {
                lines: std::mem::take(lines),
                arcs: Vec::new(),
                arrows: std::mem::take(arrows),
                text,
                text_at: at,
                text_width: tw,
                text_rotation: rot,
                text_anchor: Anchor::CENTER,
                normal: n_read,
            })
        } else {
            // No room between the witness lines: run the dimension line
            // through, put the arrows outside pointing in, and set the text
            // past the far end with a short tail under it.
            let tail = self.dim.arrow_size_mm * 2.0;
            let e0 = P2::new(qa.x - along.x * tail, qa.y - along.y * tail);
            let e1 = P2::new(qb.x + along.x * tail, qb.y + along.y * tail);
            lines.push((e0, e1));
            arrows.clear();
            arrows.push((qa, P2::new(-along.x, -along.y)));
            arrows.push((qb, along));
            let end = if dot(d_read, P2::new(qb.x - qa.x, qb.y - qa.y)) > 0.0 {
                e1
            } else {
                e0
            };
            let pad = self.dim.text_gap_mm * 2.0;
            let off = lift + self.text_shift;
            let at = P2::new(
                end.x + d_read.x * pad + n_read.x * off,
                end.y + d_read.y * pad + n_read.y * off,
            );
            Some(Layout {
                lines: std::mem::take(lines),
                arcs: Vec::new(),
                arrows: std::mem::take(arrows),
                text,
                text_at: at,
                text_width: tw,
                text_rotation: rot,
                text_anchor: Anchor::new(HAlign::Left, VAlign::Middle),
                normal: n_read,
            })
        }
    }
}

struct Layout {
    lines: Vec<(P2, P2)>,
    /// (centre, radius, start angle, end angle) for angular dimensions.
    arcs: Vec<(P2, f64, f64, f64)>,
    /// (tip, direction the body extends in).
    arrows: Vec<(P2, P2)>,
    text: String,
    text_at: P2,
    text_width: f64,
    text_rotation: f64,
    text_anchor: Anchor,
    /// The direction the placement pass nudges the text in.
    normal: P2,
}

/// An axis-aligned box, used for text collision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub lo: P2,
    pub hi: P2,
}

impl Rect {
    pub fn new(lo: P2, hi: P2) -> Rect {
        Rect { lo, hi }
    }

    /// The bounding box of a text run. Rotated text is bounded by the box of
    /// its four rotated corners, which is conservative and never lets two
    /// strings touch.
    pub fn for_text(at: P2, width: f64, height: f64, rotation_deg: f64, anchor: Anchor) -> Rect {
        let x0 = match anchor.h {
            HAlign::Left => 0.0,
            HAlign::Center => -width / 2.0,
            HAlign::Right => -width,
        };
        let y0 = match anchor.v {
            VAlign::Baseline | VAlign::Bottom => 0.0,
            VAlign::Middle => -height / 2.0,
            VAlign::Top => -height,
        };
        let a = rotation_deg.to_radians();
        let (c, s) = (a.cos(), a.sin());
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        for (dx, dy) in [
            (x0, y0),
            (x0 + width, y0),
            (x0, y0 + height),
            (x0 + width, y0 + height),
        ] {
            let p = P2::new(at.x + dx * c - dy * s, at.y + dx * s + dy * c);
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        Rect { lo, hi }
    }

    pub fn overlaps(&self, o: &Rect) -> bool {
        self.lo.x < o.hi.x && o.lo.x < self.hi.x && self.lo.y < o.hi.y && o.lo.y < self.hi.y
    }

    /// Grown by `m` on every side.
    pub fn grown(&self, m: f64) -> Rect {
        Rect {
            lo: P2::new(self.lo.x - m, self.lo.y - m),
            hi: P2::new(self.hi.x + m, self.hi.y + m),
        }
    }

    pub fn width(&self) -> f64 {
        self.hi.x - self.lo.x
    }
    pub fn height(&self) -> f64 {
        self.hi.y - self.lo.y
    }
}

/// Arrowhead geometry, before it picks up a style.
pub(crate) enum ArrowGeom {
    Fill(Loop),
    Line(P2, P2),
}

impl ArrowGeom {
    pub(crate) fn into_prim(self, style: Resolved) -> Prim {
        match self {
            ArrowGeom::Fill(l) => Prim::Fill {
                style,
                name: String::new(),
                outer: l,
                holes: Vec::new(),
            },
            ArrowGeom::Line(a, b) => Prim::Path {
                style,
                name: String::new(),
                path: Path::new(a).line_to(b),
                closed: false,
            },
        }
    }
}

/// The geometry of one arrowhead: tip at `tip`, body extending along `dir`.
pub(crate) fn arrow_head(tip: P2, dir: P2, size: f64, kind: &Arrow) -> Vec<ArrowGeom> {
    let d = unit(dir);
    if (d.x == 0.0 && d.y == 0.0) || size <= 0.0 {
        return Vec::new();
    }
    let n = P2::new(-d.y, d.x);
    let base = P2::new(tip.x + d.x * size, tip.y + d.y * size);
    // 3:1, the drafting proportion.
    let half = size / 6.0;
    match kind {
        Arrow::None => Vec::new(),
        Arrow::Filled => vec![ArrowGeom::Fill(Loop::polygon(&[
            tip,
            P2::new(base.x + n.x * half, base.y + n.y * half),
            P2::new(base.x - n.x * half, base.y - n.y * half),
        ]))],
        Arrow::Open => vec![
            ArrowGeom::Line(
                tip,
                P2::new(base.x + n.x * half * 2.0, base.y + n.y * half * 2.0),
            ),
            ArrowGeom::Line(
                tip,
                P2::new(base.x - n.x * half * 2.0, base.y - n.y * half * 2.0),
            ),
        ],
        Arrow::Tick => {
            // A 45° slash through the tip, the architectural terminator.
            let k = size * 0.5;
            let u = P2::new((d.x + n.x) * k, (d.y + n.y) * k);
            vec![ArrowGeom::Line(
                P2::new(tip.x - u.x, tip.y - u.y),
                P2::new(tip.x + u.x, tip.y + u.y),
            )]
        }
        Arrow::Dot => {
            vec![ArrowGeom::Fill(Loop::circle(tip, size / 4.0))]
        }
    }
}

/// An open arc from `t0` to `t1` as two arc segments, so it stays exact
/// through every writer even when it is more than half a turn.
fn arc_path(center: P2, r: f64, t0: f64, t1: f64) -> Option<Path> {
    if r <= 0.0 || (t1 - t0).abs() < 1e-9 {
        return None;
    }
    let at = |t: f64| P2::new(center.x + t.cos() * r, center.y + t.sin() * r);
    let tm = (t0 + t1) / 2.0;
    let p = Path::new(at(t0))
        .arc_to(at(tm), at((t0 + tm) / 2.0))
        .arc_to(at(t1), at((tm + t1) / 2.0));
    Some(p)
}

/// The shortest sweep from `a` to `b`, as an ordered pair of angles.
fn sweep(a: f64, b: f64) -> (f64, f64) {
    let mut d = b - a;
    while d <= -std::f64::consts::PI {
        d += std::f64::consts::TAU;
    }
    while d > std::f64::consts::PI {
        d -= std::f64::consts::TAU;
    }
    if d >= 0.0 {
        (a, a + d)
    } else {
        (a + d, a)
    }
}

/// Flip a direction so text set along it reads from the bottom or the right of
/// the sheet, and give the rotation in degrees.
fn readable(d: P2) -> (P2, f64) {
    let flip = d.x < -1e-12 || (d.x.abs() <= 1e-12 && d.y < 0.0);
    let d = if flip { P2::new(-d.x, -d.y) } else { d };
    (d, d.y.atan2(d.x).to_degrees())
}

pub(crate) fn unit(v: P2) -> P2 {
    let l = (v.x * v.x + v.y * v.y).sqrt();
    if l < 1e-15 {
        P2::new(0.0, 0.0)
    } else {
        P2::new(v.x / l, v.y / l)
    }
}

pub(crate) fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.y * b.y
}

pub(crate) fn dist(a: P2, b: P2) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

fn angle_of(c: P2, p: P2) -> f64 {
    (p.y - c.y).atan2(p.x - c.x)
}

/// A point on a `Loop`'s segment list, for callers building dimensions from
/// sketch geometry.
pub fn segment_midpoint(s: &Segment) -> P2 {
    let (a, b) = (s.start(), s.end());
    P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{layer, Drawing, Item};

    fn drawing_with(d: Dim) -> Vec<Prim> {
        let mut dr = Drawing::new();
        let _ = dr.layer(layer::DIMS);
        dr.push(Item::Dim(d));
        dr.flatten()
    }

    #[test]
    fn a_rectangles_dimensions_read_the_right_value_in_both_units() {
        // A 12'-6" by 8'-0" room: 3810 mm by 2438.4 mm.
        let mut arch = DimStyle::architectural();
        arch.format = UnitFormat::ft_in();
        let w = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(3810.0, 0.0),
            P2::new(1.0, 0.0),
            -10.0,
            arch.clone(),
        );
        let h = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(0.0, 2438.4),
            P2::new(0.0, 1.0),
            -10.0,
            arch,
        );
        assert_eq!(w.text(), "12'-6\"");
        assert_eq!(h.text(), "8'-0\"");

        let mm = DimStyle::mechanical();
        let w = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(3810.0, 0.0),
            P2::new(1.0, 0.0),
            -10.0,
            mm.clone(),
        );
        let h = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(0.0, 2438.4),
            P2::new(0.0, 1.0),
            -10.0,
            mm,
        );
        assert_eq!(w.text(), "3810");
        assert_eq!(h.text(), "2438.4");
    }

    #[test]
    fn a_linear_dimension_draws_two_witness_lines_a_line_and_two_arrows() {
        let d = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(100.0, 0.0),
            P2::new(1.0, 0.0),
            12.0,
            DimStyle::mechanical(),
        );
        let prims = drawing_with(d);
        let paths = prims
            .iter()
            .filter(|p| matches!(p, Prim::Path { .. }))
            .count();
        let fills = prims
            .iter()
            .filter(|p| matches!(p, Prim::Fill { .. }))
            .count();
        let texts = prims
            .iter()
            .filter(|p| matches!(p, Prim::Text { .. }))
            .count();
        assert_eq!(paths, 3, "two witness lines and the dimension line");
        assert_eq!(fills, 2, "two filled arrowheads");
        assert_eq!(texts, 1);
        let Prim::Text { at, text, .. } = prims
            .iter()
            .find(|p| matches!(p, Prim::Text { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(text, "100");
        assert!((at.x - 50.0).abs() < 1e-9, "centred over the span: {at:?}");
        assert!(at.y > 12.0, "above the dimension line: {at:?}");
    }

    #[test]
    fn a_dimension_with_no_room_puts_its_text_outside() {
        let narrow = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(3.0, 0.0),
            P2::new(1.0, 0.0),
            10.0,
            DimStyle::mechanical(),
        );
        let b = narrow.text_box(Xform2::IDENTITY).unwrap();
        assert!(b.lo.x > 3.0, "text pushed past the far witness line: {b:?}");
        let wide = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(60.0, 0.0),
            P2::new(1.0, 0.0),
            10.0,
            DimStyle::mechanical(),
        );
        let b = wide.text_box(Xform2::IDENTITY).unwrap();
        assert!(
            b.lo.x > 0.0 && b.hi.x < 60.0,
            "text fits between the witness lines: {b:?}"
        );
    }

    #[test]
    fn radial_diameter_and_angular_read_their_prefixes() {
        let r = Dim::new(
            Style::default(),
            DimKind::Radial {
                center: P2::new(0.0, 0.0),
                radius: 25.0,
                angle_deg: 45.0,
                leader_mm: 6.0,
            },
            DimStyle::mechanical(),
        );
        assert_eq!(r.text(), "R25");
        let d = Dim::new(
            Style::default(),
            DimKind::Diameter {
                center: P2::new(0.0, 0.0),
                radius: 25.0,
                angle_deg: 0.0,
            },
            DimStyle::mechanical(),
        );
        assert_eq!(d.text(), "\u{d8}50");
        let a = Dim::new(
            Style::default(),
            DimKind::Angular {
                center: P2::new(0.0, 0.0),
                a: P2::new(10.0, 0.0),
                b: P2::new(0.0, 10.0),
                radius: 8.0,
            },
            DimStyle::mechanical(),
        );
        assert_eq!(a.text(), "90\u{b0}");
        // All three produce ink.
        for dim in [r, d, a] {
            assert!(!drawing_with(dim).is_empty());
        }
    }

    #[test]
    fn text_is_never_upside_down() {
        for (dx, dy) in [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (-1.0, -1.0),
        ] {
            let d = Dim::aligned(
                Style::default(),
                P2::new(0.0, 0.0),
                P2::new(dx * 80.0, dy * 80.0),
                10.0,
                DimStyle::mechanical(),
            );
            let prims = drawing_with(d);
            let Prim::Text { rotation_deg, .. } = prims
                .iter()
                .find(|p| matches!(p, Prim::Text { .. }))
                .unwrap()
            else {
                unreachable!()
            };
            assert!(
                *rotation_deg > -90.001 && *rotation_deg <= 90.001,
                "reads from the bottom or the right: {rotation_deg}"
            );
        }
    }

    #[test]
    fn a_group_transform_scales_the_ink_but_not_the_number() {
        use crate::ir::{Group, Xform2};
        let d = Dim::linear(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(100.0, 0.0),
            P2::new(1.0, 0.0),
            8.0,
            DimStyle::mechanical(),
        );
        let mut dr = Drawing::new();
        let _ = dr.layer(layer::DIMS);
        dr.push(Item::Group(Group {
            transform: Xform2::new(0.5, 0.0, 0.0),
            items: vec![Item::Dim(d)],
            ..Group::default()
        }));
        let prims = dr.flatten();
        let Prim::Text {
            text,
            height_mm,
            at,
            ..
        } = prims
            .iter()
            .find(|p| matches!(p, Prim::Text { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(
            text, "100",
            "the number is the model length, not the paper length"
        );
        assert_eq!(
            *height_mm, 2.5,
            "text height is paper millimetres at any view scale"
        );
        assert!((at.x - 25.0).abs() < 1e-9, "geometry did scale: {at:?}");
    }
}
