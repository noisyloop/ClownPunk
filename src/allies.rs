//! Friendly clowns: your crew.
//!
//! A few clowns start out with you; more stand waving along the Strip and
//! join when you roll up to them (on the board or in a car).
//!
//! Each crew clown is in one of three modes:
//!   * Waiting  - not recruited yet, just waving at the side of the road
//!   * Following - on foot, hovering around the leader and shooting zombies
//!   * Riding   - you are driving: they pile onto the car (trunk, fenders,
//!                roof) and keep shooting from there
//!
//! The AI is deliberately tiny: steer toward a formation spot around the
//! leader, and every second or so shoot the nearest zombie in sight.
use crate::collision::{resolve_circle, Obb};
use crate::math::{approach, Color, Vec2};
use crate::platform::rand_range;

pub const RADIUS: f32 = 0.45;
pub const MAX_CREW: usize = 8;
/// How close the leader must pass to recruit a waiting clown.
pub const RECRUIT_RANGE: f32 = 9.0;
pub const AIM_RANGE: f32 = 26.0;
const FOLLOW_SPEED: f32 = 8.5;

/// Where riders stand on a car, in the car's frame: (along, right, height).
pub const SEATS: [(f32, f32, f32); MAX_CREW] = [
    (-1.75, -0.5, 1.05),
    (-1.75, 0.5, 1.05),
    (-0.9, -1.3, 0.3),
    (-0.9, 1.3, 0.3),
    (-1.9, -1.3, 0.3),
    (-1.9, 1.3, 0.3),
    (-0.5, -0.45, 1.8),
    (-0.5, 0.45, 1.8),
];

pub struct Ally {
    pub pos: Vec2,
    pub vel: Vec2,
    pub yaw: f32,
    /// Height above the ground (only non-zero while riding a car).
    pub y: f32,
    pub recruited: bool,
    /// Seat index while riding a car.
    pub seat: Option<usize>,
    pub shoot_cd: f32,
    /// Briefly true after a shot so the clown points its pistol.
    pub aim_t: f32,
    pub phase: f32,
    pub suit: Color,
    pub hair: Color,
}

impl Ally {
    pub fn new(pos: Vec2, recruited: bool) -> Ally {
        let suits = [
            Color::rgb(0.95, 0.2, 0.3),
            Color::rgb(0.2, 0.55, 1.0),
            Color::rgb(1.0, 0.75, 0.1),
            Color::rgb(0.7, 0.3, 0.95),
            Color::rgb(0.15, 0.85, 0.55),
            Color::rgb(1.0, 0.45, 0.75),
        ];
        let hairs = [
            Color::rgb(1.0, 0.3, 0.1),
            Color::rgb(0.2, 0.95, 0.4),
            Color::rgb(0.3, 0.7, 1.0),
            Color::rgb(1.0, 0.9, 0.2),
            Color::rgb(0.9, 0.3, 0.9),
        ];
        Ally {
            pos,
            vel: Vec2::ZERO,
            yaw: rand_range(0.0, 6.28),
            y: 0.0,
            recruited,
            seat: None,
            shoot_cd: rand_range(0.4, 1.2),
            aim_t: 0.0,
            phase: rand_range(0.0, 6.28),
            suit: suits[(rand_range(0.0, 6.0) as usize) % 6],
            hair: hairs[(rand_range(0.0, 5.0) as usize) % 5],
        }
    }

    /// One tick of following: hover around `spot`, bumping into `obstacles`.
    /// Far-behind clowns run faster, and absurdly far ones skip ahead.
    pub fn follow(&mut self, dt: f32, spot: Vec2, leader: Vec2, obstacles: &[Obb]) {
        self.shoot_cd -= dt;
        self.aim_t = (self.aim_t - dt).max(0.0);
        let to = spot - self.pos;
        let dist = to.len();
        if leader.dist(self.pos) > 70.0 {
            self.pos = spot; // fell way behind (turbo, a car jump...): catch up
            self.vel = Vec2::ZERO;
        }
        let speed = if dist > 14.0 { FOLLOW_SPEED * 2.4 } else { FOLLOW_SPEED };
        let want = if dist > 0.6 { to.norm() * speed * (dist / 3.0).min(1.0) } else { Vec2::ZERO };
        self.vel += (want - self.vel) * (5.0 * dt).min(1.0);
        self.pos += self.vel * dt;
        resolve_circle(&mut self.pos, RADIUS, obstacles);
        self.y = approach(self.y, 0.0, 12.0 * dt);
        if self.vel.len_sq() > 0.2 && self.aim_t <= 0.0 {
            self.yaw = self.vel.y.atan2(self.vel.x);
        }
        self.phase += dt * (2.0 + self.vel.len() * 1.6);
    }

    /// Waiting clowns just wave.
    pub fn wave(&mut self, dt: f32) {
        self.phase += dt * 3.0;
    }
}
