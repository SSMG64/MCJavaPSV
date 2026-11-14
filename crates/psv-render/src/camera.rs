//! A first person camera with the game's angle conventions, and a view frustum.

use crate::math::{Mat4, Vec3, cross, dot, normalize, sub};

/// A comfortable flying speed in blocks per second. It is a starting point, not a measurement
/// of the game's own flying speed.
pub const BASE_SPEED: f32 = 10.0;
pub const SPRINT_FACTOR: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub position: Vec3,
    /// Degrees. 0 faces south (+Z), 90 faces west (-X), 180 faces north, 270 faces east.
    pub yaw: f32,
    /// Degrees. Positive looks down, negative looks up, limited to a straight line either way.
    pub pitch: f32,
    pub fov_y_degrees: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlyInput {
    /// -1 to 1: backward to forward.
    pub forward: f32,
    /// -1 to 1: left to right.
    pub strafe: f32,
    /// -1 to 1: down to up.
    pub rise: f32,
    pub sprint: bool,
}

impl Camera {
    pub fn new(position: Vec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov_y_degrees: 70.0,
            aspect,
            near: 0.05,
            far: 256.0,
        }
    }

    pub fn forward(&self) -> Vec3 {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        [
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        ]
    }

    pub fn right(&self) -> Vec3 {
        normalize(cross(self.forward(), [0.0, 1.0, 0.0]))
    }

    pub fn look(&mut self, delta_yaw: f32, delta_pitch: f32) {
        self.yaw = (self.yaw + delta_yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + delta_pitch).clamp(-90.0, 90.0);
    }

    /// Moves along the view direction, sideways and straight up or down.
    pub fn fly(&mut self, input: FlyInput, seconds: f32) {
        let speed = BASE_SPEED * if input.sprint { SPRINT_FACTOR } else { 1.0 } * seconds;
        let forward = self.forward();
        let right = self.right();
        for axis in 0..3 {
            let mut step = forward[axis] * input.forward + right[axis] * input.strafe;
            if axis == 1 {
                step += input.rise;
            }
            self.position[axis] += step * speed;
        }
    }

    pub fn view(&self) -> Mat4 {
        let f = self.forward();
        let s = normalize(cross(f, [0.0, 1.0, 0.0]));
        let s = if dot(s, s) == 0.0 { [1.0, 0.0, 0.0] } else { s };
        let u = cross(s, f);
        let p = self.position;
        Mat4([
            s[0],
            u[0],
            -f[0],
            0.0,
            s[1],
            u[1],
            -f[1],
            0.0,
            s[2],
            u[2],
            -f[2],
            0.0,
            -dot(s, p),
            -dot(u, p),
            dot(f, p),
            1.0,
        ])
    }

    pub fn projection(&self) -> Mat4 {
        Mat4::perspective(
            self.fov_y_degrees.to_radians(),
            self.aspect,
            self.near,
            self.far,
        )
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection().mul(&self.view())
    }

    pub fn frustum(&self) -> Frustum {
        Frustum::from_matrix(&self.view_projection())
    }
}

/// The six planes of a view volume, each as `[a, b, c, d]` with the inside where
/// `a*x + b*y + c*z + d >= 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    pub planes: [[f32; 4]; 6],
}

impl Frustum {
    pub fn from_matrix(m: &Mat4) -> Self {
        let row = |r: usize| [m.at(r, 0), m.at(r, 1), m.at(r, 2), m.at(r, 3)];
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
        let combine = |a: [f32; 4], sign: f32| std::array::from_fn(|i| r3[i] + sign * a[i]);
        let mut planes = [
            combine(r0, 1.0),
            combine(r0, -1.0),
            combine(r1, 1.0),
            combine(r1, -1.0),
            combine(r2, 1.0),
            combine(r2, -1.0),
        ];
        for plane in &mut planes {
            let length = (plane[0] * plane[0] + plane[1] * plane[1] + plane[2] * plane[2]).sqrt();
            if length > 0.0 {
                plane.iter_mut().for_each(|v| *v /= length);
            }
        }
        Self { planes }
    }

    /// False only when the box is entirely outside one plane, so it never hides something
    /// that is visible.
    pub fn intersects_box(&self, min: Vec3, max: Vec3) -> bool {
        self.planes.iter().all(|p| {
            let far_corner = [
                if p[0] >= 0.0 { max[0] } else { min[0] },
                if p[1] >= 0.0 { max[1] } else { min[1] },
                if p[2] >= 0.0 { max[2] } else { min[2] },
            ];
            p[0] * far_corner[0] + p[1] * far_corner[1] + p[2] * far_corner[2] + p[3] >= 0.0
        })
    }
}

