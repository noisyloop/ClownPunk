//! Tiny engine-independent math types.
//!
//! Gameplay code uses these instead of macroquad's types so the game logic has
//! zero dependency on the renderer. World conventions used everywhere:
//!   * +Y is up, the ground is y = 0
//!   * ground-plane positions are `Vec2 { x: world X, y: world Z }`
//!   * a heading `yaw` points along (cos yaw, sin yaw) on the ground plane;
//!     "right" is (-sin yaw, cos yaw). Increasing yaw turns to the right.
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }
    /// Unit vector for a heading angle.
    pub fn from_yaw(yaw: f32) -> Self {
        Vec2::new(yaw.cos(), yaw.sin())
    }
    /// The vector 90 degrees clockwise (to the "right" of a forward vector).
    pub fn right_of(self) -> Self {
        Vec2::new(-self.y, self.x)
    }
    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    pub fn len_sq(self) -> f32 {
        self.dot(self)
    }
    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }
    pub fn dist(self, o: Vec2) -> f32 {
        (self - o).len()
    }
    /// Normalised copy; returns zero for a (near) zero vector instead of NaN.
    pub fn norm(self) -> Vec2 {
        let l = self.len();
        if l < 1e-6 {
            Vec2::ZERO
        } else {
            self * (1.0 / l)
        }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }
    /// Lift a ground-plane point to 3D at height `y`.
    pub fn from_xz(p: Vec2, y: f32) -> Self {
        Vec3::new(p.x, y, p.y)
    }
    pub fn xz(self) -> Vec2 {
        Vec2::new(self.x, self.z)
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> Vec3 {
        let l = self.len();
        if l < 1e-6 {
            Vec3::ZERO
        } else {
            self * (1.0 / l)
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// 3x3 rotation matrix stored as three column vectors (the images of the
/// local X, Y, Z axes). Used to orient boxes: model-local -> world.
#[derive(Clone, Copy, Debug)]
pub struct Mat3 {
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 {
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 1.0, 0.0),
        z: Vec3::new(0.0, 0.0, 1.0),
    };
    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }
    /// self * o  (apply `o` first, then `self`).
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        Mat3 { x: self.mul_vec(o.x), y: self.mul_vec(o.y), z: self.mul_vec(o.z) }
    }
    pub fn rot_x(a: f32) -> Mat3 {
        let (s, c) = a.sin_cos();
        Mat3 { x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, c, s), z: Vec3::new(0.0, -s, c) }
    }
    pub fn rot_y(a: f32) -> Mat3 {
        let (s, c) = a.sin_cos();
        Mat3 { x: Vec3::new(c, 0.0, -s), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(s, 0.0, c) }
    }
    pub fn rot_z(a: f32) -> Mat3 {
        let (s, c) = a.sin_cos();
        Mat3 { x: Vec3::new(c, s, 0.0), y: Vec3::new(-s, c, 0.0), z: Vec3::new(0.0, 0.0, 1.0) }
    }
    /// Orientation for something whose local +X faces along heading `yaw`
    /// (local +Z then points to its right, local +Y stays up).
    pub fn from_yaw(yaw: f32) -> Mat3 {
        Mat3::rot_y(-yaw)
    }
}

/// Linear RGBA colour, 0..1 per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }
    pub fn lerp(self, o: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color {
            r: self.r + (o.r - self.r) * t,
            g: self.g + (o.g - self.g) * t,
            b: self.b + (o.b - self.b) * t,
            a: self.a + (o.a - self.a) * t,
        }
    }
    pub fn scale(self, k: f32) -> Color {
        Color { r: self.r * k, g: self.g * k, b: self.b * k, a: self.a }
    }
}

/// Smallest signed difference between two angles, in (-pi, pi].
pub fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Move `v` toward `target` by at most `step`.
pub fn approach(v: f32, target: f32, step: f32) -> f32 {
    if v < target {
        (v + step).min(target)
    } else {
        (v - step).max(target)
    }
}
