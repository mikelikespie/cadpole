//! Height fields on a surface: a grid of values in `[0, 1]` turned into a
//! wall whose thickness follows them — a relief panel, a lithophane, a
//! textured lamp shade.
//!
//! **The solid is a box, bent.** Every body this module makes is the image
//! of the parameter box `[0, U] × [0, V] × [0, 1]` under
//! `(u, v, w) ↦ B(u, v) + w · t(u, v) · N(u, v)`: `B` is the base surface
//! (a plane, a cylinder or a revolved profile) developed so that `u` and
//! `v` are millimetres along it, `N` its unit normal on the relief side and
//! `t` the wall thickness at that point. The six faces of the box are the
//! six face groups: `relief` (`w = 1`), `back` (`w = 0`), `bottom` and
//! `top` (`v = 0`, `v = V`), `start` and `end` (`u = 0`, `u = U`; absent on
//! a full turn, where `u` wraps and the seam's vertices are shared). Each
//! face is triangulated on the same structured grid in the box and wound
//! the way the box's outward normal says, then the whole mesh is flipped
//! when the map reverses orientation (a relief on the inside of a
//! cylinder). Closedness is therefore a property of the indexing, not of a
//! weld or a search: every edge of the grid is used once each way by
//! construction, and a mesh of millions of triangles costs one pass.
//!
//! **A flat back is two rows.** The back of a plane or a cylinder is ruled
//! in `v` (a straight line from `v = 0` to `v = V` at every `u`), so it is
//! one strip of long quads, not a grid: half the triangles of the body are
//! the relief's. The `start` and `end` walls do have every node in `v`, so
//! the back's first and last columns are fanned onto them, which keeps the
//! shell closed without a T-junction. A revolved profile's back is curved
//! in `v` and keeps its full grid.
//!
//! **What a value means.** Values are in `[0, 1]`, sampled at the grid's
//! nodes (bilinear on a picture, with a Gaussian prefilter when the picture
//! is finer than the grid), optionally smoothed by a Gaussian of
//! `smoothing` mm, and mapped to a thickness `t = lo + (hi − lo) · value`
//! (`1 − value` when inverted, which is what a lithophane wants: a bright
//! pixel is thin wall). A border of `border` mm round the field's open
//! edges is set to `border_thickness` (by default the thicker of the two),
//! which frames a panel and gives a shade a solid foot and lip.
//!
//! Every transcendental goes through [`crate::fmath`], so a body is the
//! same on every target.

use crate::fmath;
use crate::geom::Vec3;
use crate::mesh_kernel::MeshBody;
use crate::tessellate::{FaceInfo, Mesh, Topology};
use crate::{CadpoleError, Result};

/// The most triangles one height field may make. A 0.1 mm grid over a
/// 200 mm × 200 mm panel is eight million; past this the body is more
/// memory than any printer's slicer will take.
pub const MAX_TRIANGLES: usize = 24_000_000;

/// The most pixels a picture source may have.
pub const MAX_PIXELS: usize = 64_000_000;

/// A grid of values, row-major with row 0 at the **top** (the way a
/// picture and a list of rows are both written).
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

impl Grid {
    pub fn new(width: usize, height: usize, values: Vec<f32>) -> Result<Grid> {
        if width == 0 || height == 0 {
            return Err(CadpoleError::invalid("heightfield: the source has no pixels"));
        }
        if values.len() != width * height {
            return Err(CadpoleError::invalid(format!(
                "heightfield: a {width} x {height} grid needs {} values, got {}",
                width * height,
                values.len()
            )));
        }
        if width * height > MAX_PIXELS {
            return Err(CadpoleError::invalid(format!(
                "heightfield: the source has {} pixels; the most is {MAX_PIXELS}",
                width * height
            )));
        }
        if let Some(i) = values.iter().position(|v| !v.is_finite()) {
            return Err(CadpoleError::invalid(format!(
                "heightfield: value {i} (row {}, column {}) is not a finite number",
                i / width,
                i % width
            )));
        }
        Ok(Grid { width, height, values: values.into_iter().map(|v| v.clamp(0.0, 1.0)).collect() })
    }

    /// A picture: PNG of any colour type and depth, or a binary PGM (`P5`).
    /// The value is the luma of the sRGB-encoded pixel (Rec. 601 weights) in
    /// `[0, 1]`, with any alpha composited over white — the brightness a
    /// lithophane is asked to reproduce, linear in the grey level as
    /// lithophane makers expect.
    pub fn from_image(bytes: &[u8]) -> Result<Grid> {
        if bytes.starts_with(b"\x89PNG") {
            return Self::from_png(bytes);
        }
        if bytes.starts_with(b"P5") {
            return Self::from_pgm(bytes);
        }
        Err(CadpoleError::invalid(
            "heightfield: the picture is not a PNG or a binary PGM (convert it to a PNG first)",
        ))
    }

