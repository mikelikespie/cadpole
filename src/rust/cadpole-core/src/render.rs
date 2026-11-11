//! Headless software rendering: two backends behind one entry point, neither
//! of which needs a GPU, a driver or a display, so `cadpole render` and
//! `cadpole sweep` work in CI and in wasm.
//!
//! * [`Backend::Raster`] (the default) is a small, deterministic rasterizer,
//!   milliseconds per frame and good enough for an assistant to judge a shape
//!   from a contact sheet. It is the rest of this module.
//! * [`Backend::Path`] is the CPU path tracer in `trace`: seconds to minutes,
//!   with real soft shadows, refraction through clear parts, Beer-Lambert
//!   absorption inside them and emissive surfaces that light the scene. See
//!   docs/design.md.
//!
//! The two share the camera ([`View`]), the material contract
//! ([`crate::context::Material`]), the light rig, the procedural textures and
//! the ACES tonemap, so they are one look at two costs. Neither is a
//! substitute for the WebGPU viewport.
//!
//! Structure of the rasterizer (see docs/design.md):
//! [`render_with_stats`] is a fixed driver that runs one G-buffer pass per
//! anti-aliasing sample and averages the display-linear results. Every stage
//! lives in its own module:
//!
//! * `aa`        — sample patterns, the opaque geometry pass, deterministic
//!                 thread helpers, tonemap and the final resolve.
//! * `shading`   — lights and the BRDF; fills `Frame::hdr`.
//! * `occlusion` — SSAO and the ground contact shadow.
//! * `oit`       — order-independent translucency, composited into `hdr`.
//! * `lines`     — grid, feature edges, silhouettes and highlights (ink into
//!                 `Frame::display`).
//! * `textures`  — procedural surface patterns; `presets` — named materials.
//!
//! Determinism: same inputs give byte-identical output regardless of thread
//! count. Stages only ever write disjoint bands/tiles and depend on nothing but
//! their inputs.

use crate::context::Material;
use crate::geom::Vec3;
use crate::tessellate::Mesh;

mod aa;
pub mod alpha;
pub mod anim;
pub mod bvh;
pub mod debug;
mod lines;
mod occlusion;
mod oit;
mod presets;
mod projection;
pub use projection::Projection;
pub mod section;
mod shading;
#[cfg(test)]
pub(crate) mod testing;
mod textures;
mod trace;

pub use aa::{inverse_tonemap_rgb, pattern as sample_pattern, resolve_threads, tonemap_rgb};
pub use debug::DebugView;
pub use presets::{lookup as material_preset, names as material_preset_names};
pub use textures::{Kind as TextureKind, default_scale as texture_default_scale, kind as texture_kind, names as texture_names};
pub use trace::PathTraceOptions;

/// `Frame::item` value for "no item".
pub const NO_ITEM: u16 = u16::MAX;
/// `Frame::face` value for "no face".
pub const NO_FACE: u32 = u32::MAX;
/// `Frame::tri` value for "no triangle".
pub const NO_TRI: u32 = u32::MAX;
/// `Frame::coverage` bit set by the opaque geometry pass.
pub const COV_OPAQUE: u8 = 1;
/// `Frame::coverage` bit OR-ed in by the translucent pass where alpha > 0.
pub const COV_TRANSLUCENT: u8 = 2;

#[derive(Clone, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_degrees: f64,
    pub orthographic: bool,
}

impl Camera {
    /// Unit view direction for a preset name, or `None` if the name is not one.
    ///
    /// Separate from [`Camera::preset`] so the angular form can be tested
    /// without a bounding box, and so an unknown name is distinguishable from
    /// `iso` at the call site.
    pub fn preset_direction(preset: &str) -> Option<Vec3> {
        Some(match preset {
            "iso" | "" => Vec3::new(1.0, -1.0, 0.8).normalized(),
            "front" => Vec3::new(0.0, -1.0, 0.0),
            "back" => Vec3::new(0.0, 1.0, 0.0),
            "top" => Vec3::new(0.0, 0.0, 1.0),
            "bottom" => Vec3::new(0.0, 0.0, -1.0),
            "left" => Vec3::new(-1.0, 0.0, 0.0),
            "right" => Vec3::new(1.0, 0.0, 0.0),
            // Looking *up* at the part from a corner, which is how you check a
            // printed base: feet, chamfers and the first layer are all down
            // there, and every axis-aligned preset flattens them.
            "iso-bottom" | "bottom-iso" | "underside" => Vec3::new(1.0, -1.0, -0.8).normalized(),
            "iso-back" => Vec3::new(-1.0, 1.0, 0.8).normalized(),
            // The product shot:
            // a three-quarter view near eye level, the way a catalogue
            // photographs a part, where `iso` is 29.5° up and reads as a
            // drawing. `hero` is the same idea on a longer lens.
            "product" => return Camera::angular_direction("az=30,el=18"),
            "hero" => return Camera::angular_direction("az=35,el=12"),
            _ => return Camera::angular_direction(preset),
        })
    }

    /// Vertical field of view a preset is shot at, in degrees. 35° (a 38 mm
    /// lens on a 24 mm frame) for the engineering views; `product` is 24°
    /// (≈ 56 mm) and `hero` 18° (≈ 76 mm), because product photography is
    /// shot on a normal-to-long lens so the part's faces do not converge.
    pub fn preset_fov(preset: &str) -> f64 {
        match preset {
            "product" => 24.0,
            "hero" => 18.0,
            _ => 35.0,
        }
    }

    /// Padding around the fitted model a preset frames with, as a fraction
    /// of its size: the engineering views fill the frame (0.15); a product
    /// shot leaves air (0.25, `hero` 0.30).
    pub fn preset_margin(preset: &str) -> f64 {
        match preset {
            "product" => 0.25,
            "hero" => 0.30,
            _ => 0.15,
        }
    }

    /// Where the fitted subject's centre sits in the frame, as a fraction
    /// of the frame height from the bottom. 0.5 is centred; the product
    /// presets put it at 0.45, so there is floor in the foreground and
    /// headroom above — the composition every catalogue uses.
    pub fn preset_subject_height(preset: &str) -> f64 {
        match preset {
            "product" | "hero" => 0.45,
            _ => 0.5,
        }
    }

    /// Slide the camera so that what is now at the centre of the frame sits
    /// at `height` of the frame height from the bottom, keeping direction,
    /// distance and projection. The frame's height at the target is
    /// `2 · dist · tan(fov / 2)` under both projections (the orthographic
    /// scale is derived from the same fov and distance), so a fraction of
    /// it is a world-space shift along the camera's up vector.
    pub fn place_subject(&mut self, height: f64) {
        let shift = 0.5 - height;
        if !shift.is_finite() || shift.abs() < 1e-9 {
            return;
        }
        let dir = (self.target - self.eye).normalized();
        let right = dir.cross(self.up).normalized();
        let up = right.cross(dir);
        let dist = (self.target - self.eye).length();
        let frame_h = 2.0 * dist * (self.fov_degrees.clamp(1.0, 170.0).to_radians() * 0.5).tan();
        let d = up * (shift * frame_h);
        self.eye = self.eye + d;
        self.target = self.target + d;
    }

    /// `az=35,el=-25` — azimuth and elevation in degrees, for the views no
    /// name covers. Azimuth is measured from the front view (-Y) turning
    /// toward +X; elevation is above the XY plane, so negative looks up from
    /// underneath. `iso` is `az=45,el=29.5`.
    fn angular_direction(spec: &str) -> Option<Vec3> {
        let (mut az, mut el) = (f64::NAN, f64::NAN);
        for part in spec.split(',') {
            let (k, v) = part.trim().split_once('=')?;
            let v: f64 = v.trim().parse().ok()?;
            match k.trim() {
                "az" | "azimuth" => az = v,
                "el" | "elev" | "elevation" => el = v,
                _ => return None,
            }
        }
        if az.is_nan() && el.is_nan() {
            return None;
        }
        // A spec may give only one of the two. The missing one is zero: the
        // front view's azimuth, and the horizon's elevation.
        let az = if az.is_nan() { 0.0 } else { az }.to_radians();
        let el = if el.is_nan() { 0.0 } else { el }.to_radians();
        let dir = Vec3::new(az.sin() * el.cos(), -az.cos() * el.cos(), el.sin());
        Some(dir.normalized())
    }

    /// A camera looking at the box `(lo, hi)` from a named direction.
    ///
    /// `preset`: iso | front | back | top | bottom | left | right |
    /// iso-bottom | iso-back, or `az=<deg>,el=<deg>` for anything else.
    /// An unrecognised string is `iso`, because a render that comes back from
    /// a typo is more useful than one that does not come back.
    pub fn preset(preset: &str, lo: Vec3, hi: Vec3, margin: f64) -> Camera {
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(1e-3);
        let dir = Camera::preset_direction(preset).unwrap_or_else(|| Vec3::new(1.0, -1.0, 0.8).normalized());
        let fov = Camera::preset_fov(preset);
        let dist = radius * (1.0 + margin) / (fov.to_radians() / 2.0).sin();
        let up = if dir.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
        Camera { eye: center + dir * dist, target: center, up, fov_degrees: fov, orthographic: false }
    }

    /// A camera that frames the box tightly for a given image aspect ratio.
    ///
    /// [`Camera::preset`] fits a bounding *sphere*, which wastes most of the
    /// frame for anything that is not roughly cubic: a row of parts on a
    /// bench ends up as small objects in a sea of grid. This projects the
    /// eight corners of the box into camera space and pulls back only as far
    /// as the widest corner needs, horizontally and vertically.
    pub fn fitted(preset: &str, lo: Vec3, hi: Vec3, margin: f64, aspect: f64) -> Camera {
        let mut cam = Camera::preset(preset, lo, hi, margin);
        cam.fit(lo, hi, margin, aspect);
        cam
    }

