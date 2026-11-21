//! Cars: parked in the lot, and drive arcade-style.
//!
//! DRIVING MODEL (arcade, no real physics)
//!   * one scalar `speed` along the heading; W/S change it, drag coasts it down
//!   * steering turns the heading at a rate proportional to speed, so a
//!     stationary car can't spin on the spot
//!   * collision: 3 circles along the car's length vs the static boxes; on
//!     contact we push the car out and delete the speed pointing into the wall
//!   * TURBO (hold Shift): a nitro tank that drains while burning and refills
//!     while you let off; it pushes the car well past the normal top speed
//!   * RAMPS: the car has a height `y`. On a ramp it follows the surface; when
//!     the surface ends it keeps its upward speed and flies (simple gravity).
//!     In the air obstacles lower than the car are ignored, steering is looser,
//!     and a landing assist straightens the car out along the road, so a jump
//!     always ends on the road facing the right way.
use crate::collision::{resolve_circle, Obb};
use crate::math::{angle_diff, approach, Color, Vec2};
use crate::platform::Input;
use crate::world::{self, World};

pub const HALF: Vec2 = Vec2::new(2.2, 1.0);
pub const HEIGHT: f32 = 1.5;
pub const MAX_SPEED: f32 = 18.0;
/// Top speed while the turbo is burning.
pub const TURBO_SPEED: f32 = 31.0;
const TURBO_DRAIN: f32 = 0.28; // tank fractions per second (about 3.5 s)
const TURBO_REGEN: f32 = 0.16;
const GRAVITY: f32 = 20.0;
const MAX_REVERSE: f32 = 5.0;
const CIRCLE_R: f32 = 1.05;
const CIRCLE_OFFSETS: [f32; 3] = [-1.15, 0.0, 1.15];

pub struct Car {
    pub pos: Vec2,
    pub yaw: f32,
    pub speed: f32,
    /// Smoothed steering input, -1..1 (wheel visuals).
    pub steer: f32,
    pub color: Color,
    pub wheel_spin: f32,
    /// Height of the car's floor above the ground, and its vertical speed.
    pub y: f32,
    pub vy: f32,
    pub airborne: bool,
    pub air_time: f32,
    /// Nose-up angle (radians) for drawing and the camera.
    pub pitch: f32,
    /// Turbo tank 0..1, whether it is burning now, and the "too empty to
    /// start" lockout that lasts until the tank is a quarter full again.
    pub turbo: f32,
    pub turbo_on: bool,
    turbo_locked: bool,
    /// Vertical speed of the ground we're driving over (what a launch inherits).
    ground_vy: f32,
}

/// What happened during one driven tick.
#[derive(Default, Clone, Copy, Debug)]
pub struct DriveEvents {
    /// Speed lost into a wall (0 if none).
    pub impact: f32,
    pub launched: bool,
    /// Set on the tick a flight ends: how long it lasted (seconds).
    pub landed_after: Option<f32>,
}

