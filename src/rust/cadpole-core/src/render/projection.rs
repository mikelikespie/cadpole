//! A camera as matrices, for whoever draws over a render: a page laying a
//! perspective-correct ground grid under a transparent frame, a test asking
//! where a world point landed.
//!
//! The renderers never use these. They cast rays from `View`, and
//! `matrices_agree_with_the_renderer` holds the two to the same pixel.

use super::Camera;
use crate::geom::Vec3;

/// A camera's view and projection for an image of a given size.
///
/// Both matrices are row-major 4x4 and multiply column vectors. `view`
/// takes world coordinates to camera space (+x right, +y up, the camera
/// looking down -z); `projection` takes camera space to clip space in the
/// OpenGL convention (`ndc = clip.xyz / clip.w`, each in -1..1, +y up).
/// A pixel is then
///
/// ```text
/// x_px = (ndc.x + 1) / 2 * width
/// y_px = (1 - ndc.y) / 2 * height
/// ```
///
/// with (0, 0) the top-left corner of the image and pixel centres at
/// half-integers.
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub view: [f64; 16],
    pub projection: [f64; 16],
    /// The clip planes the projection was built with, along the view
    /// direction. They bound the depth range only; the renderers clip
    /// nothing.
    pub near: f64,
    pub far: f64,
    /// Orthographic cameras: the height of the frame in world units.
    /// Zero for a perspective camera.
    pub ortho_height: f64,
}

impl Camera {
    fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let fwd = (self.target - self.eye).normalized();
        let right = fwd.cross(self.up).normalized();
        let up = right.cross(fwd);
        (right, up, fwd)
    }

    /// The camera as matrices for a `width` x `height` image. `lo`/`hi`
    /// are the scene's bounds, used only to place the clip planes around it.
    pub fn projection(&self, width: u32, height: u32, lo: Vec3, hi: Vec3) -> Projection {
        let (right, up, fwd) = self.basis();
        let e = self.eye;
        let view = [
            right.x, right.y, right.z, -right.dot(e),
            up.x, up.y, up.z, -up.dot(e),
            -fwd.x, -fwd.y, -fwd.z, fwd.dot(e),
            0.0, 0.0, 0.0, 1.0,
        ];
        let dist = (self.target - self.eye).length().max(1e-9);
        let (mut z0, mut z1) = (f64::MAX, f64::MIN);
        for i in 0..8 {
            let c = Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            let z = (c - e).dot(fwd);
            z0 = z0.min(z);
            z1 = z1.max(z);
        }
        let near = (z0 * 0.5).max(dist * 1e-3);
        let far = (z1 * 4.0).max(dist * 4.0).max(near * 2.0);
        let aspect = width.max(1) as f64 / height.max(1) as f64;
        let tan = (self.fov_degrees.to_radians() * 0.5).tan();
        let (projection, ortho_height) = if self.orthographic {
            let half_h = dist * tan;
            let half_w = half_h * aspect;
            (
                [
                    1.0 / half_w, 0.0, 0.0, 0.0,
                    0.0, 1.0 / half_h, 0.0, 0.0,
                    0.0, 0.0, -2.0 / (far - near), -(far + near) / (far - near),
                    0.0, 0.0, 0.0, 1.0,
                ],
                2.0 * half_h,
            )
        } else {
            let f = 1.0 / tan;
            (
                [
                    f / aspect, 0.0, 0.0, 0.0,
                    0.0, f, 0.0, 0.0,
                    0.0, 0.0, (far + near) / (near - far), 2.0 * far * near / (near - far),
                    0.0, 0.0, -1.0, 0.0,
                ],
                0.0,
            )
        };
        Projection { view, projection, near, far, ortho_height }
    }

    /// Where the world point `p` lands in a `width` x `height` image, in
    /// pixels from the top-left corner; `None` when it is behind a
    /// perspective camera.
    pub fn project(&self, p: Vec3, width: u32, height: u32) -> Option<(f64, f64)> {
        let (right, up, fwd) = self.basis();
        let v = p - self.eye;
        let (x, y, z) = (v.dot(right), v.dot(up), v.dot(fwd));
        let focal = (height as f64 * 0.5) / (self.fov_degrees.to_radians() * 0.5).tan();
        let scale = if self.orthographic {
            focal / (self.target - self.eye).length().max(1e-9)
        } else {
            if z <= 1e-9 {
                return None;
            }
            focal / z
        };
        Some((width as f64 * 0.5 + x * scale, height as f64 * 0.5 - y * scale))
    }
}

