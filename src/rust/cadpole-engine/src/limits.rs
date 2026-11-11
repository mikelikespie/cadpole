//! Process-wide resource limits: what a request may ask this engine for
//! (docs/security.md, "Starlark sandbox" and "Render and sweep limits").
//!
//! Two kinds of limit live here.
//!
//! * **Evaluation limits** ([`Limits`], from `cadpole-script`) bound a script.
//!   Most requests do not carry them — `RenderRequest` has no `deadline_ms`
//!   field, and neither does `ExportRequest` — so the process that is exposed
//!   to the network sets a default once at startup with
//!   [`set_default_limits`], and every evaluation picks it up. `cadpole serve`
//!   sets [`Limits::interactive`], the CLI [`Limits::batch`], and a library
//!   embedder that says nothing gets the structural caps and no deadline.
//! * **Render limits** ([`RenderLimits`]) bound one image: pixels, samples,
//!   tiles, sweep cells. A request over the limit is *clamped* and told so in
//!   a diagnostic, rather than refused — an assistant that asked for 8 K gets
//!   a picture and an explanation, which is more useful than an error.
//!
//! Clamping rather than refusing is deliberate everywhere except the two
//! cases where quietly doing less would be a lie: an oversize request body
//! (the HTTP layer refuses it) and a script that will not terminate (the
//! deadline stops it and says so).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use cadpole_proto::v1 as pb;

pub use cadpole_script::Limits;

static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static MAX_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static MAX_CALLSTACK: AtomicUsize = AtomicUsize::new(0);

/// Set the limits every evaluation in this process gets when the request does
/// not carry its own. Call once at startup; later calls simply replace it.
pub fn set_default_limits(l: Limits) {
    let l = l.or_defaults();
    DEADLINE_MS.store(l.deadline_ms, Ordering::Relaxed);
    MAX_TICKS.store(l.max_ticks, Ordering::Relaxed);
    MAX_HEAP_BYTES.store(l.max_heap_bytes, Ordering::Relaxed);
    MAX_CALLSTACK.store(l.max_callstack, Ordering::Relaxed);
}

/// The limits set by [`set_default_limits`], or the structural caps with no
/// deadline when nobody set any.
pub fn default_limits() -> Limits {
    Limits {
        deadline_ms: DEADLINE_MS.load(Ordering::Relaxed),
        max_ticks: MAX_TICKS.load(Ordering::Relaxed),
        max_heap_bytes: MAX_HEAP_BYTES.load(Ordering::Relaxed),
        max_callstack: MAX_CALLSTACK.load(Ordering::Relaxed),
        max_print_lines: 0,
        // Never a process default: a host turns it on per evaluation, for
        // the ones its owner asks for.
        solve_jobs: false,
    }
    .or_defaults()
}

/// The evaluation limits an `EvaluateRequest` asks for, over the process
/// default. A zero field means "whatever the process says".
pub fn limits_from_request(req: &pb::EvaluateRequest) -> Limits {
    limits_over(req, default_limits())
}

/// The merge rule, without the process globals so it can be tested.
///
/// A request may ask for **less** than the process allows and never for more.
/// Taking any non-zero request field as given would make all four of these
/// limits advisory. The restricted listener is granted `Evaluate` -- and
/// `capability.rs` argues that is safe because it runs "under the same bounds
/// every other caller gets" -- so it could hand itself a longer deadline, a
/// larger tick budget and a higher heap ceiling than `cadpole serve` was
/// started with: the bounds cited as the reason for the grant would be fields
/// the grantee could edit.
fn limits_over(req: &pb::EvaluateRequest, d: Limits) -> Limits {
    Limits {
        deadline_ms: tighter(req.deadline_ms, d.deadline_ms),
        max_ticks: tighter(req.max_ticks, d.max_ticks),
        max_heap_bytes: tighter(usize::try_from(req.max_heap_bytes).unwrap_or(usize::MAX), d.max_heap_bytes),
        max_callstack: tighter(req.max_callstack as usize, d.max_callstack),
        max_print_lines: d.max_print_lines,
        // Not a request field at all: a caller cannot ask for jobs.
        solve_jobs: d.solve_jobs,
    }
}

