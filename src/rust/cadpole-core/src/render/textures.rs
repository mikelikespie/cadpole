//! Procedural textures: pure functions of object-space position,
//! triplanar for the 2-D patterns and footprint-filtered so minification
//! converges to the pattern mean instead of aliasing.
//!
//! Everything here is expressible in WGSL with `floor`, `fract`, `abs`,
//! `min`/`max`, `mix`, `smoothstep`, `dot`, `sqrt` and integer ops — no
//! `sin`/`cos`/`pow`/`exp` and no state — so the same words give the same
//! pixels in the rasterizer, the viewport shader and the path tracer
//! (docs/design.md). The results are bit-stable across native and wasm.
//!
//! A texture is a *multiplier*: `albedo` is in 0..1, so textures darken and
//! modulate, never brighten. `roughness_delta` is added to the material
//! roughness before the shading clamp.

use super::{lerp3, smoothstep};
#[cfg(test)]
use super::{Image, to_srgb8};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    None,
    Checker,
    Stripes,
    Noise,
    Wood,
    Dots,
    /// Fine concentric grooves about an axis: a turned finish. A relief
    /// kind (see [`machined`]), not an albedo one.
    Turned,
    /// Fine parallel scallops along a direction: a flank-milled finish. A
    /// relief kind (see [`machined`]).
    Milled,
    /// `Turned` on the faces a lathe made — surfaces of revolution about
    /// the body's axis and planes square to it — and `Milled` on the rest:
    /// a billet part, which is one body with both finishes on it.
    Machined,
}

impl Kind {
    /// The machined finishes shape the surface (a normal perturbation and
    /// a roughness across the grooves) rather than colouring it; [`sample`]
    /// leaves the albedo alone for them and [`machined`] is their function.
    pub fn is_relief(self) -> bool {
        matches!(self, Kind::Turned | Kind::Milled | Kind::Machined)
    }
}

/// Every texture name a script may use, in documentation order (`""` for
/// none is always accepted and is not listed).
pub fn names() -> &'static [&'static str] {
    &["checker", "stripes", "noise", "wood", "dots", "turned", "milled", "machined"]
}

/// `""` -> `Some(Kind::None)`; unknown names -> `None` (the script reports
/// the error, the renderer uses `Kind::None`).
pub fn kind(name: &str) -> Option<Kind> {
    match name {
        "" | "none" => Some(Kind::None),
        "checker" => Some(Kind::Checker),
        "stripes" => Some(Kind::Stripes),
        "noise" => Some(Kind::Noise),
        "wood" => Some(Kind::Wood),
        "dots" => Some(Kind::Dots),
        "turned" => Some(Kind::Turned),
        "milled" => Some(Kind::Milled),
        "machined" | "billet" => Some(Kind::Machined),
        _ => None,
    }
}

/// The repeat a kind takes when a script names the texture and not the
/// scale: 10 mm for the patterns, and for the machined finishes the pitch
/// of a real one, 0.15 mm — a turning feed, a flank mill's stepover.
pub fn default_scale(kind: Kind) -> f32 {
    if kind.is_relief() { MACHINED_PITCH } else { DEFAULT_SCALE }
}

pub struct Texel {
    /// Multiplier 0..1.
    pub albedo: [f32; 3],
    /// -0.3..0.3
    pub roughness_delta: f32,
}

impl Texel {
    /// The "no texture" texel: leaves the material untouched.
    pub const IDENTITY: Texel = Texel { albedo: [1.0; 3], roughness_delta: 0.0 };
}

/// Repeat length used when `scale_mm` is not a positive number.
const DEFAULT_SCALE: f32 = 10.0;
/// Coordinates are clamped to this before any `floor`, so no cast overflows.
const COORD_LIMIT: f32 = 1.0e6;

// Pattern tones and their means over one period.
const CHECKER_MEAN: f32 = 0.775;
const CHECKER_AMP: f32 = 0.225;
const STRIPES_MEAN: f32 = 0.8;
const STRIPES_AMP: f32 = 0.2;
const DOT_DEPTH: f32 = 0.45;
/// Mean of the dot pattern over a cell: `1 - 0.45 * (dot volume fraction)`.
const DOTS_MEAN: f32 = 0.9585;
/// Tint of the dark growth rings.
const WOOD_DARK: [f32; 3] = [0.75, 0.6, 0.45];
/// Mean of the wood pattern over one ring period (analytic; see tests).
const WOOD_MEAN: [f32; 3] = [0.7019, 0.6581, 0.6142];
const WOOD_ROUGHNESS: f32 = -0.15;
/// How far the noise pushes a ring, in ring radii (one ring is 1/1.5 wide).
const WOOD_WARP: f32 = 0.4;
/// Contrast stretch applied to the noise fBm about its mean.
const NOISE_CONTRAST: f32 = 2.5;

/// Sample a texture at object-space position `pos` (mm) with unit normal `n`;
/// `footprint` is the world size of one pixel there ([`super::View::footprint`]),
/// used to band-limit the pattern. Pure, deterministic and free of NaN.
pub fn sample(kind: Kind, scale_mm: f32, pos: [f32; 3], n: [f32; 3], footprint: f32) -> Texel {
    if kind == Kind::None {
        return Texel::IDENTITY;
    }
    let scale = if scale_mm.is_finite() && scale_mm > 0.0 { scale_mm } else { DEFAULT_SCALE };
    let inv = 1.0 / scale;
    let u = [san(pos[0]) * inv, san(pos[1]) * inv, san(pos[2]) * inv];
    // Pixel footprint measured in pattern periods.
    let f = if footprint.is_finite() { (footprint * inv).clamp(0.0, COORD_LIMIT) } else { 0.0 };
    let texel = match kind {
        Kind::None | Kind::Turned | Kind::Milled | Kind::Machined => Texel::IDENTITY,
        Kind::Checker => grey(triplanar(u, n, f, checker)),
        Kind::Stripes => grey(triplanar(u, n, f, stripes)),
        Kind::Noise => noise_texel(u, f),
        Kind::Wood => wood(u, f),
        Kind::Dots => grey(dots(u, f)),
    };
    Texel {
        albedo: [
            finite_clamp(texel.albedo[0], 0.0, 1.0, 1.0),
            finite_clamp(texel.albedo[1], 0.0, 1.0, 1.0),
            finite_clamp(texel.albedo[2], 0.0, 1.0, 1.0),
        ],
        roughness_delta: finite_clamp(texel.roughness_delta, -0.3, 0.3, 0.0),
    }
}