pub fn distance(a: Vec3, b: Vec3) -> f32 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    fn facing(yaw: f32, pitch: f32) -> Vec3 {
        let mut camera = Camera::new([0.0; 3], 1.0);
        camera.yaw = yaw;
        camera.pitch = pitch;
        camera.forward()
    }

    #[test]
    fn yaw_follows_the_game_compass() {
        assert!(near(facing(0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(near(facing(90.0, 0.0), [-1.0, 0.0, 0.0]));
        assert!(near(facing(180.0, 0.0), [0.0, 0.0, -1.0]));
        assert!(near(facing(270.0, 0.0), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn positive_pitch_looks_down() {
        assert!(near(facing(0.0, 90.0), [0.0, -1.0, 0.0]));
        assert!(near(facing(0.0, -90.0), [0.0, 1.0, 0.0]));
    }

    #[test]
    fn east_is_on_the_right_when_facing_north() {
        let mut camera = Camera::new([0.0; 3], 1.0);
        camera.yaw = 180.0;
        assert!(near(camera.right(), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn look_wraps_yaw_and_clamps_pitch() {
        let mut camera = Camera::new([0.0; 3], 1.0);
        camera.look(-10.0, 200.0);
        assert!((camera.yaw - 350.0).abs() < 1e-4);
        assert_eq!(camera.pitch, 90.0);
        camera.look(20.0, -400.0);
        assert!((camera.yaw - 10.0).abs() < 1e-4);
        assert_eq!(camera.pitch, -90.0);
    }

    #[test]
    fn flying_moves_along_the_view_and_sprint_doubles_it() {
        let mut camera = Camera::new([0.0; 3], 1.0);
        camera.fly(
            FlyInput {
                forward: 1.0,
                ..FlyInput::default()
            },
            0.5,
        );
        assert!(near(camera.position, [0.0, 0.0, 5.0]));
        camera.fly(
            FlyInput {
                forward: 1.0,
                sprint: true,
                ..FlyInput::default()
            },
            0.5,
        );
        assert!(near(camera.position, [0.0, 0.0, 15.0]));
        camera.fly(
            FlyInput {
                rise: 1.0,
                strafe: 1.0,
                ..FlyInput::default()
            },
            1.0,
        );
        assert!(near(camera.position, [-10.0, 10.0, 15.0]));
    }

    #[test]
    fn the_view_matrix_puts_the_eye_at_the_origin_looking_down_negative_z() {
        let mut camera = Camera::new([3.0, 4.0, 5.0], 1.0);
        camera.yaw = 90.0;
        camera.pitch = 20.0;
        let view = camera.view();
        let eye = view.transform([3.0, 4.0, 5.0, 1.0]);
        assert!(near([eye[0], eye[1], eye[2]], [0.0; 3]));
        let ahead = camera
            .position
            .iter()
            .zip(camera.forward())
            .map(|(p, f)| p + f * 2.0)
            .collect::<Vec<_>>();
        let seen = view.transform([ahead[0], ahead[1], ahead[2], 1.0]);
        assert!(near([seen[0], seen[1], seen[2]], [0.0, 0.0, -2.0]));
    }

    #[test]
    fn the_frustum_keeps_what_is_ahead_and_drops_the_rest() {
        let mut camera = Camera::new([0.0, 0.0, 0.0], 16.0 / 9.0);
        camera.yaw = 180.0;
        let frustum = camera.frustum();
        let cube = |c: Vec3| {
            frustum.intersects_box(
                [c[0] - 0.5, c[1] - 0.5, c[2] - 0.5],
                [c[0] + 0.5, c[1] + 0.5, c[2] + 0.5],
            )
        };
        assert!(cube([0.0, 0.0, -10.0]), "straight ahead");
        assert!(!cube([0.0, 0.0, 10.0]), "behind");
        assert!(!cube([50.0, 0.0, -5.0]), "far to the side");
        assert!(!cube([0.0, 0.0, -400.0]), "beyond the far plane");
        assert!(
            frustum.intersects_box([-1.0, -1.0, -3.0], [1.0, 1.0, 1.0]),
            "overlapping the eye"
        );
    }

    #[test]
    fn distance_is_euclidean() {
        assert_eq!(distance([0.0; 3], [3.0, 4.0, 0.0]), 5.0);
    }
}