impl Car {
    pub fn new(pos: Vec2, yaw: f32, color: Color) -> Car {
        Car {
            pos,
            yaw,
            speed: 0.0,
            steer: 0.0,
            color,
            wheel_spin: 0.0,
            y: 0.0,
            vy: 0.0,
            airborne: false,
            air_time: 0.0,
            pitch: 0.0,
            turbo: 1.0,
            turbo_on: false,
            turbo_locked: false,
            ground_vy: 0.0,
        }
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
    /// Driver door: left side, just ahead of the middle.
    pub fn door_point(&self) -> Vec2 {
        self.pos - self.right() * (HALF.y + 0.8) + self.forward() * 0.4
    }

    /// Driver controls -> speed, heading and height.
    pub fn update_driven(&mut self, input: &Input, dt: f32, obstacles: &[Obb], world: &World) -> DriveEvents {
        // --- Turbo tank.
        if self.turbo <= 0.0 {
            self.turbo_locked = true;
        }
        if self.turbo >= 0.25 {
            self.turbo_locked = false;
        }
        self.turbo_on = input.turbo && !self.turbo_locked && self.turbo > 0.0;
        if self.turbo_on {
            self.turbo = (self.turbo - TURBO_DRAIN * dt).max(0.0);
        } else {
            self.turbo = (self.turbo + TURBO_REGEN * dt).min(1.0);
        }

        let grounded = !self.airborne;
        let cap = if self.turbo_on { TURBO_SPEED } else { MAX_SPEED };
        if self.turbo_on {
            // Nitro shoves the car forward whether or not W is held.
            if self.speed < 0.0 {
                self.speed += 30.0 * dt;
            } else {
                self.speed += 26.0 * (1.0 - 0.8 * (self.speed / TURBO_SPEED).min(1.0)) * dt;
            }
        } else if grounded {
            // Throttle / brake / reverse. Acceleration tapers near top speed.
            if input.forward {
                if self.speed < 0.0 {
                    self.speed += 30.0 * dt; // braking out of reverse
                } else if self.speed < MAX_SPEED {
                    self.speed += 9.0 * (1.0 - 0.6 * self.speed / MAX_SPEED) * dt;
                }
            } else if input.back {
                self.speed -= if self.speed > 0.5 { 28.0 } else { 9.0 } * dt;
            } else {
                self.speed = approach(self.speed, 0.0, 4.0 * dt); // coast
            }
        }
        // Past the cap (turbo just ended): bleed back down. No drag in the air.
        if self.speed > cap && grounded {
            self.speed = approach(self.speed, cap, 9.0 * dt);
        }
        self.speed = self.speed.clamp(-MAX_REVERSE, TURBO_SPEED);

        // Steering: yaw rate scales with speed, and eases off when very fast.
        // In the air the wheels do nothing, so only a gentle twist is allowed.
        let steer_in = (input.right as i32 - input.left as i32) as f32;
        self.steer = approach(self.steer, steer_in, 5.0 * dt);
        let grip = (self.speed / 9.0).clamp(-1.0, 1.0);
        let air = if grounded { 1.0 } else { 0.35 };
        let fast = (self.speed.abs() / TURBO_SPEED).min(1.0);
        self.yaw += self.steer * 1.7 * grip * (1.0 - 0.45 * fast) * air * dt;

                let mut ev = DriveEvents { impact: self.integrate(dt, obstacles), ..Default::default() };
        self.vertical(dt, world, &mut ev);
        ev
    }

    /// Height physics: ride ramps, fly off their ends, land (with assist).
    fn vertical(&mut self, dt: f32, world: &World, ev: &mut DriveEvents) {
        let gh = world.ground_height(self.pos);
        if self.airborne {
            self.vy -= GRAVITY * dt;
            self.y += self.vy * dt;
            self.air_time += dt;
            // LANDING ASSIST: line the car up with the road and keep it on it,
            // so wherever the flight ends the car can simply drive on.
            let road = world::road_yaw(self.pos.x);
            let d = angle_diff(road, self.yaw);
            self.yaw += d.clamp(-2.4 * dt, 2.4 * dt);
            let off = self.pos.y - world::road_z(self.pos.x);
            let edge = world::ROAD_HALF - 3.0;
            if off.abs() > edge {
                self.pos.y -= off.signum() * 7.0 * dt;
            }
            if self.y <= gh {
                self.y = gh;
                self.vy = 0.0;
                self.airborne = false;
                ev.landed_after = Some(self.air_time);
                self.air_time = 0.0;
            }
        } else {
            let old = self.y;
            if gh < old - 0.01 {
                if old > 0.25 && self.ground_vy > 1.0 {
                    // The surface just ended under us: fly, keeping the climb speed.
                    self.airborne = true;
                    self.vy = self.ground_vy;
                    self.air_time = 0.0;
                    ev.launched = true;
                } else {
                    self.y = (old - 12.0 * dt).max(gh);
                }
            } else {
                self.y = gh.min(old + 14.0 * dt);
            }
            self.ground_vy = ((self.y - old) / dt).max(0.0);
        }
        // Visual nose angle: along the ramp, or along the flight path.
        let target = if self.airborne {
            (self.vy / self.speed.abs().max(6.0)).atan().clamp(-0.55, 0.55)
        } else {
            let f = self.forward();
            let slope = (world.ground_height(self.pos + f * 0.9) - world.ground_height(self.pos - f * 0.9)) / 1.8;
            (slope * self.speed.signum()).atan()
        };
        self.pitch = approach(self.pitch, target, 5.0 * dt);
    }

    /// Nobody inside: coast to a stop.
    pub fn update_parked(&mut self, dt: f32, obstacles: &[Obb]) {
        self.speed = approach(self.speed, 0.0, 12.0 * dt);
        self.steer = approach(self.steer, 0.0, 3.0 * dt);
        if self.speed != 0.0 {
            self.integrate(dt, obstacles);
        }
    }

    /// Move along the heading and collide. Returns the impact speed (0 if none).
    fn integrate(&mut self, dt: f32, obstacles: &[Obb]) -> f32 {
        let fwd = self.forward();
        self.pos += fwd * self.speed * dt;
        self.wheel_spin += self.speed * dt;

        // Flying over anything lower than the car's floor is fine.
        let low = self.y > 0.05;
        let hit: Vec<Obb>;
        let obstacles = if low {
            hit = obstacles.iter().filter(|o| o.height > self.y - 0.05).copied().collect();
            &hit[..]
        } else {
            obstacles
        };
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
