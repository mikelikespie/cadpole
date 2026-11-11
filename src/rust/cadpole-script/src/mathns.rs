//! The `math` namespace (docs/starlark-api.md).
//!
//! Starlark has integer and float arithmetic and nothing else: no square root,
//! no trigonometry, not even a constant for pi. That is fine for a
//! configuration language and a real problem for a parametric modelling one.
//! Every script in `examples/` that needed a chord length or an angle had to
//! work around it — `gridfinity_bin.star` carries a hand-written table of
//! sines for its quarter arc, and `gridfinity_bin_future.star` hard-codes
//! `TAN_22_5 = 0.41421356` with a comment saying why. Those are not
//! parametric: change the step count and the table is wrong.
//!
//! **Angles are degrees.** Every angle a Cadpole script writes is in degrees —
//! `rotate(45)`, `revolve(angle=90)`, the crease threshold, the camera's
//! `el=-28` — so `math.sin(45)` means the same 45 as `rotate(45)` does. That
//! is the opposite of C and of Python's `math`, and it is a deliberate choice
//! of internal consistency over external familiarity: a script that mixes the
//! two conventions produces a wrong model silently, and the convention a
//! reader of *this* language will assume is the one the rest of *this*
//! language uses. `math.radians` and `math.degrees` convert for anyone who
//! needs the other one, and every function below says which it takes.
//!
//! Nothing here is a feature and nothing here touches the document: these are
//! pure functions on numbers, so they record nothing in the feature tree.

use starlark::environment::GlobalsBuilder;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;

/// Refuse a non-finite result rather than letting it reach the kernel.
///
/// A NaN coordinate does not fail where it is made; it fails much later, as a
/// degenerate face or an empty boolean, with nothing pointing back here. The
/// scale parser in the drawing pipeline shipped with exactly that bug — it
/// accepted `inf` and wrote a PDF no reader could open — so this checks at the
/// source. The geometry builtins apply the same rule to their arguments
/// through [`crate::check`], which is where the check now lives.
use crate::check::finite;
use cadpole_core::fmath;

fn nonneg(v: f64, what: &str) -> anyhow::Result<f64> {
    if v < 0.0 {
        anyhow::bail!("{what} must not be negative, got {v}");
    }
    finite(v, what)
}