    /// Move this camera along its own view direction so the box fills the
    /// frame. Keeps direction, up, fov and projection.
    ///
    /// Fitting a bounding *sphere* (what [`Camera::preset`] does) wastes most
    /// of a wide frame, and adding the depth extent to a sphere fit wastes
    /// more: the corner that is widest on screen is usually not the corner
    /// nearest the camera. This solves the real condition per corner —
    /// `|x| <= (d + z) * tan(h)` and `|y| <= (d + z) * tan(v)` — and takes the
    /// largest required `d`.
    pub fn fit(&mut self, lo: Vec3, hi: Vec3, margin: f64, aspect: f64) {
        let center = (lo + hi) * 0.5;
        let dir = (self.target - self.eye).normalized();
        let dist = self.fit_distance(lo, hi, margin, aspect, (0.0, 0.0));
        self.target = center;
        self.eye = center - dir * dist;
    }

    /// How far from the centre of the box `(lo, hi)` this camera has to
    /// stand, looking the way it looks now, for the box to be inside the
    /// frame: what [`Camera::fit`] moves it to.
    ///
    /// `pan` is a drift of the frame the caller is about to apply, as
    /// fractions of the frame's width and height at the target (what
    /// [`Camera::pan`] takes): the box then has to fit in the part of the
    /// frame the drift leaves on its side, so the half-angles shrink by
    /// `1 - 2|pan|`. An animation takes the largest of these over its
    /// frames and stands there (`cadpole_engine::animate`).
    pub fn fit_distance(&self, lo: Vec3, hi: Vec3, margin: f64, aspect: f64, pan: (f64, f64)) -> f64 {
        let center = (lo + hi) * 0.5;
        let dir = (self.target - self.eye).normalized();
        let right = dir.cross(self.up).normalized();
        let up = right.cross(dir);
        let aspect = if aspect.is_finite() && aspect > 0.01 { aspect } else { 1.0 };
        let tan_v = (self.fov_degrees.clamp(1.0, 170.0).to_radians() * 0.5).tan();
        let tan_h = tan_v * aspect;
        let pad = 1.0 + margin.max(0.0);
        let keep = |p: f64| (1.0 - 2.0 * p.abs()).max(0.05);
        let (keep_x, keep_y) = (keep(pan.0), keep(pan.1));

        let corners = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
            Vec3::new(hi.x, hi.y, hi.z),
        ];
        let mut dist: f64 = 1e-3;
        let mut max_x = 0.0f64;
        let mut max_y = 0.0f64;
        let mut max_depth = 0.0f64;
        for c in corners {
            let v = c - center;
            let (x, y, z) = (v.dot(right).abs() * pad, v.dot(up).abs() * pad, v.dot(dir));
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            max_depth = max_depth.max(z.abs());
            if !self.orthographic {
                // Distance needed so this corner is inside both half-angles.
                dist = dist.max((x / tan_h - z) / keep_x).max((y / tan_v - z) / keep_y);
            }
        }
        if self.orthographic {
            // The renderer derives the orthographic scale from fov and the
            // eye-target distance, so the same relation applies at the centre
            // depth; stand off far enough to clear the near geometry.
            dist = (max_x / tan_h / keep_x).max(max_y / tan_v / keep_y).max(1e-3) + max_depth;
        }
        dist
    }

    /// Slide the camera sideways and up by fractions of the frame's width
    /// and height at the target, keeping direction, distance and lens: a
    /// positive `x` moves the subject left in the frame, a positive `y`
    /// moves it down. `aspect` is the image's width over its height.
    pub fn pan(&mut self, x: f64, y: f64, aspect: f64) {
        if !(x.is_finite() && y.is_finite()) || (x == 0.0 && y == 0.0) {
            return;
        }
        let dir = (self.target - self.eye).normalized();
        let right = dir.cross(self.up).normalized();
        let up = right.cross(dir);
        let dist = (self.target - self.eye).length();
        let frame_h = 2.0 * dist * (self.fov_degrees.clamp(1.0, 170.0).to_radians() * 0.5).tan();
        let d = right * (x * frame_h * aspect) + up * (y * frame_h);
        self.eye = self.eye + d;
        self.target = self.target + d;
    }

    /// Frame the given points exactly, rather than their bounding box.
    ///
    /// A round or diagonal model is much smaller than its box in an isometric
    /// view, so box fitting leaves a third of the frame empty. Fitting the
    /// mesh vertices themselves costs one pass over positions we already have
    /// and fills the frame. `points` is called twice (centre, then extent).
    pub fn fit_points<'a, I, F>(&mut self, points: F, margin: f64, aspect: f64)
    where
        F: Fn() -> I,
        I: Iterator<Item = Vec3> + 'a,
    {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        let mut any = false;
        for p in points() {
            lo = lo.min(p);
            hi = hi.max(p);
            any = true;
        }
        if !any {
            return;
        }
        // Start from the box fit: it sets the target and a safe distance.
        self.fit(lo, hi, margin, aspect);
        let center = self.target;
        let dir = (self.target - self.eye).normalized();
        let right = dir.cross(self.up).normalized();
        let up = right.cross(dir);
        let aspect = if aspect.is_finite() && aspect > 0.01 { aspect } else { 1.0 };
        let tan_v = (self.fov_degrees.clamp(1.0, 170.0).to_radians() * 0.5).tan();
        let tan_h = tan_v * aspect;
        let pad = 1.0 + margin.max(0.0);
        let mut dist: f64 = 1e-3;
        let mut max_x = 0.0f64;
        let mut max_y = 0.0f64;
        let mut max_depth = 0.0f64;
        for p in points() {
            let v = p - center;
            let (x, y, z) = (v.dot(right).abs() * pad, v.dot(up).abs() * pad, v.dot(dir));
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            max_depth = max_depth.max(z.abs());
            if !self.orthographic {
                dist = dist.max(x / tan_h - z).max(y / tan_v - z);
            }
        }
        if self.orthographic {
            dist = (max_x / tan_h).max(max_y / tan_v).max(1e-3) + max_depth;
        }
        self.eye = center - dir * dist;
    }
}

/// Which static feature edges the overlay draws. Selection highlights are
/// drawn in every mode. See docs/views.md for the comparison that picked the
/// default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EdgeMode {
    /// No static edges at all: shading, ambient occlusion and the contact
    /// shadow carry the form, and ink appears only on a selection.
    Off,
    /// Object outlines only.
    Silhouette,
    /// Outlines plus the edges whose dihedral angle exceeds
    /// `RenderOptions::edge_crease_degrees`, outlines weighted a little
    /// heavier than interior creases. Tangent B-rep seams are never drawn.
    #[default]
    Creases,
    /// Every feature edge, tangent seams included.
    All,
}

impl EdgeMode {
    pub fn parse(s: &str) -> Option<EdgeMode> {
        match s {
            "off" | "none" => Some(EdgeMode::Off),
            "silhouette" | "outline" => Some(EdgeMode::Silhouette),
            "creases" | "crease" => Some(EdgeMode::Creases),
            "all" | "seams" => Some(EdgeMode::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeMode::Off => "off",
            EdgeMode::Silhouette => "silhouette",
            EdgeMode::Creases => "creases",
            EdgeMode::All => "all",
        }
    }
}

/// Ground grid style.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GridMode {
    Off,
    /// A square patch of segments sized to the scene.
    Finite,
    /// An unbounded plane rasterized per pixel, minor/major lines, coloured
    /// axes, fading with distance and as the line spacing approaches a few
    /// pixels so it never aliases.
    #[default]
    Infinite,
}

impl GridMode {
    pub fn parse(s: &str) -> Option<GridMode> {
        match s {
            "off" | "none" => Some(GridMode::Off),
            "finite" | "patch" => Some(GridMode::Finite),
            "infinite" | "on" => Some(GridMode::Infinite),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GridMode::Off => "off",
            GridMode::Finite => "finite",
            GridMode::Infinite => "infinite",
        }
    }
}

/// Which renderer draws the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Backend {
    /// The deterministic software rasterizer in this module: milliseconds,
    /// runs everywhere, good enough to judge a shape.
    #[default]
    Raster,
    /// The CPU path tracer in [`trace`]: seconds to minutes, real shadows,
    /// refraction, volumetric absorption inside the solid and emissive
    /// surfaces that light the scene. See docs/design.md.
    Path,
}

impl Backend {
    pub fn parse(s: &str) -> Option<Backend> {
        match s {
            "raster" | "rasterizer" => Some(Backend::Raster),
            "path" | "pathtrace" | "path-trace" | "pt" => Some(Backend::Path),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Raster => "raster",
            Backend::Path => "path",
        }
    }
}

/// How scene-linear radiance is squeezed into the display range
/// (`render/aa.rs`). One enum for both backends and the wgpu port, so an
/// image says which curve it was made with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToneMap {
    /// A plain clamp to 0..1, no exposure: what the tests measure through
    /// and what the debug views draw with, where a value must mean itself.
    None,
    /// Narkowicz's ACES fit, per channel — the look every image has had
    /// since the renderer began, and the engineering default. Lifts the
    /// mids, caps white at 0.8, and rotates saturated hues a little.
    #[default]
    Aces,
    /// Khronos PBR Neutral: hue-preserving, exact for base colours under
    /// unit light, invertible in closed form. The product-shot default,
    /// because a catalogue image's job is to show the colour the part is.
    Neutral,
}

impl ToneMap {
    pub fn parse(s: &str) -> Option<ToneMap> {
        match s {
            "" | "aces" => Some(ToneMap::Aces),
            "neutral" | "pbr-neutral" | "pbr_neutral" => Some(ToneMap::Neutral),
            "none" | "clamp" | "off" => Some(ToneMap::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ToneMap::None => "none",
            ToneMap::Aces => "aces",
            ToneMap::Neutral => "neutral",
        }
    }

    /// The names `parse` accepts, for a diagnostic.
    pub fn names() -> &'static [&'static str] {
        &["aces", "neutral", "none"]
    }

    /// The curve as a small integer, for a shader uniform: 0 none, 1 ACES,
    /// 2 Neutral (`cadpole-render-wgpu`'s `G.background.w`).
    pub fn index(self) -> u32 {
        match self {
            ToneMap::None => 0,
            ToneMap::Aces => 1,
            ToneMap::Neutral => 2,
        }
    }
}

