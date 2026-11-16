//! Green zombies: chase AI, dying, and bullet hit volumes.
//!
//! State machine per zombie:
//!   Walking --(shot / hit by car)--> Dead   (stays down for good)
use crate::collision::{ray_sphere, resolve_circle, Obb};
use crate::math::{Color, Vec2, Vec3};
use crate::platform::rand_range;

pub const RADIUS: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZState {
    Walking,
    Dead,
}

pub struct Zombie {
    pub pos: Vec2,
    pub vel: Vec2,
    pub yaw: f32,
    pub state: ZState,
    /// Seconds since death (drives the fall-over animation).
    pub dead_t: f32,
    /// After hitting the player a zombie reels back for a moment.
    pub recoil: f32,
    pub speed: f32,
    /// Only chases when the target is within this range (otherwise wanders).
    pub aggro: f32,
    pub phase: f32, // walk-cycle phase, for animation
    pub wander_dir: f32,
    pub wander_t: f32,
    pub shirt: Color,
}

impl Zombie {
    pub fn new(pos: Vec2, aggro: f32) -> Zombie {
        let shirts = [
            Color::rgb(0.85, 0.3, 0.3),
            Color::rgb(0.3, 0.45, 0.85),
            Color::rgb(0.9, 0.7, 0.2),
            Color::rgb(0.7, 0.4, 0.8),
            Color::rgb(0.9, 0.5, 0.15),
        ];
        Zombie {
            pos,
            vel: Vec2::ZERO,
            yaw: rand_range(0.0, 6.28),
            state: ZState::Walking,
            dead_t: 0.0,
            recoil: 0.0,
            speed: rand_range(2.4, 3.1), // the skater tops out at 7, so you can outskate them
            aggro,
            phase: rand_range(0.0, 6.28),
            wander_dir: rand_range(0.0, 6.28),
            wander_t: 0.0,
            shirt: shirts[(rand_range(0.0, 5.0) as usize) % 5],
        }
    }

    /// 0 = standing, 1 = lying flat. Falls over quickly when killed.
    pub fn tilt(&self) -> f32 {
        match self.state {
            ZState::Dead => (self.dead_t * 6.0).min(1.0),
            ZState::Walking => 0.0,
        }
    }

    /// Kill it. `push` is the direction (and rough strength) it slides.
    pub fn kill(&mut self, push: Vec2) {
        self.state = ZState::Dead;
        self.dead_t = 0.0;
        self.vel = push;
        // Face the attacker so the zombie falls over backwards, away from them.
        if push.len_sq() > 1e-4 {
            self.yaw = (-push).y.atan2((-push).x);
        }
    }

    pub fn update(&mut self, dt: f32, target: Vec2, obstacles: &[Obb]) {
        match self.state {
            ZState::Walking => self.update_walking(dt, target, obstacles),
            ZState::Dead => {
                self.dead_t += dt;
                // Slide to a stop, still colliding with walls.
                self.vel = self.vel * (1.0 - (6.0 * dt).min(1.0));
                self.pos += self.vel * dt;
                resolve_circle(&mut self.pos, RADIUS, obstacles);
            }
        }
    }

    fn update_walking(&mut self, dt: f32, target: Vec2, obstacles: &[Obb]) {
        let to_target = target - self.pos;
        let dist = to_target.len();
        let mut desired = Vec2::ZERO;

        if self.recoil > 0.0 {
            self.recoil -= dt; // reeling after a hit: no steering
        } else if dist < self.aggro {
            // Shamble toward the target, weaving a little so they look drunk.
            let weave = (self.phase * 0.5).sin() * 0.35;
            let heading = to_target.y.atan2(to_target.x) + weave;
            desired = Vec2::from_yaw(heading) * self.speed;
        } else {
            // Idle wander: pick a direction, walk slowly, pick another.
            self.wander_t -= dt;
            if self.wander_t <= 0.0 {
                self.wander_dir = rand_range(0.0, 6.28);
                self.wander_t = rand_range(2.0, 5.0);
            }
            desired = Vec2::from_yaw(self.wander_dir) * self.speed * 0.35;
        }

        // Smooth acceleration so direction changes aren't instant.
        self.vel += (desired - self.vel) * (3.0 * dt).min(1.0);
        self.pos += self.vel * dt;
        let n = resolve_circle(&mut self.pos, RADIUS, obstacles);
        if n.len_sq() > 0.0 {
            self.wander_dir += 1.7; // turn away from whatever we bumped
        }
        if self.vel.len_sq() > 0.05 {
            self.yaw = self.vel.y.atan2(self.vel.x);
        }
        self.phase += dt * (2.0 + self.vel.len() * 2.2);
    }

    /// Bullet hit volumes: three spheres along the body axis. Standing, the
    /// axis points up; lying flat it points backwards along the ground - the
    /// same maths as the renderer's tilt.
    pub fn hit_spheres(&self) -> [(Vec3, f32); 3] {
        let theta = self.tilt() * std::f32::consts::FRAC_PI_2;
        let f = Vec2::from_yaw(self.yaw);
        let axis = Vec3::new(-f.x * theta.sin(), theta.cos(), -f.y * theta.sin());
        let base = Vec3::from_xz(self.pos, 0.2 * theta.sin());
        [(base + axis * 0.5, 0.45), (base + axis * 1.1, 0.5), (base + axis * 1.65, 0.4)]
    }

    pub fn ray(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<f32> {
        self.hit_spheres()
            .iter()
            .filter_map(|(c, r)| ray_sphere(origin, dir, *c, *r))
            .filter(|t| *t <= max_t)
            .fold(None, |best: Option<f32>, t| Some(best.map_or(t, |b| b.min(t))))
    }
}