// ------------------------------------------------------------------ patterns

/// Two-tone squares, 0.55 / 1.0, box-filtered in both axes. The exact box
/// filter of `(-1)^(floor(u) + floor(v))` is separable, so it is the product
/// of the two filtered square waves.
fn checker(u: f32, v: f32, f: f32) -> f32 {
    CHECKER_MEAN + CHECKER_AMP * filtered_square(u, f) * filtered_square(v, f)
}

/// Two-tone bands, 0.6 / 1.0, box-filtered along `u`.
fn stripes(u: f32, _v: f32, f: f32) -> f32 {
    STRIPES_MEAN + STRIPES_AMP * filtered_square(u, f)
}

/// Polka dots on the 3-D unit lattice; fades to the pattern mean under
/// minification (the dot boundary has no closed-form box filter).
fn dots(u: [f32; 3], f: f32) -> f32 {
    let q = [fract(u[0]) - 0.5, fract(u[1]) - 0.5, fract(u[2]) - 0.5];
    let d = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    let a = 1.0 - DOT_DEPTH * smoothstep(0.30, 0.26, d);
    lerp(a, DOTS_MEAN, minify(f))
}

/// 4-octave value-noise mottling: an uneven darkening plus a roughness
/// modulation, so the highlight breaks up the way a matte print does.
///
/// fBm of four octaves has a standard deviation of only about 0.1, so the
/// contrast is stretched about the mean (which leaves the mean, and therefore
/// the minified colour, where it is).
fn noise_texel(u: [f32; 3], f: f32) -> Texel {
    let n = ((fbm_filtered(u, 4, f) - 0.5) * NOISE_CONTRAST + 0.5).clamp(0.0, 1.0);
    Texel { albedo: [0.75 + 0.25 * n; 3], roughness_delta: 0.3 * (n - 0.5) }
}

/// Growth rings around the Z axis, warped by noise that is stretched along
/// the axis so the grain runs with the log. Dark rings are tinted brown and
/// slightly rougher than the light ones.
fn wood(u: [f32; 3], f: f32) -> Texel {
    let warp = fbm_filtered([u[0] * 0.5, u[1] * 0.5, u[2] * 0.5], 3, f * 0.5) - 0.5;
    let r = (u[0] * u[0] + u[1] * u[1]).sqrt() + WOOD_WARP * warp;
    let t = smoothstep(0.35, 0.65, fract(1.5 * r));
    let tone = 0.55 + 0.45 * t;
    let albedo = scale3(lerp3(WOOD_DARK, [1.0; 3], t), tone);
    let m = minify(f);
    Texel {
        albedo: lerp3(albedo, WOOD_MEAN, m),
        roughness_delta: lerp(WOOD_ROUGHNESS * t, WOOD_ROUGHNESS * 0.5, m),
    }
}

// ------------------------------------------------------------- triplanar map

/// Triplanar weights `|n|^4`, normalised. An axis-aligned normal selects
/// exactly one plane.
fn triplanar_weights(n: [f32; 3]) -> [f32; 3] {
    let sq = [n[0] * n[0], n[1] * n[1], n[2] * n[2]];
    let w = [sq[0] * sq[0], sq[1] * sq[1], sq[2] * sq[2]];
    let s = w[0] + w[1] + w[2];
    if s.is_finite() && s > 1e-12 { [w[0] / s, w[1] / s, w[2] / s] } else { [0.0, 0.0, 1.0] }
}

/// Blends a 2-D pattern evaluated on the YZ, XZ and XY planes.
fn triplanar(u: [f32; 3], n: [f32; 3], f: f32, pat: fn(f32, f32, f32) -> f32) -> f32 {
    let w = triplanar_weights([san(n[0]), san(n[1]), san(n[2])]);
    let mut v = 0.0;
    if w[0] > 0.0 {
        v += w[0] * pat(u[1], u[2], f);
    }
    if w[1] > 0.0 {
        v += w[1] * pat(u[0], u[2], f);
    }
    if w[2] > 0.0 {
        v += w[2] * pat(u[0], u[1], f);
    }
    v
}

// ----------------------------------------------------------------- filtering

/// `p(u) = (-1)^floor(u)`, without an integer cast (so huge coordinates stay
/// finite instead of wrapping).
#[inline]
fn square(u: f32) -> f32 {
    if fract(u * 0.5) >= 0.5 { -1.0 } else { 1.0 }
}

/// Antiderivative of [`square`]: a triangle wave in 0..1 with period 2 and
/// `tri(0) = 0`. Since `square` has zero mean, the periodic `tri` integrates
/// it exactly over any interval.
#[inline]
fn tri(u: f32) -> f32 {
    1.0 - (1.0 - 2.0 * fract(u * 0.5)).abs()
}

/// Mean of [`square`] over `[u - f/2, u + f/2]`, in -1..1: exact box
/// filtering of the square wave, so a minified checker converges smoothly to
/// its mean instead of aliasing.
#[inline]
fn filtered_square(u: f32, f: f32) -> f32 {
    if f < 1.0e-4 {
        return square(u);
    }
    ((tri(u + 0.5 * f) - tri(u - 0.5 * f)) / f).clamp(-1.0, 1.0)
}

/// How far a pattern without a closed-form filter has faded to its mean:
/// 0 below a quarter period per pixel, 1 at one period per pixel.
#[inline]
fn minify(f: f32) -> f32 {
    smoothstep(0.25, 1.0, f)
}

// --------------------------------------------------------------------- noise

/// lowbias32 (Chris Wellons): a well-mixed 32-bit integer hash.
#[inline]
fn lowbias32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// lowbias32(0) == 0, so the hash chain is salted: without this the lattice
/// point at the origin would hash to 0 and leave a dark node on every model.
const HASH_SALT: u32 = 0x9e37_79b9;

