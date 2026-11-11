//! The transcendentals, pinned to one implementation on every target.
//!
//! `f64::sin` and its siblings are documented as having non-deterministic
//! precision: on `x86_64-unknown-linux-gnu` they lower to glibc, on
//! `wasm32-unknown-unknown` to the `libm` port that `compiler-builtins`
//! bundles, and the two disagree in the last bit often enough that truck's
//! discrete decisions (how many segments a face gets) come out differently
//! in the browser than on the desktop, and the names built on those
//! tessellations follow. Every transcendental on the
//! geometry path therefore goes through here, and here is the `libm` crate:
//! pure Rust, `+ − × ÷` and integer bit manipulation only, so the same
//! source compiles to the same bits on every IEEE-754 target. Its
//! architecture-specific paths are `sqrt` (an instruction, correctly rounded
//! everywhere) and `fma` (correctly rounded everywhere), which cannot differ.
//!
//! **No FMA contraction.** Rust never fuses `a * b + c` (RFC 3514), on any
//! target and at any optimisation level, so the arithmetic *around* these
//! calls is also identical; [`tests::no_fma_contraction`] proves it on the
//! toolchain that built the test. An explicit `mul_add` would be correctly
//! rounded on both sides too, but it is banned from the geometry path with
//! the direct methods (`//src/rust/cadpole-wasm:transcendental_lint_test`) so
//! that the policy has one exception list rather than two.
//!
//! `sqrt` is IEEE-exact everywhere and stays `f64::sqrt`; it is re-exported
//! so a caller has one module to import. `to_radians` / `to_degrees` are one
//! multiplication each and need no pinning.

/// Sine, radians.
#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// Cosine, radians.
#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// Sine and cosine of one angle, radians.
#[inline]
pub fn sin_cos(x: f64) -> (f64, f64) {
    libm::sincos(x)
}

/// Tangent, radians.
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// Arcsine, radians in `[-π/2, π/2]`; NaN outside `[-1, 1]`.
#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// Arccosine, radians in `[0, π]`; NaN outside `[-1, 1]`.
#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// Arctangent, radians in `(-π/2, π/2)`.
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// Four-quadrant arctangent of `y / x`, radians in `[-π, π]`.
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// Hyperbolic sine.
#[inline]
pub fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}

/// Hyperbolic cosine.
#[inline]
pub fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}

/// Hyperbolic tangent.
#[inline]
pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// `e^x`.
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// `2^x`.
#[inline]
pub fn exp2(x: f64) -> f64 {
    libm::exp2(x)
}

/// Natural logarithm.
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// Base-2 logarithm.
#[inline]
pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

/// Base-10 logarithm.
#[inline]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

/// `x^y`.
#[inline]
pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// `sqrt(x² + y²)` without the intermediate overflow.
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// Cube root.
#[inline]
pub fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

/// `x` to the integer power `n`, by repeated multiplication in one fixed
/// order. `f64::powi` is LLVM's `llvm.powi` intrinsic, which the back end may
/// expand inline or lower to a library call depending on the target and the
/// exponent, and the two need not associate the multiplications the same
/// way; this always does.
#[inline]
pub fn powi(x: f64, n: i32) -> f64 {
    let mut acc = 1.0;
    let mut base = x;
    let mut e = n.unsigned_abs();
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        e >>= 1;
        if e > 0 {
            base *= base;
        }
    }
    if n < 0 { 1.0 / acc } else { acc }
}

