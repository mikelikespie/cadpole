//! Daylight: the colour of the sun after the atmosphere, colour
//! temperatures, and an analytic clear sky driven by the sun.
//!
//! Every value here is scene-linear RGB in the renderers' units, where the
//! sun's irradiance at 60° elevation is [`super::SunLight::IRRADIANCE`]
//! (about 100 klx of daylight). The sky is converted at the same rate, so
//! the sun, the sky and the shadows the sky fills keep the ratios a camera
//! sees outdoors: on a clear day at 45° the open sky delivers roughly a
//! fifth to a quarter of what the sun does.
//!
//! The sky is the Preetham, Shirley and Smits analytic model (*A Practical
//! Analytic Model for Daylight*, SIGGRAPH 1999): a Perez luminance
//! distribution for luminance and both chromaticities, scaled by the
//! zenith values the paper fits to turbidity and sun elevation. The model is
//! pale and drifts toward mauve and red where the sun is low, so a few
//! corrections are layered on it, each keeping the luminance it gave: the
//! colour is deepened, the horizon of a high sun is a pale haze, ozone's
//! absorption along a low sun's slant path keeps the sky blue, the sky near
//! a low sun and along the horizon under it takes the colour of the
//! reddened sunlight it scatters, the rest of an evening sky stays blue with
//! a rose band opposite the sun, and once the sun is below the horizon the
//! sky keeps the shape it had at sunset and fades by an e-fold per two
//! degrees, which carries a scene through civil twilight.

use super::{dot, lerp3, luminance, normalize, scale};

/// Zenith optical depth per channel (R, G, B at 680, 550 and 440 nm):
/// Rayleigh scattering plus a clear-day aerosol load (Ångström β 0.04,
/// α 1.3). What turns the sun yellow, then orange, then red as its path
/// through the air lengthens.
const TAU: [f32; 3] = [0.106, 0.184, 0.358];

/// The elevation at which the sun's colour and strength are exactly
/// [`super::SunLight::IRRADIANCE`]: higher is a little bluer and brighter,
/// lower warmer and dimmer.
pub const REFERENCE_ELEVATION_DEGREES: f32 = 60.0;

/// Scene units per kcd/m² of sky luminance: the sun's reference
/// irradiance stands for 100 klx, so one klx is this much irradiance.
const SKY_UNITS_PER_KCD: f32 = 0.033;

/// The atmosphere's turbidity for the sky: 2 is a very clear day, 3 clear
/// (the default), 6 hazy, 10 a polluted summer afternoon.
pub const DEFAULT_TURBIDITY: f32 = 3.0;

/// Relative air mass along a path at `el_deg` above the horizon (Kasten and
/// Young 1989): 1 at the zenith, about 38 at the horizon.
pub fn air_mass(el_deg: f32) -> f32 {
    let el = el_deg.max(-0.5);
    1.0 / (el.to_radians().sin().max(0.0) + 0.50572 * (el + 6.07995).max(0.1).powf(-1.6364))
}

/// Transmittance of the atmosphere to the sun at `el_deg`, relative to the
/// sun at [`REFERENCE_ELEVATION_DEGREES`].
pub fn sun_transmittance(el_deg: f32) -> [f32; 3] {
    let dm = air_mass(el_deg) - air_mass(REFERENCE_ELEVATION_DEGREES);
    [(-TAU[0] * dm).exp(), (-TAU[1] * dm).exp(), (-TAU[2] * dm).exp()]
}

/// CIE xyY to linear sRGB (D65 white).
fn xyy_to_rgb(x: f32, y: f32, lum: f32) -> [f32; 3] {
    let y = y.max(1e-4);
    let (cx, cz) = (x / y * lum, (1.0 - x - y) / y * lum);
    [
        3.2406 * cx - 1.5372 * lum - 0.4986 * cz,
        -0.9689 * cx + 1.8758 * lum + 0.0415 * cz,
        0.0557 * cx - 0.2040 * lum + 1.0570 * cz,
    ]
}

