//! Screen-space ambient occlusion and the ground contact shadow.
//!
//! Both stages are deterministic by construction: the sample kernel, the
//! per-pixel rotation table and the contact disc are compile-time constants
//! (no RNG, no per-thread state), every worker writes its own band of the
//! output, and nothing depends on the order the bands are finished in.
//!
//! SSAO darkens creases and cavities in the ambient term only (`Frame::ao`,
//! consumed by `shading`); the contact shadow darkens the background where
//! the model comes close to the ground plane (`Frame::ground`, multiplied
//! into the background by the tonemap) and feeds a weak extra occlusion back
//! into `Frame::ao` so an underside meets its own shadow.

use super::aa::{par_bands, par_bands2};
use super::{COV_OPAQUE, Ctx, Frame, add, cross, normalize, scale, smoothstep};

/// 16 cosine-distributed hemisphere samples (Hammersley), pulled toward the
/// origin by `lerp(0.1, 1, (i/16)^2)` so near-field detail is sampled more
/// densely than the rim of the hemisphere.
const KERNEL: [[f32; 3]; 16] = [
    [0.017678, 0.000000, 0.098425],
    [-0.031695, 0.000000, 0.098544],
    [0.000000, 0.045087, 0.104773],
    [0.000000, -0.061569, 0.116355],
    [0.058594, 0.058594, 0.132467],
    [-0.077895, -0.077895, 0.152209],
    [-0.102110, 0.102110, 0.174578],
    [0.131810, -0.131810, 0.198446],
    [0.218851, 0.090651, 0.222512],
    [-0.273913, -0.113459, 0.245241],
    [-0.139988, 0.337962, 0.264752],
    [0.170455, -0.411516, 0.278630],
    [0.205063, 0.495065, 0.283547],
    [-0.244002, -0.589074, 0.274383],
    [-0.693986, 0.287458, 0.241600],
    [0.810227, -0.335607, 0.157511],
];

/// Per-pixel rotation of the kernel, indexed by `(y & 3) * 4 + (x & 3)`
/// (cos, sin of a Bayer-ordered angle). The AA jitter averages the four
/// phases away, and the depth-aware blur below cleans up what is left.
const ROT: [[f32; 2]; 16] = [
    [1.000000, 0.000000],
    [-1.000000, 0.000000],
    [0.707107, 0.707107],
    [-0.707107, -0.707107],
    [0.000000, -1.000000],
    [0.000000, 1.000000],
    [0.707107, -0.707107],
    [-0.707107, 0.707107],
    [0.382683, 0.923880],
    [-0.382683, -0.923880],
    [0.923880, 0.382683],
    [-0.923880, -0.382683],
    [0.923880, -0.382683],
    [-0.923880, 0.382683],
    [0.382683, -0.923880],
    [-0.382683, 0.923880],
];

/// 12-tap disc (golden-angle spiral), radii in 0..1 of the contact radius.
const DISC: [[f32; 2]; 12] = [
    [0.179136, 0.097862],
    [-0.343282, 0.084600],
    [0.253008, -0.379895],
    [0.082890, 0.533663],
    [-0.478055, -0.382706],
    [0.675504, -0.045024],
    [-0.508424, 0.532138],
    [0.016587, -0.790395],
    [0.555364, 0.632380],
    [-0.884522, -0.096367],
    [0.754124, -0.553442],
    [-0.190703, 0.960191],
];

/// SSAO radius as a fraction of the scene diagonal. The kernel's own radii
/// span 10%..100% of it, so one radius covers both tight creases and
/// finger-sized pockets.
const AO_RADIUS: f32 = 0.08;

/// Depth bias as a fraction of the scene diagonal.
const AO_BIAS: f32 = 0.002;
/// Blur taps further than this (fraction of the diagonal) in depth are not
/// blended, so the blur never bleeds across a silhouette.
const AO_BLUR_DEPTH: f32 = 0.02;
/// Contact shadow radius as a fraction of the scene diagonal.
const CONTACT_RADIUS: f32 = 0.25;
/// Darkening of the ground directly under the model.
const CONTACT_STRENGTH: f32 = 0.85;