/// Which light rig a render is lit by (`shading::Lights`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lighting {
    /// `Lights::studio()`: a warm key from the front-left *in world space*,
    /// a cool fill, a rim from behind and above, at 7.8:1. Reads form well
    /// in an engineering view; every golden was taken under it.
    #[default]
    Studio,
    /// `Lights::product(azimuth)`: a neutral catalogue rig — key 40° left
    /// of the camera at 40° up, a large soft fill 55° right, a narrow rim
    /// behind — that turns with the camera so a `--view back` shot is not
    /// lit from behind, over a brighter grey surround.
    Product,
}

impl Lighting {
    pub fn parse(s: &str) -> Option<Lighting> {
        match s {
            "" | "studio" => Some(Lighting::Studio),
            "product" | "catalogue" | "catalog" => Some(Lighting::Product),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Lighting::Studio => "studio",
            Lighting::Product => "product",
        }
    }

    pub fn names() -> &'static [&'static str] {
        &["studio", "product"]
    }
}

/// The surround: the environment gradient the path tracer shows and
/// reflects, the floor's albedo, and the flat colour the rasterizer paints
/// behind the part. `None`
/// on [`RenderOptions`] keeps the rig's own gradient, the dark floor and
/// the default `background`, which is what every image had before.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Backdrop {
    /// A mid-grey infinite cyclorama with a soft top light: sky 0.60,
    /// horizon 0.35, ground 0.25, and a floor of albedo 0.40 that, lit by
    /// the product rig, lands on the horizon tone so the two meet without
    /// a line.
    Studio,
    /// A white cyc: 0.95 / 0.90 / 0.80, floor 0.85. Lifts the shadow side
    /// by bounce light, as a real one does.
    White,
    /// Black: 0.15 / 0.02 / 0.01, floor 0.03. Reflections and rim only.
    Black,
    /// Any colour, linear RGB: sky ×1.4, horizon ×1, ground ×0.7, floor at
    /// horizon / 0.85.
    Color([f32; 3]),
}

impl Backdrop {
    /// `studio | white | black | #rrggbb` (the hex is sRGB, as every hex
    /// colour is).
    pub fn parse(s: &str) -> Option<Backdrop> {
        match s {
            "studio" | "grey" | "gray" => Some(Backdrop::Studio),
            "white" => Some(Backdrop::White),
            "black" => Some(Backdrop::Black),
            _ => {
                let hex = s.strip_prefix('#')?;
                if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return None;
                }
                Some(Backdrop::Color(crate::paint::unhex(hex).map(|v| v as f32)))
            }
        }
    }

    pub fn names() -> &'static [&'static str] {
        &["studio", "white", "black", "#rrggbb"]
    }

    /// `(sky, horizon, ground)` of the environment gradient, linear.
    pub fn env(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        match *self {
            Backdrop::Studio => ([0.60; 3], [0.35; 3], [0.25; 3]),
            Backdrop::White => ([0.95; 3], [0.90; 3], [0.80; 3]),
            Backdrop::Black => ([0.15; 3], [0.02; 3], [0.01; 3]),
            Backdrop::Color(c) => (scale(c, 1.4), c, scale(c, 0.7)),
        }
    }

    /// Albedo of the path tracer's floor under this backdrop.
    pub fn floor_albedo(&self) -> [f32; 3] {
        match *self {
            Backdrop::Studio => [0.40; 3],
            Backdrop::White => [0.85; 3],
            Backdrop::Black => [0.03; 3],
            Backdrop::Color(c) => scale(c, 1.0 / 0.85).map(|v| v.clamp(0.0, 1.0)),
        }
    }

    /// The flat colour the rasterizer paints behind the part so the two
    /// backends agree in tone: the horizon, through the tone map.
    pub fn background(&self, exposure: f32, tm: ToneMap) -> [f32; 3] {
        tonemap_rgb(self.env().1, exposure, tm)
    }
}

/// The path tracer's floor: its finish, or none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Floor {
    /// Roughness 0.85: the matte sweep every image had.
    #[default]
    Matte,
    /// Roughness 0.35 with the dielectric F0 of 0.04: a 4 % mirror at
    /// normal incidence rising at grazing, so a faint reflection anchors
    /// the part the way a slightly glossy table does in a product shot.
    Gloss,
    /// No floor at all.
    None,
}

impl Floor {
    pub fn parse(s: &str) -> Option<Floor> {
        match s {
            "" | "matte" => Some(Floor::Matte),
            "gloss" | "glossy" => Some(Floor::Gloss),
            "none" | "off" => Some(Floor::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Floor::Matte => "matte",
            Floor::Gloss => "gloss",
            Floor::None => "none",
        }
    }

    pub fn names() -> &'static [&'static str] {
        &["matte", "gloss", "none"]
    }

    /// The floor's GGX roughness (`None` has no floor; the value is moot).
    pub fn roughness(self) -> f32 {
        match self {
            Floor::Matte | Floor::None => 0.85,
            Floor::Gloss => 0.35,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    /// Display-linear RGB; written bit-exactly to uncovered pixels.
    pub background: [f32; 3],
    /// Master switch for static edges; `false` forces [`EdgeMode::Off`].
    pub draw_edges: bool,
    /// Master switch for the ground grid; `false` forces [`GridMode::Off`].
    pub draw_grid: bool,
    /// Which static edges to draw (subject to `draw_edges`).
    pub edges: EdgeMode,
    /// Minimum dihedral angle in degrees for an edge to count as a crease.
    /// Smooth spline surfaces are split into several B-rep faces whose seams
    /// are tangent (0 degrees); this is what keeps them out of the image.
    pub edge_crease_degrees: f32,
    /// Feature-edge width in pixels, independent of resolution and of the
    /// anti-aliasing sample count.
    pub edge_width_px: f32,
    /// Ground grid style (subject to `draw_grid`).
    pub grid: GridMode,
    /// Draw every body translucent so interiors are visible (docs/views.md
    /// `xray`). Applied by the caller when it builds the items.
    pub xray: bool,
    /// Anti-aliasing samples per pixel: 1, 4 or 8 (other values snap to the
    /// nearest). Above 16 Mpx the driver forces 1.
    pub samples: u32,
    /// Worker threads; 0 = every hardware thread. Always 1 on wasm32.
    /// Output never depends on this value.
    pub threads: u32,
    pub ambient_occlusion: bool,
    pub contact_shadow: bool,
    pub draw_silhouettes: bool,
    /// Scene-linear exposure multiplier applied before the tonemap.
    pub exposure: f32,
    /// The tone curve; [`ToneMap::None`] is the plain clamp the tests use.
    pub tonemap: ToneMap,
    /// The light rig. Both backends read it; the wgpu port knows only
    /// [`Lighting::Studio`].
    pub lighting: Lighting,
    /// The surround, or `None` for the rig's own gradient and the default
    /// floor and `background` (see [`Backdrop`]).
    pub backdrop: Option<Backdrop>,
    /// Bloom on the HDR image before the tone map (`aa::bloom`). Off by
    /// default: the halo crosses the silhouette, so backdrop pixels next to
    /// a highlight stop being bit-exactly `background`.
    pub bloom: bool,
    /// Which renderer draws the image. [`Backend::Path`] ignores `edges`,
    /// `grid`, `ambient_occlusion` and `contact_shadow`: it gets those from
    /// the light transport itself.
    pub backend: Backend,
    /// Settings that only [`Backend::Path`] reads.
    pub path: PathTraceOptions,
    /// Draw a debug view of the mesh instead of the part ([`debug`]):
    /// the studio rig gives way to a flat headlight, the tonemap to a plain
    /// clamp, and `seams`/`wire` take over the line overlay. Set it through
    /// [`RenderOptions::set_debug`], which also turns off everything that
    /// would get between the reading and the eye.
    pub debug: Option<DebugView>,
    /// The sun: a directional light with sharp shadows added to
    /// whichever rig `lighting` names. The rasterizer casts one shadow ray
    /// per sample through the opaque geometry and darkens its backdrop
    /// where the ground plane is shaded; the path tracer samples a disc of
    /// the sun's own 0.53°, so its shadows have a real penumbra. `None` is
    /// every render before this.
    pub sun: Option<SunLight>,
    /// No backdrop: a pixel's alpha is the share of it the model covers,
    /// and `background` is not drawn at all. The image is straight-alpha
    /// RGBA whose colour is the model's own — it was never mixed with a
    /// background, so it composites over any colour without a fringe. The
    /// environment still lights and reflects in the model; a translucent
    /// body is as opaque as it is, and a highlight on it stays (alpha is
    /// raised to hold it). See [`alpha`].
    pub transparent: bool,
    /// With `transparent`: keep the shadow the model casts on the ground as
    /// black at the shadow's own alpha, so the image still stands on
    /// whatever it is composited over. The rasterizer writes its contact
    /// shadow; the path tracer measures how much of the ground's light the
    /// model blocks.
    pub shadow_catcher: bool,
    /// Render `supersample`² sub-pixels per pixel and average them after
    /// the tone map (1, the default, is off; 2 to 4). The tone curve is
    /// applied per sub-pixel, so an edge between a highlight and a dark
    /// surface is a coverage ramp rather than the clipped average of two
    /// radiances; the path tracer spreads `samples` over the sub-pixels, so
    /// the cost there is the same, and the rasterizer's fixed patterns are
    /// multiplied.
    pub supersample: u32,
}

/// A sun for a render: the unit vector *toward* it in model space and its
/// irradiance on a surface square to it (scene-linear). See
/// [`crate::sun`] for turning a place, a date and a time into the
/// direction, and `SunLight::new` for the irradiance every render uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLight {
    pub dir: [f32; 3],
    pub irradiance: [f32; 3],
}

impl SunLight {
    /// Daylight: a neutral, faintly warm white at about two thirds of the
    /// studio key, which under the rig's own reduction (`Lights::with_sun`)
    /// leaves a sunlit face brighter than any rig light alone and a shaded
    /// one lit by the fill and the sky.
    pub const IRRADIANCE: [f32; 3] = [3.4, 3.3, 3.1];

    /// A sun along `dir` (toward the sun; normalised here) at the standard
    /// irradiance.
    pub fn new(dir: Vec3) -> SunLight {
        let d = dir.normalized();
        SunLight { dir: [d.x as f32, d.y as f32, d.z as f32], irradiance: SunLight::IRRADIANCE }
    }