    fn from_png(bytes: &[u8]) -> Result<Grid> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader =
            decoder.read_info().map_err(|e| CadpoleError::invalid(format!("heightfield: unreadable PNG: {e}")))?;
        let (w, h) = (reader.info().width as usize, reader.info().height as usize);
        if w * h > MAX_PIXELS {
            return Err(CadpoleError::invalid(format!(
                "heightfield: the picture is {w} x {h}; the most is {MAX_PIXELS} pixels"
            )));
        }
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(w * h * 4)];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| CadpoleError::invalid(format!("heightfield: unreadable PNG: {e}")))?;
        let data = &buf[..info.buffer_size()];
        let luma = |r: u8, g: u8, b: u8| (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0;
        let over_white = |v: f32, a: u8| {
            let a = a as f32 / 255.0;
            v * a + (1.0 - a)
        };
        let values: Vec<f32> = match info.color_type {
            png::ColorType::Grayscale => data.iter().map(|g| *g as f32 / 255.0).collect(),
            png::ColorType::GrayscaleAlpha => {
                data.chunks_exact(2).map(|p| over_white(p[0] as f32 / 255.0, p[1])).collect()
            }
            png::ColorType::Rgb => data.chunks_exact(3).map(|p| luma(p[0], p[1], p[2])).collect(),
            png::ColorType::Rgba => data.chunks_exact(4).map(|p| over_white(luma(p[0], p[1], p[2]), p[3])).collect(),
            png::ColorType::Indexed => {
                return Err(CadpoleError::invalid("heightfield: an indexed PNG did not expand to colour"))
            }
        };
        Grid::new(w, h, values)
    }

    fn from_pgm(bytes: &[u8]) -> Result<Grid> {
        // P5 <ws> width <ws> height <ws> maxval <one ws> data, with # comments.
        let bad = || CadpoleError::invalid("heightfield: a malformed PGM header");
        let mut fields = Vec::new();
        let mut i = 2;
        while fields.len() < 3 {
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'#') {
                if bytes[i] == b'#' {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n: usize = std::str::from_utf8(&bytes[start..i]).ok().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
            fields.push(n);
        }
        i += 1;
        let (w, h, max) = (fields[0], fields[1], fields[2]);
        if max == 0 || max > 65535 || w.saturating_mul(h) > MAX_PIXELS {
            return Err(bad());
        }
        let wide = max > 255;
        let need = w * h * if wide { 2 } else { 1 };
        let data = bytes.get(i..i + need).ok_or_else(|| CadpoleError::invalid("heightfield: the PGM is truncated"))?;
        let values = if wide {
            data.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]]) as f32 / max as f32).collect()
        } else {
            data.iter().map(|g| *g as f32 / max as f32).collect()
        };
        Grid::new(w, h, values)
    }

    fn at(&self, x: isize, y: isize, wrap_x: bool) -> f32 {
        let w = self.width as isize;
        let x = if wrap_x { x.rem_euclid(w) } else { x.clamp(0, w - 1) };
        let y = y.clamp(0, self.height as isize - 1);
        self.values[y as usize * self.width + x as usize]
    }

    /// Bilinear at `(s, t)` in `[0, 1]²`, `s` across and `t` **up** (so
    /// `t = 0` is the bottom row), pixel centres at `(i + ½) / width`.
    fn sample(&self, s: f64, t: f64, wrap_x: bool) -> f64 {
        let x = s * self.width as f64 - 0.5;
        let y = (1.0 - t) * self.height as f64 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let a = self.at(x0, y0, wrap_x);
        let b = self.at(x0 + 1, y0, wrap_x);
        let c = self.at(x0, y0 + 1, wrap_x);
        let d = self.at(x0 + 1, y0 + 1, wrap_x);
        let top = a + (b - a) * fx;
        let bottom = c + (d - c) * fx;
        (top + (bottom - top) * fy) as f64
    }
}

/// A separable Gaussian blur of a row-major `w × h` array, `sx` and `sy`
/// the standard deviations in samples along x and y; x wraps when asked,
/// edges clamp otherwise. A sigma under a tenth of a sample is no blur.
fn blur(values: &mut [f32], w: usize, h: usize, sx: f64, sy: f64, wrap_x: bool) {
    let kernel = |sigma: f64| -> Vec<f32> {
        let r = (3.0 * sigma).ceil() as isize;
        let mut k: Vec<f32> =
            (-r..=r).map(|i| fmath::exp(-((i * i) as f64) / (2.0 * sigma * sigma)) as f32).collect();
        let s: f32 = k.iter().sum();
        k.iter_mut().for_each(|x| *x /= s);
        k
    };
    if sx >= 0.1 {
        let k = kernel(sx);
        let r = (k.len() / 2) as isize;
        let mut row = vec![0f32; w];
        for y in 0..h {
            let line = &mut values[y * w..(y + 1) * w];
            for (x, out) in row.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let xi = x as isize + j as isize - r;
                    let xi = if wrap_x { xi.rem_euclid(w as isize) } else { xi.clamp(0, w as isize - 1) };
                    acc += kv * line[xi as usize];
                }
                *out = acc;
            }
            line.copy_from_slice(&row);
        }
    }
    if sy >= 0.1 {
        let k = kernel(sy);
        let r = (k.len() / 2) as isize;
        let mut col = vec![0f32; h];
        for x in 0..w {
            for (y, out) in col.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let yi = (y as isize + j as isize - r).clamp(0, h as isize - 1);
                    acc += kv * values[yi as usize * w + x];
                }
                *out = acc;
            }
            for (y, v) in col.iter().enumerate() {
                values[y * w + x] = *v;
            }
        }
    }
}

/// The surface the field stands on.
#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    /// A `width × height` rectangle in the XY plane, centred on the origin,
    /// the relief rising in +z from the back at z = 0.
    Plane { width: f64, height: f64 },
    /// A cylinder about +z from z = 0 to `height`, `angle` degrees of it
    /// centred on −y (the side a front view sees), the picture reading left
    /// to right from outside.
    Cylinder { radius: f64, height: f64, angle: f64 },
    /// An open profile `(r, z)` from its first point to its last, revolved
    /// about +z through `angle` degrees centred on −y. The relief stands on
    /// the side the profile's left-hand normal `(dz, −dr)` points to: out
    /// of a profile listed from the foot up.
    Revolve { profile: Vec<(f64, f64)>, angle: f64 },
}

/// Which side of the base the relief stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The base's own normal side: +z on a plane, away from the axis on a
    /// cylinder or a profile listed from the foot up.
    Outside,
    /// The other side: a cylinder's bore, smooth outside.
    Inside,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Outside, Side::Inside];

    pub fn name(self) -> &'static str {
        match self {
            Side::Outside => "outside",
            Side::Inside => "inside",
        }
    }

    pub fn parse(s: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|v| v.name() == s)
    }
}

