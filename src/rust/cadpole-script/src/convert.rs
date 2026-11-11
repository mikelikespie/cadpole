//! Conversions between Starlark values and core types.

use cadpole_core::Vec3;
use cadpole_core::sketch::P2;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::{Heap, Value};

/// A 2- or 3-vector given as a list or tuple of numbers.
pub type VecArg = UnpackListOrTuple<UnpackFloat>;

pub fn vec3(v: &VecArg, what: &str) -> anyhow::Result<Vec3> {
    match v.items.as_slice() {
        [x, y] => Ok(Vec3::new(x.0, y.0, 0.0)),
        [x, y, z] => Ok(Vec3::new(x.0, y.0, z.0)),
        other => anyhow::bail!("{what} must have 2 or 3 numbers, got {}", other.len()),
    }
}

pub fn p2(v: &VecArg, what: &str) -> anyhow::Result<P2> {
    match v.items.as_slice() {
        [x, y] => Ok(P2::new(x.0, y.0)),
        other => anyhow::bail!("{what} must be a 2D point (x, y), got {} numbers", other.len()),
    }
}

pub fn alloc_vec3<'v>(heap: Heap<'v>, v: Vec3) -> Value<'v> {
    heap.alloc((v.x, v.y, v.z))
}

pub fn alloc_str_list<'v>(heap: Heap<'v>, items: &[String]) -> Value<'v> {
    heap.alloc(items.iter().map(|s| heap.alloc(s.as_str())).collect::<Vec<_>>())
}

/// Parse a color from `(r, g, b)` in 0..1 or `"#rrggbb"`.
pub fn color(v: Value<'_>) -> anyhow::Result<[f64; 3]> {
    if let Some(s) = v.unpack_str() {
        let s = s.trim_start_matches('#');
        if s.len() != 6 {
            anyhow::bail!("color string must look like #rrggbb");
        }
        let c = u32::from_str_radix(s, 16).map_err(|_| anyhow::anyhow!("bad hex color {s:?}"))?;
        // Hex bytes are sRGB; `Material::base_color` is linear, because the
        // renderer multiplies it by light. Only the byte form is decoded: a
        // tuple is linear already, as documented.
        let channel = |shift: u32| cadpole_core::paint::srgb_to_linear(((c >> shift) & 0xff) as f64 / 255.0);
        return Ok([channel(16), channel(8), channel(0)]);
    }
    let arg = starlark::values::UnpackValue::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("color must be (r, g, b) or \"#rrggbb\""))?;
    let c: VecArg = arg;
    match c.items.as_slice() {
        // Every other number a builtin takes refuses NaN and infinity
        // (`check::num`); a colour reached the material unchecked, so
        // `material(color=(nan, 0, 0))` stored a NaN that JSON shows as null.
        [r, g, b] => {
            for (channel, v) in [("r", r.0), ("g", g.0), ("b", b.0)] {
                if !v.is_finite() {
                    anyhow::bail!("color channel {channel} is {v}, not a finite number");
                }
            }
            Ok([r.0, g.0, b.0])
        }
        _ => anyhow::bail!("color tuple must have 3 numbers"),
    }
}
