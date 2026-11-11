//! Animations: a script declares `animation(name, keyframes, ...)`,
//! a list of keyframes that each give a time in seconds, the values some of
//! the document's parameters take at that time and, optionally, a camera.
//! Nothing here touches geometry: a declaration is data, and the engine
//! evaluates the document once per frame with the values this module
//! interpolates, through one memo, so a pose that only moves instances
//! costs no kernel work after the first frame.
//!
//! **The hold rule.** A keyframe's *state* is the previous keyframe's state
//! with that keyframe's values written over it, so a parameter a keyframe
//! does not name holds its previous value there; before its first mention
//! a parameter is the document's own value (the declared default, or the
//! override the document carries). Between two keyframes every parameter
//! eases from the earlier state to the later one — numbers and vectors by
//! the segment's easing, a bool or a string switching at the later
//! keyframe. Before the first keyframe everything is the first state, after
//! the last everything is the last. A camera keyframe (`view(az=, el=,
//! fov=, zoom=, pan=)`) follows the same rule field by field, azimuth
//! interpolating the shortest way round.
//!
//! **Easing** is one of four curves (`ease.linear`, `.smooth`, `.ease_in`,
//! `.ease_out` — `in` is a Starlark keyword, so the two one-sided curves
//! carry the prefix), chosen for the whole animation and overridable per
//! keyframe for the segment that *starts* there — CSS's convention, which
//! is the one most people have met.
//!
//! Everything here is arithmetic on `f64`s — no transcendental — so the
//! frames a script asks for are the same on every target.

use crate::ParamValue;
use crate::Vec3;
use crate::context::Span;
use crate::params::ParamDecl;
use cadpole_proto::v1 as pb;

/// The shape of a segment between two keyframes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    /// Constant speed.
    Linear,
    /// Smoothstep: eases in and out, `3u² − 2u³`.
    Smooth,
    /// Starts slowly, `u²`.
    In,
    /// Ends slowly, `1 − (1 − u)²`.
    Out,
}

impl Ease {
    /// Every curve, in documentation order; the `ease` vocabulary
    /// and `parse` both read this list.
    pub const ALL: [Ease; 4] = [Ease::Linear, Ease::Smooth, Ease::In, Ease::Out];

    pub fn parse(s: &str) -> Option<Ease> {
        Self::ALL.into_iter().find(|e| e.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Ease::Linear => "linear",
            Ease::Smooth => "smooth",
            Ease::In => "ease_in",
            Ease::Out => "ease_out",
        }
    }

    /// The eased fraction for `u` in `0..=1`: 0 at 0 and 1 at 1 for every
    /// curve, so a keyframe's value is exact at its own time.
    pub fn apply(self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Ease::Linear => u,
            Ease::Smooth => u * u * (3.0 - 2.0 * u),
            Ease::In => u * u,
            Ease::Out => 1.0 - (1.0 - u) * (1.0 - u),
        }
    }
}

/// What a keyframe says about the camera: each field optional, so
/// `view(az=30)` turns the camera and leaves its elevation and lens alone.
/// `eye` and `target` are a free camera in model space — a
/// neighbour's window, a point on the sidewalk — and once a keyframe sets
/// the eye it stands there, easing point to point, whatever `az`/`el` say.
///
/// `zoom` and `pan` move a fitted camera without turning it. `zoom` is a
/// magnification: the camera stands at the sequence's fitted distance
/// divided by it, and the fit is made for the frame that needs the most
/// room, so the subject stays in the picture at every zoom (the engine's
/// `animate` says how). `pan` slides the frame by fractions of its width
/// and height: `(0.05, 0)` moves the camera right, so the subject drifts
/// left by a twentieth of the frame. Neither applies to a placed `eye`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraKey {
    pub az: Option<f64>,
    pub el: Option<f64>,
    pub fov: Option<f64>,
    pub eye: Option<Vec3>,
    pub target: Option<Vec3>,
    pub zoom: Option<f64>,
    pub pan: Option<(f64, f64)>,
}

/// The range a camera keyframe's `zoom` may take.
pub const ZOOM_RANGE: (f64, f64) = (0.1, 10.0);
/// The largest drift a camera keyframe's `pan` may ask for, per axis, as a
/// fraction of the frame.
pub const PAN_MAX: f64 = 0.45;

impl CameraKey {
    pub fn is_empty(&self) -> bool {
        self.az.is_none()
            && self.el.is_none()
            && self.fov.is_none()
            && self.eye.is_none()
            && self.target.is_none()
            && self.zoom.is_none()
            && self.pan.is_none()
    }

    fn over(&self, prev: CameraKey) -> CameraKey {
        CameraKey {
            az: self.az.or(prev.az),
            el: self.el.or(prev.el),
            fov: self.fov.or(prev.fov),
            eye: self.eye.or(prev.eye),
            target: self.target.or(prev.target),
            zoom: self.zoom.or(prev.zoom),
            pan: self.pan.or(prev.pan),
        }
    }
}

