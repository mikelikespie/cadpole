//! Comparing two images, for the CPU-versus-GPU tests and for anything that
//! wants to say how close two renders are without pulling in an image crate.
//!
//! The metric is deliberately simple and stated in the units a reader can
//! check by eye: absolute difference per sRGB8 channel. A GPU and a CPU render
//! of the same scene can never be bit-identical -- different rasterization
//! fill rules, different multisample positions, `normalize` on a GPU may use a
//! reciprocal square root -- so the tests bound the *mean* and the tail rather
//! than asserting equality.

use cadpole_core::render::Image;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Diff {
    /// Mean absolute difference over every colour channel, in sRGB8 units.
    pub mean: f64,
    /// Largest single-channel difference.
    pub max: u8,
    /// Difference no more than this for 99% of channels.
    pub p99: u8,
    /// Fraction of *pixels* where any channel differs by more than 8/255 --
    /// roughly the point at which a difference is visible side by side.
    pub over_8: f64,
    /// Fraction of pixels whose coverage disagrees: one image has the
    /// background colour there and the other does not. This is the geometric
    /// part of the difference, and it is what a silhouette shift shows up as.
    pub coverage: f64,
}

impl std::fmt::Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mean {:.3}/255, p99 {}/255, max {}/255, {:.3}% of pixels over 8/255, {:.3}% coverage disagreement",
            self.mean,
            self.p99,
            self.max,
            self.over_8 * 100.0,
            self.coverage * 100.0
        )
    }
}

/// Compare two images of the same size. `background` is the sRGB8 colour that
/// counts as "nothing drawn here" for the coverage figure.
///
/// Panics if the sizes differ, which in a test is the right failure.
pub fn compare(a: &Image, b: &Image, background: [u8; 3]) -> Diff {
    assert_eq!((a.width, a.height), (b.width, b.height), "image sizes differ");
    let n = (a.width as usize) * (a.height as usize);
    let mut hist = [0u64; 256];
    let mut sum = 0u64;
    let mut max = 0u8;
    let mut over = 0usize;
    let mut coverage = 0usize;
    for i in 0..n {
        let pa = &a.rgba[i * 4..i * 4 + 3];
        let pb = &b.rgba[i * 4..i * 4 + 3];
        let mut worst = 0u8;
        for c in 0..3 {
            let d = pa[c].abs_diff(pb[c]);
            hist[d as usize] += 1;
            sum += d as u64;
            worst = worst.max(d);
        }
        max = max.max(worst);
        over += usize::from(worst > 8);
        coverage += usize::from((pa == background) != (pb == background));
    }
    let channels = (n * 3) as u64;
    let mut seen = 0u64;
    let mut p99 = 0u8;
    for (d, &count) in hist.iter().enumerate() {
        seen += count;
        if seen * 100 >= channels * 99 {
            p99 = d as u8;
            break;
        }
    }
    Diff {
        mean: sum as f64 / channels.max(1) as f64,
        max,
        p99,
        over_8: over as f64 / n.max(1) as f64,
        coverage: coverage as f64 / n.max(1) as f64,
    }
}

/// An image of the per-pixel difference, scaled by `gain`, for eyeballing a
/// failure: write it next to the two renders and look at where the error is.
pub fn difference_image(a: &Image, b: &Image, gain: u32) -> Image {
    assert_eq!((a.width, a.height), (b.width, b.height), "image sizes differ");
    let mut rgba = vec![255u8; a.rgba.len()];
    for i in 0..(a.width as usize) * (a.height as usize) {
        for c in 0..3 {
            let d = a.rgba[i * 4 + c].abs_diff(b.rgba[i * 4 + c]) as u32;
            rgba[i * 4 + c] = (d * gain).min(255) as u8;
        }
        rgba[i * 4 + 3] = 255;
    }
    Image { width: a.width, height: a.height, rgba }
}
