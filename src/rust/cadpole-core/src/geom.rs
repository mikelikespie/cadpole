//! Small geometry helpers on top of truck's cgmath types.

use crate::fmath;

pub use truck_modeling::{Matrix4, Point3, Rad, Vector3};

/// Plain 3-vector used at API boundaries (Starlark tuples, protobuf).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const X: Vec3 = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l == 0.0 { self } else { self * (1.0 / l) }
    }
    pub fn point(self) -> Point3 {
        Point3::new(self.x, self.y, self.z)
    }
    pub fn vector(self) -> Vector3 {
        Vector3::new(self.x, self.y, self.z)
    }
    pub fn from_point(p: Point3) -> Vec3 {
        Vec3::new(p.x, p.y, p.z)
    }
    pub fn from_vector(v: Vector3) -> Vec3 {
        Vec3::new(v.x, v.y, v.z)
    }
    pub fn to_proto(self) -> cadpole_proto::v1::Vec3 {
        cadpole_proto::v1::Vec3 { x: self.x, y: self.y, z: self.z }
    }
    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl std::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl std::ops::Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// A sketch plane: origin, normal, and the in-plane x direction. The in-plane
/// y direction is `normal x x_dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub origin: Vec3,
    pub normal: Vec3,
    pub x_dir: Vec3,
}

impl Plane {
    /// XY plane: sketch x -> world X, sketch y -> world Y, normal +Z.
    pub const XY: Plane = Plane { origin: Vec3::ZERO, normal: Vec3::Z, x_dir: Vec3::X };
    /// XZ plane: sketch x -> world X, sketch y -> world Z, normal -Y.
    pub const XZ: Plane =
        Plane { origin: Vec3::ZERO, normal: Vec3 { x: 0.0, y: -1.0, z: 0.0 }, x_dir: Vec3::X };
    /// YZ plane: sketch x -> world Y, sketch y -> world Z, normal +X.
    pub const YZ: Plane = Plane { origin: Vec3::ZERO, normal: Vec3::X, x_dir: Vec3::Y };

    pub fn new(origin: Vec3, normal: Vec3, x_dir: Vec3) -> crate::Result<Plane> {
        let normal = normal.normalized();
        if normal.length() == 0.0 {
            return Err(crate::CadpoleError::invalid("plane normal must be non-zero"));
        }
        // Gram-Schmidt x_dir against the normal.
        let x_dir = (x_dir - normal * x_dir.dot(normal)).normalized();
        if x_dir.length() == 0.0 {
            return Err(crate::CadpoleError::invalid("plane x_dir must not be parallel to normal"));
        }
        Ok(Plane { origin, normal, x_dir })
    }

    pub fn y_dir(&self) -> Vec3 {
        self.normal.cross(self.x_dir)
    }

    /// Map 2D sketch coordinates to world space.
    pub fn to_world(&self, x: f64, y: f64) -> Vec3 {
        self.origin + self.x_dir * x + self.y_dir() * y
    }

    /// A parallel plane shifted along the normal.
    pub fn offset(&self, d: f64) -> Plane {
        Plane { origin: self.origin + self.normal * d, ..*self }
    }

    pub fn to_proto(&self) -> cadpole_proto::v1::Plane {
        cadpole_proto::v1::Plane {
            origin: Some(self.origin.to_proto()),
            normal: Some(self.normal.to_proto()),
            x_dir: Some(self.x_dir.to_proto()),
        }
    }
}

/// Rigid transform as a 4x4 matrix (row-major cgmath semantics handled inside).
#[derive(Clone, Copy, Debug)]
pub struct Transform(pub Matrix4);

/// The rotation matrix of `angle_rad` about the unit vector `axis`: cgmath's
/// `Matrix4::from_axis_angle`, entry for entry, except that the sine and
/// cosine come from [`crate::fmath`]. cgmath's `Rad::sin_cos` is `f64::sin_cos`
/// -- glibc natively, the bundled libm in wasm -- and a rotation was the one
/// transcendental on the geometry path that a method-name search could not
/// see.
fn axis_angle(axis: Vec3, angle_rad: f64) -> Matrix4 {
    let (s, c) = crate::fmath::sin_cos(angle_rad);
    let k = 1.0 - c;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    #[rustfmt::skip]
    let m = Matrix4::new(
        k * x * x + c,     k * x * y + s * z, k * x * z - s * y, 0.0,
        k * x * y - s * z, k * y * y + c,     k * y * z + s * x, 0.0,
        k * x * z + s * y, k * y * z - s * x, k * z * z + c,     0.0,
        0.0,               0.0,               0.0,               1.0,
    );
    m
}

impl Transform {
    pub fn identity() -> Transform {
        Transform(Matrix4::from_scale(1.0))
    }
    pub fn translation(v: Vec3) -> Transform {
        Transform(Matrix4::from_translation(v.vector()))
    }
    /// Rotation of `angle_rad` about `axis` through `origin`.
    pub fn rotation(origin: Vec3, axis: Vec3, angle_rad: f64) -> Transform {
        let t = Matrix4::from_translation(origin.vector());
        let r = axis_angle(axis.normalized(), angle_rad);
        let ti = Matrix4::from_translation(-origin.vector());
        Transform(t * r * ti)
    }
    pub fn then(self, next: Transform) -> Transform {
        Transform(next.0 * self.0)
    }

    /// Apply to a point.
    pub fn point(&self, p: Vec3) -> Vec3 {
        let v = self.0 * truck_modeling::Vector4::new(p.x, p.y, p.z, 1.0);
        Vec3::new(v.x / v.w, v.y / v.w, v.z / v.w)
    }

    /// Apply to a direction (normals of a rigid transform; no shear expected).
    pub fn direction(&self, d: Vec3) -> Vec3 {
        let v = self.0 * truck_modeling::Vector4::new(d.x, d.y, d.z, 0.0);
        Vec3::new(v.x, v.y, v.z).normalized()
    }

    /// Uniform scale factor implied by the matrix (1 for rigid transforms),
    /// used to scale cached areas and lengths.
    pub fn scale_factor(&self) -> f64 {
        let x = self.direction_unnormalized(Vec3::X).length();
        let y = self.direction_unnormalized(Vec3::Y).length();
        let z = self.direction_unnormalized(Vec3::Z).length();
        fmath::cbrt((x * y * z).abs())
    }

    fn direction_unnormalized(&self, d: Vec3) -> Vec3 {
        let v = self.0 * truck_modeling::Vector4::new(d.x, d.y, d.z, 0.0);
        Vec3::new(v.x, v.y, v.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_axes_are_right_handed() {
        assert_eq!(Plane::XY.y_dir(), Vec3::Y);
        assert_eq!(Plane::XZ.y_dir(), Vec3::Z);
        assert_eq!(Plane::YZ.y_dir(), Vec3::Z);
    }

    #[test]
    fn plane_offset_moves_along_normal() {
        let p = Plane::XY.offset(5.0);
        assert_eq!(p.origin, Vec3::new(0.0, 0.0, 5.0));
        assert_eq!(p.to_world(1.0, 2.0), Vec3::new(1.0, 2.0, 5.0));
    }
}