/// `size x size` top-down min-z raster of every item over the scene
/// footprint, padded so the whole contact shadow fits inside it. Empty cells
/// hold `ground_z + 4 * diag` (a height no contact tap can see) rather than
/// infinity, so the bilinear lookup stays finite and smooth.
///
/// The contact shadow itself is baked here too, once per render: it is a
/// function of world xy only, so computing it per grid cell (16 k cells) and
/// interpolating is both far cheaper than 24 heightfield taps per background
/// pixel per AA pass and smoother (no per-pixel tap pattern to band).
pub(crate) struct Heightfield {
    pub(crate) size: u32,
    pub(crate) min_z: Vec<f32>,
    /// Per-cell ground darkening, 1 = untouched.
    shadow: Vec<f32>,
    /// World-space xy of cell (0, 0)'s lower corner.
    origin: [f32; 2],
    /// World size of one cell (x, y).
    cell: [f32; 2],
    ground_z: f32,
    /// Height stored where nothing was rasterized.
    empty: f32,
}

const HF_SIZE: u32 = 128;

impl Heightfield {
    pub(crate) fn build(ctx: &Ctx<'_>) -> Heightfield {
        let sc = ctx.scene;
        let size = HF_SIZE;
        let radius = CONTACT_RADIUS * sc.diag;
        if !ctx.opts.contact_shadow {
            // Nothing else reads the heightfield; don't raster the model for
            // a shadow the caller switched off (this is the draft path).
            return Heightfield {
                size: 0,
                min_z: Vec::new(),
                shadow: Vec::new(),
                origin: [0.0; 2],
                cell: [1.0; 2],
                ground_z: sc.ground_z,
                empty: sc.ground_z,
            };
        }
        let ext = [(sc.hi[0] - sc.lo[0]).max(1e-4), (sc.hi[1] - sc.lo[1]).max(1e-4)];
        // Pad by a third of the footprint, but never by less than the contact
        // radius: the shadow has to reach zero inside the grid, or its edge
        // would show as a straight line on the ground.
        let pad = [(0.35 * ext[0]).max(1.25 * radius), (0.35 * ext[1]).max(1.25 * radius)];
        let origin = [sc.center[0] - ext[0] * 0.5 - pad[0], sc.center[1] - ext[1] * 0.5 - pad[1]];
        let cell = [(ext[0] + 2.0 * pad[0]) / size as f32, (ext[1] + 2.0 * pad[1]) / size as f32];
        let empty = sc.ground_z + 4.0 * sc.diag;
        let n = (size * size) as usize;
        let mut hf = Heightfield {
            size,
            min_z: vec![f32::INFINITY; n],
            shadow: vec![1.0; n],
            origin,
            cell,
            ground_z: sc.ground_z,
            empty,
        };

        let z_cut = sc.ground_z + 2.0 * radius;
        for it in ctx.items {
            // Translucent items cast only a partial shadow; treat anything
            // more transparent than half as no shadow at all (the contract's
            // "weight = opacity" rule, degenerate while every item is
            // opaque). Guide geometry casts none by declaration.
            if it.is_translucent() || !it.casts_shadow() {
                continue;
            }
            let m = it.mesh;
            for tri in m.indices.chunks_exact(3) {
                let p = |i: u32| {
                    let i = i as usize * 3;
                    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
                };
                let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
                // Only the lowest surface of a cell matters, and anything
                // more than two radii above the ground is outside the
                // shadow's falloff window, so the whole upper body of a model
                // can be skipped before it is rasterized.
                if a[2].min(b[2]).min(c[2]) > z_cut {
                    continue;
                }
                // Vertices first: a triangle smaller than a cell still marks
                // the cells it touches.
                for v in [a, b, c] {
                    hf.mark(v[0], v[1], v[2]);
                }
                hf.scan(a, b, c);
            }
        }
        hf.bake_shadow(ctx.opts.contact_shadow, radius);
        hf
    }