    /// Above the horizon: a sun that lights anything.
    pub fn is_up(&self) -> bool {
        self.dir[2] > 0.0
    }
}

/// The opaque geometry as a ray-traceable structure, for the rasterizer's
/// sun shadows: built once per render, shared by every anti-aliasing pass.
pub struct SunShadow {
    pub bvh: bvh::Bvh,
}

impl SunShadow {
    /// Over every opaque item that casts a shadow; translucent ones let the
    /// sun through, as they let the eye through in the geometry pass, and
    /// guide geometry (`Material::shadow` false) is not there at all.
    pub fn build(items: &[Item<'_>]) -> SunShadow {
        SunShadow { bvh: bvh::Bvh::build_where(items, |it| !it.is_translucent() && it.casts_shadow()) }
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            width: 800,
            height: 600,
            background: [0.13, 0.14, 0.16],
            draw_edges: true,
            draw_grid: true,
            edges: EdgeMode::default(),
            edge_crease_degrees: 30.0,
            edge_width_px: 1.15,
            grid: GridMode::default(),
            xray: false,
            samples: 4,
            threads: 0,
            ambient_occlusion: true,
            contact_shadow: true,
            draw_silhouettes: true,
            exposure: 1.0,
            tonemap: ToneMap::Aces,
            lighting: Lighting::Studio,
            backdrop: None,
            bloom: false,
            backend: Backend::Raster,
            path: PathTraceOptions::default(),
            debug: None,
            sun: None,
            transparent: false,
            shadow_catcher: false,
            supersample: 1,
        }
    }
}

impl RenderOptions {
    /// Fast preview: one sample, no ambient occlusion, no contact shadow.
    pub fn draft(width: u32, height: u32) -> RenderOptions {
        RenderOptions { width, height, samples: 1, ambient_occlusion: false, contact_shadow: false, ..Default::default() }
    }

    /// Switch to a debug view. A debug view is a measurement, so everything
    /// that would blur it goes: one sample (the colours are exact, and the
    /// overlay antialiases itself), no ambient occlusion or contact shadow
    /// (they darken the colour that *is* the reading), no tonemap and unit
    /// exposure (a face's colour must come out as the palette put it in),
    /// no x-ray (an opacity is forced to 1 by the caller for the same
    /// reason a glass cover must not hide the mesh under it), and the
    /// rasterizer, which is the cheap backend and the one with a G-buffer
    /// to read normals and face ids from.
    pub fn set_debug(&mut self, view: DebugView) {
        self.debug = Some(view);
        self.backend = Backend::Raster;
        self.samples = 1;
        self.ambient_occlusion = false;
        self.contact_shadow = false;
        self.tonemap = ToneMap::None;
        self.bloom = false;
        self.exposure = 1.0;
        self.xray = false;
    }

    /// Edge style after the `draw_edges` master switch.
    pub fn edge_mode(&self) -> EdgeMode {
        if self.draw_edges { self.edges } else { EdgeMode::Off }
    }

    /// Grid style after the `draw_grid` master switch.
    pub fn grid_mode(&self) -> GridMode {
        if self.draw_grid { self.grid } else { GridMode::Off }
    }
}

/// Per topology face, the frame a machined texture is laid out in
/// (`textures::Kind::Turned`, `Milled`; `textures::machined`): the axis the
/// part was turned about, a point on the face, and the direction the cutter
/// ran. Built by the engine from the body's topology — a face of revolution
/// gives its own axis, any other face the body's, and a ruled face its
/// ruling — and empty when the caller has none, in which case the renderers
/// fall back to the Z axis through the scene centre and the X direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceFrame {
    pub axis_point: [f32; 3],
    pub axis: [f32; 3],
    pub origin: [f32; 3],
    pub dir: [f32; 3],
    /// A face a lathe made: a surface of revolution about the body's axis,
    /// or a plane square to it. What `texture="machined"` turns rather
    /// than mills.
    pub turned: bool,
}

impl FaceFrame {
    /// The frame used where no topology gave one.
    pub fn fallback(center: [f32; 3]) -> FaceFrame {
        FaceFrame { axis_point: center, axis: [0.0, 0.0, 1.0], origin: center, dir: [1.0, 0.0, 0.0], turned: false }
    }
}

/// An item to draw: a mesh, its material and its feature edges (polylines).
pub struct Item<'a> {
    pub mesh: &'a Mesh,
    pub material: &'a Material,
    /// One [`FaceFrame`] per topology face index (`Mesh::face_ids`), for
    /// the machined textures; empty when the caller has no topology.
    pub face_frames: &'a [FaceFrame],
    pub edges: &'a [Vec<Vec3>],
    /// Crease angle in degrees for each entry of `edges`, from
    /// [`crate::tessellate::EdgeInfo::dihedral_degrees`]: 0 is a tangent seam
    /// between two patches of one smooth surface, 90 a box corner, and -1 an
    /// open boundary. Empty (or shorter than `edges`) when the caller has no
    /// topology, in which case the overlay estimates the angle from the
    /// G-buffer normals instead.
    pub edge_dihedrals: &'a [f64],
    /// One display-linear colour per triangle, overriding the material's base
    /// colour. Empty unless something paints the surface: a script's face
    /// `paint(...)`, or the `thickness` and `overhang` analysis views painting
    /// a measured field without inventing a second shading path
    /// (docs/views.md).
    pub tri_colors: &'a [[f32; 3]],
    /// `tri_colors` are paint: they replace only the base colour, and the
    /// material's roughness, metal, texture, opacity and emission stay. When
    /// false they are an analysis or debug colour, which replaces the whole
    /// material so the reading is the colour and nothing else.
    pub paint: bool,
    /// Topology face indices to draw in the highlight colour (selection).
    pub highlight_faces: &'a [u32],
    /// Edge polylines to draw in the highlight colour (selection).
    pub highlight_edges: &'a [Vec<Vec3>],
    /// Opacity override in 0..1; `None` uses the material's. The override is
    /// how the `xray` view mode makes everything translucent without
    /// rewriting materials.
    pub opacity: Option<f64>,
}

impl<'a> Item<'a> {
    pub fn new(mesh: &'a Mesh, material: &'a Material, edges: &'a [Vec<Vec3>]) -> Item<'a> {
        Item {
            mesh,
            material,
            face_frames: &[],
            edges,
            edge_dihedrals: &[],
            tri_colors: &[],
            paint: false,
            highlight_faces: &[],
            highlight_edges: &[],
            opacity: None,
        }
    }

    /// The opacity this item is drawn with, 0..1.
    #[inline]
    pub fn opacity(&self) -> f32 {
        self.opacity.unwrap_or(self.material.opacity).clamp(0.0, 1.0) as f32
    }

    /// Translucent items skip the opaque G-buffer and go through `oit`.
    #[inline]
    pub fn is_translucent(&self) -> bool {
        self.opacity() < 0.999
    }

    /// Whether the item shadows anything. False is guide geometry:
    /// drawn in the colour pass as any body is, left out of the sun-shadow
    /// BVH and the contact shadow here, and out of every shadow ray, bounce
    /// and occlusion query in the path tracer.
    #[inline]
    pub fn casts_shadow(&self) -> bool {
        self.material.shadow
    }
}

/// Selection highlight colour (linear RGB).
pub const HIGHLIGHT: [f32; 3] = [0.98, 0.62, 0.12];

/// An RGBA8 image.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, bg: [f32; 3]) -> Image {
        let px = [to_srgb8(bg[0]), to_srgb8(bg[1]), to_srgb8(bg[2]), 255];
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            rgba.extend_from_slice(&px);
        }
        Image { width, height, rgba }
    }

    pub fn put(&mut self, x: u32, y: u32, c: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = ((y * self.width + x) * 4) as usize;
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }

    pub fn blit(&mut self, src: &Image, x0: u32, y0: u32) {
        for y in 0..src.height {
            for x in 0..src.width {
                let i = ((y * src.width + x) * 4) as usize;
                let c = [src.rgba[i], src.rgba[i + 1], src.rgba[i + 2], src.rgba[i + 3]];
                self.put(x0 + x, y0 + y, c);
            }
        }
    }

    /// Decode any PNG — palette, grey, grey with alpha, RGB, RGBA, at 8 or
    /// 16 bits — to eight-bit RGBA. For a picture a user placed (a render
    /// on a drawing sheet), where the file was not written here.
    pub fn from_png(bytes: &[u8]) -> Result<Image, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
        let size = reader.output_buffer_size().ok_or("png: no output size")?;
        let mut buf = vec![0u8; size];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("reading the PNG: {e}"))?;
        let n = (info.width as usize) * (info.height as usize);
        let data = &buf[..info.buffer_size()];
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => data.to_vec(),
            png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::Grayscale => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            other => return Err(format!("png: colour type {other:?} after expansion")),
        };
        if rgba.len() != n * 4 {
            return Err(format!("png: {} bytes decoded for {n} pixels", rgba.len()));
        }
        Ok(Image { width: info.width, height: info.height, rgba })
    }

    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("png header");
            w.write_image_data(&self.rgba).expect("png data");
        }
        out
    }

    /// Draw text with the built-in 5x7 font. `scale` is the pixel size of a
    /// font pixel.
    pub fn text(&mut self, x: u32, y: u32, s: &str, scale: u32, color: [u8; 4]) {
        let mut cx = x;
        for ch in s.chars() {
            let g = font::glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.put(cx + col * scale + dx, y + row as u32 * scale + dy, color);
                            }
                        }
                    }
                }
            }
            cx += 6 * scale;
        }
    }
}