/// Everything about a height field except its values.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub base: Base,
    /// `(lo, hi)`: the wall at value 0 and at value 1 (swapped by `invert`).
    pub thickness: (f64, f64),
    pub invert: bool,
    /// Grid pitch in mm along the surface.
    pub resolution: f64,
    /// Gaussian sigma in mm applied to the values before they become
    /// thickness; 0 is none.
    pub smoothing: f64,
    /// Width in mm of the band round the open edges held at
    /// `border_thickness`.
    pub border: f64,
    pub border_thickness: Option<f64>,
    pub side: Side,
}

/// Where the grid's nodes are: `nu × nv` of them, `u` and `v` in mm on the
/// developed surface (`u` along the width or round the turn at the widest
/// radius, `v` up the height or along the profile).
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub nu: usize,
    pub nv: usize,
    pub u_len: f64,
    pub v_len: f64,
    /// A full turn: node `nu` would be node 0, so it is not repeated.
    pub periodic: bool,
}

impl Layout {
    pub fn u(&self, i: usize) -> f64 {
        let cells = if self.periodic { self.nu } else { self.nu - 1 };
        self.u_len * i as f64 / cells as f64
    }
    pub fn v(&self, j: usize) -> f64 {
        self.v_len * j as f64 / (self.nv - 1) as f64
    }
    /// Triangles the body will have, before it is built.
    pub fn triangles(&self, full_back: bool) -> usize {
        let cu = if self.periodic { self.nu } else { self.nu - 1 };
        let cv = self.nv - 1;
        let relief = 2 * cu * cv;
        // An open field's first and last back columns are fans of `nv`
        // triangles onto the side walls (one column of quads when there is
        // only one column).
        let back = if full_back {
            relief
        } else if self.periodic {
            2 * cu
        } else if cu == 1 {
            2 * cv
        } else {
            2 * (cu - 2) + 2 * (cv + 1)
        };
        let rims = 4 * cu;
        let sides = if self.periodic { 0 } else { 4 * cv };
        relief + back + rims + sides
    }
}

/// The profile's arc-length table: cumulative length at each point.
fn arc_lengths(profile: &[(f64, f64)]) -> Vec<f64> {
    let mut s = vec![0.0];
    for w in profile.windows(2) {
        let d = fmath::hypot(w[1].0 - w[0].0, w[1].1 - w[0].1);
        s.push(s.last().copied().unwrap_or(0.0) + d);
    }
    s
}

