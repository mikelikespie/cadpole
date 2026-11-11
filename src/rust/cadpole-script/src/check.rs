//! Argument validation every geometry builtin shares: numbers and vectors
//! must be finite, and a count a script chooses must be one the process can
//! afford.
//!
//! **Finiteness.** NaN and infinity are refused at the boundary because
//! nothing downstream can cope, and the failure is never where the value was
//! made. `revolve` by a NaN angle never returns — truck's sweep spins forever
//! and the deadline cannot interrupt one native operation — and pinned ten
//! cores of a server until it was restarted; `cylinder(5, inf)` is the same
//! hang. A NaN offset builds a body whose every vertex is NaN, whose `bbox()`
//! is inverted and whose `volume()` is NaN, and `cadpole export` writes the OBJ
//! and exits 0. `math.*` has refused NaN at the source since the drawing
//! scale parser shipped accepting `inf`; this is the same rule for every
//! other builtin, applied through the two conversions every point goes
//! through ([`vec3`], [`p2`]) and [`num`] for the scalars.
//!
//! **Counts.** `pattern_linear(b, 2147483647, ...)` built a `Vec` of two
//! billion transforms — 274 GB — before any limit ran, and the failed
//! allocation aborted the process. The Starlark heap cap does not see a Rust
//! `Vec`, and the deadline only runs between builtins. So a count is capped
//! *before* anything is sized by it, at a number that is still far past what
//! a model needs: a pattern of ten thousand copies is ten thousand bodies.

use cadpole_core::Vec3;
use cadpole_core::sketch::P2;
use starlark::values::float::UnpackFloat;

pub use crate::convert::VecArg;

/// A finite number, or a diagnostic saying which argument was not one.
pub fn finite(v: f64, what: &str) -> anyhow::Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else if v.is_nan() {
        anyhow::bail!("{what} is not a number (NaN)")
    } else {
        anyhow::bail!("{what} is infinite")
    }
}

/// A finite scalar argument. Takes a reference because `UnpackFloat` is not
/// `Copy` and most call sites also want the original for a range check.
pub fn num(v: &UnpackFloat, what: &str) -> anyhow::Result<f64> {
    finite(v.0, what)
}

/// A finite optional scalar argument.
pub fn opt_num(v: &Option<UnpackFloat>, what: &str) -> anyhow::Result<Option<f64>> {
    v.as_ref().map(|v| finite(v.0, what)).transpose()
}

/// A 2- or 3-vector given as a list or tuple of finite numbers.
pub fn vec3(v: &VecArg, what: &str) -> anyhow::Result<Vec3> {
    let p = crate::convert::vec3(v, what)?;
    finite(p.x, &format!("{what}.x"))?;
    finite(p.y, &format!("{what}.y"))?;
    finite(p.z, &format!("{what}.z"))?;
    Ok(p)
}

/// A 2D point given as a list or tuple of two finite numbers.
pub fn p2(v: &VecArg, what: &str) -> anyhow::Result<P2> {
    let p = crate::convert::p2(v, what)?;
    finite(p.x, &format!("{what}.x"))?;
    finite(p.y, &format!("{what}.y"))?;
    Ok(p)
}

/// A count: a whole number from `min` to `max`, checked before anything is
/// allocated by it.
pub fn count(v: &UnpackFloat, what: &str, min: usize, max: usize) -> anyhow::Result<usize> {
    if !v.0.is_finite() || v.0.fract() != 0.0 {
        anyhow::bail!("{what} must be a whole number, got {}", v.0);
    }
    if v.0 < min as f64 {
        anyhow::bail!("{what} must be at least {min}, got {}", v.0);
    }
    if v.0 > max as f64 {
        anyhow::bail!("{what} is {}, over the limit of {max}", v.0);
    }
    Ok(v.0 as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_and_inf_are_named_by_argument() {
        assert!(finite(1.5, "x").is_ok());
        let e = finite(f64::NAN, "extrude: distance").unwrap_err().to_string();
        assert!(e.contains("extrude: distance") && e.contains("NaN"), "{e}");
        let e = finite(f64::INFINITY, "h").unwrap_err().to_string();
        assert!(e.contains("infinite"), "{e}");
        assert!(finite(f64::NEG_INFINITY, "h").is_err());
    }

    #[test]
    fn a_count_is_whole_and_bounded_before_use() {
        assert_eq!(count(&UnpackFloat(3.0), "n", 1, 10).unwrap(), 3);
        assert!(count(&UnpackFloat(2.5), "n", 1, 10).is_err());
        assert!(count(&UnpackFloat(f64::NAN), "n", 1, 10).is_err());
        assert!(count(&UnpackFloat(f64::INFINITY), "n", 1, 10).is_err());
        assert!(count(&UnpackFloat(0.0), "n", 1, 10).is_err());
        let e = count(&UnpackFloat(2147483647.0), "pattern_linear: count", 1, 10_000).unwrap_err().to_string();
        assert!(e.contains("over the limit of 10000"), "{e}");
    }
}