/// Display-linear 0..1 -> sRGB 8-bit.
pub(crate) fn to_srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// sRGB 8-bit -> display-linear 0..1.
pub(crate) fn from_srgb8(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// The albedo multiplier a mesh's per-vertex colour gives the point
/// `p` of triangle `tri`: the three corners' colours, display-linear,
/// blended by `p`'s barycentric coordinates — so a scan's colour runs
/// smoothly across a triangle as it does in the web viewer. `None` for a
/// mesh without colours, which is every mesh Cadpole builds itself. The
/// material's own base colour, texture and character still apply; this
/// multiplies the base colour, as three.js's `vertexColors` does.
pub(crate) fn vertex_albedo(mesh: &Mesh, tri: u32, p: [f32; 3]) -> Option<[f32; 3]> {
    use std::sync::OnceLock;
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    let colors = mesh.vertex_colors()?;
    let t = tri as usize * 3;
    let idx = mesh.indices.get(t..t + 3)?;
    let lut = LINEAR.get_or_init(|| std::array::from_fn(|i| from_srgb8(i as u8)));
    let corner = |i: u32| -> Option<([f32; 3], [f32; 3])> {
        let i = i as usize;
        let pos = mesh.positions.get(i * 3..i * 3 + 3)?;
        let c = colors.get(i * 3..i * 3 + 3)?;
        Some(([pos[0], pos[1], pos[2]], [lut[c[0] as usize], lut[c[1] as usize], lut[c[2] as usize]]))
    };
    let (a, ca) = corner(idx[0])?;
    let (b, cb) = corner(idx[1])?;
    let (c, cc) = corner(idx[2])?;
    let (v0, v1, v2) = (sub(b, a), sub(c, a), sub(p, a));
    let (d00, d01, d11) = (dot(v0, v0), dot(v0, v1), dot(v1, v1));
    let (d20, d21) = (dot(v2, v0), dot(v2, v1));
    let den = d00 * d11 - d01 * d01;
    let (u, v, w) = if den.abs() > f32::MIN_POSITIVE {
        let v = ((d11 * d20 - d01 * d21) / den).clamp(0.0, 1.0);
        let w = ((d00 * d21 - d01 * d20) / den).clamp(0.0, 1.0);
        let u = (1.0 - v - w).max(0.0);
        let sum = u + v + w;
        (u / sum, v / sum, w / sum)
    } else {
        (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0)
    };
    Some([
        u * ca[0] + v * cb[0] + w * cc[0],
        u * ca[1] + v * cb[1] + w * cc[1],
        u * ca[2] + v * cb[2] + w * cc[2],
    ])
}

// ---------------------------------------------------------------------------
// Small f32 vector helpers shared by the stages (some are only used by
// stages that are still stubs).

#[inline]
pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[inline]
pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Component-wise product.
#[inline]
pub(crate) fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

#[inline]
pub(crate) fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { v }
}

#[inline]
pub(crate) fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[inline]
pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Rec. 709 luminance of a linear RGB triple.
#[inline]
pub(crate) fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

// ---------------------------------------------------------------------------
// Frame, Scene, Ctx, View.

/// Per-pixel G-buffer, SoA, index = `y * w + x`, y down. Rebuilt (cleared)
/// once per AA pass.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    /// View-space z along `fwd` of the nearest OPAQUE surface; `INFINITY` = none.
    pub depth: Vec<f32>,
    /// World-space unit normal, flipped toward the viewer.
    pub normal: Vec<[f32; 3]>,
    /// World-space position (perspective-correct).
    pub pos: Vec<[f32; 3]>,
    /// Index into `ctx.items`; `NO_ITEM` = none.
    pub item: Vec<u16>,
    /// Topology face id of the triangle; `NO_FACE` = none.
    pub face: Vec<u32>,
    /// Index of the triangle inside its item's mesh; `NO_TRI` = none. This is
    /// what lets a per-triangle field (wall thickness, overhang angle) be
    /// looked up per pixel, which is how the analysis views are drawn.
    pub tri: Vec<u32>,
    /// 0 = untouched background, else `COV_OPAQUE | COV_TRANSLUCENT` bits.
    pub coverage: Vec<u8>,
    /// 1.0 = unoccluded; background stays 1.0.
    pub ao: Vec<f32>,
    /// 0..1 darkening for background pixels (contact shadow); 1.0 = none.
    pub ground: Vec<f32>,
    /// Scene-linear radiance >= 0, unbounded.
    pub hdr: Vec<[f32; 3]>,
    /// Display-linear 0..1 after tonemap; overlays blend here.
    pub display: Vec<[f32; 3]>,
    /// How opaque `display` is, 0..1: written by the tonemap and raised by
    /// every overlay that inks the pixel. With a background it is 1
    /// everywhere; without one (`RenderOptions::transparent`) `display` is
    /// premultiplied by it.
    pub alpha: Vec<f32>,
}

impl Frame {
    pub fn new(w: u32, h: u32) -> Frame {
        let n = (w as usize) * (h as usize);
        Frame {
            w,
            h,
            depth: vec![f32::INFINITY; n],
            normal: vec![[0.0, 0.0, 1.0]; n],
            pos: vec![[0.0; 3]; n],
            item: vec![NO_ITEM; n],
            face: vec![NO_FACE; n],
            tri: vec![NO_TRI; n],
            coverage: vec![0; n],
            ao: vec![1.0; n],
            ground: vec![1.0; n],
            hdr: vec![[0.0; 3]; n],
            display: vec![[0.0; 3]; n],
            alpha: vec![1.0; n],
        }
    }

    /// depth = INF, item = NO_ITEM, face = NO_FACE, coverage = 0, ao = 1,
    /// ground = 1, hdr = 0, display = 0.
    pub fn clear(&mut self) {
        self.depth.fill(f32::INFINITY);
        self.item.fill(NO_ITEM);
        self.face.fill(NO_FACE);
        self.tri.fill(NO_TRI);
        self.coverage.fill(0);
        self.ao.fill(1.0);
        self.ground.fill(1.0);
        self.hdr.fill([0.0; 3]);
        self.display.fill([0.0; 3]);
        self.alpha.fill(1.0);
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.w + x) as usize
    }
}

/// Scene extents, computed once per render from every item's mesh bbox
/// (translucent items included).
#[derive(Clone, Debug)]
pub struct Scene {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub center: [f32; 3],
    /// Bounding-box diagonal, >= 1e-3.
    pub diag: f32,
    /// = lo.z
    pub ground_z: f32,
}

impl Scene {
    pub fn from_items(items: &[Item<'_>]) -> Scene {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for it in items {
            if let Some((a, b)) = it.mesh.bbox() {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if lo.x == f64::MAX {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        let f = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let (lo, hi) = (f(lo), f(hi));
        let center = scale(add(lo, hi), 0.5);
        let d = sub(hi, lo);
        Scene { lo, hi, center, diag: dot(d, d).sqrt().max(1e-3), ground_z: lo[2] }
    }
}

/// Everything a stage needs. `view` is the jittered view of the current AA pass.
pub struct Ctx<'a> {
    pub items: &'a [Item<'a>],
    pub opts: &'a RenderOptions,
    pub scene: &'a Scene,
    pub view: &'a View,
    /// Resolved worker count, >= 1 (1 on wasm32).
    pub threads: u32,
    /// The shadow structure for `opts.sun`, when there is a sun above the
    /// horizon; `None` otherwise, and the shading skips the shadow rays.
    pub sun_shadow: Option<&'a SunShadow>,
}

/// Camera basis for one AA pass.
#[derive(Clone, Debug)]
pub struct View {
    pub(crate) eye: [f32; 3],
    pub(crate) right: [f32; 3],
    pub(crate) up: [f32; 3],
    pub(crate) fwd: [f32; 3],
    pub(crate) focal: f32,
    pub(crate) ortho_scale: f32,
    pub(crate) orthographic: bool,
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// Sample position within the pixel, each in (0, 1); (0.5, 0.5) = centre.
    pub(crate) jitter: [f32; 2],
    /// Thin-lens aperture radius in world units (mm); 0 is a pinhole, which
    /// every view is unless [`View::with_dof`] says otherwise.
    pub(crate) lens_radius: f32,
    /// Distance along `fwd` at which the lens is focused: the eye-to-target
    /// distance.
    pub(crate) focus: f32,
}

impl View {
    /// Unjittered view (sample at the pixel centre).
    pub fn new(cam: &Camera, w: u32, h: u32) -> View {
        let fwd = (cam.target - cam.eye).normalized();
        let right = fwd.cross(cam.up).normalized();
        let up = right.cross(fwd);
        let focal = (h as f64 * 0.5) / (cam.fov_degrees.to_radians() * 0.5).tan();
        let dist = (cam.target - cam.eye).length();
        let ortho_scale = focal / dist as f64;
        let f = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        View {
            eye: f(cam.eye),
            right: f(right),
            up: f(up),
            fwd: f(fwd),
            focal: focal as f32,
            ortho_scale: ortho_scale as f32,
            orthographic: cam.orthographic,
            w: w as f32,
            h: h as f32,
            jitter: [0.5, 0.5],
            lens_radius: 0.0,
            focus: dist as f32,
        }
    }

    /// The same view through a thin lens at f-number `f_number` (0 or less
    /// = pinhole, unchanged). The lens is sized as a real one: a 24 mm
    /// frame height makes the focal length `12 / tan(fov / 2)` mm, and the
    /// aperture radius is `f / (2 N)` — the scene is in millimetres, so f/8
    /// means f/8. Focus is at the camera target. Perspective only; an
    /// orthographic view has no lens to speak of.
    pub fn with_dof(&self, f_number: f32) -> View {
        if self.orthographic || !(f_number > 0.0) || self.h <= 0.0 {
            return self.clone();
        }
        let tan_half = (self.h * 0.5) / self.focal.max(1e-6);
        let focal_mm = 12.0 / tan_half.max(1e-6);
        View { lens_radius: focal_mm / (2.0 * f_number), ..self.clone() }
    }

    /// Whether [`View::ray_dof`] does anything [`View::ray`] does not.
    #[inline]
    pub fn has_dof(&self) -> bool {
        self.lens_radius > 0.0 && !self.orthographic
    }