/// Whichever of the two binds first, where zero means "no limit of my own".
fn tighter<T: Ord + Default>(requested: T, allowed: T) -> T {
    if requested == T::default() {
        allowed
    } else if allowed == T::default() {
        requested
    } else {
        requested.min(allowed)
    }
}

/// What one render or sweep request may ask for. The defaults are
/// docs/security.md's table; a host with more (or less) hardware can lower
/// them with [`set_render_limits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderLimits {
    /// Largest image, width x height, in pixels.
    pub max_pixels: u64,
    /// Largest width or height on its own, so 1 x 16_000_000 is refused too.
    pub max_dimension: u32,
    /// Largest sample count per pixel.
    pub max_samples: u32,
    /// Largest number of raster worker threads a request may ask for. A
    /// request that asks for a thousand threads would spend its whole budget
    /// in the scheduler.
    pub max_threads: u32,
    /// Largest number of cells in one sweep.
    pub max_sweep_cells: u32,
    /// Largest path-tracer bounce count.
    pub max_bounces: u32,
}

impl RenderLimits {
    pub const DEFAULT: RenderLimits = RenderLimits {
        max_pixels: 16 * 1024 * 1024,
        max_dimension: 8192,
        max_samples: 1024,
        max_threads: 256,
        max_sweep_cells: 64,
        max_bounces: 64,
    };
}

impl Default for RenderLimits {
    fn default() -> RenderLimits {
        RenderLimits::DEFAULT
    }
}

static MAX_PIXELS: AtomicU64 = AtomicU64::new(0);
static MAX_DIMENSION: AtomicU64 = AtomicU64::new(0);
static MAX_SAMPLES: AtomicU64 = AtomicU64::new(0);
static MAX_THREADS: AtomicU64 = AtomicU64::new(0);
static MAX_SWEEP_CELLS: AtomicU64 = AtomicU64::new(0);
static MAX_BOUNCES: AtomicU64 = AtomicU64::new(0);

/// Replace the render limits for this process.
pub fn set_render_limits(l: RenderLimits) {
    MAX_PIXELS.store(l.max_pixels, Ordering::Relaxed);
    MAX_DIMENSION.store(u64::from(l.max_dimension), Ordering::Relaxed);
    MAX_SAMPLES.store(u64::from(l.max_samples), Ordering::Relaxed);
    MAX_THREADS.store(u64::from(l.max_threads), Ordering::Relaxed);
    MAX_SWEEP_CELLS.store(u64::from(l.max_sweep_cells), Ordering::Relaxed);
    MAX_BOUNCES.store(u64::from(l.max_bounces), Ordering::Relaxed);
}

fn or(v: u64, fallback: u64) -> u64 {
    if v == 0 { fallback } else { v }
}

pub fn render_limits() -> RenderLimits {
    let d = RenderLimits::DEFAULT;
    RenderLimits {
        max_pixels: or(MAX_PIXELS.load(Ordering::Relaxed), d.max_pixels),
        max_dimension: or(MAX_DIMENSION.load(Ordering::Relaxed), u64::from(d.max_dimension)) as u32,
        max_samples: or(MAX_SAMPLES.load(Ordering::Relaxed), u64::from(d.max_samples)) as u32,
        max_threads: or(MAX_THREADS.load(Ordering::Relaxed), u64::from(d.max_threads)) as u32,
        max_sweep_cells: or(MAX_SWEEP_CELLS.load(Ordering::Relaxed), u64::from(d.max_sweep_cells)) as u32,
        max_bounces: or(MAX_BOUNCES.load(Ordering::Relaxed), u64::from(d.max_bounces)) as u32,
    }
}

