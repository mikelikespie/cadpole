//! Wall-clock timing that compiles for WebAssembly.
//!
//! `std::time::Instant::now()` panics on `wasm32-unknown-unknown` with "time
//! not implemented on this platform". Because the kernel traces every
//! operation and the engine reports an evaluation time, a bare `Instant`
//! anywhere on the evaluation path makes the browser build panic on the first
//! script it runs — and that is invisible until
//! an end-to-end test runs the real engine in a browser.
//!
//! On wasm the elapsed time is reported as zero rather than faked: the page
//! measures evaluation with `performance.now()` on the JavaScript side, where
//! a real clock exists.

/// A monotonic stopwatch, or a stub that always reads zero on wasm.
#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl Stopwatch {
    #[inline]
    pub fn start() -> Stopwatch {
        Stopwatch {
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
        }
    }

    /// Milliseconds since `start()`; always 0.0 on wasm.
    #[inline]
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            0.0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_secs_f64() * 1000.0
        }
    }

    /// Whole microseconds since `start()`; always 0 on wasm.
    #[inline]
    pub fn elapsed_us(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_micros() as u64
        }
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Stopwatch::start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopwatch_measures_forward() {
        let w = Stopwatch::start();
        let mut acc = 0u64;
        for i in 0..200_000u64 {
            acc = acc.wrapping_add(i);
        }
        std::hint::black_box(acc);
        assert!(w.elapsed_ms() >= 0.0);
        assert!(w.elapsed_us() as f64 <= w.elapsed_ms() * 1000.0 + 1000.0);
    }
}