/// One `keyframe(t, ...)`.
#[derive(Clone, Debug)]
pub struct Keyframe {
    /// Seconds from the start of the animation.
    pub t: f64,
    /// The parameters this keyframe names, in the order written.
    pub params: Vec<(String, ParamValue)>,
    pub camera: Option<CameraKey>,
    /// The easing of the segment that starts here; `None` takes the
    /// animation's.
    pub ease: Option<Ease>,
    /// What the keyframe's `view(layers=)` says about the display layers:
    /// each entry holds from this keyframe on, its opacity easing
    /// to the next keyframe that names the layer and its visibility
    /// switching there, as a number and a bool do.
    pub layers: Vec<crate::layers::LayerOverride>,
}

/// One `animation(...)` declaration, as the script wrote it.
#[derive(Clone, Debug)]
pub struct AnimationDecl {
    pub name: String,
    /// In time order (`validate` refuses anything else).
    pub keyframes: Vec<Keyframe>,
    pub fps: f64,
    pub ease: Ease,
    /// The animation is meant to repeat: the frame after the last is the
    /// first, so the last instant is not rendered twice.
    pub looped: bool,
    pub span: Option<Span>,
}

/// The parameters and the camera at one instant.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// Every parameter the animation touches, with its value at this time,
    /// in first-mention order.
    pub params: Vec<(String, ParamValue)>,
    /// The camera at this time: every field the keyframes have set so far,
    /// `None` when no keyframe up to here says anything about the camera.
    pub camera: Option<CameraKey>,
    /// The display-layer overrides in force at this time: every
    /// layer a keyframe up to here named, with its opacity eased between
    /// the keyframes that name it. Empty when no keyframe names a layer.
    pub layers: Vec<crate::layers::LayerOverride>,
}

/// The largest frame count one animation may ask for, whatever `fps` and
/// the duration say: every frame is an evaluation and a render.
pub const MAX_FRAMES: usize = 4096;

impl AnimationDecl {
    /// The listing form: `Model.animations` and `AnimateResponse.animations`.
    pub fn to_proto(&self) -> pb::AnimationInfo {
        pb::AnimationInfo {
            name: self.name.clone(),
            duration_s: self.duration(),
            fps: self.fps,
            frames: self.frame_count(0.0) as u32,
            r#loop: self.looped,
            ease: self.ease.name().to_owned(),
            params: self.param_names().iter().map(|s| s.to_string()).collect(),
            keyframe_times: self.keyframes.iter().map(|k| k.t).collect(),
            has_camera: self.has_camera(),
            span: self.span.as_ref().map(Span::to_proto),
        }
    }

    /// The time of the last keyframe: frames run from 0 to here.
    pub fn duration(&self) -> f64 {
        self.keyframes.last().map(|k| k.t).unwrap_or(0.0)
    }

