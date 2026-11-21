//! The clown on the skateboard: first-person movement, look, and aiming.
//!
//! SKATEBOARD MODEL
//! The board has a *heading* (`yaw`, also the camera yaw) and a separate
//! *velocity*. Each tick we split the velocity into a part along the heading
//! (`vf`, pushing / braking happens here) and a sideways part (`vr`). Sideways
//! speed is bled off by "grip", but not instantly - that lag is the drift.
//! Turning rotates the heading, instantly creating sideways velocity, which is
//! what makes carving feel slidey.
use crate::collision::{resolve_circle, Obb};
use crate::math::{approach, Vec2, Vec3};
use crate::platform::{rand_range, Input};

pub const RADIUS: f32 = 0.5;
pub const EYE_HEIGHT: f32 = 1.55;
pub const MAX_HEALTH: i32 = 3;
const TOP_SPEED: f32 = 7.0;
const TURBO_TOP_SPEED: f32 = 12.0;
const LOOK_SENS: f32 = 0.0025;

pub struct Player {
    pub pos: Vec2,
    pub vel: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    /// Height of the board above the ground (ollie) and vertical speed.
    pub hop: f32,
    pub hop_vel: f32,
    /// Height of the ground (a ramp surface) under the board.
    pub ground_y: f32,
    /// Turbo tank 0..1 (Shift): a burst of speed on the skateboard.
    pub turbo: f32,
    pub turbo_on: bool,
    turbo_locked: bool,
    pub health: i32,
    pub hurt_timer: f32,   // invulnerability after a hit
    pub damage_flash: f32, // red screen flash
    pub shoot_cd: f32,
    pub recoil: f32,       // visual gun kick, 0..1
    pub muzzle: f32,       // visual muzzle flash timer
    pub lean: f32,         // visual camera roll while carving
    pub bob: f32,          // visual bob phase
    pub sway_t: f32,       // drives the wobbly pistol
}

impl Player {
    pub fn new(pos: Vec2) -> Player {
        Player {
            pos,
            vel: Vec2::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            hop: 0.0,
            hop_vel: 0.0,
            ground_y: 0.0,
            turbo: 1.0,
            turbo_on: false,
            turbo_locked: false,
            health: MAX_HEALTH,
            hurt_timer: 0.0,
            damage_flash: 0.0,
            shoot_cd: 0.0,
            recoil: 0.0,
            muzzle: 0.0,
            lean: 0.0,
            bob: 0.0,
            sway_t: 0.0,
        }
    }

    pub fn speed(&self) -> f32 {
        self.vel.len()
    }

    /// Camera position.
    pub fn eye(&self) -> Vec3 {
        Vec3::from_xz(self.pos, EYE_HEIGHT + self.hop + self.ground_y)
    }

    /// Mouse look: yaw follows horizontal motion, pitch vertical (clamped so
    /// you can't flip over the top).
    pub fn look(&mut self, input: &Input) {
        self.yaw += input.look_dx * LOOK_SENS;
        self.pitch = (self.pitch - input.look_dy * LOOK_SENS).clamp(-1.35, 1.35);
    }

    pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
        Vec3::new(pitch.cos() * yaw.cos(), pitch.sin(), pitch.cos() * yaw.sin())
    }

    /// Cartoon pistol wobble: a slow sway that gets worse the faster you skate.
    /// Used for both the on-screen gun and the actual shot direction.
    pub fn wobble(&self) -> (f32, f32) {
        let amp = 0.012 + 0.0025 * self.speed();
        ((self.sway_t * 2.3).sin() * amp, (self.sway_t * 3.1 + 1.0).cos() * amp)
    }

    /// Direction a bullet actually leaves the gun (sway + a little random spread).
    pub fn shot_dir(&self) -> Vec3 {
        let (wy, wp) = self.wobble();
        let spread = 0.006 + 0.0015 * self.speed();
        Player::look_dir(
            self.yaw + wy + rand_range(-spread, spread),
            self.pitch + wp + rand_range(-spread, spread),
        )
    }

    pub fn update_skate(&mut self, input: &Input, dt: f32, obstacles: &[Obb]) {
        self.sway_t += dt;
        self.hurt_timer = (self.hurt_timer - dt).max(0.0);
        self.damage_flash = (self.damage_flash - dt).max(0.0);
        self.shoot_cd = (self.shoot_cd - dt).max(0.0);
        self.recoil = (self.recoil - dt * 6.0).max(0.0);
        self.muzzle = (self.muzzle - dt).max(0.0);

        let grounded = self.hop <= 0.001 && self.hop_vel <= 0.0;
        // Turbo (Shift): same tank rules as the car's nitro.
        if self.turbo <= 0.0 {
            self.turbo_locked = true;
        }
        if self.turbo >= 0.25 {
            self.turbo_locked = false;
        }
        self.turbo_on = input.turbo && !self.turbo_locked && self.turbo > 0.0;
        self.turbo = if self.turbo_on { (self.turbo - 0.28 * dt).max(0.0) } else { (self.turbo + 0.16 * dt).min(1.0) };
        let max_speed = if self.turbo_on { TURBO_TOP_SPEED } else { TOP_SPEED };
        let accel = if self.turbo_on { 16.0 } else { 7.0 };

        // 1) Steering: A/D rotate the heading (mouse already did in `look`).
        let steer = (input.right as i32 - input.left as i32) as f32;
        let turn_rate = 2.1 * (0.5 + 0.5 * (self.speed() / 4.0).min(1.0));
        self.yaw += steer * turn_rate * dt;

        // 2) Split velocity into forward / sideways parts of the (new) heading.
        let fwd = Vec2::from_yaw(self.yaw);
        let right = fwd.right_of();
        let mut vf = self.vel.dot(fwd);
        let mut vr = self.vel.dot(right);

        // 3) Push / brake / roll.
        if grounded {
            if (input.forward || self.turbo_on) && vf < max_speed {
                vf += accel * dt;
            }
            if input.back {
                vf = approach(vf, 0.0, 17.0 * dt);
            }
        }
        let rolling = if input.forward { 0.10 } else { 0.28 };
        vf -= vf * rolling * dt;
        if vf > max_speed {
            vf = approach(vf, max_speed, 8.0 * dt);
        }

        // 4) Grip: bleed sideways speed. Some of it is converted to forward
        //    speed so carving doesn't kill all your momentum.
        let grip = if grounded { 3.2 } else { 0.4 };
        let lost = vr * (1.0 - (-grip * dt).exp());
        vr -= lost;
        vf += lost.abs() * 0.35 * if vf >= 0.0 { 1.0 } else { -1.0 };

        self.vel = fwd * vf + right * vr;

        // 5) Ollie: a small hop with simple gravity.
        if input.jump && grounded {
            self.hop_vel = 4.4;
        }
        self.hop_vel -= 14.0 * dt;
        self.hop += self.hop_vel * dt;
        if self.hop <= 0.0 {
            self.hop = 0.0;
            self.hop_vel = 0.0;
        }

        // 6) Move, then collide: push the circle out of any box and remove the
        //    velocity component pointing into the surface (with a tiny bounce).
        self.pos += self.vel * dt;
        let n = resolve_circle(&mut self.pos, RADIUS, obstacles);
        if n.len_sq() > 0.0 {
            let n = n.norm();
            let vn = self.vel.dot(n);
            if vn < 0.0 {
                self.vel -= n * (vn * 1.25);
            }
        }

        // Visual-only state: lean into turns, bob with speed.
        let target_lean = steer * 0.05 * (self.speed() / 8.0).min(1.0) + vr * 0.01;
        self.lean += (target_lean - self.lean) * (6.0 * dt).min(1.0);
        self.bob += self.speed() * dt * 0.9;
    }

    /// Take a zombie hit: lose a heart and get knocked away. Returns false if
    /// still invulnerable from a previous hit.
    pub fn hurt(&mut self, away: Vec2) -> bool {
        if self.hurt_timer > 0.0 {
            return false;
        }
        self.health -= 1;
        self.hurt_timer = 1.5;
        self.damage_flash = 0.6;
        self.vel = away * 10.0;
        self.hop_vel = 2.5;
        true
    }
}