#[starlark_module]
pub(crate) fn math_namespace(builder: &mut GlobalsBuilder) {
    /// The ratio of a circle's circumference to its diameter.
    const pi: f64 = std::f64::consts::PI;
    /// Two pi: a whole turn in radians. `revolve` takes degrees, so this is
    /// for arithmetic rather than for calls.
    const tau: f64 = std::f64::consts::TAU;
    /// The base of the natural logarithm.
    const e: f64 = std::f64::consts::E;

    /// Square root. Negative input is an error, not a NaN that travels.
    fn sqrt(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(nonneg(x.0, "math.sqrt")?.sqrt())
    }

    /// Cube root. Defined for negative numbers, unlike `sqrt`.
    fn cbrt(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(fmath::cbrt(finite(x.0, "math.cbrt")?))
    }

    /// Length of the vector `(x, y)`: `sqrt(x*x + y*y)`, without the overflow.
    /// This is the one most scripts actually want — a chord length, the
    /// distance between two points of a profile.
    fn hypot(#[starlark(require = pos)] x: UnpackFloat, #[starlark(require = pos)] y: UnpackFloat) -> anyhow::Result<f64> {
        Ok(fmath::hypot(finite(x.0, "math.hypot x")?, finite(y.0, "math.hypot y")?))
    }

    /// `x` raised to the power `y`.
    fn pow(#[starlark(require = pos)] x: UnpackFloat, #[starlark(require = pos)] y: UnpackFloat) -> anyhow::Result<f64> {
        finite(fmath::powf(finite(x.0, "math.pow x")?, finite(y.0, "math.pow y")?), "math.pow result")
    }

    /// Sine of an angle **in degrees**.
    fn sin(#[starlark(require = pos)] degrees: UnpackFloat) -> anyhow::Result<f64> {
        Ok(fmath::sin(finite(degrees.0, "math.sin")?.to_radians()))
    }

    /// Cosine of an angle **in degrees**.
    fn cos(#[starlark(require = pos)] degrees: UnpackFloat) -> anyhow::Result<f64> {
        Ok(fmath::cos(finite(degrees.0, "math.cos")?.to_radians()))
    }

    /// Tangent of an angle **in degrees**. An angle at or near 90 degrees,
    /// where the tangent is unbounded, is an error rather than a huge number
    /// that becomes a wild coordinate.
    fn tan(#[starlark(require = pos)] degrees: UnpackFloat) -> anyhow::Result<f64> {
        let d = finite(degrees.0, "math.tan")?;
        let t = fmath::tan(d.to_radians());
        if t.abs() > 1e12 {
            anyhow::bail!("math.tan({d}) is unbounded: the angle is a quarter turn from the axis");
        }
        Ok(t)
    }

    /// Angle **in degrees** whose sine is `x`. `x` outside -1..1 is an error.
    fn asin(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        let x = finite(x.0, "math.asin")?;
        if !(-1.0..=1.0).contains(&x) {
            anyhow::bail!("math.asin({x}) is undefined: the argument must be between -1 and 1");
        }
        Ok(fmath::asin(x).to_degrees())
    }

    /// Angle **in degrees** whose cosine is `x`. `x` outside -1..1 is an error.
    fn acos(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        let x = finite(x.0, "math.acos")?;
        if !(-1.0..=1.0).contains(&x) {
            anyhow::bail!("math.acos({x}) is undefined: the argument must be between -1 and 1");
        }
        Ok(fmath::acos(x).to_degrees())
    }

    /// Angle **in degrees** whose tangent is `x`, between -90 and 90.
    fn atan(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(fmath::atan(finite(x.0, "math.atan")?).to_degrees())
    }

    /// Angle **in degrees** of the vector `(x, y)` measured from the +X axis,
    /// between -180 and 180. Note the argument order: `y` first, as everywhere
    /// else this function exists.
    fn atan2(#[starlark(require = pos)] y: UnpackFloat, #[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        let (y, x) = (finite(y.0, "math.atan2 y")?, finite(x.0, "math.atan2 x")?);
        Ok(fmath::atan2(y, x).to_degrees())
    }

    /// Degrees to radians, for arithmetic that wants them.
    fn radians(#[starlark(require = pos)] degrees: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(degrees.0, "math.radians")?.to_radians())
    }

    /// Radians to degrees.
    fn degrees(#[starlark(require = pos)] radians: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(radians.0, "math.degrees")?.to_degrees())
    }

    /// `e` raised to the power `x`.
    fn exp(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        finite(fmath::exp(finite(x.0, "math.exp")?), "math.exp result")
    }

    /// Logarithm of `x`. Natural by default; pass `base` for another.
    fn log(
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = named)] base: Option<UnpackFloat>,
    ) -> anyhow::Result<f64> {
        let x = finite(x.0, "math.log")?;
        if x <= 0.0 {
            anyhow::bail!("math.log({x}) is undefined: the argument must be positive");
        }
        match base {
            None => Ok(fmath::ln(x)),
            Some(b) => {
                let b = finite(b.0, "math.log base")?;
                if b <= 0.0 || b == 1.0 {
                    anyhow::bail!("math.log base {b} is undefined: it must be positive and not 1");
                }
                Ok(x.log(b))
            }
        }
    }

    /// Largest whole number not greater than `x`.
    fn floor(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(x.0, "math.floor")?.floor())
    }

    /// Smallest whole number not less than `x`.
    fn ceil(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(x.0, "math.ceil")?.ceil())
    }

    /// `x` rounded to `digits` decimal places (0 by default), half away from
    /// zero. Useful for keeping a generated name or a label stable when a
    /// parameter has a long tail.
    fn round(
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = pos, default = 0)] digits: i32,
    ) -> anyhow::Result<f64> {
        let x = finite(x.0, "math.round")?;
        if !(-15..=15).contains(&digits) {
            anyhow::bail!("math.round digits must be between -15 and 15, got {digits}");
        }
        let f = fmath::powi(10f64, digits);
        finite((x * f).round() / f, "math.round result")
    }

    /// `x` with any fractional part discarded, towards zero.
    fn trunc(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(x.0, "math.trunc")?.trunc())
    }

    /// Magnitude of `x`, without its sign.
    fn fabs(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        Ok(finite(x.0, "math.fabs")?.abs())
    }

    /// -1, 0 or 1, following the sign of `x`. Zero has sign zero, which is
    /// what a script branching on direction wants; `(-0.0)` is also zero.
    fn sign(#[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<f64> {
        let x = finite(x.0, "math.sign")?;
        Ok(if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        })
    }

    /// `x` held between `lo` and `hi`. `lo` above `hi` is an error rather than
    /// a silently empty range.
    fn clamp(
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = pos)] lo: UnpackFloat,
        #[starlark(require = pos)] hi: UnpackFloat,
    ) -> anyhow::Result<f64> {
        let (x, lo, hi) = (finite(x.0, "math.clamp x")?, finite(lo.0, "math.clamp lo")?, finite(hi.0, "math.clamp hi")?);
        if lo > hi {
            anyhow::bail!("math.clamp: lo {lo} is above hi {hi}");
        }
        Ok(x.clamp(lo, hi))
    }

    /// Linear blend: `a` at `t = 0`, `b` at `t = 1`. `t` outside 0..1
    /// extrapolates, which is occasionally what you want, so it is allowed.
    fn lerp(
        #[starlark(require = pos)] a: UnpackFloat,
        #[starlark(require = pos)] b: UnpackFloat,
        #[starlark(require = pos)] t: UnpackFloat,
    ) -> anyhow::Result<f64> {
        let (a, b, t) = (finite(a.0, "math.lerp a")?, finite(b.0, "math.lerp b")?, finite(t.0, "math.lerp t")?);
        finite(a + (b - a) * t, "math.lerp result")
    }

    /// True when `a` and `b` agree to within `tol` (1e-9 by default).
    ///
    /// Comparing two computed lengths with `==` is a bug waiting for a
    /// parameter change, and this is the thing to reach for instead.
    fn close(
        #[starlark(require = pos)] a: UnpackFloat,
        #[starlark(require = pos)] b: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(1e-9))] tol: UnpackFloat,
    ) -> anyhow::Result<bool> {
        let (a, b) = (finite(a.0, "math.close a")?, finite(b.0, "math.close b")?);
        Ok((a - b).abs() <= nonneg(tol.0, "math.close tol")?)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    /// The identities a modelling script actually leans on, checked through
    /// Starlark rather than against the Rust functions, because what matters is
    /// that the namespace is reachable and the argument conventions are the
    /// documented ones.
    #[test]
    fn the_math_namespace_computes_what_it_says() {
        let ctx = run(r#"
assert_true(math.close(math.sqrt(2) * math.sqrt(2), 2, tol=1e-12), msg="sqrt")
assert_true(math.close(math.hypot(3, 4), 5), msg="hypot")
assert_true(math.close(math.cbrt(-27), -3, tol=1e-12), msg="cbrt of a negative")
assert_true(math.close(math.pow(2, 10), 1024), msg="pow")

# Angles are DEGREES, deliberately: 45 here is the same 45 as rotate(45).
assert_true(math.close(math.sin(30), 0.5, tol=1e-12), msg="sin takes degrees")
assert_true(math.close(math.cos(60), 0.5, tol=1e-12), msg="cos takes degrees")
assert_true(math.close(math.atan2(1, 1), 45), msg="atan2 returns degrees")
assert_true(math.close(math.asin(0.5), 30, tol=1e-12), msg="asin returns degrees")
assert_true(math.close(math.acos(0.5), 60, tol=1e-12), msg="acos returns degrees")

# The constant gridfinity_bin_future.star hard-codes, computed instead.
assert_true(math.close(math.tan(22.5), 0.41421356, tol=1e-8), msg="tan 22.5")

assert_true(math.close(math.radians(180), math.pi), msg="radians")
assert_true(math.close(math.degrees(math.pi), 180), msg="degrees")
assert_true(math.close(math.tau, 2 * math.pi), msg="tau")
assert_true(math.close(math.log(math.e), 1), msg="natural log")
assert_true(math.close(math.log(8, base=2), 3, tol=1e-12), msg="log base 2")
assert_true(math.close(math.exp(0), 1), msg="exp")

assert_eq(math.floor(-1.2), -2.0)
assert_eq(math.ceil(-1.2), -1.0)
assert_eq(math.trunc(-1.8), -1.0)
assert_eq(math.round(2.34567, 3), 2.346)
assert_eq(math.round(2.5), 3.0)
assert_eq(math.fabs(-3), 3.0)
assert_eq(math.sign(-2), -1.0)
assert_eq(math.sign(0), 0.0)
assert_eq(math.clamp(9, 0, 5), 5.0)
assert_eq(math.lerp(10, 20, 0.25), 12.5)
assert_true(not math.close(0.1, 0.2), msg="close is not always true")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// Every domain error is a diagnostic naming the call and the value, never
    /// a NaN that travels into the kernel and fails somewhere unrelated.
    #[test]
    fn a_domain_error_is_a_diagnostic_not_a_nan() {
        for (src, want) in [
            ("x = math.sqrt(-1)", "must not be negative"),
            ("x = math.asin(2)", "between -1 and 1"),
            ("x = math.acos(-2)", "between -1 and 1"),
            ("x = math.tan(90)", "unbounded"),
            ("x = math.tan(-270)", "unbounded"),
            ("x = math.log(0)", "must be positive"),
            ("x = math.log(8, base=1)", "not 1"),
            ("x = math.clamp(1, 5, 0)", "is above hi"),
            ("x = math.round(1.0, 40)", "between -15 and 15"),
            ("x = math.close(1, 1, tol=-1)", "must not be negative"),
        ] {
            let ctx = run(src);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(want)), "{src:?} should say {want:?}, said {errs:?}");
        }
    }

    /// The reason this namespace exists: without it a profile that needs a
    /// chord length carries a hand-written table, which stops being
    /// parametric the moment the step count changes.
    #[test]
    fn a_polygon_can_be_generated_rather_than_tabulated() {
        let ctx = run(r#"
def ngon(n, r):
    pts = []
    for i in range(n):
        a = 360.0 * i / n
        pts.append((r * math.cos(a), r * math.sin(a)))
    return pts

# A regular pentagon with a vertex at angle 0 is deliberately asymmetric, so
# its bounding box pins down cos and sin rather than just their symmetry:
# x spans cos(180) .. 1 and y spans -sin(72) .. sin(72), times r.
r = 10.0
body = extrude(sketch(XY).polygon(ngon(5, r)), 5.0, name="pentagon")
lo, hi = body.bbox()
# 1e-5, because a bounding box is measured off the mesh and mesh positions are
# f32: 10 mm is exact there, -8.09016994... is not.
assert_true(math.close(hi[0], r, tol=1e-5), msg="pentagon max x")
assert_true(math.close(lo[0], r * math.cos(144), tol=1e-5), msg="pentagon min x")
assert_true(math.close(hi[1], r * math.sin(72), tol=1e-5), msg="pentagon max y")
assert_true(math.close(lo[1], -r * math.sin(72), tol=1e-5), msg="pentagon min y")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }
}
