pub type Vec3 = [f32; 3];

/// A 4x4 matrix stored column by column, the layout OpenGL style APIs take.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);

    pub fn at(&self, row: usize, column: usize) -> f32 {
        self.0[column * 4 + row]
    }

    pub fn translation(t: Vec3) -> Self {
        let mut m = Self::IDENTITY;
        m.0[12..15].copy_from_slice(&t);
        m
    }

    /// `self * other`: `other` is applied first.
    pub fn mul(&self, other: &Mat4) -> Mat4 {
        let mut out = [0.0; 16];
        for column in 0..4 {
            for row in 0..4 {
                out[column * 4 + row] = (0..4).map(|k| self.at(row, k) * other.at(k, column)).sum();
            }
        }
        Mat4(out)
    }

    pub fn transform(&self, p: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0; 4];
        for (row, slot) in out.iter_mut().enumerate() {
            *slot = (0..4).map(|k| self.at(row, k) * p[k]).sum();
        }
        out
    }

    /// OpenGL style perspective: clip depth runs from -1 at the near plane to 1 at the far plane.
    pub fn perspective(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y_radians / 2.0).tan();
        let mut m = [0.0; 16];
        m[0] = f / aspect;
        m[5] = f;
        m[10] = (far + near) / (near - far);
        m[11] = -1.0;
        m[14] = 2.0 * far * near / (near - far);
        Mat4(m)
    }
}

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn normalize(v: Vec3) -> Vec3 {
    let length = dot(v, v).sqrt();
    if length == 0.0 {
        v
    } else {
        [v[0] / length, v[1] / length, v[2] / length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_translation_compose() {
        let t = Mat4::translation([1.0, 2.0, 3.0]);
        assert_eq!(Mat4::IDENTITY.mul(&t), t);
        assert_eq!(t.transform([1.0, 1.0, 1.0, 1.0]), [2.0, 3.0, 4.0, 1.0]);
        let twice = t.mul(&t);
        assert_eq!(twice.transform([0.0, 0.0, 0.0, 1.0]), [2.0, 4.0, 6.0, 1.0]);
    }

    #[test]
    fn perspective_maps_near_and_far_to_clip_depth() {
        let p = Mat4::perspective(1.0, 1.5, 0.1, 100.0);
        let near = p.transform([0.0, 0.0, -0.1, 1.0]);
        let far = p.transform([0.0, 0.0, -100.0, 1.0]);
        assert!((near[2] / near[3] + 1.0).abs() < 1e-4);
        assert!((far[2] / far[3] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn vector_helpers() {
        assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
        assert_eq!(dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]), 32.0);
        assert_eq!(normalize([0.0, 3.0, 4.0]), [0.0, 0.6, 0.8]);
        assert_eq!(normalize([0.0; 3]), [0.0; 3]);
    }
}
