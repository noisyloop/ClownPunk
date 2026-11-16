//! Cars: parked in the lot, hold zombies in the trunk, and drive arcade-style.
//!
//! DRIVING MODEL (arcade, no real physics)
//!   * one scalar `speed` along the heading; W/S change it, drag coasts it down
//!   * steering turns the heading at a rate proportional to speed, so a
//!     stationary car can't spin on the spot
//!   * collision: 3 circles along the car's length vs the static boxes; on
//!     contact we push the car out and delete the speed pointing into the wall
use crate::collision::{resolve_circle, Obb};
use crate::math::{approach, Color, Vec2};
use crate::platform::Input;

pub const HALF: Vec2 = Vec2::new(2.2, 1.0);
pub const HEIGHT: f32 = 1.5;
pub const CAPACITY: u8 = 3;
pub const MAX_SPEED: f32 = 30.0;
const MAX_REVERSE: f32 = 8.0;
const CIRCLE_R: f32 = 1.05;
const CIRCLE_OFFSETS: [f32; 3] = [-1.15, 0.0, 1.15];

pub struct Car {
    pub pos: Vec2,
    pub yaw: f32,
    pub speed: f32,
    /// Smoothed steering input, -1..1 (wheel visuals).
    pub steer: f32,
    pub color: Color,
    pub trunk: u8,
    /// 0 closed .. 1 fully open, animated toward `open_timer > 0`.
    pub trunk_open: f32,
    pub open_timer: f32,
    pub wheel_spin: f32,
}

impl Car {
    pub fn new(pos: Vec2, yaw: f32, color: Color) -> Car {
        Car { pos, yaw, speed: 0.0, steer: 0.0, color, trunk: 0, trunk_open: 0.0, open_timer: 0.0, wheel_spin: 0.0 }
    }

    pub fn forward(&self) -> Vec2 {
        Vec2::from_yaw(self.yaw)
    }
    pub fn right(&self) -> Vec2 {
        self.forward().right_of()
    }
    pub fn obb(&self) -> Obb {
        Obb::new(self.pos, HALF, self.yaw, HEIGHT)
    }
    /// Where you stand to load the trunk (behind the rear bumper).
    pub fn rear_point(&self) -> Vec2 {
        self.pos - self.forward() * (HALF.x + 0.8)
    }
    /// Driver door: left side, just ahead of the middle.
    pub fn door_point(&self) -> Vec2 {
        self.pos - self.right() * (HALF.y + 0.8) + self.forward() * 0.4
    }
    pub fn drivable(&self) -> bool {
        self.trunk >= CAPACITY
    }

    pub fn open_trunk_briefly(&mut self) {
        self.open_timer = 0.9;
    }

    /// Driver controls -> speed and heading. Returns impact speed if we hit something.
    pub fn update_driven(&mut self, input: &Input, dt: f32, obstacles: &[Obb]) -> f32 {
        // Throttle / brake / reverse. Acceleration tapers near top speed.
        if input.forward {
            if self.speed < 0.0 {
                self.speed += 30.0 * dt; // braking out of reverse
            } else {
                self.speed += 14.0 * (1.0 - 0.6 * self.speed / MAX_SPEED) * dt;
            }
        } else if input.back {
            self.speed -= if self.speed > 0.5 { 28.0 } else { 9.0 } * dt;
        } else {
            self.speed = approach(self.speed, 0.0, 4.0 * dt); // coast
        }
        self.speed = self.speed.clamp(-MAX_REVERSE, MAX_SPEED);

        // Steering: yaw rate scales with speed, and eases off when very fast.
        let steer_in = (input.right as i32 - input.left as i32) as f32;
        self.steer = approach(self.steer, steer_in, 5.0 * dt);
        let grip = (self.speed / 9.0).clamp(-1.0, 1.0);
        self.yaw += self.steer * 1.7 * grip * (1.0 - 0.35 * self.speed.abs() / MAX_SPEED) * dt;

        self.integrate(dt, obstacles)
    }

    /// Nobody inside: coast to a stop.
    pub fn update_parked(&mut self, dt: f32, obstacles: &[Obb]) {
        self.speed = approach(self.speed, 0.0, 12.0 * dt);
        self.steer = approach(self.steer, 0.0, 3.0 * dt);
        if self.speed != 0.0 {
            self.integrate(dt, obstacles);
        }
        self.open_timer = (self.open_timer - dt).max(0.0);
        let target = if self.open_timer > 0.0 { 1.0 } else { 0.0 };
        self.trunk_open = approach(self.trunk_open, target, dt * 4.0);
    }

    pub fn tick_trunk_anim(&mut self, dt: f32) {
        self.open_timer = (self.open_timer - dt).max(0.0);
        let target = if self.open_timer > 0.0 { 1.0 } else { 0.0 };
        self.trunk_open = approach(self.trunk_open, target, dt * 4.0);
    }

    /// Move along the heading and collide. Returns the impact speed (0 if none).
    fn integrate(&mut self, dt: f32, obstacles: &[Obb]) -> f32 {
        let fwd = self.forward();
        self.pos += fwd * self.speed * dt;
        self.wheel_spin += self.speed * dt;

        let mut normal_sum = Vec2::ZERO;
        for off in CIRCLE_OFFSETS {
            let mut c = self.pos + fwd * off;
            let n = resolve_circle(&mut c, CIRCLE_R, obstacles);
            if n.len_sq() > 0.0 {
                self.pos = c - fwd * off;
                normal_sum += n;
            }
        }
        let mut impact = 0.0;
        if normal_sum.len_sq() > 0.0 {
            let n = normal_sum.norm();
            let v = fwd * self.speed;
            let vn = v.dot(n);
            if vn < 0.0 {
                // Remove the part of the velocity that points into the obstacle.
                let slid = v - n * vn;
                self.speed = slid.dot(fwd) * 0.92;
                impact = -vn;
            }
        }
        impact
    }

    /// Centres of the three collision circles (also used for zombie roadkill).
    pub fn circles(&self) -> [Vec2; 3] {
        let f = self.forward();
        [self.pos + f * CIRCLE_OFFSETS[0], self.pos + f * CIRCLE_OFFSETS[1], self.pos + f * CIRCLE_OFFSETS[2]]
    }
    pub const fn circle_radius() -> f32 {
        CIRCLE_R
    }
}