/// Hash of a lattice point: `h(ix + h(iy + h(iz ^ seed)))`, wrapping ops only,
/// identical on every platform.
#[cfg(test)]
pub fn hash3(ix: i32, iy: i32, iz: i32, seed: u32) -> u32 {
    let h = lowbias32((iz as u32) ^ seed ^ HASH_SALT);
    let h = lowbias32((iy as u32).wrapping_add(h));
    lowbias32((ix as u32).wrapping_add(h))
}

/// Hash of a lattice point as a float in 0..1.
#[cfg(test)]
#[inline]
fn hash_unit(ix: i32, iy: i32, iz: i32) -> f32 {
    hash3(ix, iy, iz, 0) as f32 * (1.0 / 4_294_967_296.0)
}

/// The eight corner hashes of the cell at `i`, in 0..1, ordered
/// `[dx + 2*dy + 4*dz]`. Same values as eight [`hash_unit`] calls, but the
/// z and y links of the chain are shared, which is 14 mixes instead of 24 —
/// value noise is the hot loop of every fBm texture.
#[inline]
fn cell_corners(i: [i32; 3]) -> [f32; 8] {
    let mut out = [0.0f32; 8];
    for dz in 0..2 {
        let hz = lowbias32((i[2].wrapping_add(dz) as u32) ^ HASH_SALT);
        for dy in 0..2 {
            let hy = lowbias32((i[1].wrapping_add(dy) as u32).wrapping_add(hz));
            for dx in 0..2 {
                let h = lowbias32((i[0].wrapping_add(dx) as u32).wrapping_add(hy));
                out[(dx + 2 * dy + 4 * dz) as usize] = h as f32 * (1.0 / 4_294_967_296.0);
            }
        }
    }
    out
}

/// Quintic fade `6t^5 - 15t^4 + 10t^3` (C2, so no lattice creases).
#[inline]
fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Trilinear value noise in 0..1 with a quintic fade. `p` must already be
/// finite (`fbm_filtered` sanitises once for the whole octave stack).
fn value_noise(p: [f32; 3]) -> f32 {
    let fl = [p[0].floor(), p[1].floor(), p[2].floor()];
    let c = cell_corners([fl[0] as i32, fl[1] as i32, fl[2] as i32]);
    let t = [quintic(p[0] - fl[0]), quintic(p[1] - fl[1]), quintic(p[2] - fl[2])];
    let x0 = lerp(c[0], c[1], t[0]);
    let x1 = lerp(c[2], c[3], t[0]);
    let x2 = lerp(c[4], c[5], t[0]);
    let x3 = lerp(c[6], c[7], t[0]);
    lerp(lerp(x0, x1, t[1]), lerp(x2, x3, t[1]), t[2])
}

/// Value-noise fBm in 0..1: `octaves` octaves, each half the amplitude and
/// twice the frequency of the last.
#[cfg(test)]
pub fn fbm(p: [f32; 3], octaves: u32) -> f32 {
    fbm_filtered(p, octaves, 0.0)
}

/// fBm that drops octaves whose wavelength falls below twice the pixel
/// footprint `f` (measured in periods), fading each one out so nothing pops.
/// With every octave gone the result is the mean, 0.5.
fn fbm_filtered(p: [f32; 3], octaves: u32, f: f32) -> f32 {
    let p = [san(p[0]), san(p[1]), san(p[2])];
    let (mut sum, mut norm, mut amp, mut freq) = (0.0f32, 0.0f32, 1.0f32, 1.0f32);
    for _ in 0..octaves.min(8) {
        // This octave's wavelength is 1 / freq; band-limit it at 2 * f.
        let w = if f <= 0.0 { 1.0 } else { 1.0 - smoothstep(0.5, 1.0, 2.0 * f * freq) };
        if w > 0.0 {
            sum += amp * w * value_noise([p[0] * freq, p[1] * freq, p[2] * freq]);
            norm += amp * w;
        }
        amp *= 0.5;
        freq *= 2.0;
    }
    if norm > 0.0 { sum / norm } else { 0.5 }
}

// ------------------------------------------------------------------ machined

/// Groove pitch of the machined finishes when a script gives none, in mm.
pub const MACHINED_PITCH: f32 = 0.15;
/// Groove depth as a fraction of the pitch when a script gives none: a
/// 0.4 mm-nose tool at a 0.15 mm feed leaves cusps of `f² / 8R` ≈ 0.007 mm,
/// a twentieth of the pitch, and a flank mill's scallops are shallower.
const TURNED_DEPTH: f32 = 0.05;
const MILLED_DEPTH: f32 = 0.03;

/// What a machined finish does to a shading point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relief {
    /// Added to the unit normal before renormalising: the slope of the
    /// groove profile, along the in-surface direction across the grooves.
    /// Zero once the grooves are smaller than a pixel.
    pub bump: [f32; 3],
    /// Unit direction the grooves run in, in the surface: the anisotropic
    /// lobe's smooth axis. Zero when the finish does not apply here.
    pub groove: [f32; 3],
    /// Variance of the slope the pixel footprint averaged away, to be
    /// folded into the roughness across the grooves (Toksvig's argument:
    /// ridges below a pixel *are* anisotropic roughness).
    pub slope_var: f32,
}

impl Relief {
    pub const NONE: Relief = Relief { bump: [0.0; 3], groove: [0.0; 3], slope_var: 0.0 };
}