/// Clamp a render request in place, returning one warning per knob that moved.
///
/// The warnings are `Diagnostic`s and travel back in `RenderResponse
/// .diagnostics`, so the caller that asked for 8192 x 8192 at 4096 samples
/// learns exactly what it got instead.
pub fn clamp_render(req: &mut pb::RenderRequest) -> Vec<pb::Diagnostic> {
    let l = render_limits();
    let mut out = Vec::new();
    let mut warn = |m: String| out.push(warning(m));

    // 0 means "the engine's default", so only non-zero values are clamped.
    if req.width > l.max_dimension {
        warn(format!("width {} clamped to {} (max_dimension)", req.width, l.max_dimension));
        req.width = l.max_dimension;
    }
    if req.height > l.max_dimension {
        warn(format!("height {} clamped to {} (max_dimension)", req.height, l.max_dimension));
        req.height = l.max_dimension;
    }
    let (w, h) = (u64::from(if req.width == 0 { 800 } else { req.width }), u64::from(if req.height == 0 { 600 } else { req.height }));
    if w * h > l.max_pixels {
        // Keep the aspect ratio: scale both sides by sqrt(limit / pixels).
        let scale = (l.max_pixels as f64 / (w * h) as f64).sqrt();
        let nw = ((w as f64 * scale) as u32).max(16);
        let nh = ((h as f64 * scale) as u32).max(16);
        warn(format!("{w}x{h} is {} pixels, over the {} limit; rendered at {nw}x{nh}", w * h, l.max_pixels));
        req.width = nw;
        req.height = nh;
    }
    // Supersampling renders k x k times the pixels, and it is that image
    // the pixel limit is about.
    if req.supersample > 1 {
        let (w, h) = (u64::from(if req.width == 0 { 800 } else { req.width }), u64::from(if req.height == 0 { 600 } else { req.height }));
        let asked = req.supersample.min(4);
        let mut k = asked;
        while k > 1 && w * h * u64::from(k * k) > l.max_pixels {
            k -= 1;
        }
        if k != asked {
            warn(format!("supersample {asked} lowered to {k}: {w}x{h} at {asked}x{asked} sub-pixels is over the {} pixel limit", l.max_pixels));
        }
        req.supersample = k;
    }
    if req.samples > l.max_samples {
        warn(format!("samples {} clamped to {}", req.samples, l.max_samples));
        req.samples = l.max_samples;
    }
    if req.threads > l.max_threads {
        warn(format!("threads {} clamped to {}", req.threads, l.max_threads));
        req.threads = l.max_threads;
    }
    if req.max_bounces > l.max_bounces {
        warn(format!("max_bounces {} clamped to {}", req.max_bounces, l.max_bounces));
        req.max_bounces = l.max_bounces;
    }
    out
}

/// Clamp a sweep request in place. Cells are the expensive axis: each one is
/// a whole evaluation plus a render, so the cell count is capped before the
/// cell size is.
pub fn clamp_sweep(req: &mut pb::SweepRequest) -> Vec<pb::Diagnostic> {
    let l = render_limits();
    let mut out = Vec::new();

    if req.max_cells == 0 || req.max_cells > l.max_sweep_cells {
        if req.max_cells > l.max_sweep_cells {
            out.push(warning(format!("max_cells {} clamped to {}", req.max_cells, l.max_sweep_cells)));
        }
        req.max_cells = l.max_sweep_cells;
    }
    // Saturating: a product past u64 is "too many" (and `sweep` refuses it
    // from the same counts), never a wrapped small number.
    let cells: u64 = req.params.values().fold(1u64, |acc, v| acc.saturating_mul(v.values.len().max(1) as u64));
    let per_cell = u64::from(if req.width == 0 { 320 } else { req.width }) * u64::from(if req.height == 0 { 240 } else { req.height });
    let sheet = cells.saturating_mul(per_cell);
    if sheet > l.max_pixels {
        let scale = (l.max_pixels as f64 / sheet as f64).sqrt();
        let nw = ((u64::from(if req.width == 0 { 320 } else { req.width }) as f64 * scale) as u32).max(16);
        let nh = ((u64::from(if req.height == 0 { 240 } else { req.height }) as f64 * scale) as u32).max(16);
        out.push(warning(format!(
            "{cells} cells at {}x{} is over the {} pixel limit for one sheet; cells rendered at {nw}x{nh}",
            if req.width == 0 { 320 } else { req.width },
            if req.height == 0 { 240 } else { req.height },
            l.max_pixels
        )));
        req.width = nw;
        req.height = nh;
    }
    if req.samples > l.max_samples {
        out.push(warning(format!("samples {} clamped to {}", req.samples, l.max_samples)));
        req.samples = l.max_samples;
    }
    if req.threads > l.max_threads {
        out.push(warning(format!("threads {} clamped to {}", req.threads, l.max_threads)));
        req.threads = l.max_threads;
    }
    out
}