    /// Fills `shadow` with the ground darkening of every cell: two rings of
    /// disc taps (a tight one for the contact core, a wide one for the
    /// falloff) asking the heightfield how close the model is overhead.
    fn bake_shadow(&mut self, enabled: bool, radius: f32) {
        if !enabled {
            return;
        }
        let soft = 0.35 * radius;
        for j in 0..self.size {
            let y = self.origin[1] + (j as f32 + 0.5) * self.cell[1];
            for i in 0..self.size {
                let x = self.origin[0] + (i as f32 + 0.5) * self.cell[0];
                let mut num = 0.0f32;
                let mut den = 0.0f32;
                for ring in [0.45f32, 1.0] {
                    for d in DISC {
                        let h = self.height_above_ground(x + d[0] * radius * ring, y + d[1] * radius * ring);
                        // Window the falloff to exactly zero well before the
                        // heightfield's "nothing here" height, so ground far
                        // from the model stays bit-exactly the background and
                        // the fade has no step in it.
                        let window = smoothstep(2.0 * radius, 0.5 * radius, h);
                        if window > 0.0 {
                            let t = h / soft;
                            num += window / (1.0 + t * t);
                        }
                        den += 1.0;
                    }
                }
                self.shadow[(j * self.size + i) as usize] = 1.0 - CONTACT_STRENGTH * (num / den);
            }
        }
    }

    #[inline]
    fn cell_of(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let fx = (x - self.origin[0]) / self.cell[0];
        let fy = (y - self.origin[1]) / self.cell[1];
        if !(fx >= 0.0 && fy >= 0.0) {
            return None;
        }
        let (ix, iy) = (fx as u32, fy as u32);
        if ix >= self.size || iy >= self.size { None } else { Some((ix, iy)) }
    }

    #[inline]
    fn mark(&mut self, x: f32, y: f32, z: f32) {
        if let Some((ix, iy)) = self.cell_of(x, y) {
            self.store((iy * self.size + ix) as usize, z);
        }
    }

    #[inline]
    fn store(&mut self, i: usize, z: f32) {
        if z < self.min_z[i] {
            self.min_z[i] = z;
        }
    }

    /// Rasterizes the triangle top-down, marking every cell whose centre is
    /// inside it with the interpolated z.
    fn scan(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3]) {
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if !(area.abs() > 1e-12) {
            return;
        }
        let inv = 1.0 / area;
        let min_x = a[0].min(b[0]).min(c[0]);
        let max_x = a[0].max(b[0]).max(c[0]);
        let min_y = a[1].min(b[1]).min(c[1]);
        let max_y = a[1].max(b[1]).max(c[1]);
        let i0 = ((min_x - self.origin[0]) / self.cell[0]).floor().max(0.0) as u32;
        let i1 = (((max_x - self.origin[0]) / self.cell[0]).ceil() as i64).clamp(0, self.size as i64 - 1) as u32;
        let j0 = ((min_y - self.origin[1]) / self.cell[1]).floor().max(0.0) as u32;
        let j1 = (((max_y - self.origin[1]) / self.cell[1]).ceil() as i64).clamp(0, self.size as i64 - 1) as u32;
        if i0 > i1 || j0 > j1 || i0 >= self.size || j0 >= self.size {
            return;
        }
        for j in j0..=j1.min(self.size - 1) {
            let py = self.origin[1] + (j as f32 + 0.5) * self.cell[1];
            for i in i0..=i1.min(self.size - 1) {
                let px = self.origin[0] + (i as f32 + 0.5) * self.cell[0];
                let w0 = ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])) * inv;
                let w1 = ((c[0] - b[0]) * (py - b[1]) - (c[1] - b[1]) * (px - b[0])) * inv;
                let w2 = ((a[0] - c[0]) * (py - c[1]) - (a[1] - c[1]) * (px - c[0])) * inv;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                // Barycentrics for (a, b, c) are the opposite sub-areas.
                let z = w1 * a[2] + w2 * b[2] + w0 * c[2];
                self.store((j * self.size + i) as usize, z);
            }
        }
    }

    #[inline]
    fn raw(&self, ix: i64, iy: i64) -> f32 {
        if ix < 0 || iy < 0 || ix >= self.size as i64 || iy >= self.size as i64 {
            return self.empty;
        }
        let z = self.min_z[(iy as u32 * self.size + ix as u32) as usize];
        if z.is_finite() { z } else { self.empty }
    }

    /// Cell-space coordinates of a world xy, offset so that `floor` gives the
    /// lower cell of the bilinear quad.
    #[inline]
    fn uv(&self, x: f32, y: f32) -> (f32, f32, f32, f32) {
        let fx = (x - self.origin[0]) / self.cell[0] - 0.5;
        let fy = (y - self.origin[1]) / self.cell[1] - 0.5;
        (fx.floor(), fy.floor(), fx - fx.floor(), fy - fy.floor())
    }

    /// Height of the lowest surface above the ground plane at `(x, y)`,
    /// bilinearly interpolated; a large finite height where nothing is above.
    pub(crate) fn height_above_ground(&self, x: f32, y: f32) -> f32 {
        if self.min_z.is_empty() {
            return f32::INFINITY;
        }
        let (ix, iy, tx, ty) = self.uv(x, y);
        let (ix, iy) = (ix as i64, iy as i64);
        let z00 = self.raw(ix, iy);
        let z10 = self.raw(ix + 1, iy);
        let z01 = self.raw(ix, iy + 1);
        let z11 = self.raw(ix + 1, iy + 1);
        let z = (z00 * (1.0 - tx) + z10 * tx) * (1.0 - ty) + (z01 * (1.0 - tx) + z11 * tx) * ty;
        (z - self.ground_z).max(0.0)
    }

    /// Contact darkening of the ground plane at world `(x, y)`: exactly 1
    /// outside the baked map and where nothing is overhead, down to
    /// `1 - CONTACT_STRENGTH` under a surface lying on the ground.
    pub(crate) fn ground_at(&self, x: f32, y: f32) -> f32 {
        if self.shadow.is_empty() {
            return 1.0;
        }
        let (ix, iy, tx, ty) = self.uv(x, y);
        let (ix, iy) = (ix as i64, iy as i64);
        let s = |ix: i64, iy: i64| -> f32 {
            if ix < 0 || iy < 0 || ix >= self.size as i64 || iy >= self.size as i64 {
                1.0
            } else {
                self.shadow[(iy as u32 * self.size + ix as u32) as usize]
            }
        };
        let a = s(ix, iy) * (1.0 - tx) + s(ix + 1, iy) * tx;
        let b = s(ix, iy + 1) * (1.0 - tx) + s(ix + 1, iy + 1) * tx;
        a * (1.0 - ty) + b * ty
    }
}