    /// The thin-lens ray through screen sample `(sx, sy)` for lens sample
    /// `(u1, u2)` in [0, 1)²: the pinhole ray's point at the focus distance
    /// is what every ray through the lens converges on, and the origin is
    /// a concentric-disc sample of the aperture.
    pub fn ray_dof(&self, sx: f32, sy: f32, u1: f32, u2: f32) -> ([f32; 3], [f32; 3]) {
        let (o, d) = self.ray(sx, sy);
        if !self.has_dof() {
            return (o, d);
        }
        let along = dot(d, self.fwd).max(1e-4);
        let focus_point = add(o, scale(d, self.focus / along));
        // Shirley-Chiu concentric mapping: uniform on the disc, no clumping
        // at the centre.
        let (a, b) = (2.0 * u1 - 1.0, 2.0 * u2 - 1.0);
        let (r, phi) = if a == 0.0 && b == 0.0 {
            (0.0, 0.0)
        } else if a * a > b * b {
            (a, std::f32::consts::FRAC_PI_4 * (b / a))
        } else {
            (b, std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4 * (a / b))
        };
        let (lx, ly) = (r * phi.cos() * self.lens_radius, r * phi.sin() * self.lens_radius);
        let origin = add(add(self.eye, scale(self.right, lx)), scale(self.up, ly));
        (origin, normalize(sub(focus_point, origin)))
    }

    /// The camera's azimuth in radians, in the convention of
    /// `Camera::preset_direction`'s `az=`: 0 is the front view (the eye on
    /// −Y looking +Y) and it turns toward +X. What `shading::Lights::product`
    /// locks the rig to.
    pub fn azimuth(&self) -> f32 {
        // The eye direction is −fwd, and az = atan2(dir.x, −dir.y).
        (-self.fwd[0]).atan2(self.fwd[1])
    }

    /// The same camera with the sample moved to `(jx, jy)` within the pixel.
    pub fn with_jitter(&self, jx: f32, jy: f32) -> View {
        View { jitter: [jx, jy], ..self.clone() }
    }

    #[inline]
    fn offset(&self) -> [f32; 2] {
        [self.jitter[0] - 0.5, self.jitter[1] - 0.5]
    }

    /// World -> (screen x, screen y, depth along `fwd`). Screen coordinates
    /// include the jitter offset so that rasterizing at pixel centres samples
    /// the jittered position. `None` if z <= 1e-4 in perspective.
    #[inline]
    pub fn project(&self, p: [f32; 3]) -> Option<[f32; 3]> {
        let d = sub(p, self.eye);
        let x = dot(d, self.right);
        let y = dot(d, self.up);
        let z = dot(d, self.fwd);
        let o = self.offset();
        if self.orthographic {
            Some([self.w * 0.5 + x * self.ortho_scale + o[0], self.h * 0.5 - y * self.ortho_scale + o[1], z])
        } else {
            if z <= 1e-4 {
                return None;
            }
            Some([self.w * 0.5 + x * self.focal / z + o[0], self.h * 0.5 - y * self.focal / z + o[1], z])
        }
    }

    /// Inverse of [`View::project`] for the same jitter.
    #[inline]
    pub fn unproject(&self, sx: f32, sy: f32, z: f32) -> [f32; 3] {
        let o = self.offset();
        let (nx, ny) = (sx - o[0] - self.w * 0.5, self.h * 0.5 - (sy - o[1]));
        let (x, y) = if self.orthographic {
            (nx / self.ortho_scale, ny / self.ortho_scale)
        } else {
            (nx * z / self.focal, ny * z / self.focal)
        };
        add(add(self.eye, scale(self.right, x)), add(scale(self.up, y), scale(self.fwd, z)))
    }

    /// (origin, unit direction) of the ray through screen sample `(sx, sy)`.
    /// Orthographic: the origin varies and the direction is `fwd`.
    ///
    /// The perspective direction is built from the camera basis, never by
    /// differencing two world points: [`View::unproject`] at depth 1 is a
    /// millimetre from the eye, and two f32 world points a millimetre apart
    /// near 1e7 mm (a site in projected coordinates) differ by a handful of
    /// ulps, so every ray snapped to a few directions -- and near the 1e9 mm
    /// eye of a very large model, to none.
    pub fn ray(&self, sx: f32, sy: f32) -> ([f32; 3], [f32; 3]) {
        let (o, d) = self.eye_ray(sx, sy);
        if self.orthographic { (add(self.eye, o), d) } else { (self.eye, normalize(d)) }
    }

    /// The ray through screen sample `(sx, sy)` with its origin relative to
    /// the eye and its direction scaled so that its dot product with `fwd`
    /// is 1 (the ray parameter at a hit is the view depth). Nothing in it is
    /// a difference of world points, so it keeps its precision however far
    /// the camera is from the origin; a caller that needs a world point adds
    /// `eye` itself, or keeps working relative to it.
    pub(crate) fn eye_ray(&self, sx: f32, sy: f32) -> ([f32; 3], [f32; 3]) {
        let o = self.offset();
        let (nx, ny) = (sx - o[0] - self.w * 0.5, self.h * 0.5 - (sy - o[1]));
        if self.orthographic {
            let (x, y) = (nx / self.ortho_scale, ny / self.ortho_scale);
            (add(scale(self.right, x), scale(self.up, y)), self.fwd)
        } else {
            let (x, y) = (nx / self.focal, ny / self.focal);
            ([0.0; 3], add(add(scale(self.right, x), scale(self.up, y)), self.fwd))
        }
    }

    /// Unit vector from `p` toward the eye (orthographic: `-fwd`).
    #[inline]
    pub fn to_eye(&self, p: [f32; 3]) -> [f32; 3] {
        if self.orthographic { scale(self.fwd, -1.0) } else { normalize(sub(self.eye, p)) }
    }

    /// World-space size of one pixel at depth `z`.
    #[inline]
    pub fn footprint(&self, z: f32) -> f32 {
        if self.orthographic { 1.0 / self.ortho_scale } else { z.max(1e-4) / self.focal }
    }