    /// Every parameter the keyframes name, each once, in first-mention order.
    pub fn param_names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for k in &self.keyframes {
            for (n, _) in &k.params {
                if !out.contains(&n.as_str()) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// True when some keyframe says something about the camera.
    pub fn has_camera(&self) -> bool {
        self.keyframes.iter().any(|k| k.camera.map(|c| !c.is_empty()).unwrap_or(false))
    }

    /// True when some keyframe's `view(layers=)` names a display layer.
    pub fn has_layers(&self) -> bool {
        self.keyframes.iter().any(|k| !k.layers.is_empty())
    }

    /// Every layer name the keyframes mention, each once, in first-mention
    /// order; the empty name is the implicit layer.
    pub fn layer_names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for k in &self.keyframes {
            for l in &k.layers {
                if !out.contains(&l.name.as_str()) {
                    out.push(&l.name);
                }
            }
        }
        out
    }

    /// The layer names the keyframes use against what the document
    /// declares: a name nothing declares is refused, keyframe named, so a
    /// misspelt layer in an animation is an error at the call rather than
    /// a frame that quietly shows everything.
    pub fn validate_layers(&self, declared: &[crate::layers::LayerDecl]) -> Result<(), String> {
        let who = format!("animation {:?}", self.name);
        for (i, k) in self.keyframes.iter().enumerate() {
            for l in &k.layers {
                if !l.name.is_empty() && !declared.iter().any(|d| d.name == l.name) {
                    let known: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
                    return Err(format!(
                        "{who}: keyframe {i} (t={}): layer {:?} is not declared (declared so far: {}); declare layers before animations",
                        crate::context::fmt_num(k.t),
                        l.name,
                        if known.is_empty() { "none".to_owned() } else { known.join(", ") }
                    ));
                }
                if let Some(o) = l.opacity {
                    if !(0.0..=1.0).contains(&o) || o.is_nan() {
                        return Err(format!(
                            "{who}: keyframe {i} (t={}): layer {:?}: an opacity is between 0 and 1, got {}",
                            crate::context::fmt_num(k.t),
                            l.name,
                            crate::context::fmt_num(o)
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// How many frames `fps` gives: one per `1/fps` from 0 to the duration,
    /// the end included unless the animation loops (then the frame after
    /// the last is the first). Never more than [`MAX_FRAMES`].
    pub fn frame_count(&self, fps: f64) -> usize {
        self.uncapped_frame_count(fps).min(MAX_FRAMES)
    }

    /// [`frame_count`](Self::frame_count) before the [`MAX_FRAMES`] cap:
    /// over the cap, the animation is resampled rather than cut short.
    pub fn uncapped_frame_count(&self, fps: f64) -> usize {
        let fps = if fps > 0.0 { fps } else { self.fps };
        let n = (self.duration() * fps).round().max(0.0) as usize;
        if self.looped { n.max(1) } else { n.saturating_add(1) }
    }

    /// The instants to render: `frames` of them if given (a resample), else
    /// [`frame_count`](Self::frame_count) at `fps` (0 = the declared rate).
    pub fn frame_times(&self, fps: f64, frames: usize) -> Vec<f64> {
        let d = self.duration();
        let n = if frames > 0 { frames.min(MAX_FRAMES) } else { self.frame_count(fps) };
        if n <= 1 {
            return vec![0.0];
        }
        // A looping animation's last frame stops one step short of the end,
        // which is where the first frame already is.
        let steps = if self.looped { n } else { n - 1 };
        (0..n).map(|i| d * i as f64 / steps as f64).collect()
    }

    /// The seconds one frame is shown for, at `fps` (0 = the declared rate)
    /// or, for a resample to `frames`, the duration divided out. A rate
    /// that would need more than [`MAX_FRAMES`] frames is that resample
    /// too: [`frame_times`](Self::frame_times) spreads the capped frames
    /// over the whole duration, so each one must be shown for longer than
    /// `1/fps` or the file plays fast.
    pub fn frame_seconds(&self, fps: f64, frames: usize) -> f64 {
        let frames = if frames == 0 && self.uncapped_frame_count(fps) > MAX_FRAMES { MAX_FRAMES } else { frames };
        if frames > 0 {
            let n = frames.min(MAX_FRAMES);
            let steps = if self.looped { n } else { n.saturating_sub(1) };
            if steps == 0 || self.duration() <= 0.0 {
                return 1.0 / if fps > 0.0 { fps } else { self.fps };
            }
            return self.duration() / steps as f64;
        }
        1.0 / if fps > 0.0 { fps } else { self.fps }
    }

    /// Refuse what cannot be animated: no keyframes, a time out of order or
    /// negative, a parameter the document does not declare, a value of the
    /// wrong kind or outside the parameter's `min`/`max`, a camera field
    /// out of its range, a rate of zero. The message names the keyframe
    /// (by index and time) so the span of the `animation(...)` call is
    /// enough to find it.
    pub fn validate(&self, params: &[(ParamDecl, ParamValue)]) -> Result<(), String> {
        let who = format!("animation {:?}", self.name);
        if self.name.trim().is_empty() {
            return Err("animation: the name is empty".to_owned());
        }
        if self.keyframes.is_empty() {
            return Err(format!("{who}: needs at least one keyframe"));
        }
        if !(self.fps > 0.0) || !self.fps.is_finite() {
            return Err(format!("{who}: fps must be a positive number, got {}", self.fps));
        }
        let mut prev: Option<f64> = None;
        for (i, k) in self.keyframes.iter().enumerate() {
            let at = format!("keyframe {i} (t={})", crate::context::fmt_num(k.t));
            if !k.t.is_finite() || k.t < 0.0 {
                return Err(format!("{who}: {at}: t must be a non-negative number of seconds"));
            }
            if let Some(p) = prev {
                if k.t <= p {
                    return Err(format!(
                        "{who}: {at}: keyframes must be in increasing time order, and the one before is at t={}",
                        crate::context::fmt_num(p)
                    ));
                }
            }
            prev = Some(k.t);
            for (name, value) in &k.params {
                let Some((decl, _)) = params.iter().find(|(d, _)| &d.name == name) else {
                    let known: Vec<&str> = params.iter().map(|(d, _)| d.name.as_str()).collect();
                    return Err(format!(
                        "{who}: {at}: {name:?} is not a param of this document (declared so far: {}); declare parameters before animations",
                        if known.is_empty() { "none".to_owned() } else { known.join(", ") }
                    ));
                };
                if value.kind_name() != decl.default.kind_name() {
                    return Err(format!(
                        "{who}: {at}: {name} is a {} but the keyframe gives it a {}",
                        decl.default.kind_name(),
                        value.kind_name()
                    ));
                }
                if let ParamValue::Number(n) = value {
                    if let Some(lo) = decl.min {
                        if *n < lo {
                            return Err(format!("{who}: {at}: {name}={} is below its min {}", crate::context::fmt_num(*n), crate::context::fmt_num(lo)));
                        }
                    }
                    if let Some(hi) = decl.max {
                        if *n > hi {
                            return Err(format!("{who}: {at}: {name}={} is above its max {}", crate::context::fmt_num(*n), crate::context::fmt_num(hi)));
                        }
                    }
                }
                if let ParamValue::Text(t) = value {
                    if !decl.choices.is_empty() && !decl.choices.contains(t) {
                        return Err(format!("{who}: {at}: {name}={t:?} is not one of its choices {:?}", decl.choices));
                    }
                }
            }
            if let Some(c) = k.camera {
                if let Some(el) = c.el {
                    if !(-90.0..=90.0).contains(&el) {
                        return Err(format!("{who}: {at}: camera el={} is outside -90..90", crate::context::fmt_num(el)));
                    }
                }
                if let Some(fov) = c.fov {
                    if !(1.0..=170.0).contains(&fov) {
                        return Err(format!("{who}: {at}: camera fov={} is outside 1..170", crate::context::fmt_num(fov)));
                    }
                }
                if let Some(z) = c.zoom {
                    if !(ZOOM_RANGE.0..=ZOOM_RANGE.1).contains(&z) {
                        return Err(format!("{who}: {at}: camera zoom={} is outside {}..{}", crate::context::fmt_num(z), ZOOM_RANGE.0, ZOOM_RANGE.1));
                    }
                }
                if let Some((x, y)) = c.pan {
                    if !(x.abs() <= PAN_MAX && y.abs() <= PAN_MAX) {
                        return Err(format!(
                            "{who}: {at}: camera pan=({}, {}) is outside -{PAN_MAX}..{PAN_MAX} of the frame",
                            crate::context::fmt_num(x),
                            crate::context::fmt_num(y)
                        ));
                    }
                }
                if let Some(az) = c.az {
                    if !az.is_finite() {
                        return Err(format!("{who}: {at}: camera az must be a number of degrees"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The full state at every keyframe under the hold rule. `document`
    /// gives a parameter's own value (its default or the document's
    /// override) for what a keyframe leaves unsaid before the first mention;
    /// `base` is the camera the request asked for, which plays the same
    /// part for the camera (absent, the first camera keyframe's fields hold
    /// before it).
    fn states(
        &self,
        document: &dyn Fn(&str) -> Option<ParamValue>,
        base: Option<CameraKey>,
    ) -> Vec<(Vec<(String, ParamValue)>, Option<CameraKey>, Vec<crate::layers::LayerOverride>)> {
        let names = self.param_names();
        let mut state: Vec<(String, Option<ParamValue>)> = names.iter().map(|n| (n.to_string(), document(n))).collect();
        let mut camera: Option<CameraKey> =
            base.or_else(|| self.keyframes.iter().find_map(|k| k.camera.filter(|c| !c.is_empty())));
        // A layer holds its first keyframed state before that keyframe, as
        // the camera does: nothing to ease from, so nothing jumps.
        let mut layers: Vec<crate::layers::LayerOverride> = self
            .layer_names()
            .iter()
            .filter_map(|n| self.keyframes.iter().flat_map(|k| k.layers.iter()).find(|l| l.name == *n).cloned())
            .collect();
        let mut out = Vec::with_capacity(self.keyframes.len());
        for k in &self.keyframes {
            for (n, v) in &k.params {
                if let Some(slot) = state.iter_mut().find(|(s, _)| s == n) {
                    slot.1 = Some(v.clone());
                }
            }
            if let Some(c) = k.camera {
                if !c.is_empty() {
                    camera = Some(c.over(camera.unwrap_or_default()));
                }
            }
            layers = crate::layers::merge_overrides(&layers, &k.layers);
            out.push((state.iter().filter_map(|(n, v)| v.clone().map(|v| (n.clone(), v))).collect(), camera, layers.clone()));
        }
        out
    }

    /// The parameters and the camera at time `t` (seconds). `base` is the
    /// camera the caller starts from — the request's `--view`, resolved to
    /// angles — so the first camera keyframe eases from it rather than
    /// jumping; `None` holds the first keyframed camera before its time.
    pub fn sample(&self, t: f64, document: &dyn Fn(&str) -> Option<ParamValue>, base: Option<CameraKey>) -> Sample {
        let states = self.states(document, base);
        if states.is_empty() {
            return Sample::default();
        }
        let last = self.keyframes.len() - 1;
        if t <= self.keyframes[0].t || last == 0 {
            return Sample { params: states[0].0.clone(), camera: states[0].1, layers: states[0].2.clone() };
        }
        if t >= self.keyframes[last].t {
            return Sample { params: states[last].0.clone(), camera: states[last].1, layers: states[last].2.clone() };
        }
        // The segment that holds t: keyframe k at or before it, k+1 after.
        let k = (0..last).rev().find(|&k| self.keyframes[k].t <= t).unwrap_or(0);
        let (t0, t1) = (self.keyframes[k].t, self.keyframes[k + 1].t);
        let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
        let e = self.keyframes[k].ease.unwrap_or(self.ease).apply(u);
        let (a, cam_a, lay_a) = &states[k];
        let (b, cam_b, lay_b) = &states[k + 1];
        let mut params = Vec::with_capacity(b.len());
        for (name, vb) in b {
            let va = a.iter().find(|(n, _)| n == name).map(|(_, v)| v);
            let v = match (va, vb) {
                (Some(ParamValue::Number(x)), ParamValue::Number(y)) => ParamValue::Number(x + (y - x) * e),
                (Some(ParamValue::Vector(x)), ParamValue::Vector(y)) => ParamValue::Vector(*x + (*y - *x) * e),
                // A bool or a string switches at the later keyframe; with no
                // earlier value there is nothing to ease from.
                (Some(other), _) => other.clone(),
                (None, _) => vb.clone(),
            };
            params.push((name.clone(), v));
        }
        let camera = match (cam_a, cam_b) {
            (Some(ca), Some(cb)) => Some(CameraKey {
                az: lerp_az(ca.az, cb.az, e),
                el: lerp_opt(ca.el, cb.el, e),
                fov: lerp_opt(ca.fov, cb.fov, e),
                eye: lerp_vec(ca.eye, cb.eye, e),
                target: lerp_vec(ca.target, cb.target, e),
                zoom: lerp_zoom(ca.zoom, cb.zoom, e),
                pan: match (ca.pan, cb.pan) {
                    (Some(a), Some(b)) => Some((a.0 + (b.0 - a.0) * e, a.1 + (b.1 - a.1) * e)),
                    (None, b) => b,
                    (a, None) => a,
                },
            }),
            (None, Some(cb)) => Some(*cb),
            (Some(ca), None) => Some(*ca),
            (None, None) => None,
        };
        // A layer's opacity eases like a number; its visibility switches at
        // the later keyframe like a bool.
        let layers = lay_b
            .iter()
            .map(|lb| {
                let la = lay_a.iter().find(|l| l.name == lb.name);
                crate::layers::LayerOverride {
                    name: lb.name.clone(),
                    visible: la.and_then(|l| l.visible).or(lb.visible),
                    opacity: lerp_opt(la.and_then(|l| l.opacity), lb.opacity, e),
                }
            })
            .collect();
        Sample { params, camera, layers }
    }
}

fn lerp_opt(a: Option<f64>, b: Option<f64>, e: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + (y - x) * e),
        (None, y) => y,
        (x, None) => x,
    }
}

fn lerp_vec(a: Option<Vec3>, b: Option<Vec3>, e: f64) -> Option<Vec3> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + (y - x) * e),
        (None, y) => y,
        (x, None) => x,
    }
}

/// Zoom eases as a distance does: the camera stands at `fit / zoom`, and it
/// is that distance which moves evenly, so a push from 1 to 2 does not
/// spend most of its time on the first few percent.
fn lerp_zoom(a: Option<f64>, b: Option<f64>, e: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) if x > 0.0 && y > 0.0 => Some(1.0 / (1.0 / x + (1.0 / y - 1.0 / x) * e)),
        (Some(x), Some(_)) => Some(x),
        (None, y) => y,
        (x, None) => x,
    }
}

/// Azimuth the shortest way round: from 350° to 10° is +20°, not −340°.
fn lerp_az(a: Option<f64>, b: Option<f64>, e: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => {
            let d = (y - x + 180.0).rem_euclid(360.0) - 180.0;
            Some(x + d * e)
        }
        (None, y) => y,
        (x, None) => x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(name: &str, keyframes: Vec<Keyframe>) -> AnimationDecl {
        AnimationDecl { name: name.into(), keyframes, fps: 30.0, ease: Ease::Smooth, looped: false, span: None }
    }

    /// A keyframe's `view(layers=)` holds like the camera -- a layer's
    /// first keyframed state before its first mention, a visibility until
    /// the keyframe that changes it, an opacity eased between the keyframes
    /// that name it -- and a document with no such keyframe samples none.
    #[test]
    fn a_keyframe_view_switches_and_ghosts_layers_under_the_hold_rule() {
        use crate::layers::LayerOverride;
        let lo = |name: &str, visible: Option<bool>, opacity: Option<f64>| LayerOverride { name: name.into(), visible, opacity };
        let mut k0 = kf(0.0, &[]);
        k0.layers = vec![lo("envelope", Some(false), None)];
        let mut k1 = kf(2.0, &[]);
        k1.layers = vec![lo("envelope", Some(true), Some(1.0)), lo("terrain", Some(true), Some(0.2))];
        let mut k2 = kf(4.0, &[]);
        k2.layers = vec![lo("terrain", Some(true), Some(1.0))];
        let d = decl("a", vec![k0, k1, k2]);
        assert!(d.has_layers());
        assert_eq!(d.layer_names(), ["envelope", "terrain"]);
        let doc = |_: &str| None;
        let s = d.sample(0.0, &doc, None);
        assert_eq!(s.layers, vec![lo("envelope", Some(false), None), lo("terrain", Some(true), Some(0.2))]);
        let s = d.sample(1.0, &doc, None);
        assert_eq!((s.layers[0].visible, s.layers[0].opacity), (Some(false), Some(1.0)), "visibility holds, opacity takes the later value with nothing to ease from");
        assert_eq!(s.layers[1], lo("terrain", Some(true), Some(0.2)));
        let s = d.sample(3.0, &doc, None);
        assert_eq!(s.layers[0], lo("envelope", Some(true), Some(1.0)));
        assert!((s.layers[1].opacity.unwrap() - 0.6).abs() < 1e-9, "0.2 -> 1.0 halfway, smoothstep: {:?}", s.layers[1]);
        let s = d.sample(9.0, &doc, None);
        assert_eq!(s.layers[1].opacity, Some(1.0));
        let plain = decl("b", vec![kf(0.0, &[("x", 1.0)]), kf(1.0, &[("x", 2.0)])]);
        assert!(!plain.has_layers());
        assert!(plain.sample(0.5, &doc, None).layers.is_empty());
        // An undeclared name is refused by name; a declared one and the
        // implicit layer pass.
        let declared = [crate::layers::LayerDecl { name: "envelope".into(), doc: String::new(), visible: true, opacity: 1.0, color: None, span: None }];
        assert!(d.validate_layers(&declared).unwrap_err().contains("layer \"terrain\" is not declared"));
        let mut ok = kf(0.0, &[]);
        ok.layers = vec![lo("envelope", Some(false), None), lo("", Some(true), Some(0.5))];
        assert_eq!(decl("c", vec![ok]).validate_layers(&declared), Ok(()));
    }

    fn kf(t: f64, params: &[(&str, f64)]) -> Keyframe {
        Keyframe {
            t,
            params: params.iter().map(|(n, v)| (n.to_string(), ParamValue::Number(*v))).collect(),
            camera: None,
            ease: None,
            layers: Vec::new(),
        }
    }

    fn param(name: &str, default: f64, min: Option<f64>, max: Option<f64>) -> (ParamDecl, ParamValue) {
        (
            ParamDecl {
                name: name.into(),
                default: ParamValue::Number(default),
                description: String::new(),
                unit: String::new(),
                min,
                max,
                step: None,
                choices: Vec::new(),
                span: None,
            },
            ParamValue::Number(default),
        )
    }

    fn number(s: &Sample, name: &str) -> f64 {
        match s.params.iter().find(|(n, _)| n == name).map(|(_, v)| v) {
            Some(ParamValue::Number(n)) => *n,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// The curves are 0 at 0 and 1 at 1, so keyframe values are exact at
    /// their own times, and smoothstep's midpoint is a half with zero slope
    /// at the ends.
    #[test]
    fn every_curve_is_exact_at_the_ends_and_smoothstep_halves_the_middle() {
        for e in Ease::ALL {
            assert_eq!(e.apply(0.0), 0.0, "{}", e.name());
            assert_eq!(e.apply(1.0), 1.0, "{}", e.name());
            assert_eq!(Ease::parse(e.name()), Some(e));
        }
        assert_eq!(Ease::Smooth.apply(0.5), 0.5);
        assert_eq!(Ease::Linear.apply(0.25), 0.25);
        assert_eq!(Ease::In.apply(0.5), 0.25);
        assert_eq!(Ease::Out.apply(0.5), 0.75);
        assert!(Ease::Smooth.apply(0.1) < 0.1 && Ease::Smooth.apply(0.9) > 0.9);
        assert_eq!(Ease::parse("ease-in"), None);
    }

    /// Values at keyframe times are the keyframe's own; between them the
    /// segment eases; a parameter a keyframe does not name holds.
    #[test]
    fn sampling_holds_eases_and_is_exact_at_keyframes() {
        let a = decl("a", vec![kf(0.0, &[("lift", 80.0)]), kf(2.0, &[("lift", 0.0)]), kf(3.0, &[]), kf(5.0, &[("turn", 720.0)])]);
        let doc = |n: &str| if n == "turn" { Some(ParamValue::Number(0.0)) } else { None };
        assert_eq!(a.duration(), 5.0);
        assert_eq!(a.param_names(), ["lift", "turn"]);
        let s0 = a.sample(0.0, &doc, None);
        assert_eq!(number(&s0, "lift"), 80.0);
        assert_eq!(number(&s0, "turn"), 0.0, "before its first mention a param is the document's value");
        assert_eq!(number(&a.sample(1.0, &doc, None), "lift"), 40.0, "smoothstep's midpoint");
        assert_eq!(number(&a.sample(2.0, &doc, None), "lift"), 0.0);
        assert_eq!(number(&a.sample(2.5, &doc, None), "lift"), 0.0, "held after its last mention");
        assert_eq!(number(&a.sample(2.5, &doc, None), "turn"), 0.0, "held until the keyframe that moves it");
        assert_eq!(number(&a.sample(4.0, &doc, None), "turn"), 360.0);
        assert_eq!(number(&a.sample(5.0, &doc, None), "turn"), 720.0);
        assert_eq!(number(&a.sample(9.0, &doc, None), "turn"), 720.0, "clamped past the end");
        assert_eq!(number(&a.sample(-1.0, &doc, None), "lift"), 80.0, "clamped before the start");
        // A per-keyframe ease overrides the animation's for its own segment.
        let mut b = a.clone();
        b.keyframes[0].ease = Some(Ease::Linear);
        assert_eq!(number(&b.sample(0.5, &doc, None), "lift"), 60.0);
        assert_eq!(number(&b.sample(4.0, &doc, None), "turn"), 360.0, "the other segments keep the animation's");
        // A bool switches at the later keyframe.
        let c = AnimationDecl {
            keyframes: vec![
                Keyframe { t: 0.0, params: vec![("open".into(), ParamValue::Bool(false))], camera: None, ease: None, layers: Vec::new() },
                Keyframe { t: 1.0, params: vec![("open".into(), ParamValue::Bool(true))], camera: None, ease: None, layers: Vec::new() },
            ],
            ..decl("c", Vec::new())
        };
        assert_eq!(c.sample(0.99, &|_| None, None).params[0].1, ParamValue::Bool(false));
        assert_eq!(c.sample(1.0, &|_| None, None).params[0].1, ParamValue::Bool(true));
    }

    /// The camera holds field by field and the azimuth takes the short way.
    #[test]
    fn camera_keyframes_interpolate_field_by_field_and_azimuth_the_short_way() {
        let mut a = decl("cam", vec![kf(0.0, &[]), kf(1.0, &[]), kf(2.0, &[])]);
        a.ease = Ease::Linear;
        assert!(!a.has_camera());
        a.keyframes[0].camera = Some(CameraKey { az: Some(350.0), el: Some(20.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None });
        a.keyframes[2].camera = Some(CameraKey { az: Some(10.0), el: None, fov: None, eye: None, target: None, zoom: None, pan: None });
        assert!(a.has_camera());
        let doc = |_: &str| None;
        assert_eq!(a.sample(0.0, &doc, None).camera, Some(CameraKey { az: Some(350.0), el: Some(20.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None }));
        // The middle keyframe says nothing about the camera, so it holds
        // 350 there and the turn to 10 runs over the last segment, +20°
        // rather than −340°.
        assert_eq!(a.sample(1.0, &doc, None).camera.unwrap().az, Some(350.0));
        assert_eq!(a.sample(1.5, &doc, None).camera.unwrap().az, Some(360.0));
        assert_eq!(a.sample(2.0, &doc, None).camera.unwrap(), CameraKey { az: Some(10.0), el: Some(20.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None });
        // Going the other way is negative.
        a.keyframes[0].camera = Some(CameraKey { az: Some(10.0), el: None, fov: None, eye: None, target: None, zoom: None, pan: None });
        a.keyframes[2].camera = Some(CameraKey { az: Some(350.0), el: None, fov: None, eye: None, target: None, zoom: None, pan: None });
        assert_eq!(a.sample(1.5, &doc, None).camera.unwrap().az, Some(0.0));
        // A camera set only at a later keyframe holds that value before it,
        // unless the caller has a base camera, which it then eases from.
        a.keyframes[0].camera = None;
        assert_eq!(a.sample(0.5, &doc, None).camera.unwrap().az, Some(350.0));
        let base = Some(CameraKey { az: Some(30.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None });
        assert_eq!(a.sample(0.5, &doc, base).camera.unwrap(), CameraKey { az: Some(30.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None });
        assert_eq!(a.sample(1.5, &doc, base).camera.unwrap().az, Some(10.0), "half way from 30 to 350 the short way");
        assert_eq!(a.sample(1.5, &doc, base).camera.unwrap().el, Some(18.0), "a field the keyframe leaves alone holds");
        assert_eq!(a.sample(2.0, &doc, base).camera.unwrap(), CameraKey { az: Some(350.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None });
    }

    /// Zoom and pan ease from the base's 1 and (0, 0), hold where a
    /// keyframe leaves them alone, and zoom moves evenly in distance.
    #[test]
    fn zoom_and_pan_ease_and_hold() {
        let key = |zoom, pan| Some(CameraKey { az: None, el: None, fov: None, eye: None, target: None, zoom, pan });
        let mut a = decl("cam", vec![kf(0.0, &[]), kf(1.0, &[]), kf(2.0, &[])]);
        a.ease = Ease::Linear;
        a.keyframes[1].camera = key(Some(2.0), Some((0.2, -0.1)));
        a.keyframes[2].camera = key(None, Some((0.0, 0.0)));
        let doc = |_: &str| None;
        let base = Some(CameraKey { az: Some(30.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: Some(1.0), pan: Some((0.0, 0.0)) });
        let at = |t: f64| a.sample(t, &doc, base).camera.unwrap();
        assert_eq!((at(0.0).zoom, at(0.0).pan), (Some(1.0), Some((0.0, 0.0))));
        // Half way in distance between 1/1 and 1/2 is 1/0.75.
        assert!((at(0.5).zoom.unwrap() - 1.0 / 0.75).abs() < 1e-12);
        assert_eq!(at(0.5).pan, Some((0.1, -0.05)));
        assert_eq!((at(1.0).zoom, at(1.0).pan), (Some(2.0), Some((0.2, -0.1))));
        // The last keyframe names no zoom, so it holds while the pan returns.
        assert_eq!((at(1.5).zoom, at(1.5).pan), (Some(2.0), Some((0.1, -0.05))));
        assert_eq!(at(1.5).az, Some(30.0), "a key with no angle leaves the camera's direction alone");
        let mut bad = decl("cam", vec![kf(0.0, &[])]);
        bad.keyframes[0].camera = key(Some(11.0), None);
        assert!(bad.validate(&[]).unwrap_err().contains("zoom"));
        bad.keyframes[0].camera = key(None, Some((0.0, 0.5)));
        assert!(bad.validate(&[]).unwrap_err().contains("pan"));
    }

    /// Frame times: one per 1/fps, the end included, unless the animation
    /// loops; a resample divides the duration evenly; both capped.
    #[test]
    fn frame_times_cover_the_duration_and_a_loop_stops_one_short() {
        let mut a = decl("f", vec![kf(0.0, &[]), kf(2.0, &[])]);
        a.fps = 10.0;
        assert_eq!(a.frame_count(0.0), 21);
        let t = a.frame_times(0.0, 0);
        assert_eq!(t.len(), 21);
        assert_eq!((t[0], t[20]), (0.0, 2.0));
        assert!((t[1] - 0.1).abs() < 1e-12);
        assert_eq!(a.frame_seconds(0.0, 0), 0.1);
        a.looped = true;
        assert_eq!(a.frame_count(0.0), 20);
        let t = a.frame_times(0.0, 0);
        assert_eq!(t.len(), 20);
        assert!((t[19] - 1.9).abs() < 1e-12, "{}", t[19]);
        assert_eq!(a.frame_times(5.0, 0).len(), 10, "an fps override");
        assert_eq!(a.frame_times(0.0, 4), vec![0.0, 0.5, 1.0, 1.5], "a resample of a loop");
        assert_eq!(a.frame_seconds(0.0, 4), 0.5);
        a.looped = false;
        assert_eq!(a.frame_times(0.0, 5), vec![0.0, 0.5, 1.0, 1.5, 2.0], "a resample to the end");
        assert_eq!(a.frame_times(0.0, 1), vec![0.0]);
        a.fps = 1e9;
        assert_eq!(a.frame_count(0.0), MAX_FRAMES);
        assert_eq!(a.frame_times(0.0, 100_000).len(), MAX_FRAMES);
    }

    /// An animation longer than [`MAX_FRAMES`] at its rate is resampled to
    /// that many frames over its whole duration, so each frame is shown for
    /// longer than 1/fps: 300 s at 30 fps is 4096 frames of 300/4095 s, not
    /// of 1/30 s (which played the file 2.2 times fast).
    #[test]
    fn a_capped_frame_count_stretches_the_frame_time_over_the_duration() {
        let mut a = decl("slow", vec![kf(0.0, &[]), kf(300.0, &[])]);
        a.fps = 30.0;
        assert_eq!(a.frame_count(0.0), MAX_FRAMES);
        assert_eq!(a.uncapped_frame_count(0.0), 9001);
        let t = a.frame_times(0.0, 0);
        let played = a.frame_seconds(0.0, 0) * (t.len() - 1) as f64;
        assert!((played - 300.0).abs() < 1e-9, "the frames play for the duration: {played}");
        assert!((a.frame_seconds(0.0, 0) - t[1]).abs() < 1e-12, "one frame lasts one step of the resample");
        a.looped = true;
        let played = a.frame_seconds(0.0, 0) * a.frame_times(0.0, 0).len() as f64;
        assert!((played - 300.0).abs() < 1e-9, "a loop too: {played}");
        // An fps override past the cap is the same resample.
        a.looped = false;
        let short = decl("short", vec![kf(0.0, &[]), kf(2.0, &[])]);
        let played = short.frame_seconds(1e9, 0) * (short.frame_times(1e9, 0).len() - 1) as f64;
        assert!((played - 2.0).abs() < 1e-9, "{played}");
        // Under the cap nothing changes.
        assert_eq!(short.frame_seconds(10.0, 0), 0.1);
        assert_eq!(short.uncapped_frame_count(10.0), 21);
    }

    /// The refusals, each naming the keyframe.
    #[test]
    fn validate_refuses_what_cannot_be_animated() {
        let params = vec![param("lift", 0.0, Some(0.0), Some(80.0)), param("turn", 0.0, None, None)];
        let ok = decl("ok", vec![kf(0.0, &[("lift", 80.0)]), kf(2.0, &[("lift", 0.0), ("turn", 720.0)])]);
        assert_eq!(ok.validate(&params), Ok(()));
        let err = |d: AnimationDecl| d.validate(&params).unwrap_err();
        assert!(err(decl("e", Vec::new())).contains("at least one keyframe"));
        let e = err(decl("e", vec![kf(1.0, &[]), kf(1.0, &[])]));
        assert!(e.contains("keyframe 1 (t=1)") && e.contains("increasing"), "{e}");
        let e = err(decl("e", vec![kf(-1.0, &[])]));
        assert!(e.contains("non-negative"), "{e}");
        let e = err(decl("e", vec![kf(0.0, &[("nope", 1.0)])]));
        assert!(e.contains("\"nope\" is not a param") && e.contains("lift, turn"), "{e}");
        let e = err(decl("e", vec![kf(0.0, &[("lift", 81.0)])]));
        assert!(e.contains("lift=81 is above its max 80"), "{e}");
        let e = err(decl("e", vec![kf(0.0, &[("lift", -1.0)])]));
        assert!(e.contains("below its min 0"), "{e}");
        let mut wrong_kind = decl("e", vec![kf(0.0, &[])]);
        wrong_kind.keyframes[0].params.push(("lift".into(), ParamValue::Bool(true)));
        let e = err(wrong_kind);
        assert!(e.contains("lift is a number but the keyframe gives it a bool"), "{e}");
        let mut bad_fps = ok.clone();
        bad_fps.fps = 0.0;
        assert!(err(bad_fps).contains("fps"));
        let mut bad_cam = ok.clone();
        bad_cam.keyframes[0].camera = Some(CameraKey { az: None, el: Some(95.0), fov: None, eye: None, target: None, zoom: None, pan: None });
        assert!(err(bad_cam).contains("el=95 is outside -90..90"));
        let mut bad_fov = ok.clone();
        bad_fov.keyframes[1].camera = Some(CameraKey { az: None, el: None, fov: Some(0.5), eye: None, target: None, zoom: None, pan: None });
        let e = err(bad_fov);
        assert!(e.contains("keyframe 1 (t=2)") && e.contains("fov=0.5"), "{e}");
    }
}