/// Square root. IEEE 754 requires it correctly rounded, so the hardware
/// instruction every target has is already bit-identical; this is
/// `f64::sqrt`, here so a caller imports one module.
#[inline]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SplitMix64: a fixed sequence, so a failing input can be reproduced.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        /// Uniform in `[lo, hi)`.
        fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
            let u = (self.next() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
            lo + (hi - lo) * u
        }
    }

    /// Distance in units in the last place between two finite doubles of the
    /// same sign; `u64::MAX` when they are not comparable that way.
    fn ulps(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        if !a.is_finite() || !b.is_finite() || a.is_sign_negative() != b.is_sign_negative() {
            return u64::MAX;
        }
        (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
    }

    /// Run `n` random inputs from `sample` through `ours` and `std`, and
    /// return the worst distance in ulps and the input that produced it.
    fn worst(n: usize, mut sample: impl FnMut(&mut Rng) -> f64, ours: fn(f64) -> f64, std: fn(f64) -> f64) -> (u64, f64) {
        let mut rng = Rng(0x5EED_D185);
        let mut w = (0u64, 0.0f64);
        for _ in 0..n {
            let x = sample(&mut rng);
            let d = ulps(ours(x), std(x));
            if d > w.0 {
                w = (d, x);
            }
        }
        w
    }

    const N: usize = 1_000_000;

    /// The pinned functions agree with the platform's own to within one ulp
    /// on a million inputs each. This is not what makes them deterministic —
    /// bit-identity across targets is proved by
    /// `//src/rust/cadpole-wasm:native_vs_wasm_test`, which runs the same
    /// inputs natively and in wasm — it is what shows the pin costs no
    /// accuracy: both glibc and libm are within one ulp of the true value.
    #[test]
    fn agrees_with_std_within_one_ulp() {
        let big = |r: &mut Rng| r.uniform(-1e4, 1e4);
        let unit = |r: &mut Rng| r.uniform(-1.0, 1.0);
        let pos = |r: &mut Rng| r.uniform(1e-6, 1e4);
        let cases: &[(&str, (u64, f64))] = &[
            ("sin", worst(N, big, sin, f64::sin)),
            ("cos", worst(N, big, cos, f64::cos)),
            ("tan", worst(N, big, tan, f64::tan)),
            ("asin", worst(N, unit, asin, f64::asin)),
            ("acos", worst(N, unit, acos, f64::acos)),
            ("atan", worst(N, big, atan, f64::atan)),
            ("exp", worst(N, |r| r.uniform(-700.0, 700.0), exp, f64::exp)),
            ("exp2", worst(N, |r| r.uniform(-1000.0, 1000.0), exp2, f64::exp2)),
            ("ln", worst(N, pos, ln, f64::ln)),
            ("log2", worst(N, pos, log2, f64::log2)),
            ("log10", worst(N, pos, log10, f64::log10)),
            ("cbrt", worst(N, big, cbrt, f64::cbrt)),
        ];
        let mut bad = Vec::new();
        for (name, (d, x)) in cases {
            if *d > 1 {
                bad.push(format!("{name}({x:e}) differs from std by {d} ulps"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// The hyperbolics: neither glibc nor libm claims better than two ulps
    /// for these (both build them from `exp` and `expm1`, and glibc's own
    /// bound for `sinh` is 2 ulps on x86-64), so they agree to two. Nothing
    /// on the geometry path calls them; they are here so the pinned surface
    /// is complete.
    #[test]
    fn hyperbolics_agree_with_std_within_two_ulps() {
        let mid = |r: &mut Rng| r.uniform(-20.0, 20.0);
        let cases: &[(&str, (u64, f64))] = &[
            ("sinh", worst(N, mid, sinh, f64::sinh)),
            ("cosh", worst(N, mid, cosh, f64::cosh)),
            ("tanh", worst(N, mid, tanh, f64::tanh)),
        ];
        let mut bad = Vec::new();
        for (name, (d, x)) in cases {
            if *d > 2 {
                bad.push(format!("{name}({x:e}) differs from std by {d} ulps"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// The two-argument functions, same bound.
    #[test]
    fn two_argument_functions_agree_with_std_within_one_ulp() {
        let mut rng = Rng(0xD185_0002);
        let mut bad = Vec::new();
        for _ in 0..N {
            let y = rng.uniform(-1e4, 1e4);
            let x = rng.uniform(-1e4, 1e4);
            let d = ulps(atan2(y, x), y.atan2(x));
            if d > 1 {
                bad.push(format!("atan2({y:e}, {x:e}) differs by {d} ulps"));
            }
            let d = ulps(hypot(y, x), y.hypot(x));
            if d > 1 {
                bad.push(format!("hypot({y:e}, {x:e}) differs by {d} ulps"));
            }
            let b = rng.uniform(1e-3, 1e3);
            let e = rng.uniform(-10.0, 10.0);
            let d = ulps(powf(b, e), b.powf(e));
            if d > 1 {
                bad.push(format!("powf({b:e}, {e:e}) differs by {d} ulps"));
            }
            if bad.len() > 20 {
                break;
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// The exact values a modelling script leans on. `sin_cos` is the pair
    /// the single functions give, not a third implementation.
    #[test]
    fn exact_points() {
        use std::f64::consts::{FRAC_PI_2, PI};
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(tan(0.0), 0.0);
        assert_eq!(atan2(0.0, -1.0), PI);
        assert_eq!(atan2(1.0, 0.0), FRAC_PI_2);
        assert_eq!(asin(1.0), FRAC_PI_2);
        assert_eq!(acos(1.0), 0.0);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(log2(8.0), 3.0);
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(powf(2.0, 10.0), 1024.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(cbrt(27.0), 3.0);
        assert_eq!(sqrt(2.0), 2.0f64.sqrt());
        assert_eq!(powi(3.0, 0), 1.0);
        assert_eq!(powi(3.0, 1), 3.0);
        assert_eq!(powi(3.0, 2), 9.0);
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(2.0, -2), 0.25);
        assert_eq!(powi(1.1, 2), 1.1 * 1.1);
        assert_eq!(powi(1.7, 3), 1.7 * (1.7 * 1.7));
        let mut rng = Rng(7);
        for _ in 0..1000 {
            let x = rng.uniform(-1e3, 1e3);
            assert_eq!(sin_cos(x), (sin(x), cos(x)), "sin_cos({x}) is not the pair");
        }
    }

    /// `a * b + c` is two roundings, never one: the toolchain that built this
    /// test does not contract to a fused multiply-add. With `a = 1 + 2⁻⁵²`
    /// and `b = 1 − 2⁻⁵²`, the exact product `1 − 2⁻¹⁰⁴` rounds to exactly
    /// 1, so the unfused sum with `c = −1` is `0`; a fused one would keep the
    /// product exact and give `−2⁻¹⁰⁴`. `black_box` keeps the compiler from
    /// folding the expression at build time, where the answer would say
    /// nothing about the code it generates.
    #[test]
    fn no_fma_contraction() {
        use std::hint::black_box;
        let a = black_box(1.0 + f64::EPSILON);
        let b = black_box(1.0 - f64::EPSILON);
        let c = black_box(-1.0);
        let unfused = a * b + c;
        assert_eq!(unfused, 0.0, "a*b+c was contracted into a fused multiply-add: {unfused:e}");
        // And the fused one, asked for explicitly, is different — which is
        // why `mul_add` is banned on the geometry path with the rest.
        assert_ne!(a.mul_add(b, c), 0.0);
    }
}