/// Horizon-style SSAO over the depth/normal G-buffer: 16 hemisphere taps per
/// pixel, rotated per pixel from a constant table, depth-compared against the
/// opaque depth buffer with a range check that keeps distant geometry from
/// haloing, then a separable depth-aware blur. Writes `frame.ao` for
/// `COV_OPAQUE` pixels; background pixels keep 1.0. No-op when
/// `!opts.ambient_occlusion`.
///
/// Screen space bounds what this can see: it darkens what is hidden *from the
/// camera* by nearer geometry — creases, pockets, bolt holes, the gap between
/// two parts — but a cavity whose mouth faces the camera (the inside of the
/// planter) is fully visible in the depth buffer and therefore unoccluded, at
/// any radius. Turning that into shade needs world-space visibility: a
/// heightfield sun shadow or a baked AO pass, neither of which is this.
pub(crate) fn ssao(frame: &mut Frame, ctx: &Ctx<'_>) {
    if !ctx.opts.ambient_occlusion {
        return;
    }
    let (w, h) = (frame.w as usize, frame.h as usize);
    if w == 0 || h == 0 {
        return;
    }
    let view = ctx.view;
    let radius = AO_RADIUS * ctx.scene.diag;
    let bias = AO_BIAS * ctx.scene.diag;
    let (cov, depth, pos, normal) = (&frame.coverage, &frame.depth, &frame.pos, &frame.normal);

    let mut raw = vec![1.0f32; w * h];
    par_bands(&mut raw, w, 32, ctx.threads, |y0, band| {
        for (i, a) in band.iter_mut().enumerate() {
            let idx = y0 * w + i;
            if cov[idx] & COV_OPAQUE == 0 {
                continue;
            }
            let x = (idx % w) as u32;
            let y = (idx / w) as u32;
            let p = pos[idx];
            let n = normal[idx];
            let frag_z = depth[idx];
            // Tangent frame around the normal, spun by the per-pixel angle.
            let r = ROT[((y & 3) * 4 + (x & 3)) as usize];
            let up = if n[2].abs() > 0.9 { [1.0f32, 0.0, 0.0] } else { [0.0f32, 0.0, 1.0] };
            let t0 = normalize(cross(up, n));
            let b0 = cross(n, t0);
            let t = add(scale(t0, r[0]), scale(b0, r[1]));
            let b = add(scale(t0, -r[1]), scale(b0, r[0]));
            let mut occ = 0.0f32;
            for k in KERNEL {
                let off = add(add(scale(t, k[0]), scale(b, k[1])), scale(n, k[2]));
                let sp = add(p, scale(off, radius));
                let Some(s) = view.project(sp) else { continue };
                if !(s[0] >= 0.0 && s[1] >= 0.0 && s[0] < w as f32 && s[1] < h as f32) {
                    continue;
                }
                let si = (s[1] as usize) * w + (s[0] as usize);
                let sd = depth[si];
                if !sd.is_finite() || sd >= s[2] - bias {
                    continue;
                }
                // Something is in front of the sample point: occluded, unless
                // it is so much closer that it is a different surface.
                let range = smoothstep(0.0, 1.0, (radius / (frag_z - sd).abs()).min(1.0));
                occ += range;
            }
            *a = 1.0 - occ / KERNEL.len() as f32;
        }
    });

    // Separable depth-aware 5-tap blur (1, 4, 6, 4, 1).
    let dmax = AO_BLUR_DEPTH * ctx.scene.diag;
    let mut tmp = vec![1.0f32; w * h];
    par_bands(&mut tmp, w, 32, ctx.threads, |y0, band| {
        for (i, o) in band.iter_mut().enumerate() {
            let idx = y0 * w + i;
            if cov[idx] & COV_OPAQUE == 0 {
                continue;
            }
            let x = (idx % w) as i64;
            let row = idx - x as usize;
            *o = blur_tap(&raw, depth, cov, row, x, w as i64, 1, depth[idx], dmax);
        }
    });
    par_bands(&mut frame.ao, w, 32, ctx.threads, |y0, band| {
        for (i, o) in band.iter_mut().enumerate() {
            let idx = y0 * w + i;
            if cov[idx] & COV_OPAQUE == 0 {
                continue;
            }
            let y = (idx / w) as i64;
            let col = idx % w;
            *o = blur_tap(&tmp, depth, cov, col, y, h as i64, w as i64, depth[idx], dmax);
        }
    });
}