    #[inline]
    pub fn is_orthographic(&self) -> bool {
        self.orthographic
    }
}

/// Timing of one render, milliseconds (all 0 on wasm32).
///
/// [`Backend::Path`] fills the same struct with the closest equivalents:
/// `geometry_ms` is the BVH build, `shade_ms` the path tracing itself,
/// `resolve_ms` the tonemap and encode, `passes` the sample count. The
/// stages it has no analogue for stay 0.
#[derive(Clone, Debug, Default)]
pub struct RenderStats {
    pub geometry_ms: f64,
    pub ssao_ms: f64,
    pub contact_ms: f64,
    pub shade_ms: f64,
    pub oit_ms: f64,
    pub overlay_ms: f64,
    pub resolve_ms: f64,
    pub threads_used: u32,
    pub passes: u32,
    /// Scene triangles the path tracer's BVH covers (0 for the rasterizer).
    pub triangles: usize,
    /// Nodes in that BVH.
    pub bvh_nodes: usize,
}

/// Render items into an image.
pub fn render(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> Image {
    render_with_stats(items, cam, opts).0
}

/// Render items into an image and report per-stage timings.
///
/// This driver is frozen: stages are filled in by their modules and never
/// reordered here.
pub fn render_with_stats(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> (Image, RenderStats) {
    let k = opts.supersample.clamp(1, 4);
    if k > 1 && opts.debug.is_none() {
        // Sub-pixels are whole renders' pixels: everything measured in
        // pixels grows with them, and the tracer's samples are shared out.
        let fine = RenderOptions {
            width: opts.width.max(1) * k,
            height: opts.height.max(1) * k,
            supersample: 1,
            edge_width_px: opts.edge_width_px * k as f32,
            samples: if opts.backend == Backend::Path { opts.samples.div_ceil(k * k).max(1) } else { opts.samples },
            ..opts.clone()
        };
        let (img, stats) = render_with_stats(items, cam, &fine);
        return (alpha::downsample(&img, k), stats);
    }
    if opts.backend == Backend::Path {
        // The tracer shares the camera, the material contract, the light rig
        // and the tonemap with the driver below; everything between is its
        // own. See `render/trace.rs`.
        return trace::render(items, cam, opts);
    }
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    let scene = Scene::from_items(items);
    let base = View::new(cam, w, h);
    let threads = aa::resolve_threads(opts.threads);
    let sun_shadow = opts.sun.filter(|s| s.is_up() && opts.debug.is_none()).map(|_| SunShadow::build(items));
    // Without a backdrop the stages draw over black, which makes `display`
    // the premultiplied colour, and the contact shadow is kept only when it
    // is asked for as the image's own.
    let clear;
    let opts = if opts.transparent {
        clear = RenderOptions { background: [0.0; 3], contact_shadow: opts.contact_shadow && opts.shadow_catcher, ..opts.clone() };
        &clear
    } else {
        opts
    };
    let ctx0 = Ctx { items, opts, scene: &scene, view: &base, threads, sun_shadow: sun_shadow.as_ref() };
    let hf = occlusion::Heightfield::build(&ctx0);
    let mut frame = Frame::new(w, h);
    let mut oit_buf = oit::Buffers::new(w, h);
    let pattern = aa::pattern(if (w as u64 * h as u64) > 16_000_000 { 1 } else { opts.samples });
    let mut acc = vec![[0f32; 3]; (w as usize) * (h as usize)];
    let mut acc_alpha = vec![0f32; if opts.transparent { (w as usize) * (h as usize) } else { 0 }];
    let mut stats = RenderStats { threads_used: threads, passes: pattern.len() as u32, ..Default::default() };
    for &(jx, jy) in pattern {
        let view = base.with_jitter(jx, jy);
        let ctx = Ctx { view: &view, ..ctx0 };
        frame.clear();
        let t0 = aa::now_ms();
        aa::geometry_pass(&mut frame, &ctx);
        let t1 = aa::now_ms();
        occlusion::ssao(&mut frame, &ctx);
        let t2 = aa::now_ms();
        occlusion::ground_contact(&mut frame, &ctx, &hf);
        let t3 = aa::now_ms();
        shading::shade(&mut frame, &ctx);
        let t4 = aa::now_ms();
        oit::translucent_pass(&mut frame, &ctx, &mut oit_buf);
        let t5 = aa::now_ms();
        if opts.bloom {
            aa::bloom(&mut frame.hdr, w, h);
        }
        aa::tonemap(&mut frame, &ctx, &oit_buf);
        let t6 = aa::now_ms();
        lines::overlay(&mut frame, &ctx, &oit_buf);
        let t7 = aa::now_ms();
        aa::accumulate(&mut acc, &frame.display);
        for (a, f) in acc_alpha.iter_mut().zip(&frame.alpha) {
            *a += f;
        }
        let t8 = aa::now_ms();
        stats.geometry_ms += t1 - t0;
        stats.ssao_ms += t2 - t1;
        stats.contact_ms += t3 - t2;
        stats.shade_ms += t4 - t3;
        stats.oit_ms += t5 - t4;
        stats.resolve_ms += (t6 - t5) + (t8 - t7);
        stats.overlay_ms += t7 - t6;
    }
    let t0 = aa::now_ms();
    let img = if opts.transparent {
        alpha::resolve(&acc, &acc_alpha, pattern.len() as u32, w, h)
    } else {
        aa::resolve(&acc, pattern.len() as u32, w, h, Some(opts.background))
    };
    stats.resolve_ms += aa::now_ms() - t0;
    (img, stats)
}

/// Camera fitted to a box, for callers that want one camera across several
/// renders (sweeps), instead of per-scene fitting. `aspect` is width/height
/// of the target image.
pub fn fit_camera(preset: &str, lo: Vec3, hi: Vec3, margin: f64, aspect: f64) -> Camera {
    Camera::fitted(preset, lo, hi, margin, aspect)
}

/// Lay out labelled tiles into one contact sheet, `cols` per row: the
/// sweep's sheet, with its 16 px label strip and light text.
pub fn contact_sheet(tiles: &[(String, Image)], cols: u32, bg: [f32; 3]) -> Image {
    contact_sheet_with(tiles, cols, bg, 1, [230, 232, 236, 255])
}

/// [`contact_sheet`] with the label's font scale and colour chosen by the
/// caller: a sheet of large tiles wants a label that can be read at their
/// size, and a light sheet wants dark text. The label strip is
/// `7 * scale + 9` px tall and the gutter `6 * scale` px, so the layout is
/// the sweep's exactly at scale 1.
pub fn contact_sheet_with(tiles: &[(String, Image)], cols: u32, bg: [f32; 3], scale: u32, color: [u8; 4]) -> Image {
    sheet_of(tiles, cols, Some(bg), scale, color)
}

/// A contact sheet with nothing behind the tiles: for tiles rendered on a
/// transparent background, whose alpha a coloured sheet would have to
/// flatten. The gutters are alpha 0 and the labels opaque.
pub fn contact_sheet_clear(tiles: &[(String, Image)], cols: u32, scale: u32, color: [u8; 4]) -> Image {
    sheet_of(tiles, cols, None, scale, color)
}

fn sheet_of(tiles: &[(String, Image)], cols: u32, bg: Option<[f32; 3]>, scale: u32, color: [u8; 4]) -> Image {
    let cols = cols.max(1);
    let scale = scale.max(1);
    let (tw, th) = tiles.first().map(|(_, i)| (i.width, i.height)).unwrap_or((1, 1));
    let label_h = 7 * scale + 9;
    let pad = 6 * scale;
    let rows = (tiles.len() as u32).div_ceil(cols);
    let mut sheet = Image::new(cols * (tw + pad) + pad, rows * (th + label_h + pad) + pad, bg.unwrap_or([0.0; 3]));
    if bg.is_none() {
        sheet.rgba.fill(0);
    }
    for (i, (label, img)) in tiles.iter().enumerate() {
        let (r, c) = ((i as u32) / cols, (i as u32) % cols);
        let x = pad + c * (tw + pad);
        let y = pad + r * (th + label_h + pad);
        sheet.blit(img, x, y);
        sheet.text(x + 2, y + th + 3, label, scale, color);
    }
    sheet
}

/// A compact 5x7 bitmap font covering printable ASCII (unknown -> box).
pub mod font {
    pub fn glyph(c: char) -> [u8; 7] {
        match c.to_ascii_uppercase() {
            ' ' => [0, 0, 0, 0, 0, 0, 0],
            '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
            '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
            '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
            '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
            '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
            '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
            '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
            '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
            '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
            '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
            'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
            'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
            'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
            'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
            'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
            'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
            'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
            'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
            'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
            'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
            'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
            'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
            'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
            'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
            'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
            'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
            'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
            'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
            'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
            'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
            'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
            'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
            'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
            'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
            'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
            'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
            '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
            ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
            ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
            '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
            '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
            '=' => [0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00],
            '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
            '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
            '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
            ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
            '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
            '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
            '*' => [0x00, 0x0A, 0x04, 0x1F, 0x04, 0x0A, 0x00],
            '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
            '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
            '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
            '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
            '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
            ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
            '\'' => [0x04, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00],
            '"' => [0x0A, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00],
            _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    /// A placed picture comes in any colour type; the decoder hands back
    /// RGBA either way, and a round trip through `to_png` is exact.
    #[test]
    fn any_png_decodes_to_rgba() {
        let mut img = Image::new(3, 2, [0.0, 0.0, 0.0]);
        img.put(1, 0, [255, 0, 0, 128]);
        let back = Image::from_png(&img.to_png()).expect("decodes");
        assert_eq!((back.width, back.height), (3, 2));
        assert_eq!(back.rgba, img.rgba);
        // A grey PNG with no alpha comes back opaque grey.
        let mut grey = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut grey, 2, 1);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[10, 200]).unwrap();
        }
        let g = Image::from_png(&grey).expect("grey decodes");
        assert_eq!(g.rgba, vec![10, 10, 10, 255, 200, 200, 200, 255]);
        assert!(Image::from_png(b"not a png").is_err());
    }

    /// `az`/`el` has to agree with the named presets, or a script that moves
    /// from `view="iso"` to `view="az=45,el=29.5"` would jump.
    #[test]
    fn angular_view_specs_agree_with_the_named_presets() {
        let close = |a: Vec3, b: Vec3, what: &str| {
            assert!((a - b).length() < 2e-3, "{what}: {a:?} vs {b:?}");
        };
        let d = |s: &str| Camera::preset_direction(s).unwrap_or_else(|| panic!("{s} is not a view"));
        close(d("az=0,el=0"), d("front"), "front");
        close(d("az=180,el=0"), d("back"), "back");
        close(d("az=90,el=0"), d("right"), "right");
        close(d("az=-90,el=0"), d("left"), "left");
        close(d("az=0,el=90"), d("top"), "top");
        close(d("az=0,el=-90"), d("bottom"), "bottom");
        close(d("az=45,el=29.4964"), d("iso"), "iso");
        close(d("az=45,el=-29.4964"), d("iso-bottom"), "iso-bottom");
        // One axis on its own leaves the other at zero.
        close(d("el=-30"), d("az=0,el=-30"), "elevation alone");
        close(d("az=30"), d("az=30,el=0"), "azimuth alone");
        // Whitespace and long spellings, because these come off a command line.
        close(d(" azimuth = 45 , elevation = 30 "), d("az=45,el=30"), "spelled out");
        // Every direction is a unit vector; `preset` divides by nothing else.
        for s in ["iso", "top", "iso-bottom", "az=17,el=-71"] {
            assert!((d(s).length() - 1.0).abs() < 1e-9, "{s} is not unit length");
        }
    }

    /// The product presets are a photograph's camera, not a drawing's: a
    /// three-quarter view near eye level on a longer lens, with more air
    /// around the part and the part sitting a little below the middle of
    /// the frame. Everything the engineering views do stays at 35°, 0.15
    /// and centred.
    #[test]
    fn product_presets_frame_like_a_catalogue_shot() {
        let d = |s: &str| Camera::preset_direction(s).unwrap_or_else(|| panic!("{s} is not a view"));
        let close = |a: Vec3, b: Vec3, what: &str| assert!((a - b).length() < 2e-3, "{what}: {a:?} vs {b:?}");
        close(d("product"), d("az=30,el=18"), "product");
        close(d("hero"), d("az=35,el=12"), "hero");
        assert_eq!((Camera::preset_fov("product"), Camera::preset_fov("hero"), Camera::preset_fov("iso")), (24.0, 18.0, 35.0));
        assert_eq!((Camera::preset_margin("product"), Camera::preset_margin("hero"), Camera::preset_margin("front")), (0.25, 0.30, 0.15));
        assert_eq!((Camera::preset_subject_height("product"), Camera::preset_subject_height("iso")), (0.45, 0.5));
        let (lo, hi) = (Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 20.0));
        let cam = Camera::preset("product", lo, hi, 0.25);
        assert_eq!(cam.fov_degrees, 24.0, "the preset carries its own lens");
        assert_eq!(cam.up, Vec3::Z);
        // Placing the subject at 45 % of the frame height moves the camera
        // (eye and target together, so the direction is unchanged) up the
        // frame by 5 % of its height at the target distance, and 50 % is a
        // no-op.
        let mut placed = cam.clone();
        placed.place_subject(0.5);
        assert_eq!((placed.eye, placed.target), (cam.eye, cam.target));
        placed.place_subject(0.45);
        let dist = (cam.target - cam.eye).length();
        let frame_h = 2.0 * dist * (12.0f64).to_radians().tan();
        let shift = placed.target - cam.target;
        assert!((shift.length() - 0.05 * frame_h).abs() < 1e-6, "shift {} for a frame of {frame_h}", shift.length());
        assert!(shift.z > 0.0, "the target goes up, so the subject sits lower in the frame");
        assert!(((placed.target - placed.eye) - (cam.target - cam.eye)).length() < 1e-9, "the view direction is unchanged");
        // Looking at the old target through the placed camera lands it
        // below the centre line.
        let v = View::new(&placed, 400, 300);
        let sy = v.project([cam.target.x as f32, cam.target.y as f32, cam.target.z as f32]).unwrap()[1];
        assert!((sy - 300.0 * 0.55).abs() < 1.0, "the subject centre is at row {sy} of 300, wanted 165");
    }

    /// The thin lens: an f-number sizes a real aperture for the field of
    /// view, and every ray through it converges on the pinhole ray's point
    /// at the focus distance.
    #[test]
    fn thin_lens_rays_converge_on_the_focus_plane() {
        let cam = Camera { eye: Vec3::new(0.0, -100.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 24.0, orthographic: false };
        let v = View::new(&cam, 200, 150);
        assert!(!v.has_dof() && !v.with_dof(0.0).has_dof(), "no f-number, no lens");
        let lens = v.with_dof(4.0);
        // 24 mm frame height at 24° is a 56 mm lens; f/4 opens it to 7 mm.
        let f = 12.0 / (12.0f32).to_radians().tan();
        assert!((lens.lens_radius - f / 8.0).abs() < 1e-3, "aperture radius {} for f = {f}", lens.lens_radius);
        let (o0, d0) = lens.ray(120.0, 60.0);
        let focus = add(o0, scale(d0, lens.focus / dot(d0, lens.fwd)));
        for (u1, u2) in [(0.1f32, 0.9f32), (0.5, 0.5), (0.99, 0.01), (0.3, 0.3)] {
            let (o, d) = lens.ray_dof(120.0, 60.0, u1, u2);
            assert!((sub(o, o0).iter().map(|x| x * x).sum::<f32>()).sqrt() <= lens.lens_radius + 1e-4, "origin off the aperture");
            let t = lens.focus / dot(d, lens.fwd);
            let p = add(o, scale(d, t));
            assert!(sub(p, focus).iter().all(|x| x.abs() < 1e-2), "ray through ({u1},{u2}) misses the focus point: {p:?} vs {focus:?}");
        }
        // The camera's azimuth is what the rig turns with: the front view
        // is 0, and `iso` is 45°.
        let front = View::new(&Camera::preset("front", Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 0.1), 10, 10);
        assert!(front.azimuth().abs() < 1e-6, "front azimuth {}", front.azimuth());
        let iso = View::new(&Camera::preset("iso", Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 0.1), 10, 10);
        assert!((iso.azimuth().to_degrees() - 45.0).abs() < 1e-3, "iso azimuth {}", iso.azimuth().to_degrees());
    }

    /// A view string that is neither a name nor an angle pair must not be
    /// mistaken for one; `preset` then falls back to iso.
    #[test]
    fn nonsense_view_specs_are_not_angles() {
        for s in ["nope", "az=", "az=x", "az=45,zz=1", "45,30", "az 45"] {
            assert!(Camera::preset_direction(s).is_none(), "{s:?} parsed as a view");
        }
        let (lo, hi) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        let fallback = Camera::preset("nope", lo, hi, 0.1);
        let iso = Camera::preset("iso", lo, hi, 0.1);
        assert!((fallback.eye - iso.eye).length() < 1e-9);
    }

    fn box_scene() -> (Mesh, Vec<Vec<Vec3>>, Camera) {
        let (mesh, edges) = l_block();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        (mesh, edges, cam)
    }

    #[test]
    fn renders_a_box_with_shading_and_edges() {
        let (mesh, edges, cam) = box_scene();
        let mat = Material::default();
        let img = render(
            &[Item::new(&mesh, &mat, &edges)],
            &cam,
            &RenderOptions { width: 160, height: 120, ..Default::default() },
        );
        assert_eq!(img.rgba.len(), 160 * 120 * 4);
        // Something other than the background should have been drawn near the center.
        let bg = Image::new(1, 1, RenderOptions::default().background).rgba;
        let center = ((60 * 160 + 80) * 4) as usize;
        assert_ne!(&img.rgba[center..center + 3], &bg[..3]);
        let png = img.to_png();
        assert_eq!(&png[1..4], b"PNG");
    }

    /// A mesh's per-vertex colour shows in both backends, blended
    /// across each triangle and multiplied by the material's base colour.
    /// A quad facing the front camera, red down its left edge and blue down
    /// its right.
    #[test]
    fn per_vertex_colour_shows_in_the_raster_and_the_path_tracer() {
        let mut mesh = super::testing::quad([-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]);
        mesh.colors = vec![255, 0, 0, 0, 0, 255, 0, 0, 255, 255, 0, 0];
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("front", lo, hi, 0.1);
        let white = Material { base_color: [1.0, 1.0, 1.0], roughness: 1.0, ..Material::default() };
        let rgb = |img: &Image, fx: f32| -> [u8; 3] {
            let (x, y) = ((img.width as f32 * fx) as u32, img.height / 2);
            let i = ((y * img.width + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        for backend in [Backend::Raster, Backend::Path] {
            let opts = RenderOptions { width: 120, height: 90, backend, samples: 4, draw_edges: false, draw_grid: false, ..Default::default() };
            let img = render(&[Item::new(&mesh, &white, &[])], &cam, &opts);
            let (left, right, mid) = (rgb(&img, 0.3), rgb(&img, 0.7), rgb(&img, 0.5));
            assert!(left[0] > left[2] + 60, "{backend:?} left: {left:?}");
            assert!(right[2] > right[0] + 60, "{backend:?} right: {right:?}");
            // Blended, not a step: the middle carries both.
            assert!(mid[0] > 20 && mid[2] > 20, "{backend:?} middle: {mid:?}");
            // The base colour multiplies: a green material over red and blue
            // leaves almost nothing.
            let green = Material { base_color: [0.0, 1.0, 0.0], roughness: 1.0, ..Material::default() };
            let dark = render(&[Item::new(&mesh, &green, &[])], &cam, &opts);
            let l = rgb(&dark, 0.3);
            assert!(l[0] < left[0] / 2, "{backend:?}: base colour times vertex colour: {l:?} vs {left:?}");
        }
        // Colours that do not match the vertices are no colours at all.
        let mut stale = mesh.clone();
        stale.colors.truncate(6);
        assert!(stale.vertex_colors().is_none());
        assert!(super::vertex_albedo(&stale, 0, [0.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn contact_sheet_lays_out_tiles() {
        let t = Image::new(10, 8, [0.0, 0.0, 0.0]);
        let sheet = contact_sheet(&[("a".into(), t.clone()), ("b".into(), t)], 2, [1.0, 1.0, 1.0]);
        assert_eq!(sheet.width, 2 * 16 + 6);
        assert_eq!(sheet.height, 8 + 16 + 6 + 6);
    }

    #[test]
    fn same_input_twice_is_byte_identical() {
        let (mesh, edges, cam) = box_scene();
        let mat = Material::default();
        let opts = RenderOptions { width: 200, height: 150, ..Default::default() };
        let a = render(&[Item::new(&mesh, &mat, &edges)], &cam, &opts);
        let b = render(&[Item::new(&mesh, &mat, &edges)], &cam, &opts);
        assert_eq!(fnv1a64(&a.rgba), fnv1a64(&b.rgba));
    }

    #[test]
    fn thread_count_does_not_change_output() {
        let (mesh, edges, cam) = box_scene();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        let one = render(&items, &cam, &RenderOptions { width: 200, height: 150, threads: 1, ..Default::default() });
        let eight = render(&items, &cam, &RenderOptions { width: 200, height: 150, threads: 8, ..Default::default() });
        assert_eq!(fnv1a64(&one.rgba), fnv1a64(&eight.rgba));
    }

    /// The shading differs from a plain Lambert + Blinn rasterizer on
    /// purpose; the *geometry* must not: the same pixels are covered by the
    /// model, and the model is clearly separated from the background.
    #[test]
    fn samples_1_covers_the_same_pixels_as_the_reference_rasterizer() {
        let (mesh, edges) = planter_shell();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let mat = Material { base_color: [0.82, 0.55, 0.42], roughness: 0.85, ..Material::default() };
        let items = [Item::new(&mesh, &mat, &edges)];
        // Rasterization only: the overlay, ambient occlusion and the contact
        // shadow are not part of the reference.
        let opts = RenderOptions {
            width: 320,
            height: 240,
            samples: 1,
            draw_grid: false,
            draw_edges: false,
            ambient_occlusion: false,
            contact_shadow: false,
            ..Default::default()
        };
        let new = render(&items, &cam, &opts);
        let old = reference::render(&items, &cam, &opts);
        assert_eq!(new.rgba.len(), old.rgba.len());
        let bg = opts.background.map(to_srgb8);
        let covered = |img: &Image, i: usize| img.rgba[i * 4..i * 4 + 3] != bg;
        let n = (opts.width * opts.height) as usize;
        let (mut differ, mut covered_n) = (0usize, 0usize);
        for i in 0..n {
            covered_n += covered(&new, i) as usize;
            differ += (covered(&new, i) != covered(&old, i)) as usize;
        }
        assert!(covered_n > n / 10, "the model should cover the frame, got {covered_n}/{n}");
        assert!(differ * 100 < n, "silhouette moved: {differ} of {n} pixels disagree");
    }

    #[test]
    fn corner_pixel_is_exactly_background_for_every_sample_count() {
        let (mesh, edges, cam) = box_scene();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        for samples in [1, 4, 8] {
            let opts = RenderOptions { width: 160, height: 120, samples, draw_grid: false, ..Default::default() };
            let img = render(&items, &cam, &opts);
            let bg = opts.background.map(to_srgb8);
            assert_eq!(&img.rgba[0..3], &bg, "samples={samples}");
        }
    }

    #[test]
    fn view_unproject_inverts_project() {
        let (mesh, _, cam) = box_scene();
        let mut ortho_cam = cam.clone();
        ortho_cam.orthographic = true;
        let _ = mesh;
        for cam in [&cam, &ortho_cam] {
            for jitter in [[0.5f32, 0.5], [0.125, 0.875]] {
                let view = View::new(cam, 320, 240).with_jitter(jitter[0], jitter[1]);
                let mut checked = 0;
                for i in 0..1000u32 {
                    let h = |k: u32| (textures::hash3(i as i32, k as i32, 7, 1) % 10_000) as f32 / 100.0 - 50.0;
                    let p = [h(0), h(1), h(2)];
                    let Some(s) = view.project(p) else { continue };
                    let q = view.unproject(s[0], s[1], s[2]);
                    let err = dot(sub(p, q), sub(p, q)).sqrt();
                    assert!(err < 1e-3, "p={p:?} q={q:?} err={err}");
                    checked += 1;
                }
                assert!(checked > 500);
            }
        }
    }
}