/// A machined finish at object-space `pos` with unit normal `n`, laid out
/// in a frame the caller knows from the topology (`render::FaceFrame`):
/// `axis_point`/`axis` is what the part was turned about, `origin` a point
/// on the face, `dir` the direction the cutter ran (a ruled face's
/// ruling), and `turned` whether this face is one a lathe made, which is
/// what decides [`Kind::Machined`]. `pitch_mm` is the groove spacing
/// (`<= 0` = [`MACHINED_PITCH`]), `depth_mm` the groove depth (`<= 0` =
/// the kind's default fraction).
///
/// **Turned**: grooves are circles about the axis; the profile phase moves
/// along the meridian, which on an end face is the radius, on a cylinder
/// the height, and on a cone the generator — one coordinate,
/// `|h|·|m·a| + ρ·|m·r|` with `m` the in-surface direction across the
/// grooves, covers all three and blends smoothly over a bell. The profile
/// is a sinusoid of the pitch (a round-nosed tool), box-filtered over the
/// footprint exactly (the filtered cosine is a sinc), so a groove smaller
/// than the footprint fades to flat instead of aliasing, and the slope
/// variance it fades to is handed back for the roughness. The footprint is
/// whatever width the caller wants resolved: the path tracer passes
/// [`super::trace::RESOLVE_PIXELS`] pixels, projected onto the surface.
///
/// **Milled**: scallops run along `dir`, and the phase moves across them
/// from `origin`. The profile is a run of parabolic arcs meeting at cusps
/// (what a round cutter leaves at a stepover), whose slope is a sawtooth,
/// box-filtered exactly through its periodic antiderivative.
///
/// Pure and deterministic. These two kinds use `sin`/`cos` and are not in
/// the WGSL set the header describes; the viewport does not draw them.
pub fn machined(
    kind: Kind,
    pitch_mm: f32,
    depth_mm: f32,
    pos: [f32; 3],
    n: [f32; 3],
    footprint: f32,
    axis_point: [f32; 3],
    axis: [f32; 3],
    origin: [f32; 3],
    dir: [f32; 3],
    turned: bool,
) -> Relief {
    let pitch = if pitch_mm.is_finite() && pitch_mm > 0.0 { pitch_mm } else { MACHINED_PITCH };
    let Some(n) = unit3(n) else { return Relief::NONE };
    let kind = match kind {
        Kind::Machined if turned => Kind::Turned,
        Kind::Machined => Kind::Milled,
        k => k,
    };
    let pos = [san(pos[0]), san(pos[1]), san(pos[2])];
    let f = if footprint.is_finite() { (footprint / pitch).clamp(0.0, COORD_LIMIT) } else { 0.0 };
    match kind {
        Kind::Turned => {
            let a = unit3(axis).unwrap_or([0.0, 0.0, 1.0]);
            let rel = sub3(pos, [san(axis_point[0]), san(axis_point[1]), san(axis_point[2])]);
            let h = dot3(rel, a);
            let radial = sub3(rel, scale3(a, h));
            let rho = dot3(radial, radial).sqrt();
            let rad = if rho > 1e-6 { scale3(radial, 1.0 / rho) } else { any_perpendicular(a) };
            // The groove runs round the axis; the phase moves across it,
            // within the surface. A face whose normal is circumferential
            // (a keyway wall) has no groove of this kind.
            let Some(circ) = unit3(cross3(a, rad)) else { return Relief::NONE };
            let Some(m) = unit3(cross3(circ, n)) else { return Relief::NONE };
            let s = h.abs() * dot3(m, a).abs() + rho * dot3(m, rad).abs();
            let depth = if depth_mm.is_finite() && depth_mm > 0.0 { depth_mm } else { TURNED_DEPTH * pitch };
            let amp = std::f32::consts::PI * depth / pitch;
            let u = s / pitch;
            let phase = std::f32::consts::TAU * fract(u);
            // Box filter of `cos` over `f` periods is `cos · sinc(πf)`; the
            // sinc's negative lobes are the box's ringing and are clamped.
            let w = if f < 1e-4 { 1.0 } else { ((std::f32::consts::PI * f).sin() / (std::f32::consts::PI * f)).max(0.0) };
            let slope = amp * phase.cos() * w;
            Relief { bump: scale3(m, -slope), groove: circ, slope_var: 0.5 * amp * amp * (1.0 - w * w) }
        }
        Kind::Milled => {
            let d = unit3(dir).unwrap_or_else(|| any_perpendicular(n));
            // The cutter's direction, in the surface.
            let Some(groove) = unit3(sub3(d, scale3(n, dot3(d, n)))) else { return Relief::NONE };
            let Some(m) = unit3(cross3(groove, n)) else { return Relief::NONE };
            let s = dot3(sub3(pos, [san(origin[0]), san(origin[1]), san(origin[2])]), m);
            let depth = if depth_mm.is_finite() && depth_mm > 0.0 { depth_mm } else { MILLED_DEPTH * pitch };
            let amp = 4.0 * depth / pitch;
            let u = s / pitch;
            // The box average alone is not enough: over a width that is
            // not a whole number of periods a sawtooth's residual falls
            // only as `1/f` (13% of the slope at 1.2 periods), and on a
            // polished metal a degree of tilt is a visible pattern -- the
            // blade flanks of the compressor wheel kept a hatching after
            // the tracer's resolve width should have flattened them. So
            // the scallops also fade out between half a footprint and a
            // whole one, the same cut the turned grooves' sinc makes, and
            // what is faded goes to the roughness.
            let w = 1.0 - smoothstep(0.5, 1.0, f);
            let slope = amp * filtered_sawtooth(u, f) * w;
            Relief { bump: scale3(m, -slope), groove, slope_var: amp * amp / 3.0 * (1.0 - w * w) }
        }
        _ => Relief::NONE,
    }
}

/// `2·fract(u) − 1`, the slope of a run of parabolic scallops, box-filtered
/// over `f` periods through its periodic antiderivative `fract(u)² −
/// fract(u)`: exact, so a sub-pixel scallop averages to flat.
#[inline]
fn filtered_sawtooth(u: f32, f: f32) -> f32 {
    if f < 1e-4 {
        return 2.0 * fract(u) - 1.0;
    }
    let anti = |x: f32| {
        let t = fract(x);
        t * t - t
    };
    ((anti(u + 0.5 * f) - anti(u - 0.5 * f)) / f).clamp(-1.0, 1.0)
}

#[inline]
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Unit vector, or `None` for a zero, tiny or non-finite one.
#[inline]
fn unit3(v: [f32; 3]) -> Option<[f32; 3]> {
    let v = [san(v[0]), san(v[1]), san(v[2])];
    let l2 = dot3(v, v);
    if l2 > 1e-12 { Some(scale3(v, 1.0 / l2.sqrt())) } else { None }
}

/// Some unit vector perpendicular to unit `a`.
fn any_perpendicular(a: [f32; 3]) -> [f32; 3] {
    let t = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    unit3(cross3(a, t)).unwrap_or([1.0, 0.0, 0.0])
}

// --------------------------------------------------------------------- atlas