/// One axis of the 5-tap blur. `base` + `k * stride` addresses the taps;
/// `k` runs over `pos - 2 ..= pos + 2` clamped to `0 .. len`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn blur_tap(src: &[f32], depth: &[f32], cov: &[u8], base: usize, pos: i64, len: i64, stride: i64, z0: f32, dmax: f32) -> f32 {
    const WEIGHTS: [f32; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];
    let mut sum = 0.0f32;
    let mut wsum = 0.0f32;
    for (wi, &wt) in WEIGHTS.iter().enumerate() {
        let k = pos + wi as i64 - 2;
        if k < 0 || k >= len {
            continue;
        }
        let idx = (base as i64 + k * stride) as usize;
        if cov[idx] & COV_OPAQUE == 0 || (depth[idx] - z0).abs() > dmax {
            continue;
        }
        sum += src[idx] * wt;
        wsum += wt;
    }
    if wsum > 0.0 { sum / wsum } else { 1.0 }
}

/// Ground contact shadow: the opaque geometry is projected into a top-down
/// heightfield once per render, and every background pixel whose view ray
/// lands on the ground plane samples that heightfield over a 12-tap disc.
/// The result multiplies the background (so the plane has no visible edge and
/// the grid still shows through) and is folded weakly into `frame.ao` for
/// opaque pixels near the ground, so an underside meets its own shadow.
/// No-op when `!opts.contact_shadow`.
pub(crate) fn ground_contact(frame: &mut Frame, ctx: &Ctx<'_>, hf: &Heightfield) {
    if !ctx.opts.contact_shadow || hf.min_z.is_empty() {
        return;
    }
    let (w, h) = (frame.w as usize, frame.h as usize);
    if w == 0 || h == 0 {
        return;
    }
    let sc = ctx.scene;
    let view = ctx.view;
    let far = 4.0 * sc.diag;
    let near_ground = 0.1 * sc.diag;
    let (cov, pos) = (&frame.coverage, &frame.pos);
    par_bands2(&mut frame.ground, &mut frame.ao, w, 32, ctx.threads, |y0, gband, aband| {
        for (i, g) in gband.iter_mut().enumerate() {
            let idx = y0 * w + i;
            let x = (idx % w) as u32;
            let y = (idx / w) as u32;
            if cov[idx] == 0 {
                let (o, d) = view.ray(x as f32 + 0.5, y as f32 + 0.5);
                if d[2] >= -1e-6 {
                    continue;
                }
                let t = (sc.ground_z - o[2]) / d[2];
                if !(t > 0.0) {
                    continue;
                }
                let hit = add(o, scale(d, t));
                let dx = hit[0] - sc.center[0];
                let dy = hit[1] - sc.center[1];
                if dx * dx + dy * dy > far * far {
                    continue;
                }
                *g = hf.ground_at(hit[0], hit[1]);
            } else if cov[idx] & COV_OPAQUE != 0 {
                // Geometry close to the ground picks up a little of its own
                // contact shadow, so an underside meets the shadow it casts
                // instead of ending at the silhouette. The weight fades out
                // smoothly with height: a hard cut-off would draw a contour
                // line across the model.
                let p = pos[idx];
                let w = 0.35 * smoothstep(near_ground, 0.0, p[2] - sc.ground_z);
                if w > 0.0 {
                    let f = hf.ground_at(p[0], p[1]);
                    aband[i] *= 1.0 + (f - 1.0) * w;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{COV_OPAQUE, Camera, Item, RenderOptions, render};
    use super::*;
    use crate::context::Material;
    use crate::geom::Vec3;
    use crate::tessellate::Mesh;

    /// Two unit boxes meeting at right angles, forming an inside corner along
    /// x = 1 / y = 1, plus a wide open floor plate.
    fn inside_corner() -> Mesh {
        let (b, _) = unit_box();
        let mut mesh = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
        let mut push = |sx: f32, sy: f32, sz: f32, tx: f32, ty: f32, tz: f32, face_base: u32| {
            let base = (mesh.positions.len() / 3) as u32;
            for p in b.positions.chunks_exact(3) {
                mesh.positions.extend_from_slice(&[p[0] * sx + tx, p[1] * sy + ty, p[2] * sz + tz]);
            }
            mesh.normals.extend_from_slice(&b.normals);
            mesh.indices.extend(b.indices.iter().map(|i| i + base));
            mesh.face_ids.extend(b.face_ids.iter().map(|f| f + face_base));
        };
        // A flat plate and a wall standing on it: the join is a long crease.
        push(40.0, 40.0, 2.0, -20.0, -20.0, 0.0, 0);
        push(2.0, 40.0, 20.0, -1.0, -20.0, 2.0, 10);
        mesh
    }

    fn frame_of(mesh: &Mesh, preset: &str, opts: &RenderOptions) -> super::Frame {
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset(preset, lo, hi, 0.2);
        let mat = Material::default();
        render_frame(&[Item::new(mesh, &mat, &[])], &cam, opts)
    }

    fn opts(w: u32, h: u32) -> RenderOptions {
        RenderOptions { width: w, height: h, draw_grid: false, draw_edges: false, samples: 1, ..Default::default() }
    }

    /// An open plane occludes nothing.
    #[test]
    fn flat_plane_is_unoccluded() {
        let (mesh, _) = unit_box();
        let f = frame_of(&mesh, "top", &opts(160, 160));
        let (mut sum, mut n) = (0.0f64, 0u32);
        for i in 0..f.ao.len() {
            if f.coverage[i] & COV_OPAQUE != 0 {
                sum += f.ao[i] as f64;
                n += 1;
            }
        }
        assert!(n > 1000, "expected coverage, got {n}");
        let mean = sum / n as f64;
        assert!(mean > 0.97, "flat plane mean ao {mean}");
    }

    /// A flat surface facing the camera must not show the 4x4 rotation
    /// pattern after the blur.
    #[test]
    fn flat_surface_has_no_visible_ao_pattern() {
        let (mesh, _) = unit_box();
        let f = frame_of(&mesh, "top", &opts(160, 160));
        let mut vals = Vec::new();
        for y in 60..100 {
            for x in 60..100 {
                let i = (y * 160 + x) as usize;
                if f.coverage[i] & COV_OPAQUE != 0 {
                    vals.push(f.ao[i]);
                }
            }
        }
        assert!(vals.len() > 500);
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        let var = vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / vals.len() as f32;
        assert!(var.sqrt() <= 0.01, "ao std {} over a flat face", var.sqrt());
    }

    /// The inside corner of two boxes is darker than the open faces.
    #[test]
    fn inside_corner_is_darker_than_an_open_face() {
        let mesh = inside_corner();
        let f = frame_of(&mesh, "iso", &opts(240, 200));
        // Classify covered pixels by their distance to the crease line
        // (x = +-1 on the plate, z = 2 on the wall).
        let (mut near, mut near_n) = (0.0f64, 0u32);
        let (mut far, mut far_n) = (0.0f64, 0u32);
        for i in 0..f.ao.len() {
            if f.coverage[i] & COV_OPAQUE == 0 {
                continue;
            }
            let p = f.pos[i];
            let on_plate = p[2] < 2.5 && p[0].abs() > 1.0;
            let d = if on_plate { p[0].abs() - 1.0 } else { p[2] - 2.0 };
            if d < 1.0 {
                near += f.ao[i] as f64;
                near_n += 1;
            } else if d > 8.0 {
                far += f.ao[i] as f64;
                far_n += 1;
            }
        }
        assert!(near_n > 100 && far_n > 100, "near {near_n} far {far_n}");
        let (near, far) = (near / near_n as f64, far / far_n as f64);
        assert!(near < 0.8, "crease ao {near} should be well below 1");
        assert!(far > 0.9, "open face ao {far} should be near 1");
        assert!(far - near > 0.15, "crease {near} vs open {far}");
    }

    /// Background pixels keep ao = 1 and the model's own rim does not pick up
    /// a dark halo from the background depth.
    #[test]
    fn silhouette_has_no_ao_halo() {
        let (mesh, _) = unit_box();
        let f = frame_of(&mesh, "iso", &opts(200, 200));
        let (w, h) = (f.w as usize, f.h as usize);
        let mut rim = Vec::new();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = y * w + x;
                if f.coverage[i] & COV_OPAQUE == 0 {
                    assert_eq!(f.ao[i], 1.0, "background ao at {x},{y}");
                    continue;
                }
                let edge = [i - 1, i + 1, i - w, i + w].iter().any(|&j| f.coverage[j] & COV_OPAQUE == 0);
                if edge {
                    rim.push(f.ao[i]);
                }
            }
        }
        assert!(rim.len() > 50, "rim pixels {}", rim.len());
        let mean = rim.iter().sum::<f32>() / rim.len() as f32;
        assert!(mean > 0.85, "silhouette ao halo: mean rim ao {mean}");
    }

    /// The ground under a box is darkened, and the ground far away is not.
    #[test]
    fn contact_shadow_darkens_the_ground_under_the_model() {
        let (mesh, _) = unit_box();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 1.2);
        let mat = Material::default();
        let o = opts(240, 200);
        let f = render_frame(&[Item::new(&mesh, &mat, &[])], &cam, &o);
        let scene = super::super::Scene::from_items(&[Item::new(&mesh, &mat, &[])]);
        let view = super::super::View::new(&cam, o.width, o.height);
        // Mean ground darkening in rings of half a contact radius around the
        // box footprint: the shadow must be strong at the contact, fade
        // monotonically outward, and be exactly 1 (bit-exact background) well
        // beyond the radius.
        let radius = CONTACT_RADIUS * scene.diag;
        let mut rings = [(0.0f64, 0u32); 5];
        let mut far_n = 0u32;
        for y in 0..o.height {
            for x in 0..o.width {
                let i = (y * o.width + x) as usize;
                if f.coverage[i] != 0 {
                    continue;
                }
                let (org, d) = view.ray(x as f32 + 0.5, y as f32 + 0.5);
                if d[2] >= -1e-6 {
                    continue;
                }
                let t = (scene.ground_z - org[2]) / d[2];
                if !(t > 0.0) {
                    continue;
                }
                let hit = add(org, scale(d, t));
                // Distance to the box footprint (the unit square 0..1).
                let dx = (hit[0] - 0.5).abs() - 0.5;
                let dy = (hit[1] - 0.5).abs() - 0.5;
                let dist = dx.max(0.0).hypot(dy.max(0.0));
                let ring = (dist / (0.5 * radius)) as usize;
                if ring < rings.len() {
                    rings[ring].0 += f.ground[i] as f64;
                    rings[ring].1 += 1;
                }
                if dist > 2.0 * scene.diag {
                    assert_eq!(f.ground[i], 1.0, "ground darkened far from the model at {x},{y}");
                    far_n += 1;
                }
            }
        }
        assert!(far_n > 50, "far pixels {far_n}");
        let means: Vec<f64> = rings.iter().map(|&(s, n)| if n == 0 { 1.0 } else { s / n as f64 }).collect();
        assert!(rings.iter().all(|r| r.1 > 20), "thin rings: {rings:?}");
        assert!(means[0] < 0.85, "contact ring should be dark: {means:?}");
        assert!(means.windows(2).all(|w| w[0] < w[1] + 1e-6), "shadow must fade outward: {means:?}");
        assert!(means[4] > 0.98, "shadow should be gone by 2.5 radii: {means:?}");
    }

    /// A box centred on the camera axis casts a left/right symmetric shadow.
    #[test]
    fn contact_shadow_is_symmetric_from_the_front() {
        let (mesh, _) = unit_box();
        let mut centred = mesh;
        for p in centred.positions.chunks_exact_mut(3) {
            p[0] -= 0.5;
            p[1] -= 0.5;
        }
        let f = frame_of(&centred, "front", &opts(160, 120));
        let (w, h) = (f.w as usize, f.h as usize);
        let (mut sum, mut n) = (0.0f64, 0u32);
        for y in 0..h {
            for x in 0..w / 2 {
                let a = f.ground[y * w + x];
                let b = f.ground[y * w + (w - 1 - x)];
                sum += (a - b).abs() as f64;
                n += 1;
            }
        }
        assert!((sum / n as f64) < 0.02, "asymmetry {}", sum / n as f64);
    }

    /// Both stages are pure toggles.
    #[test]
    fn toggles_leave_the_buffers_untouched() {
        let (mesh, _) = unit_box();
        let f = frame_of(&mesh, "iso", &RenderOptions { ambient_occlusion: false, contact_shadow: false, ..opts(120, 100) });
        assert!(f.ao.iter().all(|&a| a == 1.0));
        assert!(f.ground.iter().all(|&g| g == 1.0));
        let f = frame_of(&inside_corner(), "iso", &RenderOptions { ambient_occlusion: true, contact_shadow: false, ..opts(120, 100) });
        assert!(f.ground.iter().all(|&g| g == 1.0));
        assert!(f.ao.iter().any(|&a| a < 0.9), "ao should darken the crease");
        // A convex body occludes nothing of itself.
        let f = frame_of(&mesh, "iso", &RenderOptions { contact_shadow: false, ..opts(120, 100) });
        assert!(f.ao.iter().all(|&a| a > 0.93), "a box should not occlude itself");
    }

    /// Thread count and repeated runs never change the image, with both
    /// stages on.
    #[test]
    fn occlusion_is_thread_invariant() {
        let mesh = inside_corner();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        let mut hashes = Vec::new();
        for threads in [1u32, 4, 7] {
            let o = RenderOptions { width: 200, height: 160, threads, samples: 4, ..Default::default() };
            hashes.push(fnv1a64(&render(&items, &cam, &o).rgba));
        }
        assert!(hashes.windows(2).all(|w| w[0] == w[1]), "thread-dependent output: {hashes:?}");
    }

    /// The heightfield answers the obvious questions about a box sitting on
    /// the ground.
    #[test]
    fn heightfield_measures_the_model() {
        let (mesh, _) = unit_box();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        let scene = super::super::Scene::from_items(&items);
        let cam = Camera::preset("iso", Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 0.2);
        let view = super::super::View::new(&cam, 64, 64);
        let o = opts(64, 64);
        let ctx = Ctx { items: &items, opts: &o, scene: &scene, view: &view, threads: 1, sun_shadow: None };
        let hf = Heightfield::build(&ctx);
        assert!(hf.height_above_ground(0.5, 0.5) < 0.05, "under the box: {}", hf.height_above_ground(0.5, 0.5));
        assert!(hf.height_above_ground(5.0, 5.0) > scene.diag, "away from the box");
    }
}