impl Spec {
    pub fn validate(&self) -> Result<()> {
        let pos = |v: f64, what: &str| -> Result<()> {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(CadpoleError::invalid(format!("heightfield: {what} must be a positive number of mm, got {v}")))
            }
        };
        let (lo, hi) = self.thickness;
        if !(lo.is_finite() && hi.is_finite() && lo > 0.0 && hi > 0.0) {
            return Err(CadpoleError::invalid(format!(
                "heightfield: thickness=({lo}, {hi}) must be two positive numbers of mm (the wall never \
                 reaches zero, or the shell would touch itself)"
            )));
        }
        pos(self.resolution, "resolution")?;
        if !(self.smoothing.is_finite() && self.smoothing >= 0.0) {
            return Err(CadpoleError::invalid("heightfield: smoothing must be 0 or more mm"));
        }
        if !(self.border.is_finite() && self.border >= 0.0) {
            return Err(CadpoleError::invalid("heightfield: border must be 0 or more mm"));
        }
        if let Some(b) = self.border_thickness {
            pos(b, "border_thickness")?;
        }
        let angle_ok = |a: f64| -> Result<()> {
            if a.is_finite() && a > 0.0 && a <= 360.0 {
                Ok(())
            } else {
                Err(CadpoleError::invalid(format!("heightfield: angle must be in (0, 360] degrees, got {a}")))
            }
        };
        match &self.base {
            Base::Plane { width, height } => {
                pos(*width, "width")?;
                pos(*height, "height")?;
                if self.side == Side::Inside {
                    return Err(CadpoleError::invalid(
                        "heightfield: a plane has no inside; turn the panel over with rotate()",
                    ));
                }
            }
            Base::Cylinder { radius, height, angle } => {
                pos(*radius, "radius")?;
                pos(*height, "height")?;
                angle_ok(*angle)?;
                if self.side == Side::Inside && lo.max(hi).max(self.border_thickness.unwrap_or(0.0)) >= *radius {
                    return Err(CadpoleError::invalid(format!(
                        "heightfield: a relief inside a cylinder of radius {radius} cannot be that thick"
                    )));
                }
            }
            Base::Revolve { profile, angle } => {
                angle_ok(*angle)?;
                if profile.len() < 2 {
                    return Err(CadpoleError::invalid("heightfield: a profile needs at least two (r, z) points"));
                }
                if let Some((k, p)) = profile.iter().enumerate().find(|(_, p)| !(p.0.is_finite() && p.1.is_finite() && p.0 > 0.0)) {
                    return Err(CadpoleError::invalid(format!(
                        "heightfield: profile point {k} ({}, {}) must have r > 0: a relief cannot close over the axis",
                        p.0, p.1
                    )));
                }
                if let Some(k) = arc_lengths(profile).windows(2).position(|w| w[1] - w[0] <= 1e-9) {
                    return Err(CadpoleError::invalid(format!(
                        "heightfield: profile points {k} and {} are the same point",
                        k + 1
                    )));
                }
            }
        }
        Ok(())
    }

    /// The grid's nodes for this base at this resolution.
    pub fn layout(&self) -> Result<Layout> {
        self.validate()?;
        let (u_len, v_len, periodic) = match &self.base {
            Base::Plane { width, height } => (*width, *height, false),
            Base::Cylinder { radius, height, angle } => (radius * angle.to_radians(), *height, *angle >= 360.0),
            Base::Revolve { profile, angle } => {
                let r = profile.iter().map(|p| p.0).fold(0.0, f64::max);
                (r * angle.to_radians(), *arc_lengths(profile).last().unwrap_or(&0.0), *angle >= 360.0)
            }
        };
        let cells = |len: f64| ((len / self.resolution).round() as usize).max(1);
        let cu = cells(u_len).max(if periodic { 8 } else { 1 });
        let nu = if periodic { cu } else { cu + 1 };
        let nv = cells(v_len) + 1;
        let layout = Layout { nu, nv, u_len, v_len, periodic };
        let n = layout.triangles(self.full_back());
        if n > MAX_TRIANGLES {
            return Err(CadpoleError::invalid(format!(
                "heightfield: {nu} x {nv} nodes at resolution {} mm make {n} triangles; the most is \
                 {MAX_TRIANGLES} (coarsen the resolution)",
                self.resolution
            )));
        }
        Ok(layout)
    }

    /// Whether the back needs every row (it is curved in `v`).
    fn full_back(&self) -> bool {
        matches!(self.base, Base::Revolve { .. })
    }

    /// Values at the nodes of `layout`, sampled from a picture stretched
    /// over the whole field (`wrap` on a full turn), prefiltered when the
    /// picture is finer than the grid so that detail averages rather than
    /// aliases.
    pub fn sample_grid(&self, layout: &Layout, grid: &Grid) -> Vec<f32> {
        let mut g = grid.clone();
        let px_per_node_x = grid.width as f64 / layout.nu as f64;
        let px_per_node_y = grid.height as f64 / layout.nv as f64;
        let pre = |f: f64| if f > 1.0 { 0.5 * f } else { 0.0 };
        blur(&mut g.values, g.width, g.height, pre(px_per_node_x), pre(px_per_node_y), layout.periodic);
        let mut out = Vec::with_capacity(layout.nu * layout.nv);
        for j in 0..layout.nv {
            let t = layout.v(j) / layout.v_len;
            for i in 0..layout.nu {
                // A full turn maps u in [0, U) onto pixel columns that wrap;
                // an open field maps its first and last node to the
                // picture's outer edges.
                let s = if layout.periodic { layout.u(i) / layout.u_len } else { i as f64 / (layout.nu - 1) as f64 };
                out.push(g.sample(s, t, layout.periodic) as f32);
            }
        }
        out
    }

    /// The wall thickness at every node, from values at the nodes
    /// (`values[j * nu + i]`, row `j = 0` at `v = 0`).
    pub fn thickness_at_nodes(&self, layout: &Layout, values: &[f32]) -> Result<Vec<f32>> {
        if values.len() != layout.nu * layout.nv {
            return Err(CadpoleError::invalid(format!(
                "heightfield: {} values for {} x {} nodes",
                values.len(),
                layout.nu,
                layout.nv
            )));
        }
        let mut vals: Vec<f32> = values.iter().map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }).collect();
        if self.smoothing > 0.0 {
            let du = layout.u_len / if layout.periodic { layout.nu } else { layout.nu - 1 } as f64;
            let dv = layout.v_len / (layout.nv - 1) as f64;
            blur(&mut vals, layout.nu, layout.nv, self.smoothing / du, self.smoothing / dv, layout.periodic);
        }
        let (lo, hi) = self.thickness;
        let (a, b) = if self.invert { (hi, lo) } else { (lo, hi) };
        let frame = self.border_thickness.unwrap_or(lo.max(hi)) as f32;
        let mut t: Vec<f32> = vals.iter().map(|v| (a + (b - a) * *v as f64) as f32).collect();
        if self.border > 0.0 {
            for j in 0..layout.nv {
                let v = layout.v(j);
                let edge_v = v < self.border - 1e-9 || v > layout.v_len - self.border + 1e-9;
                for i in 0..layout.nu {
                    let u = layout.u(i);
                    let edge_u = !layout.periodic && (u < self.border - 1e-9 || u > layout.u_len - self.border + 1e-9);
                    if edge_v || edge_u {
                        t[j * layout.nu + i] = frame;
                    }
                }
            }
        }
        Ok(t)
    }

    /// The base point and the unit normal on the relief side at every node,
    /// row-major like the values.
    fn frame(&self, layout: &Layout) -> (Vec<Vec3>, Vec<Vec3>) {
        let n = layout.nu * layout.nv;
        let mut base = Vec::with_capacity(n);
        let mut normal = Vec::with_capacity(n);
        let sign = if self.side == Side::Inside { -1.0 } else { 1.0 };
        match &self.base {
            Base::Plane { width, height } => {
                for j in 0..layout.nv {
                    for i in 0..layout.nu {
                        base.push(Vec3::new(layout.u(i) - width / 2.0, layout.v(j) - height / 2.0, 0.0));
                        normal.push(Vec3::Z * sign);
                    }
                }
            }
            Base::Cylinder { radius, angle, .. } => {
                let trig = turn_trig(layout, *radius, *angle);
                for j in 0..layout.nv {
                    for &(c, s) in &trig {
                        base.push(Vec3::new(radius * c, radius * s, layout.v(j)));
                        normal.push(Vec3::new(c, s, 0.0) * sign);
                    }
                }
            }
            Base::Revolve { profile, angle } => {
                let r_ref = profile.iter().map(|p| p.0).fold(0.0, f64::max);
                let trig = turn_trig(layout, r_ref, *angle);
                let s = arc_lengths(profile);
                for j in 0..layout.nv {
                    let (r, z, nr, nz) = profile_at(profile, &s, layout.v(j));
                    for &(c, sn) in &trig {
                        base.push(Vec3::new(r * c, r * sn, z));
                        normal.push(Vec3::new(nr * c, nr * sn, nz) * sign);
                    }
                }
            }
        }
        (base, normal)
    }

    /// The solid: relief, back and walls as six face groups named
    /// `<prefix>/relief`, `/back`, `/bottom`, `/top`, `/start`, `/end`.
    /// `thickness` comes from [`Spec::thickness_at_nodes`].
    pub fn mesh_body(&self, layout: &Layout, thickness: &[f32], prefix: &str) -> Result<MeshBody> {
        let (nu, nv) = (layout.nu, layout.nv);
        if thickness.len() != nu * nv {
            return Err(CadpoleError::invalid("heightfield: thickness does not match the grid"));
        }
        let (base, normal) = self.frame(layout);
        let top: Vec<Vec3> = base.iter().zip(&normal).zip(thickness).map(|((b, n), t)| *b + *n * *t as f64).collect();
        let full_back = self.full_back();
        // Orientation of (u, v, w) ↦ space: the relief's u × v points along
        // +N unless the relief is on the inside.
        let flip = self.side == Side::Inside;
        let mut b = Builder { mesh: Mesh::default(), groups: Vec::new(), flip };
        let node = |i: usize, j: usize| j * nu + (i % nu);
        let cu = if layout.periodic { nu } else { nu - 1 };

        // relief (w = 1): the grid, normals by central differences.
        b.group(&format!("{prefix}/relief"), |b| {
            let first = b.vertex_count();
            for j in 0..nv {
                for i in 0..nu {
                    let du = |i: usize| top[node(i, j)];
                    let (ia, ib) = if layout.periodic {
                        ((i + nu - 1) % nu, (i + 1) % nu)
                    } else {
                        (i.saturating_sub(1), (i + 1).min(nu - 1))
                    };
                    let (ja, jb) = (j.saturating_sub(1), (j + 1).min(nv - 1));
                    let tu = du(ib) - du(ia);
                    let tv = top[node(i, jb)] - top[node(i, ja)];
                    let mut n = tu.cross(tv);
                    if flip {
                        n = -n;
                    }
                    let n = if n.length() > 0.0 { n.normalized() } else { normal[node(i, j)] };
                    b.vertex(top[node(i, j)], n);
                }
            }
            for j in 0..nv - 1 {
                for i in 0..cu {
                    let (p00, p10, p11, p01) = (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
                    // The diagonal across the smaller step in thickness
                    // keeps a ridge a ridge.
                    let split_00_11 = (thickness[p00] - thickness[p11]).abs() <= (thickness[p10] - thickness[p01]).abs();
                    let v = |k: usize| first + k as u32;
                    b.quad(v(p00), v(p10), v(p11), v(p01), split_00_11);
                }
            }
        });

        // back (w = 0), wound the other way.
        b.group(&format!("{prefix}/back"), |b| {
            let rows: Vec<usize> = if full_back { (0..nv).collect() } else { vec![0, nv - 1] };
            // Vertices: every node of the kept rows, and on an open field
            // every node of the first and last columns too.
            let mut index = vec![u32::MAX; nu * nv];
            for &j in &rows {
                for i in 0..nu {
                    index[node(i, j)] = b.vertex(base[node(i, j)], -normal[node(i, j)]);
                }
            }
            if !layout.periodic {
                for j in 0..nv {
                    for i in [0, nu - 1] {
                        if index[node(i, j)] == u32::MAX {
                            index[node(i, j)] = b.vertex(base[node(i, j)], -normal[node(i, j)]);
                        }
                    }
                }
            }
            if full_back {
                for j in 0..nv - 1 {
                    for i in 0..cu {
                        let (p00, p10, p11, p01) = (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
                        b.quad(index[p00], index[p01], index[p11], index[p10], true);
                    }
                }
            } else {
                for i in 0..cu {
                    let i1 = (i + 1) % nu;
                    let left_full = !layout.periodic && i == 0;
                    let right_full = !layout.periodic && i1 == nu - 1;
                    let col = |i: usize, full: bool| -> Vec<u32> {
                        if full {
                            (0..nv).map(|j| index[node(i, j)]).collect()
                        } else {
                            vec![index[node(i, 0)], index[node(i, nv - 1)]]
                        }
                    };
                    // Seen from the back the strip runs right to left, so
                    // its columns are handed over swapped.
                    b.strip(&col(i1, right_full), &col(i, left_full));
                }
            }
        });

        // bottom (v = 0) and top (v = V): relief row to base row.
        for (name, j, outward_is_minus_v) in [("bottom", 0, true), ("top", nv - 1, false)] {
            b.group(&format!("{prefix}/{name}"), |b| {
                let mut lo = Vec::with_capacity(nu);
                let mut hi = Vec::with_capacity(nu);
                for i in 0..nu {
                    let a = top[node((i + 1) % nu, j)] - top[node((i + nu - 1) % nu, j)];
                    let a = if layout.periodic { a } else { top[node((i + 1).min(nu - 1), j)] - top[node(i.saturating_sub(1), j)] };
                    let w = top[node(i, j)] - base[node(i, j)];
                    // u × w is −v: outward on the bottom, inward on the top.
                    let mut n = a.cross(w);
                    if !outward_is_minus_v {
                        n = -n;
                    }
                    if flip {
                        n = -n;
                    }
                    let n = if n.length() > 0.0 { n.normalized() } else { Vec3::Z };
                    lo.push(b.vertex(base[node(i, j)], n));
                    hi.push(b.vertex(top[node(i, j)], n));
                }
                for i in 0..cu {
                    let i1 = (i + 1) % nu;
                    if outward_is_minus_v {
                        b.quad(lo[i], lo[i1], hi[i1], hi[i], true);
                    } else {
                        b.quad(lo[i], hi[i], hi[i1], lo[i1], true);
                    }
                }
            });
        }

        // start (u = 0) and end (u = U) on an open field.
        if !layout.periodic {
            for (name, i, outward_is_minus_u) in [("start", 0, true), ("end", nu - 1, false)] {
                b.group(&format!("{prefix}/{name}"), |b| {
                    let mut lo = Vec::with_capacity(nv);
                    let mut hi = Vec::with_capacity(nv);
                    for j in 0..nv {
                        let a = top[node(i, (j + 1).min(nv - 1))] - top[node(i, j.saturating_sub(1))];
                        let w = top[node(i, j)] - base[node(i, j)];
                        // w × v is −u.
                        let mut n = w.cross(a);
                        if !outward_is_minus_u {
                            n = -n;
                        }
                        if flip {
                            n = -n;
                        }
                        let n = if n.length() > 0.0 { n.normalized() } else { Vec3::X };
                        lo.push(b.vertex(base[node(i, j)], n));
                        hi.push(b.vertex(top[node(i, j)], n));
                    }
                    for j in 0..nv - 1 {
                        if outward_is_minus_u {
                            b.quad(lo[j], hi[j], hi[j + 1], lo[j + 1], true);
                        } else {
                            b.quad(lo[j], lo[j + 1], hi[j + 1], hi[j], true);
                        }
                    }
                });
            }
        }
        Ok(b.finish())
    }
}

/// `(cos θ, sin θ)` at every node round a turn of `angle` degrees centred
/// on −y, `u` measured at radius `r_ref`.
fn turn_trig(layout: &Layout, r_ref: f64, angle: f64) -> Vec<(f64, f64)> {
    let start = -std::f64::consts::FRAC_PI_2 - angle.to_radians() / 2.0;
    (0..layout.nu)
        .map(|i| {
            let (sin, cos) = fmath::sin_cos(start + layout.u(i) / r_ref);
            (cos, sin)
        })
        .collect()
}

/// The point `(r, z)` at arc length `s` along the profile and the unit
/// normal `(dz, −dr)` there — averaged over the two segments at a vertex,
/// so a corner's relief does not tear.
fn profile_at(profile: &[(f64, f64)], cum: &[f64], s: f64) -> (f64, f64, f64, f64) {
    let n = profile.len();
    let k = match cum.iter().position(|c| *c > s) {
        Some(0) => 0,
        Some(k) => k - 1,
        None => n - 2,
    }
    .min(n - 2);
    let (a, b) = (profile[k], profile[k + 1]);
    let len = cum[k + 1] - cum[k];
    let t = ((s - cum[k]) / len).clamp(0.0, 1.0);
    let seg_normal = |k: usize| {
        let (a, b) = (profile[k], profile[k + 1]);
        let (dr, dz) = (b.0 - a.0, b.1 - a.1);
        let l = fmath::hypot(dr, dz);
        (dz / l, -dr / l)
    };
    let mut nrm = seg_normal(k);
    // Within a hair of an interior vertex, blend with the neighbour.
    let at_start = (s - cum[k]).abs() <= 1e-9 * cum[n - 1].max(1.0);
    let at_end = (cum[k + 1] - s).abs() <= 1e-9 * cum[n - 1].max(1.0);
    if at_start && k > 0 {
        let o = seg_normal(k - 1);
        nrm = (nrm.0 + o.0, nrm.1 + o.1);
    } else if at_end && k + 2 < n {
        let o = seg_normal(k + 1);
        nrm = (nrm.0 + o.0, nrm.1 + o.1);
    }
    let l = fmath::hypot(nrm.0, nrm.1);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, nrm.0 / l, nrm.1 / l)
}