/// A black body's colour at `kelvin` (clamped to 1667..25000 K, where the
/// Planckian-locus fit of Kim et al. holds) as linear sRGB of unit
/// luminance. 6500 K is very nearly white; 3200 K is tungsten, 2700 K a
/// warm lamp, 1900 K a candle.
pub fn kelvin_rgb(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(1667.0, 25000.0);
    let (t2, t3) = (t * t, t * t * t);
    let x = if t <= 4000.0 {
        -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if t <= 2222.0 {
        -1.1063814 * x3 - 1.34811020 * x2 + 2.18555832 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x3 - 1.37418593 * x2 + 2.09137015 * x - 0.16748867
    } else {
        3.0817580 * x3 - 5.87338670 * x2 + 3.75112997 * x - 0.37001483
    };
    unit_luminance(xyy_to_rgb(x, y, 1.0))
}

/// `c` with negative channels dropped and its luminance scaled to 1.
pub fn unit_luminance(c: [f32; 3]) -> [f32; 3] {
    let c = c.map(|v| v.max(0.0));
    let l = luminance(c);
    if l > 1e-6 { scale(c, 1.0 / l) } else { [1.0; 3] }
}

/// Ozone's Chappuis-band optical depth per channel at the zenith: small
/// against Rayleigh's by day, it is what keeps a low sun's sky blue rather
/// than grey, because the light that reaches the zenith at sunset has
/// crossed the ozone layer along the sun's long slant path.
const TAU_OZONE: [f32; 3] = [0.016, 0.035, 0.002];

/// How much the model's chroma is deepened ([`Sky::radiance`]).
const SATURATION: f32 = 1.45;

/// The blue of a clear evening sky away from the sun, unit luminance.
const EVENING_BLUE: [f32; 3] = [0.62, 1.0, 1.95];

/// The rose of the anti-twilight arch, unit luminance.
const BELT_ROSE: [f32; 3] = [1.22, 0.92, 0.98];

/// The colour of the clear-sky haze at the horizon, unit luminance: a
/// faintly blue white.
const HORIZON_WHITE: [f32; 3] = [0.94, 1.0, 1.10];

/// The exposure a photographer would give a daylight scene lit by a sun
/// along `sun_dir`, relative to one at [`REFERENCE_ELEVATION_DEGREES`]: as
/// the sun sinks and reddens the sun and the sky both dim, and a camera
/// opens up to keep the picture a picture. Partial (the 0.6 power), as an
/// auto-exposure meter with a bias for mood is: a golden-hour scene stays
/// darker and warmer than noon, a dusk scene darker still. 1 at and above
/// the reference; at most 8.
pub fn daylight_exposure(sun_dir: [f32; 3]) -> f32 {
    let el = normalize(sun_dir)[2].clamp(-1.0, 1.0).asin().to_degrees();
    let fade = if el < 0.5 { ((el - 0.5) / 2.0).exp() } else { 1.0 };
    let light = luminance(sun_transmittance(el.max(0.5))) * fade;
    (1.0 / light.max(1e-6)).max(1.0).powf(0.6).min(8.0)
}

/// Perez et al.'s all-weather distribution: relative brightness at zenith
/// angle `theta` (as its cosine) and angle `gamma` from the sun.
#[inline]
fn perez(k: &[f32; 5], cos_theta: f32, gamma: f32, cos_gamma: f32) -> f32 {
    (1.0 + k[0] * (k[1] / cos_theta.max(0.01)).exp()) * (1.0 + k[2] * (k[3] * gamma).exp() + k[4] * cos_gamma * cos_gamma)
}

/// The analytic clear sky for one sun. Cheap to evaluate (two `exp`s and an
/// `acos` per channel set), so the path tracer calls it for every ray that
/// leaves the scene.
#[derive(Clone, Copy, Debug)]
pub struct Sky {
    /// Toward the sun, unit; may be below the horizon.
    sun: [f32; 3],
    coef: [[f32; 5]; 3],
    /// Zenith (Y, x, y) over `F(0, θs)`, per component.
    norm: [f32; 3],
    /// Radiance units per kcd/m², with twilight's fade folded in.
    units: f32,
    /// The reddened sunlight's colour, unit luminance, and how much of the
    /// sky near the sun and the horizon takes it (0 at a high sun).
    glow: [f32; 3],
    glow_weight: f32,
    /// Unit-luminance tint of the ozone the sunlight crossed: white by day,
    /// blue at sunset and in twilight.
    ozone: [f32; 3],
    /// Radiance of the ground below the horizon.
    ground: [f32; 3],
}

impl Sky {
    /// The sky for a sun along `sun_dir` (toward the sun) under `turbidity`.
    pub fn new(sun_dir: [f32; 3], turbidity: f32) -> Sky {
        let sun = normalize(sun_dir);
        let t = turbidity.clamp(1.7, 10.0);
        let el = sun[2].clamp(-1.0, 1.0).asin().to_degrees();
        // The model is fitted for a sun above the horizon; below it the sky
        // keeps the shape it had at sunset and fades.
        let el_model = el.max(0.5);
        let theta_s = (90.0 - el_model).to_radians();
        let coef = [
            [0.1787 * t - 1.4630, -0.3554 * t + 0.4275, -0.0227 * t + 5.3251, 0.1206 * t - 2.5771, -0.0670 * t + 0.3703],
            [-0.0193 * t - 0.2592, -0.0665 * t + 0.0008, -0.0004 * t + 0.2125, -0.0641 * t - 0.8989, -0.0033 * t + 0.0452],
            [-0.0167 * t - 0.2608, -0.0950 * t + 0.0092, -0.0079 * t + 0.2102, -0.0441 * t - 1.6537, -0.0109 * t + 0.0529],
        ];
        let chi = (4.0 / 9.0 - t / 120.0) * (std::f32::consts::PI - 2.0 * theta_s);
        let yz = ((4.0453 * t - 4.9710) * chi.tan() - 0.2155 * t + 2.4192).max(0.0);
        let th = [theta_s * theta_s * theta_s, theta_s * theta_s, theta_s, 1.0];
        let poly = |m: [[f32; 4]; 3]| {
            let row = |r: [f32; 4]| r[0] * th[0] + r[1] * th[1] + r[2] * th[2] + r[3] * th[3];
            t * t * row(m[0]) + t * row(m[1]) + row(m[2])
        };
        let xz = poly([[0.00166, -0.00375, 0.00209, 0.0], [-0.02903, 0.06377, -0.03202, 0.00394], [0.11693, -0.21196, 0.06052, 0.25886]]);
        let yz_c = poly([[0.00275, -0.00610, 0.00317, 0.0], [-0.04214, 0.08970, -0.04153, 0.00516], [0.15346, -0.26756, 0.06670, 0.26688]]);
        let zen = [yz, xz, yz_c];
        let mut norm = [0.0f32; 3];
        for k in 0..3 {
            let f0 = perez(&coef[k], 1.0, theta_s, theta_s.cos());
            norm[k] = zen[k] / f0.max(1e-6);
        }
        let fade = if el < 0.5 { ((el - 0.5) / 2.0).exp() } else { 1.0 };
        let glow = unit_luminance(sun_transmittance(el_model));
        // Strong below 10°, gone by 30°.
        let glow_weight = 1.0 - smooth01((el_model - 4.0) / 26.0);
        let dm = (air_mass(el_model) - air_mass(REFERENCE_ELEVATION_DEGREES)).max(0.0);
        let ozone = unit_luminance([(-TAU_OZONE[0] * dm).exp(), (-TAU_OZONE[1] * dm).exp(), (-TAU_OZONE[2] * dm).exp()]);
        let mut sky = Sky { sun, coef, norm, units: SKY_UNITS_PER_KCD * fade, glow, glow_weight, ozone, ground: [0.0; 3] };
        // The ground below the horizon: a mid-dark earth lit by the sun
        // and this sky, so a reflection or a view past the floor's edge
        // finds land rather than a hole.
        let lit = sky.irradiance_up() + super::SunLight::IRRADIANCE[1] * luminance(sun_transmittance(el)) * sun[2].max(0.0) * fade;
        sky.ground = scale([0.20, 0.18, 0.15], lit / std::f32::consts::PI);
        sky
    }

    /// The direction toward the sun this sky was built for.
    pub fn sun(&self) -> [f32; 3] {
        self.sun
    }

    /// Radiance seen along `dir` (unit).
    pub fn radiance(&self, dir: [f32; 3]) -> [f32; 3] {
        let z = dir[2];
        if z >= 0.0 {
            return self.above(dir);
        }
        // Below the horizon: the horizon's own colour fading quickly into
        // the ground, so there is no seam at z = 0.
        let h = self.above(normalize([dir[0], dir[1], 0.0]));
        lerp3(h, self.ground, smooth01(-z / 0.08))
    }

    fn above(&self, dir: [f32; 3]) -> [f32; 3] {
        let cos_t = dir[2].max(0.0);
        let cos_g = dot(dir, self.sun).clamp(-1.0, 1.0);
        let g = cos_g.acos();
        let f = |k: usize| self.norm[k] * perez(&self.coef[k], cos_t, g, cos_g);
        let (lum, x, y) = (f(0).max(0.0), f(1), f(2));
        let rgb = xyy_to_rgb(x, y, lum).map(|v| v.max(0.0));
        let rgb = super::mul(scale(rgb, self.units), self.ozone);
        // The model's colours are paler than a clear sky photographs:
        // deepen them about their own luminance.
        let l0 = luminance(rgb);
        let rgb = lerp3([l0; 3], rgb, SATURATION).map(|v| v.max(0.0));
        // The model's chromaticity runs to mauve along the horizon, where a
        // real clear sky is a pale, nearly white haze: keep the luminance
        // and pull the colour toward that white within a few degrees of
        // the horizon, except under a low sun, whose own glow (below) is
        // the horizon's colour there.
        let half = 0.5 + 0.5 * cos_g;
        let away = 1.0 - half * half;
        let low = (-cos_t / 0.15).exp();
        let l = luminance(rgb);
        let haze_w = low * 0.8 * (1.0 - self.glow_weight * (1.0 - away));
        let rgb = if haze_w > 1e-3 { lerp3(rgb, scale(HORIZON_WHITE, l), haze_w) } else { rgb };
        // And under a low sun the model drifts toward red across the whole
        // sky; away from the sun a clear evening sky stays blue.
        let blue_w = self.glow_weight * 0.65 * away * (1.0 - low);
        let rgb = if blue_w > 1e-3 { lerp3(rgb, scale(EVENING_BLUE, l), blue_w) } else { rgb };
        if self.glow_weight <= 0.0 {
            return rgb;
        }
        // The low sun's colour, near it and along the horizon, keeping the
        // luminance the model gave.
        let near = (-(g * g) / (0.55 * 0.55)).exp();
        // Strongest under the sun, faint opposite it, where the horizon
        // stays blue-grey.
        let band = (-cos_t / 0.12).exp() * (0.08 + 0.92 * half * half);
        let w = (self.glow_weight * (near + 0.8 * band)).min(1.0);
        let rgb = lerp3(rgb, scale(self.glow, l), w);
        // Opposite a low sun, a few degrees up: the rose band over the
        // earth's shadow that the reddened light paints on the haze.
        let anti = 0.5 - 0.5 * cos_g;
        let belt = ((cos_t - 0.14) / 0.09).powi(2);
        let wb = self.glow_weight * 0.3 * anti * anti * (-belt).exp();
        lerp3(rgb, scale(BELT_ROSE, l), wb)
    }

    /// Irradiance the sky (not the sun) delivers to an up-facing plane.
    pub fn irradiance_up(&self) -> f32 {
        let (nt, np) = (24usize, 48usize);
        let mut e = 0.0f32;
        for i in 0..nt {
            // Uniform in z = cos θ: dω = dz dφ.
            let z = (i as f32 + 0.5) / nt as f32;
            let r = (1.0 - z * z).max(0.0).sqrt();
            for j in 0..np {
                let phi = (j as f32 + 0.5) / np as f32 * std::f32::consts::TAU;
                let d = [r * phi.cos(), r * phi.sin(), z];
                e += luminance(self.above(d)) * z;
            }
        }
        e * (1.0 / nt as f32) * (std::f32::consts::TAU / np as f32)
    }

    /// `(sky, horizon, ground)` for a three-stop gradient that stands in for
    /// this sky where only a gradient will do (the rasterizer's diffuse
    /// ambient): the cosine-weighted mean of the upper hemisphere, the
    /// horizon averaged round the compass, and the ground.
    pub fn gradient(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let n = 48usize;
        let mut horizon = [0.0f32; 3];
        let mut upper = [0.0f32; 3];
        let mut wsum = 0.0f32;
        for j in 0..n {
            let phi = (j as f32 + 0.5) / n as f32 * std::f32::consts::TAU;
            let h = self.above(normalize([phi.cos(), phi.sin(), 0.04]));
            horizon = super::add(horizon, scale(h, 1.0 / n as f32));
            for z in [0.2f32, 0.5, 0.8, 0.97] {
                let r = (1.0 - z * z).sqrt();
                upper = super::add(upper, scale(self.above([r * phi.cos(), r * phi.sin(), z]), z));
                wsum += z;
            }
        }
        (scale(upper, 1.0 / wsum.max(1e-6)), horizon, self.ground)
    }
}

#[inline]
fn smooth01(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(az_deg: f32, el_deg: f32) -> [f32; 3] {
        let (a, e) = (az_deg.to_radians(), el_deg.to_radians());
        [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()]
    }

    fn warmth(c: [f32; 3]) -> f32 {
        c[0] / c[2].max(1e-6)
    }

    #[test]
    fn the_sun_reddens_and_dims_as_it_sets() {
        let mut last = sun_transmittance(90.0);
        assert!((sun_transmittance(REFERENCE_ELEVATION_DEGREES)[1] - 1.0).abs() < 1e-6);
        for el in [60.0, 30.0, 15.0, 8.0, 4.0, 1.0] {
            let t = sun_transmittance(el);
            assert!(warmth(t) > warmth(last), "warmer at {el}: {t:?} vs {last:?}");
            assert!(luminance(t) < luminance(last), "dimmer at {el}");
            last = t;
        }
        let t = sun_transmittance(5.0);
        assert!(t[0] > 2.0 * t[1] && t[1] > 2.0 * t[2], "a 5° sun is orange: {t:?}");
    }

    #[test]
    fn kelvin_runs_from_red_through_white_to_blue() {
        let candle = kelvin_rgb(1900.0);
        let tungsten = kelvin_rgb(3200.0);
        let d65 = kelvin_rgb(6504.0);
        let blue = kelvin_rgb(12000.0);
        assert!(warmth(candle) > warmth(tungsten) && warmth(tungsten) > warmth(d65) && warmth(d65) > warmth(blue));
        assert!(d65.iter().all(|v| (v - 1.0).abs() < 0.08), "6500 K is white: {d65:?}");
        for c in [candle, tungsten, d65, blue] {
            assert!((luminance(c) - 1.0).abs() < 1e-3);
        }
    }

    /// The sky's shape: bluer overhead than at the horizon, brighter toward
    /// the sun than away from it, and warmer toward a low sun than away.
    #[test]
    fn the_sky_brightens_and_warms_toward_the_sun() {
        let high = Sky::new(dir(180.0, 50.0), DEFAULT_TURBIDITY);
        let zenith = high.radiance([0.0, 0.0, 1.0]);
        assert!(zenith[2] > zenith[0] * 1.3, "the zenith is blue: {zenith:?}");
        let toward = high.radiance(dir(180.0, 25.0));
        let away = high.radiance(dir(0.0, 25.0));
        assert!(luminance(toward) > 1.5 * luminance(away), "toward {toward:?} away {away:?}");

        let low = Sky::new(dir(270.0, 4.0), DEFAULT_TURBIDITY);
        let toward = low.radiance(dir(270.0, 3.0));
        let away = low.radiance(dir(90.0, 3.0));
        assert!(warmth(toward) > 1.5 * warmth(away), "toward a low sun is warm: {toward:?} away {away:?}");
        assert!(warmth(toward) > 1.0, "the sunset glow is orange: {toward:?}");
    }

    /// Ambient light: a clear sky delivers a fifth to a half of what the
    /// sun does to the ground at 45°, which is what keeps a shadow readable
    /// without flattening it.
    #[test]
    fn the_sky_fills_shadows_in_proportion_to_the_sun() {
        let sky = Sky::new(dir(180.0, 45.0), DEFAULT_TURBIDITY);
        let e_sky = sky.irradiance_up();
        let e_sun = luminance(super::super::SunLight::IRRADIANCE) * luminance(sun_transmittance(45.0)) * 45f32.to_radians().sin();
        let ratio = e_sky / e_sun;
        assert!((0.15..0.6).contains(&ratio), "sky {e_sky} against sun {e_sun}: {ratio}");
    }

    #[test]
    fn twilight_fades_and_the_ground_has_no_seam() {
        let dusk = Sky::new(dir(270.0, -3.0), DEFAULT_TURBIDITY);
        let day = Sky::new(dir(270.0, 0.5), DEFAULT_TURBIDITY);
        let z = [0.0, 0.0, 1.0];
        assert!(luminance(dusk.radiance(z)) < 0.4 * luminance(day.radiance(z)));
        assert!(luminance(dusk.radiance(z)) > 0.0);
        let s = Sky::new(dir(200.0, 30.0), DEFAULT_TURBIDITY);
        let above = s.radiance(normalize([1.0, 0.0, 1e-4]));
        let below = s.radiance(normalize([1.0, 0.0, -1e-4]));
        assert!((luminance(above) - luminance(below)).abs() < 0.02 * luminance(above).max(1e-3));
        for d in [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]] {
            assert!(s.radiance(d).iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }
}