impl Projection {
    /// `project` through the matrices, as a reader of the sidecar would.
    pub fn pixel(&self, p: Vec3, width: u32, height: u32) -> Option<(f64, f64)> {
        let mul = |m: &[f64; 16], v: [f64; 4]| -> [f64; 4] {
            let mut o = [0.0; 4];
            for r in 0..4 {
                o[r] = (0..4).map(|c| m[r * 4 + c] * v[c]).sum();
            }
            o
        };
        let clip = mul(&self.projection, mul(&self.view, [p.x, p.y, p.z, 1.0]));
        if clip[3] <= 1e-12 {
            return None;
        }
        let (nx, ny) = (clip[0] / clip[3], clip[1] / clip[3]);
        Some(((nx + 1.0) * 0.5 * width as f64, (1.0 - ny) * 0.5 * height as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{Backend, Item, RenderOptions, render_with_stats};
    use super::*;
    use crate::context::Material;

    /// A small cube away from the middle of the frame is drawn where both
    /// `Camera::project` and the matrices say its centre is, under both
    /// projections and at a size that is not square.
    #[test]
    fn matrices_agree_with_the_renderer() {
        let at = Vec3::new(14.0, -6.0, 9.0);
        let (mut mesh, _) = unit_box();
        for p in mesh.positions.chunks_exact_mut(3) {
            // The unit box, 2 across and centred on `at`.
            p[0] = at.x as f32 + (p[0] - 0.5) * 2.0;
            p[1] = at.y as f32 + (p[1] - 0.5) * 2.0;
            p[2] = at.z as f32 + (p[2] - 0.5) * 2.0;
        }
        let mat = Material { base_color: [0.9, 0.9, 0.9], ..Material::default() };
        let (w, h) = (240u32, 160u32);
        for orthographic in [false, true] {
            let cam = Camera { eye: Vec3::new(60.0, -80.0, 50.0), target: Vec3::new(0.0, 0.0, 5.0), up: Vec3::Z, fov_degrees: 30.0, orthographic };
            let opts = RenderOptions {
                width: w,
                height: h,
                samples: 4,
                transparent: true,
                draw_edges: false,
                draw_grid: false,
                contact_shadow: false,
                backend: Backend::Raster,
                ..RenderOptions::default()
            };
            let img = render_with_stats(&[Item::new(&mesh, &mat, &[])], &cam, &opts).0;
            // The alpha-weighted centroid of what was drawn.
            let (mut sx, mut sy, mut sa) = (0.0, 0.0, 0.0);
            for y in 0..h {
                for x in 0..w {
                    let a = img.rgba[((y * w + x) * 4 + 3) as usize] as f64;
                    sx += (x as f64 + 0.5) * a;
                    sy += (y as f64 + 0.5) * a;
                    sa += a;
                }
            }
            assert!(sa > 0.0, "the cube is in the picture");
            let (cx, cy) = (sx / sa, sy / sa);
            let (px, py) = cam.project(at, w, h).unwrap();
            assert!((px - cx).abs() < 0.75 && (py - cy).abs() < 0.75, "ortho={orthographic}: projected ({px:.2}, {py:.2}), drawn ({cx:.2}, {cy:.2})");
            assert!((px - w as f64 * 0.5).abs() > 20.0, "the point is off-centre, so a flipped axis would show");
            let m = cam.projection(w, h, Vec3::new(13.0, -7.0, 8.0), Vec3::new(15.0, -5.0, 10.0));
            let (mx, my) = m.pixel(at, w, h).unwrap();
            assert!((mx - px).abs() < 1e-6 && (my - py).abs() < 1e-6, "ortho={orthographic}: matrices ({mx}, {my}) vs project ({px}, {py})");
            assert!(m.near > 0.0 && m.far > m.near);
            assert_eq!(m.ortho_height > 0.0, orthographic);
        }
        // Behind a perspective camera there is no pixel.
        let cam = Camera { eye: Vec3::new(0.0, -10.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 30.0, orthographic: false };
        assert_eq!(cam.project(Vec3::new(0.0, -20.0, 0.0), 10, 10), None);
    }
}