/// Accumulates triangles into named face groups.
struct Builder {
    mesh: Mesh,
    groups: Vec<(String, u32, u32)>,
    flip: bool,
}

impl Builder {
    fn vertex_count(&self) -> u32 {
        (self.mesh.positions.len() / 3) as u32
    }

    fn vertex(&mut self, p: Vec3, n: Vec3) -> u32 {
        let i = self.vertex_count();
        self.mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
        self.mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        i
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        if self.flip {
            self.mesh.indices.extend_from_slice(&[a, c, b]);
        } else {
            self.mesh.indices.extend_from_slice(&[a, b, c]);
        }
        self.mesh.face_ids.push(self.groups.len() as u32);
    }

    /// `a b c d` counter-clockwise seen from outside, split along `a–c`
    /// or `b–d`.
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32, split_ac: bool) {
        if split_ac {
            self.tri(a, b, c);
            self.tri(a, c, d);
        } else {
            self.tri(a, b, d);
            self.tri(b, c, d);
        }
    }

    /// The strip between two columns of vertices that run the same way
    /// (`left[0]` beside `right[0]`, both ending together), the region
    /// counter-clockwise as `left` up and `right` down. A column of two is
    /// one straight edge; the other column's extra vertices are on the
    /// strip's far side, so each is fanned from the nearer end of the short
    /// column and the two halves meet in one triangle.
    fn strip(&mut self, left: &[u32], right: &[u32]) {
        if left.len() == right.len() {
            for k in 0..left.len() - 1 {
                self.quad(left[k], right[k], right[k + 1], left[k + 1], true);
            }
            return;
        }
        // Exactly one is the two-vertex column.
        let (long, short, long_is_left) = if left.len() > right.len() { (left, right, true) } else { (right, left, false) };
        let m = long.len() - 1;
        let half = m / 2;
        let (s0, s1) = (short[0], short[1]);
        for k in 0..m {
            let anchor = if k < half { s0 } else { s1 };
            if long_is_left {
                self.tri(long[k], anchor, long[k + 1]);
            } else {
                self.tri(long[k], long[k + 1], anchor);
            }
        }
        if long_is_left {
            self.tri(long[half], s0, s1);
        } else {
            self.tri(long[half], s1, s0);
        }
    }

    fn group(&mut self, name: &str, f: impl FnOnce(&mut Builder)) {
        let start = self.mesh.face_ids.len() as u32;
        f(self);
        let count = self.mesh.face_ids.len() as u32 - start;
        self.groups.push((name.to_owned(), start, count));
    }

    fn finish(self) -> MeshBody {
        let mesh = self.mesh;
        let faces = self
            .groups
            .into_iter()
            .map(|(name, tri_start, tri_count)| {
                let mut area = 0.0;
                let mut centroid = Vec3::ZERO;
                let mut normal = Vec3::ZERO;
                for k in tri_start..tri_start + tri_count {
                    if let Some(tri) = mesh.triangle(k as usize) {
                        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                        let a = n.length() / 2.0;
                        area += a;
                        centroid = centroid + (tri[0] + tri[1] + tri[2]) * (a / 3.0);
                        normal = normal + n;
                    }
                }
                FaceInfo {
                    name,
                    surface_kind: "group",
                    centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
                    normal: if normal.length() > 0.0 { normal.normalized() } else { Vec3::ZERO },
                    area,
                    tri_start,
                    tri_count,
                    ..Default::default()
                }
            })
            .collect();
        (mesh, Topology { faces, edges: Vec::new(), vertices: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(base: Base) -> Spec {
        Spec {
            base,
            thickness: (0.6, 3.0),
            invert: false,
            resolution: 1.0,
            smoothing: 0.0,
            border: 0.0,
            border_thickness: None,
            side: Side::Outside,
        }
    }

    /// A deterministic pattern with sharp steps and a gradient.
    fn pattern(l: &Layout) -> Vec<f32> {
        let mut v = Vec::with_capacity(l.nu * l.nv);
        for j in 0..l.nv {
            for i in 0..l.nu {
                let step = if (i / 3 + j / 4) % 2 == 0 { 0.9 } else { 0.1 };
                v.push((step * 0.7 + 0.3 * (i as f32 / l.nu as f32)).clamp(0.0, 1.0));
            }
        }
        v
    }

    fn build(s: &Spec) -> (Layout, Vec<f32>, MeshBody) {
        let l = s.layout().unwrap();
        let t = s.thickness_at_nodes(&l, &pattern(&l)).unwrap();
        let body = s.mesh_body(&l, &t, "hf").unwrap();
        (l, t, body)
    }

    /// Closed, every edge used once each way, embedded, and facing out.
    fn assert_solid(mesh: &Mesh, what: &str) {
        let welded = mesh.welded(1e-6);
        assert!(welded.is_closed_manifold(), "{what}: not a closed 2-manifold");
        let report = crate::solid::validate(mesh);
        assert!(report.is_solid(), "{what}: {}", report.summary());
        assert!(welded.volume() > 0.0, "{what}: inside out (volume {})", welded.volume());
        assert_eq!(mesh.face_ids.len(), mesh.triangle_count());
    }

    fn bases() -> Vec<(&'static str, Base, Side)> {
        vec![
            ("plane", Base::Plane { width: 30.0, height: 20.0 }, Side::Outside),
            ("cylinder", Base::Cylinder { radius: 12.0, height: 15.0, angle: 360.0 }, Side::Outside),
            ("cylinder inside", Base::Cylinder { radius: 12.0, height: 15.0, angle: 360.0 }, Side::Inside),
            ("arc", Base::Cylinder { radius: 12.0, height: 15.0, angle: 120.0 }, Side::Outside),
            ("arc inside", Base::Cylinder { radius: 12.0, height: 15.0, angle: 200.0 }, Side::Inside),
            (
                "shade",
                Base::Revolve { profile: vec![(20.0, 0.0), (16.0, 12.0), (12.0, 20.0)], angle: 360.0 },
                Side::Outside,
            ),
            ("half shade", Base::Revolve { profile: vec![(20.0, 0.0), (14.0, 18.0)], angle: 180.0 }, Side::Outside),
        ]
    }

    #[test]
    fn every_base_makes_a_closed_outward_solid() {
        for (what, base, side) in bases() {
            let mut s = spec(base);
            s.side = side;
            let (l, _, (mesh, topo)) = build(&s);
            assert_solid(&mesh, what);
            assert_eq!(mesh.triangle_count(), l.triangles(matches!(s.base, Base::Revolve { .. })), "{what}");
            let names: Vec<&str> = topo.faces.iter().map(|f| f.name.as_str()).collect();
            if l.periodic {
                assert_eq!(names, ["hf/relief", "hf/back", "hf/bottom", "hf/top"], "{what}");
            } else {
                assert_eq!(names, ["hf/relief", "hf/back", "hf/bottom", "hf/top", "hf/start", "hf/end"], "{what}");
            }
        }
    }

    /// The wall at every node is the thickness its value asks for, measured
    /// as the distance from the base along the normal, and stays inside
    /// `[lo, hi]`; a border holds the frame thickness.
    #[test]
    fn the_wall_follows_the_values_within_the_thickness_range() {
        let mut s = spec(Base::Cylinder { radius: 12.0, height: 15.0, angle: 360.0 });
        s.border = 2.0;
        s.border_thickness = Some(3.5);
        let l = s.layout().unwrap();
        let vals = pattern(&l);
        let t = s.thickness_at_nodes(&l, &vals).unwrap();
        let (mesh, topo) = s.mesh_body(&l, &t, "hf").unwrap();
        let relief = &topo.faces[0];
        let mut radii = Vec::new();
        for k in relief.tri_start..relief.tri_start + relief.tri_count {
            for p in mesh.triangle(k as usize).unwrap() {
                radii.push((p.x * p.x + p.y * p.y).sqrt() - 12.0);
            }
        }
        let (lo, hi) = radii.iter().fold((f64::MAX, f64::MIN), |(a, b), r| (a.min(*r), b.max(*r)));
        assert!(lo >= 0.6 - 1e-4 && hi <= 3.5 + 1e-4, "{lo} .. {hi}");
        for j in 0..l.nv {
            for i in 0..l.nu {
                let k = j * l.nu + i;
                let v = l.v(j);
                let want = if v < 2.0 - 1e-9 || v > 13.0 + 1e-9 { 3.5 } else { 0.6 + 2.4 * vals[k] as f64 };
                assert!((t[k] as f64 - want).abs() < 1e-5, "node ({i}, {j}): {} vs {want}", t[k]);
            }
        }
        // Inverted, value 1 is the thin wall.
        s.invert = true;
        s.border = 0.0;
        let ones = vec![1.0f32; l.nu * l.nv];
        assert!(s.thickness_at_nodes(&l, &ones).unwrap().iter().all(|t| (*t - 0.6).abs() < 1e-6));
    }

    /// A full turn closes on itself: the seam column is the first column, so
    /// a step in the values right at the seam leaves no crack, the relief's
    /// edges there are used once each way, and the outer wall is continuous.
    #[test]
    fn a_cylinder_seam_has_no_crack() {
        let s = spec(Base::Cylinder { radius: 10.0, height: 8.0, angle: 360.0 });
        let l = s.layout().unwrap();
        // Thin on the last column, thick on the first: the worst seam.
        let vals: Vec<f32> = (0..l.nu * l.nv).map(|k| if k % l.nu == l.nu - 1 { 0.0 } else if k % l.nu == 0 { 1.0 } else { 0.5 }).collect();
        let t = s.thickness_at_nodes(&l, &vals).unwrap();
        let (mesh, topo) = s.mesh_body(&l, &t, "hf").unwrap();
        assert_solid(&mesh, "seam");
        // The relief alone, welded, has open edges only along its top and
        // bottom rows: none along the seam.
        let r = &topo.faces[0];
        let sub = Mesh {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            indices: mesh.indices[(r.tri_start * 3) as usize..((r.tri_start + r.tri_count) * 3) as usize].to_vec(),
            face_ids: vec![0; r.tri_count as usize],
            colors: Vec::new(),
        };
        let w = sub.welded(1e-6);
        let mut uses = std::collections::HashMap::new();
        for t in w.indices.chunks_exact(3) {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let p = |i: u32| Vec3::new(w.positions[3 * i as usize] as f64, w.positions[3 * i as usize + 1] as f64, w.positions[3 * i as usize + 2] as f64);
        for ((a, b), n) in uses {
            if n == 1 {
                let (za, zb) = (p(a).z, p(b).z);
                assert!(
                    (za.abs() < 1e-4 && zb.abs() < 1e-4) || ((za - 8.0).abs() < 1e-4 && (zb - 8.0).abs() < 1e-4),
                    "an open relief edge off the rims: {:?} {:?}",
                    p(a),
                    p(b)
                );
            }
        }
    }

    #[test]
    fn a_picture_is_read_and_stretched_over_the_field() {
        // A 4 x 2 greyscale PNG: left half black, right half white.
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 4, 2);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[0, 0, 255, 255, 0, 0, 255, 255]).unwrap();
        }
        let g = Grid::from_image(&png_bytes).unwrap();
        assert_eq!((g.width, g.height), (4, 2));
        let s = spec(Base::Plane { width: 40.0, height: 10.0 });
        let l = s.layout().unwrap();
        let v = s.sample_grid(&l, &g);
        assert!(v[0] < 0.01 && v[l.nu - 1] > 0.99, "{} {}", v[0], v[l.nu - 1]);
        let pgm = b"P5\n# a comment\n2 1\n255\n\x00\xff";
        let g = Grid::from_image(pgm).unwrap();
        assert_eq!(g.values, vec![0.0, 1.0]);
        assert!(Grid::from_image(b"GIF89a").is_err());
    }

    #[test]
    fn bad_specs_are_diagnostics() {
        let mut s = spec(Base::Plane { width: 10.0, height: 10.0 });
        s.thickness = (0.0, 2.0);
        assert!(s.layout().unwrap_err().to_string().contains("thickness"));
        let mut s = spec(Base::Plane { width: 10.0, height: 10.0 });
        s.side = Side::Inside;
        assert!(s.layout().unwrap_err().to_string().contains("no inside"));
        let s = spec(Base::Revolve { profile: vec![(0.0, 0.0), (5.0, 5.0)], angle: 360.0 });
        assert!(s.layout().unwrap_err().to_string().contains("r > 0"));
        let mut s = spec(Base::Plane { width: 1000.0, height: 1000.0 });
        s.resolution = 0.1;
        assert!(s.layout().unwrap_err().to_string().contains("triangles"));
    }

    /// The build is one pass over the grid: four times the nodes take about
    /// four times as long, nowhere near sixteen. Best of three, timed
    /// together, so a busy machine slows both alike; the absolute bound is
    /// generous enough for a debug build on a loaded machine.
    #[test]
    fn building_is_linear_in_the_grid() {
        let time = |res: f64| {
            let mut s = spec(Base::Cylinder { radius: 30.0, height: 60.0, angle: 360.0 });
            s.resolution = res;
            let l = s.layout().unwrap();
            let vals = pattern(&l);
            let mut best = f64::INFINITY;
            let mut tris = 0;
            for _ in 0..3 {
                let t0 = std::time::Instant::now();
                let t = s.thickness_at_nodes(&l, &vals).unwrap();
                let (m, _) = s.mesh_body(&l, &t, "hf").unwrap();
                best = best.min(t0.elapsed().as_secs_f64());
                tris = m.triangle_count();
            }
            (best, tris)
        };
        let (small, n_small) = time(0.5);
        let (large, n_large) = time(0.25);
        assert!(n_large > 3 * n_small, "{n_small} {n_large}");
        assert!(large < 10.0 * small.max(1e-3), "4x the grid took {:.1}x as long", large / small);
        assert!(large < 30.0, "{n_large} triangles took {large:.1} s");
    }
}