fn warning(message: String) -> pb::Diagnostic {
    pb::Diagnostic { severity: pb::Severity::Warning as i32, message, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oversize_image_is_clamped_with_the_aspect_ratio_kept() {
        let mut r = pb::RenderRequest { width: 8000, height: 6000, samples: 4096, ..Default::default() };
        let ds = clamp_render(&mut r);
        assert!(u64::from(r.width) * u64::from(r.height) <= RenderLimits::DEFAULT.max_pixels, "{}x{}", r.width, r.height);
        let aspect = f64::from(r.width) / f64::from(r.height);
        assert!((aspect - 8000.0 / 6000.0).abs() < 0.02, "aspect kept: {aspect}");
        assert_eq!(r.samples, RenderLimits::DEFAULT.max_samples);
        assert!(ds.iter().any(|d| d.message.contains("pixel")), "{ds:?}");
        assert!(ds.iter().any(|d| d.message.contains("samples")), "{ds:?}");
        assert!(ds.iter().all(|d| d.severity == pb::Severity::Warning as i32));
    }

    #[test]
    fn a_dimension_over_the_cap_is_clamped_even_when_the_area_is_small() {
        let mut r = pb::RenderRequest { width: 100_000, height: 8, ..Default::default() };
        let ds = clamp_render(&mut r);
        assert_eq!(r.width, RenderLimits::DEFAULT.max_dimension);
        assert!(ds.iter().any(|d| d.message.contains("max_dimension")), "{ds:?}");
    }

    /// Supersampling multiplies the pixels rendered, so the factor comes
    /// down until the larger image is inside the pixel limit.
    #[test]
    fn a_supersample_factor_is_lowered_to_fit_the_pixel_limit() {
        let mut r = pb::RenderRequest { width: 1800, height: 1200, supersample: 4, ..Default::default() };
        let ds = clamp_render(&mut r);
        assert_eq!(r.supersample, 2, "1800x1200 is 2.2 Mpx: 3x3 of it is over 16 Mpx, 2x2 is not");
        assert!(ds.iter().any(|d| d.message.contains("supersample 4 lowered to 2")), "{ds:?}");
        let mut r = pb::RenderRequest { width: 800, height: 600, supersample: 4, ..Default::default() };
        assert!(clamp_render(&mut r).is_empty());
        assert_eq!(r.supersample, 4);
    }

    #[test]
    fn a_request_within_the_limits_is_untouched() {
        let mut r = pb::RenderRequest { width: 800, height: 600, samples: 4, ..Default::default() };
        let before = r.clone();
        assert!(clamp_render(&mut r).is_empty());
        assert_eq!(r, before);
    }

    #[test]
    fn a_sweep_cell_count_is_capped() {
        let values = |n: usize| pb::ParamValues {
            values: (0..n).map(|i| pb::ParamValue { value: Some(pb::param_value::Value::Number(i as f64)) }).collect(),
        };
        let mut s = pb::SweepRequest { width: 2000, height: 2000, ..Default::default() };
        s.params.insert("w".into(), values(20));
        s.params.insert("h".into(), values(20));
        let ds = clamp_sweep(&mut s);
        assert_eq!(s.max_cells, RenderLimits::DEFAULT.max_sweep_cells, "the sheet is capped at 64 cells");
        assert!(s.width < 2000 && s.height < 2000, "cells shrank to fit the pixel budget");
        assert!(ds.iter().any(|d| d.message.contains("pixel limit")), "{ds:?}");
    }

    /// The cell product saturates: 16 x 16 values used to wrap to 0 cells,
    /// so the pixel budget check saw a 1-cell sheet and shrank nothing.
    #[test]
    fn a_sweep_cell_product_that_overflows_saturates() {
        let values = |n: usize| pb::ParamValues {
            values: (0..n).map(|i| pb::ParamValue { value: Some(pb::param_value::Value::Number(i as f64)) }).collect(),
        };
        let mut s = pb::SweepRequest { width: 320, height: 240, ..Default::default() };
        for i in 0..16 {
            s.params.insert(format!("p{i}"), values(16));
        }
        let ds = clamp_sweep(&mut s);
        assert!(ds.iter().any(|d| d.message.contains("pixel limit")), "{ds:?}");
        assert_eq!((s.width, s.height), (16, 16), "cells shrank to the floor");
    }

    /// The globals are deliberately not exercised here: they are process-wide,
    /// and `cargo`/Bazel run tests in threads, so a test that set a 7 s
    /// deadline would impose it on every other test evaluating a document in
    /// the same binary. What this asserts is the merge rule, which is where
    /// the logic is; `cadpole serve` setting the default is covered end to end.
    /// A caller may tighten its own limits and may not loosen them.
    ///
    /// `capability.rs` grants the restricted listener `Evaluate` on the
    /// argument that it runs under the bounds the server was started with.
    /// Those bounds were request fields: a listener token, refused `SetScript`
    /// with a 403, could still raise its own deadline past `--eval-deadline-ms`
    /// and its own heap ceiling past the server's.
    #[test]
    fn a_request_may_lower_a_limit_and_may_not_raise_it() {
        let server =
            Limits { deadline_ms: 20_000, max_ticks: 200_000_000, max_heap_bytes: 512 << 20, max_callstack: 64, max_print_lines: 0, solve_jobs: false };
        let greedy = pb::EvaluateRequest {
            deadline_ms: 10 * 60 * 1000,
            max_ticks: u64::MAX,
            max_heap_bytes: 9_000_000_000,
            max_callstack: 1_000_000,
            ..Default::default()
        };
        let l = limits_over(&greedy, server);
        assert_eq!(l.deadline_ms, 20_000, "a request cannot buy itself more wall clock");
        assert_eq!(l.max_ticks, 200_000_000);
        assert_eq!(l.max_heap_bytes, 512 << 20, "nor more memory");
        assert_eq!(l.max_callstack, 64);

        let modest = pb::EvaluateRequest { deadline_ms: 250, max_heap_bytes: 1 << 20, ..Default::default() };
        let l = limits_over(&modest, server);
        assert_eq!(l.deadline_ms, 250, "asking for less is still honoured");
        assert_eq!(l.max_heap_bytes, 1 << 20);
        assert_eq!(l.max_ticks, 200_000_000, "and a field the request left out is the server's");
    }

    /// A library embedder that set no limit of its own still lets a request
    /// name one: that is a tightening from unbounded, not a raise.
    #[test]
    fn a_process_with_no_limit_of_its_own_takes_the_requests() {
        let none = Limits { deadline_ms: 0, max_ticks: 0, max_heap_bytes: 0, max_callstack: 0, max_print_lines: 0, solve_jobs: false };
        let l = limits_over(&pb::EvaluateRequest { deadline_ms: 5_000, ..Default::default() }, none);
        assert_eq!(l.deadline_ms, 5_000);
    }

    #[test]
    fn a_request_deadline_wins_over_the_process_default() {
        let l = limits_from_request(&pb::EvaluateRequest { deadline_ms: 250, ..Default::default() });
        assert_eq!(l.deadline_ms, 250);
        assert_eq!(l.max_callstack, Limits::DEFAULT_CALLSTACK, "unset fields keep the structural caps");
        assert_eq!(l.max_heap_bytes, Limits::DEFAULT_HEAP_BYTES);
        let l = limits_from_request(&pb::EvaluateRequest { max_ticks: 99, max_callstack: 7, ..Default::default() });
        assert_eq!((l.max_ticks, l.max_callstack), (99, 7));
        assert_eq!(l.deadline_ms, default_limits().deadline_ms, "an unset request deadline is the process default");
    }

    /// The size the clamp warning names is the size the renderer draws:
    /// the options builder used to apply a second, smaller pixel cap
    /// (16,000,000 against 16 Mi) with no warning, so 70000 x 70000 was
    /// reported "rendered at 4096x4096" and written at 4000 x 4000.
    #[test]
    fn the_clamped_size_is_the_rendered_size() {
        for (w, h) in [(70_000, 70_000), (8000, 6000), (4000, 4050), (4096, 4096)] {
            let mut r = pb::RenderRequest { width: w, height: h, draft: true, ..Default::default() };
            let ds = clamp_render(&mut r);
            let o = crate::render_options_from_proto(&r);
            assert_eq!((o.width, o.height), (r.width, r.height), "{w}x{h}: {ds:?}");
            if let Some(d) = ds.iter().find(|d| d.message.contains("rendered at")) {
                assert!(d.message.ends_with(&format!("rendered at {}x{}", o.width, o.height)), "{}", d.message);
            }
        }
    }
}