/// 5-tile strip (checker, stripes, noise, wood, dots) sampled top-down on a
/// plane at a fixed scale: the CPU side of the cross-renderer
/// golden, and a quick way to eyeball every pattern at once.
#[cfg(test)]
pub fn atlas(size: u32) -> Image {
    const TILE_KINDS: [Kind; 5] = [Kind::Checker, Kind::Stripes, Kind::Noise, Kind::Wood, Kind::Dots];
    const SCALE: f32 = 1.0;
    const SPAN: f32 = 4.0;
    let size = size.max(1);
    let mut img = Image::new(size * 5, size, [0.0; 3]);
    let step = SPAN / size as f32;
    for (t, k) in TILE_KINDS.iter().enumerate() {
        for y in 0..size {
            for x in 0..size {
                // The plane cuts the dot lattice through its centres.
                let p = [(x as f32 + 0.5) * step, (y as f32 + 0.5) * step, 0.5 * SCALE];
                let c = sample(*k, SCALE, p, [0.0, 0.0, 1.0], step).albedo;
                img.put(t as u32 * size + x, y, [to_srgb8(c[0]), to_srgb8(c[1]), to_srgb8(c[2]), 255]);
            }
        }
    }
    img
}

// ------------------------------------------------------------------- helpers

#[inline]
fn fract(x: f32) -> f32 {
    x - x.floor()
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn scale3(c: [f32; 3], s: f32) -> [f32; 3] {
    [c[0] * s, c[1] * s, c[2] * s]
}

#[inline]
fn grey(v: f32) -> Texel {
    Texel { albedo: [v; 3], roughness_delta: 0.0 }
}

/// Finite and bounded, so `floor` never overflows a cast and NaN never
/// escapes into the pixel path.
#[inline]
fn san(x: f32) -> f32 {
    if x.is_finite() { x.clamp(-COORD_LIMIT, COORD_LIMIT) } else { 0.0 }
}

#[inline]
fn finite_clamp(x: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if x.is_finite() { x.clamp(lo, hi) } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::super::testing::fnv1a64;
    use super::*;

    const ALL: [Kind; 5] = [Kind::Checker, Kind::Stripes, Kind::Noise, Kind::Wood, Kind::Dots];

    /// Deterministic pseudo-random points in a +-40 mm box.
    fn point(i: u32) -> [f32; 3] {
        let f = |k: u32| hash3(i as i32, k as i32, 17, 0) as f32 * (1.0 / 4_294_967_296.0) * 80.0 - 40.0;
        [f(0), f(1), f(2)]
    }

    fn unit_normal(i: u32) -> [f32; 3] {
        let f = |k: u32| hash3(i as i32, k as i32, 99, 0) as f32 * (1.0 / 4_294_967_296.0) * 2.0 - 1.0;
        let n = [f(0), f(1), f(2)];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l > 1e-6 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 0.0, 1.0] }
    }

    /// The mean albedo of a kind, which is what its filtered form converges to.
    fn mean_of(kind: Kind) -> [f32; 3] {
        match kind {
            Kind::Checker => [CHECKER_MEAN; 3],
            Kind::Stripes => [STRIPES_MEAN; 3],
            Kind::Noise => [0.875; 3],
            Kind::Wood => WOOD_MEAN,
            Kind::Dots => [DOTS_MEAN; 3],
            // The relief kinds leave the albedo alone.
            Kind::None | Kind::Turned | Kind::Milled | Kind::Machined => [1.0; 3],
        }
    }

    #[test]
    fn sampling_is_deterministic_and_in_range() {
        for k in ALL {
            for i in 0..100_000u32 {
                let (p, n) = (point(i), unit_normal(i));
                let a = sample(k, 7.5, p, n, 0.05);
                let b = sample(k, 7.5, p, n, 0.05);
                for c in 0..3 {
                    assert!(a.albedo[c].is_finite() && (0.0..=1.0).contains(&a.albedo[c]), "{k:?} albedo {:?}", a.albedo);
                    assert_eq!(a.albedo[c].to_bits(), b.albedo[c].to_bits(), "{k:?} is not deterministic");
                }
                assert!(a.roughness_delta.is_finite() && a.roughness_delta.abs() <= 0.3, "{k:?} rd {}", a.roughness_delta);
            }
        }
    }

    #[test]
    fn degenerate_inputs_stay_finite() {
        let bad = [f32::NAN, f32::INFINITY, -f32::INFINITY, 1.0e30, -1.0e30];
        for k in ALL {
            for v in bad {
                for probe in [
                    sample(k, v, [v, 1.0, 2.0], [0.0, 0.0, 1.0], 0.1),
                    sample(k, 5.0, [1.0, v, 2.0], [v, 0.0, 1.0], v),
                    sample(k, 0.0, [1.0, 2.0, v], [0.0, 0.0, 0.0], -1.0),
                ] {
                    assert!(probe.albedo.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)), "{k:?}");
                    assert!(probe.roughness_delta.is_finite(), "{k:?}");
                }
            }
        }
        // A scale that is not positive falls back to the 10 mm default.
        let n = [0.0, 0.0, 1.0];
        for bad_scale in [0.0, -3.0, f32::NAN] {
            assert_eq!(
                sample(Kind::Checker, bad_scale, [3.0, 4.0, 0.0], n, 0.0).albedo[0].to_bits(),
                sample(Kind::Checker, DEFAULT_SCALE, [3.0, 4.0, 0.0], n, 0.0).albedo[0].to_bits()
            );
        }
    }

    #[test]
    fn checker_and_stripes_average_to_their_means() {
        let n = [0.0, 0.0, 1.0];
        let (mut checker_sum, mut stripe_sum) = (0.0f64, 0.0f64);
        let side = 512;
        for y in 0..side {
            for x in 0..side {
                // Eight periods across the lattice.
                let p = [(x as f32 + 0.5) / side as f32 * 8.0, (y as f32 + 0.5) / side as f32 * 8.0, 0.0];
                checker_sum += sample(Kind::Checker, 1.0, p, n, 0.0).albedo[0] as f64;
                stripe_sum += sample(Kind::Stripes, 1.0, p, n, 0.0).albedo[0] as f64;
            }
        }
        let cells = (side * side) as f64;
        assert!((checker_sum / cells - 0.775).abs() < 0.01, "checker mean {}", checker_sum / cells);
        assert!((stripe_sum / cells - 0.8).abs() < 0.01, "stripes mean {}", stripe_sum / cells);
    }

    #[test]
    fn checker_has_exactly_two_tones_when_magnified() {
        let n = [0.0, 0.0, 1.0];
        let mut tones: Vec<u32> = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                let p = [x as f32 * 5.0 + 2.5, y as f32 * 5.0 + 2.5, 0.0];
                // Far below the filter threshold: the box filter is off and
                // the pattern is its two tones exactly.
                let a = sample(Kind::Checker, 5.0, p, n, 0.0).albedo[0];
                if !tones.contains(&a.to_bits()) {
                    tones.push(a.to_bits());
                }
                // Just above it the filter is on but still imperceptible.
                let b = sample(Kind::Checker, 5.0, p, n, 5.0 * 0.01).albedo[0];
                assert!((b - 0.55).abs() < 1e-3 || (b - 1.0).abs() < 1e-3, "{b} is neither tone");
            }
        }
        let mut tones: Vec<f32> = tones.iter().map(|b| f32::from_bits(*b)).collect();
        tones.sort_by(f32::total_cmp);
        assert_eq!(tones.len(), 2, "expected two tones, got {tones:?}");
        assert!((tones[0] - 0.55).abs() < 1e-6, "{tones:?}");
        assert!((tones[1] - 1.0).abs() < 1e-6, "{tones:?}");
    }

    /// A groove no wider than the footprint is filtered flat and its slope
    /// variance handed to the roughness; one sixteen footprints wide is
    /// drawn nearly in full. The footprint is the caller's resolving width
    /// (the tracer's is `RESOLVE_PIXELS` pixels, projected onto the
    /// surface), so this is the contract that keeps a fine pitch off the
    /// pixel grid.
    #[test]
    fn machined_grooves_finer_than_the_footprint_are_flat() {
        let pitch = 0.15f32;
        let n = [0.0, 0.0, 1.0];
        let frame = ([0.0f32; 3], [0.0, 0.0, 1.0], [0.0f32; 3], [1.0, 0.0, 0.0]);
        let relief = |footprint: f32, turned: bool| {
            let mut worst_slope = 0.0f32;
            let mut var = 0.0f32;
            // Along both the radius (the turned phase) and `y` (the
            // milled phase, across scallops that run along `x`).
            for i in 0..64 {
                let p = [3.0 + pitch * i as f32 / 64.0, 0.7 + pitch * i as f32 / 64.0, 0.0];
                let r = machined(Kind::Machined, pitch, 0.0, p, n, footprint, frame.0, frame.1, frame.2, frame.3, turned);
                worst_slope = worst_slope.max(dot3(r.bump, r.bump).sqrt());
                var = var.max(r.slope_var);
            }
            (worst_slope, var)
        };
        for turned in [true, false] {
            let (full, var_full) = relief(0.0, turned);
            let (coarse, _) = relief(pitch / 16.0, turned);
            let (fine, var_fine) = relief(pitch, turned);
            assert!(full > 0.05, "turned={turned}: no relief at all ({full})");
            assert!(coarse > 0.8 * full, "turned={turned}: sixteen-pixel grooves lost too much ({coarse} of {full})");
            assert!(fine < 0.02 * full, "turned={turned}: footprint-wide grooves still drawn ({fine} of {full})");
            assert!(var_full < 1e-6 && var_fine > 0.0, "turned={turned}: variance not handed over ({var_full} -> {var_fine})");
        }
    }

    #[test]
    fn minification_converges_to_the_mean() {
        let n = [0.0, 0.0, 1.0];
        let scale = 4.0f32;
        for k in ALL {
            let mean = mean_of(k);
            let mut prev = f32::INFINITY;
            for (i, f) in [0.02f32, 0.5, 2.0, 8.0, 32.0].iter().enumerate() {
                let mut worst = 0.0f32;
                for y in 0..48 {
                    for x in 0..48 {
                        let p = [x as f32 * 0.37 + 0.11, y as f32 * 0.41 + 0.07, 0.5 * scale];
                        let a = sample(k, scale, p, n, f * scale);
                        for c in 0..3 {
                            worst = worst.max((a.albedo[c] - mean[c]).abs());
                        }
                    }
                }
                assert!(worst <= prev + 1e-4, "{k:?}: deviation grew at footprint {f} ({worst} > {prev})");
                if i == 4 {
                    assert!(worst < 0.02, "{k:?}: still {worst} from the mean at footprint {f}");
                }
                prev = worst;
            }
        }
    }

    #[test]
    fn checker_period_follows_the_scale() {
        let n = [0.0, 0.0, 1.0];
        let width_mm = 120.0f32;
        let px = 1200;
        let transitions = |scale: f32| {
            let mut last = sample(Kind::Checker, scale, [0.0, 0.5 * scale, 0.0], n, 0.0).albedo[0];
            let mut count = 0;
            for i in 1..px {
                let x = i as f32 / px as f32 * width_mm;
                let a = sample(Kind::Checker, scale, [x, 0.5 * scale, 0.0], n, 0.0).albedo[0];
                if (a - last).abs() > 0.1 {
                    count += 1;
                }
                last = a;
            }
            count
        };
        let a = transitions(10.0);
        let b = transitions(20.0);
        assert!((a as f32 - width_mm / 10.0).abs() <= 1.0, "{a} transitions at scale 10");
        assert!((b as f32 - width_mm / 20.0).abs() <= 1.0, "{b} transitions at scale 20");
    }

    #[test]
    fn triplanar_weights_select_a_plane_and_sum_to_one() {
        assert_eq!(triplanar_weights([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(triplanar_weights([0.0, -1.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(triplanar_weights([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
        for i in 0..2000u32 {
            let w = triplanar_weights(unit_normal(i));
            assert!((w[0] + w[1] + w[2] - 1.0).abs() < 1e-6, "{w:?}");
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)), "{w:?}");
        }
        // A degenerate normal falls back to the XY plane.
        assert_eq!(triplanar_weights([0.0; 3]), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn triplanar_blend_has_no_seam() {
        // Rotating the normal from +X to +Z must not make the albedo jump.
        let p = [3.3f32, -7.1, 2.6];
        let steps = 200;
        for k in [Kind::Checker, Kind::Stripes] {
            let mut prev: Option<f32> = None;
            for i in 0..=steps {
                let t = i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
                let n = [t.cos(), 0.0, t.sin()];
                let a = sample(k, 6.0, p, n, 0.02).albedo[0];
                if let Some(prev) = prev {
                    assert!((a - prev).abs() < 0.05, "{k:?} jumped by {} at step {i}", (a - prev).abs());
                }
                prev = Some(a);
            }
        }
    }

    #[test]
    fn hash3_is_pinned_and_well_mixed() {
        assert_eq!(hash3(0, 0, 0, 0), 2_488_251_732);
        assert_eq!(hash3(1, 2, 3, 0), 1_760_129_097);
        assert_eq!(hash3(-1, -2, -3, 7), 3_054_861_799);
        assert_eq!(hash3(12_345, -9_999, 42, 0), 671_314_687);
        let mut ones = [0u32; 32];
        let mut n = 0u32;
        for x in 0..47 {
            for y in 0..47 {
                for z in 0..47 {
                    let v = hash3(x, y, z, 0);
                    n += 1;
                    for (b, ones) in ones.iter_mut().enumerate() {
                        *ones += (v >> b) & 1;
                    }
                }
            }
        }
        for (b, ones) in ones.iter().enumerate() {
            let f = *ones as f64 / n as f64;
            assert!((0.49..=0.51).contains(&f), "bit {b} is 1 in {:.3}% of hashes", f * 100.0);
        }
    }

    #[test]
    fn cell_corners_match_hash3() {
        for i in 0..500u32 {
            let c = [
                hash3(i as i32, 3, -7, 0) as i32 % 4096,
                hash3(i as i32, 5, 11, 0) as i32 % 4096,
                hash3(i as i32, -2, 8, 0) as i32 % 4096,
            ];
            let got = cell_corners(c);
            for dz in 0..2 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let want = hash_unit(c[0] + dx, c[1] + dy, c[2] + dz);
                        assert_eq!(got[(dx + 2 * dy + 4 * dz) as usize], want, "corner {dx}{dy}{dz} of {c:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn fbm_is_a_well_behaved_field() {
        let mut vals = Vec::with_capacity(20_000);
        for i in 0..20_000u32 {
            let p = point(i);
            let v = fbm([p[0] * 0.1, p[1] * 0.1, p[2] * 0.1], 4);
            assert!((0.0..=1.0).contains(&v), "fbm out of range: {v}");
            vals.push(v as f64);
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let var = vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / vals.len() as f64;
        assert!((0.4..=0.6).contains(&mean), "fbm mean {mean}");
        assert!((0.08..=0.3).contains(&var.sqrt()), "fbm std {}", var.sqrt());
        // With every octave band-limited away only the mean is left.
        assert_eq!(fbm_filtered([1.0, 2.0, 3.0], 4, 1000.0), 0.5);
    }

    #[test]
    fn sample_is_golden() {
        // Integer hashes and polynomials only, so these are exact on every
        // platform: the CPU reference for textures.wgsl.
        const EXPECT: [u64; 5] = [
            0xd6b3_368a_b7e9_076e, // checker
            0x5a9c_d89d_a389_f714, // stripes
            0x5589_a5d1_12a2_69c0, // noise
            0xe57d_ac70_7ef6_226c, // wood
            0xd14f_b5f2_163d_6db6, // dots
        ];
        let mut got = [0u64; 5];
        for (slot, k) in got.iter_mut().zip(ALL) {
            let mut bytes = Vec::with_capacity(4096 * 16);
            for i in 0..4096u32 {
                let p = [(i % 16) as f32 * 1.37 - 10.0, ((i / 16) % 16) as f32 * 0.91 - 7.0, (i / 256) as f32 * 2.1 - 8.0];
                let n = unit_normal(i);
                let t = sample(k, 6.5, p, n, (i % 7) as f32 * 0.35);
                for c in t.albedo {
                    bytes.extend_from_slice(&c.to_bits().to_le_bytes());
                }
                bytes.extend_from_slice(&t.roughness_delta.to_bits().to_le_bytes());
            }
            *slot = fnv1a64(&bytes);
        }
        assert_eq!(got, EXPECT, "texture output changed for one of {ALL:?}");
    }

    /// The machined finishes are deterministic, in range, and filtered by
    /// the pixel footprint: the box-filtered slope at a point equals the
    /// mean of the unfiltered slope over that footprint (measured against
    /// 400 fine samples), and once the footprint is several pitches the
    /// relief is flat while the roughness carries the slope variance the
    /// filter removed.
    #[test]
    fn machined_relief_is_deterministic_and_footprint_filtered() {
        let (ap, ax, origin, dir) = ([0.0f32; 3], [0.0f32, 0.0, 1.0], [0.0f32; 3], [1.0f32, 0.0, 0.0]);
        for kind in [Kind::Turned, Kind::Milled] {
            // On a plane facing +z, the turned grooves are rings about the
            // origin and the milled scallops run along x.
            let n = [0.0f32, 0.0, 1.0];
            let at = |p: [f32; 3], f: f32| machined(kind, 0.2, 0.0, p, n, f, ap, ax, origin, dir, false);
            for i in 0..2000u32 {
                let p = [point(i)[0], point(i)[1], 0.0];
                let a = at(p, 0.01);
                let b = at(p, 0.01);
                assert_eq!(a, b, "{kind:?} is not deterministic");
                assert!(a.bump.iter().all(|v| v.is_finite()) && a.slope_var.is_finite() && a.slope_var >= 0.0, "{kind:?} {a:?}");
                assert!((dot3(a.groove, a.groove) - 1.0).abs() < 1e-5, "{kind:?} groove {:?}", a.groove);
                assert!(dot3(a.groove, n).abs() < 1e-5, "{kind:?}: the groove lies in the surface");
                assert!(dot3(a.bump, n).abs() < 1e-5, "{kind:?}: the bump tilts the normal within the tangent plane");
            }
            // Filtering: at the point `p` the value with footprint `f` is
            // the mean over `[s - f/2, s + f/2]` along the phase direction.
            let p = [3.17f32, 2.05, 0.0];
            let fine = at(p, 0.0);
            let m = unit3(cross3(fine.groove, n)).unwrap();
            for f in [0.05f32, 0.1, 0.2] {
                let filtered = dot3(at(p, f).bump, m);
                let mut mean = 0.0f64;
                let samples = 400;
                for k in 0..samples {
                    let t = (k as f32 + 0.5) / samples as f32 - 0.5;
                    let q = [p[0] + m[0] * f * t, p[1] + m[1] * f * t, p[2] + m[2] * f * t];
                    mean += dot3(at(q, 0.0).bump, m) as f64;
                }
                let mean = (mean / samples as f64) as f32;
                assert!((filtered - mean).abs() < 0.02, "{kind:?} footprint {f}: filtered {filtered}, mean of fine samples {mean}");
            }
            // Visible: a real slope. Sub-pixel: flat, and the variance goes
            // to the roughness instead. Also the minified relief is the
            // same at every point, which is what "no aliasing" means.
            let visible = at(p, 0.0);
            assert!(dot3(visible.bump, visible.bump).sqrt() > 0.02 || dot3(at([3.22, 2.05, 0.0], 0.0).bump, m).abs() > 0.02, "{kind:?} has no relief when magnified");
            assert_eq!(visible.slope_var, 0.0, "{kind:?}: nothing is filtered away when magnified");
            let flat = at(p, 2.0);
            assert!(dot3(flat.bump, flat.bump).sqrt() < 1e-3, "{kind:?} still has relief at ten pitches per pixel: {:?}", flat.bump);
            assert!(flat.slope_var > 1e-3, "{kind:?}: the minified slope variance went nowhere");
            assert!((flat.slope_var - at([9.9, -4.4, 0.0], 2.0).slope_var).abs() < 1e-6, "{kind:?}: the minified variance varies with position");
        }
        // Turned grooves are rings: the groove direction is tangential and
        // the relief repeats with the radius, not with x or y.
        let n = [0.0f32, 0.0, 1.0];
        let r = machined(Kind::Turned, 0.2, 0.0, [1.0, 0.0, 0.0], n, 0.0, ap, ax, origin, dir, false);
        assert!(r.groove[1].abs() > 0.999, "at (1, 0) the groove runs along y: {:?}", r.groove);
        let same_radius = machined(Kind::Turned, 0.2, 0.0, [0.0, 1.0, 0.0], n, 0.0, ap, ax, origin, dir, false);
        assert!((dot3(r.bump, r.bump) - dot3(same_radius.bump, same_radius.bump)).abs() < 1e-6, "the ring is not round");
        // And on a cylinder about the axis the grooves are horizontal
        // rings whose phase moves with height.
        let side = machined(Kind::Turned, 0.2, 0.0, [1.0, 0.0, 0.37], [1.0, 0.0, 0.0], 0.0, ap, ax, origin, dir, false);
        assert!(side.groove[1].abs() > 0.999 && side.bump[0].abs() < 1e-5, "{side:?}");
        let higher = machined(Kind::Turned, 0.2, 0.0, [1.0, 0.0, 0.42], [1.0, 0.0, 0.0], 0.0, ap, ax, origin, dir, false);
        assert!((higher.bump[2] - side.bump[2]).abs() > 1e-3, "a quarter pitch up the cylinder the slope should differ");
        // Milled scallops run along `dir`, whatever the surface's tilt.
        let tilted = machined(Kind::Milled, 0.2, 0.0, [0.3, 0.4, 0.0], unit3([0.0, 0.3, 1.0]).unwrap(), 0.0, ap, ax, origin, [0.0, 1.0, 0.0], false);
        assert!(tilted.groove[1] > 0.9 && tilted.groove[0].abs() < 1e-5, "{tilted:?}");
        // `machined` is `turned` on a lathe-made face and `milled` elsewhere.
        let p = [1.3f32, 0.2, 0.0];
        assert_eq!(machined(Kind::Machined, 0.2, 0.0, p, n, 0.0, ap, ax, origin, dir, true), machined(Kind::Turned, 0.2, 0.0, p, n, 0.0, ap, ax, origin, dir, true));
        assert_eq!(machined(Kind::Machined, 0.2, 0.0, p, n, 0.0, ap, ax, origin, dir, false), machined(Kind::Milled, 0.2, 0.0, p, n, 0.0, ap, ax, origin, dir, false));
        assert!(kind("machined") == Some(Kind::Machined) && Kind::Machined.is_relief());
        // Degenerate inputs stay finite and switch the finish off.
        for bad in [f32::NAN, f32::INFINITY, 1e30] {
            let r = machined(Kind::Turned, bad, bad, [bad, 1.0, 2.0], [0.0, 0.0, 0.0], bad, ap, [0.0; 3], origin, [0.0; 3], false);
            assert!(r.bump.iter().all(|v| v.is_finite()) && r.slope_var.is_finite(), "{r:?}");
        }
        assert_eq!(default_scale(Kind::Turned), MACHINED_PITCH);
        assert_eq!(default_scale(Kind::Checker), DEFAULT_SCALE);
        assert_eq!(sample(Kind::Turned, 0.2, [1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 0.01).albedo, [1.0; 3], "a relief kind leaves the albedo alone");
    }

    #[test]
    fn atlas_is_deterministic_with_five_distinct_tiles() {
        let a = atlas(64);
        let b = atlas(64);
        assert_eq!(a.width, 64 * 5);
        assert_eq!(a.height, 64);
        assert_eq!(fnv1a64(&a.rgba), fnv1a64(&b.rgba), "atlas is not deterministic");
        let mut tiles: Vec<(u64, usize)> = Vec::new();
        for t in 0..5u32 {
            let mut px = Vec::new();
            let mut levels: Vec<u8> = Vec::new();
            for y in 0..64 {
                for x in 0..64 {
                    let i = (((y * a.width) + t * 64 + x) * 4) as usize;
                    px.extend_from_slice(&a.rgba[i..i + 4]);
                    if !levels.contains(&a.rgba[i]) {
                        levels.push(a.rgba[i]);
                    }
                }
            }
            assert!(levels.len() > 1, "tile {t} is flat");
            tiles.push((fnv1a64(&px), levels.len()));
        }
        for (i, x) in tiles.iter().enumerate() {
            for (j, y) in tiles.iter().enumerate() {
                assert!(i == j || x.0 != y.0, "tiles {i} and {j} are the same image");
            }
        }
    }
}
