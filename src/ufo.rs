//! Flying saucers: the one you can fly, and the wild ones you shoot.
//!
//! FLIGHT MODEL (arcade, no real physics)
//!   * you steer with the mouse (yaw = heading, pitch = where the cannons aim)
//!   * W/S thrust along the heading, A/D strafe, Space climbs, C descends
//!   * velocity is smoothed toward the commanded velocity, so it floats
//!   * Shift = boost, using the same kind of tank as the car's turbo
//!
//! COLLISION AVOIDANCE AI (`avoidance`)
//!   Every saucer, including yours, looks at each neighbour's relative
//!   position and velocity and predicts its closest approach within the next
//!   few seconds. If that gap would be tighter than a safety bubble, the
//!   saucer gets an acceleration pushing it away from the predicted meeting
//!   point, stronger the closer and sooner the danger. Wild saucers steer with
//!   it on top of their wandering. Your UFO runs it as a co-pilot: it nudges
//!   you out of the way even while you push straight at a saucer, and if two
//!   saucers still end up too close they are pushed apart (a "bump").
use crate::collision::Obb;
use crate::math::{approach, Color, Vec2, Vec3};
use crate::platform::{rand_range, Input};
use crate::world::{self, World};

/// Body radius used for collisions and for shots.
pub const RADIUS: f32 = 2.4;
/// Gap (between hulls) the AI tries to keep around a saucer.
pub const SAFE_GAP: f32 = 7.0;
/// How far ahead (seconds) the AI predicts.
const LOOKAHEAD: f32 = 3.5;
const MAX_SPEED: f32 = 24.0;
pub const BOOST_SPEED: f32 = 42.0;
const MAX_CLIMB: f32 = 11.0;
const ACCEL: f32 = 2.6; // how fast velocity follows the command (1/s)
const BOOST_DRAIN: f32 = 0.3;
const BOOST_REGEN: f32 = 0.16;
pub const MAX_ALT: f32 = 70.0;
/// Hover height of a parked/landed UFO and the height you can still step out at.
pub const REST_Y: f32 = 1.0;
pub const EXIT_Y: f32 = 3.0;
pub const SHOT_COOLDOWN: f32 = 0.16;
pub const WILD_SPEED: f32 = 9.0;

pub struct Ufo {
    pub pos: Vec2,
    /// Height of the saucer's centre above the ground.
    pub y: f32,
    pub yaw: f32,
    /// Where the cannons point (mouse look); only used by the one you fly.
    pub pitch: f32,
    pub vel: Vec3,
    pub color: Color,
    /// Spinning rim lights.
    pub spin: f32,
    /// Visual tilt into the direction of travel (radians).
    pub bank: f32,
    pub tilt: f32,
    pub turbo: f32,
    pub turbo_on: bool,
    turbo_locked: bool,
    pub shoot_cd: f32,
    /// 0..1: how hard the collision-avoidance AI is steering right now.
    pub avoiding: f32,
    pub alive: bool,
    // Wild saucers only:
    target: Vec2,
    target_y: f32,
    retarget: f32,
    /// Roaming area (a wild saucer stays near this x along the road).
    home_x: f32,
}

/// What happened during one flown tick.
#[derive(Default, Clone, Copy, Debug)]
pub struct FlyEvents {
    /// Speed lost into a wall or another saucer (0 if none).
    pub impact: f32,
}

/// A neighbour as seen by the avoidance AI.
#[derive(Clone, Copy, Debug)]
pub struct Neighbour {
    pub pos: Vec3,
    pub vel: Vec3,
}

impl Ufo {
    pub fn new(pos: Vec2, yaw: f32, y: f32, color: Color) -> Ufo {
        Ufo {
            pos,
            y,
            yaw,
            pitch: 0.0,
            vel: Vec3::ZERO,
            color,
            spin: rand_range(0.0, 6.0),
            bank: 0.0,
            tilt: 0.0,
            turbo: 1.0,
            turbo_on: false,
            turbo_locked: false,
            shoot_cd: 0.0,
            avoiding: 0.0,
            alive: true,
            target: pos,
            target_y: y,
            retarget: 0.0,
            home_x: pos.x,
        }
    }

    /// A wild saucer that roams around `home_x` along the road.
    pub fn wild(home_x: f32, color: Color) -> Ufo {
        let pos = random_spot(home_x);
        let mut u = Ufo::new(pos, rand_range(0.0, 6.28), rand_range(6.0, 16.0), color);
        u.home_x = home_x;
        u.target = u.pos;
        u
    }

    pub fn centre(&self) -> Vec3 {
        Vec3::from_xz(self.pos, self.y)
    }

    pub fn obb(&self) -> Obb {
        Obb::new(self.pos, Vec2::new(RADIUS, RADIUS), self.yaw, 1.6)
    }

    pub fn speed(&self) -> f32 {
        self.vel.len()
    }

    pub fn aim_dir(&self) -> Vec3 {
        crate::player::Player::look_dir(self.yaw, self.pitch)
    }

    pub fn right(&self) -> Vec2 {
        Vec2::from_yaw(self.yaw).right_of()
    }

    /// Nobody inside: hover at rest height and drift to a stop.
    pub fn update_parked(&mut self, dt: f32, world: &World) {
        self.spin += dt * 0.6;
        self.vel = Vec3::ZERO;
        let rest = world.ground_height(self.pos) + REST_Y;
        self.y = approach(self.y, rest, 4.0 * dt);
        self.bank = approach(self.bank, 0.0, dt);
        self.tilt = approach(self.tilt, 0.0, dt);
        self.turbo = (self.turbo + BOOST_REGEN * dt).min(1.0);
        self.turbo_on = false;
        self.avoiding = 0.0;
        self.shoot_cd = (self.shoot_cd - dt).max(0.0);
    }

    /// Pilot controls -> velocity, with the co-pilot AI on top.
    pub fn update_flown(&mut self, input: &Input, dt: f32, obstacles: &[Obb], world: &World, others: &[Neighbour]) -> FlyEvents {
        // --- Boost tank.
        if self.turbo <= 0.0 {
            self.turbo_locked = true;
        }
        if self.turbo >= 0.25 {
            self.turbo_locked = false;
        }
        self.turbo_on = input.turbo && !self.turbo_locked && self.turbo > 0.0;
        if self.turbo_on {
            self.turbo = (self.turbo - BOOST_DRAIN * dt).max(0.0);
        } else {
            self.turbo = (self.turbo + BOOST_REGEN * dt).min(1.0);
        }
        self.spin += dt * (2.5 + self.speed() * 0.1);
        self.shoot_cd = (self.shoot_cd - dt).max(0.0);

        // --- Commanded velocity from the keys.
        let fwd = Vec2::from_yaw(self.yaw);
        let right = fwd.right_of();
        let thrust = (input.forward as i32 - input.back as i32) as f32;
        let strafe = (input.right as i32 - input.left as i32) as f32;
        let rise = (input.up as i32 - input.down as i32) as f32;
        let top = if self.turbo_on { BOOST_SPEED } else { MAX_SPEED };
        let mut flat = fwd * thrust + right * strafe * 0.7;
        if flat.len_sq() > 1.0 {
            flat = flat.norm();
        }
        // Boost shoves you forward even with no keys held.
        if self.turbo_on && thrust == 0.0 {
            flat = flat + fwd;
        }
        let want = Vec3::new(flat.x * top, rise * MAX_CLIMB, flat.y * top);
        let k = (ACCEL * dt).min(1.0);
        self.vel = self.vel + (want - self.vel) * k;

        // --- Co-pilot: steer away from anything we would otherwise hit.
        let (steer, danger) = avoidance(self.centre(), self.vel, others);
        self.avoiding = approach(self.avoiding, danger, 6.0 * dt);
        self.vel = self.vel + steer * dt;
        let cap = if self.turbo_on { BOOST_SPEED } else { MAX_SPEED * 1.4 };
        let h = Vec2::new(self.vel.x, self.vel.z);
        if h.len() > cap {
            let h = h.norm() * cap;
            self.vel.x = h.x;
            self.vel.z = h.y;
        }

        // --- Move, then collide with the world and (as a backstop) with saucers.
        self.pos += Vec2::new(self.vel.x, self.vel.z) * dt;
        self.y += self.vel.y * dt;
        let mut impact = self.hit_world(obstacles);
        impact = impact.max(separate(self, others));

        let floor = world.ground_height(self.pos) + 0.9;
        if self.y < floor {
            self.y = floor;
            self.vel.y = self.vel.y.max(0.0);
        }
        if self.y > MAX_ALT {
            self.y = MAX_ALT;
            self.vel.y = self.vel.y.min(0.0);
        }
        // The lot has a back wall; the sky above it is not a way out of the map.
        if self.pos.x < world::LOT_MIN_X + 1.0 {
            self.pos.x = world::LOT_MIN_X + 1.0;
            self.vel.x = self.vel.x.max(0.0);
        }

        // Banking looks: lean into acceleration.
        let local_fwd = self.vel.x * fwd.x + self.vel.z * fwd.y;
        let local_right = self.vel.x * right.x + self.vel.z * right.y;
        self.tilt = approach(self.tilt, (local_fwd / BOOST_SPEED).clamp(-1.0, 1.0) * 0.35, 2.0 * dt);
        self.bank = approach(self.bank, (local_right / BOOST_SPEED).clamp(-1.0, 1.0) * 0.45, 2.0 * dt);
        FlyEvents { impact }
    }

    /// Slide around anything solid that reaches up to our height.
    fn hit_world(&mut self, obstacles: &[Obb]) -> f32 {
        let hit: Vec<Obb> = obstacles.iter().filter(|o| o.height > self.y - 0.6).copied().collect();
        let mut c = self.pos;
        let n = crate::collision::resolve_circle(&mut c, RADIUS * 0.8, &hit);
        self.pos = c;
        if n.len_sq() > 0.0 {
            let n = n.norm();
            let vn = self.vel.x * n.x + self.vel.z * n.y;
            if vn < 0.0 {
                self.vel.x -= n.x * vn;
                self.vel.z -= n.y * vn;
                return -vn;
            }
        }
        0.0
    }

    /// Wild saucers: wander between waypoints near the road at a gentle speed,
    /// steering around every neighbour with the same avoidance AI.
    pub fn update_wild(&mut self, dt: f32, others: &[Neighbour]) {
        self.spin += dt * 3.0;
        self.retarget -= dt;
        let to = self.target - self.pos;
        if self.retarget <= 0.0 || to.len() < 4.0 {
            let x = (self.home_x + rand_range(-45.0, 45.0)).max(world::LOT_MIN_X + 3.0);
            self.target = random_spot(x);
            self.target_y = rand_range(5.0, 17.0);
            self.retarget = rand_range(6.0, 12.0);
        }
        let to = Vec3::new(self.target.x - self.pos.x, self.target_y - self.y, self.target.y - self.pos.y);
        let dist = to.len().max(1e-3);
        let want = to * (WILD_SPEED.min(dist) / dist);
        let k = (1.2 * dt).min(1.0);
        self.vel = self.vel + (want - self.vel) * k;
        let (steer, danger) = avoidance(self.centre(), self.vel, others);
        self.avoiding = approach(self.avoiding, danger, 6.0 * dt);
        self.vel = self.vel + steer * dt;
        let sp = self.vel.len();
        if sp > WILD_SPEED * 1.6 {
            self.vel = self.vel * (WILD_SPEED * 1.6 / sp);
        }
        self.pos += Vec2::new(self.vel.x, self.vel.z) * dt;
        self.y = (self.y + self.vel.y * dt).clamp(4.0, 24.0);
        separate(self, others);
        self.pos.x = self.pos.x.max(world::LOT_MIN_X + 3.0);
        self.yaw += (self.vel.x.hypot(self.vel.z) * 0.08 + 0.3) * dt;
        self.bank = approach(self.bank, (self.vel.z * 0.03).clamp(-0.4, 0.4), dt);
        self.tilt = approach(self.tilt, (self.vel.x * 0.03).clamp(-0.4, 0.4), dt);
    }
}

fn random_spot(x: f32) -> Vec2 {
    let half = if x < world::STRIP_START { 20.0 } else { 11.0 };
    world::road_point(x, rand_range(-half, half))
}

/// The collision-avoidance AI. Returns an acceleration (m/s^2) and a danger
/// level 0..1 (for the HUD / debug).
///
/// For each neighbour: relative position `r`, relative velocity `v`. Time of
/// closest approach is `t = -r.v / |v|^2` clamped to `0..LOOKAHEAD`, and the
/// separation then is `|r + v t|`. If that is inside the safety bubble we push
/// away from the predicted meeting point, harder when it is closer and sooner.
pub fn avoidance(me: Vec3, my_vel: Vec3, others: &[Neighbour]) -> (Vec3, f32) {
    let bubble = RADIUS * 2.0 + SAFE_GAP;
    let mut accel = Vec3::ZERO;
    let mut danger = 0.0f32;
    for o in others {
        let r = o.pos - me;
        let v = o.vel - my_vel;
        let vv = v.dot(v);
        let t = if vv > 1e-4 { (-r.dot(v) / vv).clamp(0.0, LOOKAHEAD) } else { 0.0 };
        let meet = r + v * t;
        let d = meet.len();
        if d >= bubble {
            continue;
        }
        // Direction away from where the other saucer will be. If we're dead
        // on a collision course the direction is undefined, so sidestep.
        let away = if d > 0.05 {
            -meet * (1.0 / d)
        } else {
            let side = Vec3::new(-v.z, 0.0, v.x);
            let side = if side.len() > 1e-3 { side.norm() } else { Vec3::new(1.0, 0.0, 0.0) };
            side + Vec3::new(0.0, 0.6, 0.0)
        };
        let closeness = 1.0 - d / bubble; // 0 at the bubble edge, 1 on top of it
        let urgency = 1.0 - t / LOOKAHEAD; // 1 = right now
        let strength = closeness * (0.35 + 0.65 * urgency);
        // Mostly sidestep: add a little of the sideways component so a
        // head-on pair slides past each other instead of braking in line.
        let mut dir = away;
        if t > 0.0 && v.len() > 0.5 {
            // Remove the part of the push that lies along our closing line, keep
            // some of it so we also slow down a bit.
            let vn = v.norm();
            let along = vn * away.dot(vn);
            dir = away - along * 0.6;
            if dir.len() < 1e-3 {
                dir = away;
            }
            dir = dir.norm();
        }
        accel = accel + dir * (strength * 55.0);
        danger = danger.max(strength);
    }
    (accel, danger.clamp(0.0, 1.0))
}

/// Hard backstop: if two hulls overlap, push them out of each other.
/// Returns the closing speed lost (0 if no contact).
fn separate(me: &mut Ufo, others: &[Neighbour]) -> f32 {
    let mut impact = 0.0f32;
    for o in others {
        let d = o.pos - me.centre();
        let dist = d.len();
        let min = RADIUS * 2.0 - 0.4;
        if dist < min {
            let n = if dist > 1e-3 { d * (1.0 / dist) } else { Vec3::new(1.0, 0.0, 0.0) };
            let push = n * (min - dist);
            me.pos -= Vec2::new(push.x, push.z);
            me.y -= push.y;
            let vn = me.vel.dot(n);
            if vn > 0.0 {
                me.vel = me.vel - n * vn;
                impact = impact.max(vn);
            }
        }
    }
    impact
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn gap(a: &Ufo, b: &Ufo) -> f32 {
        (a.centre() - b.centre()).len()
    }

    /// Two wild saucers sent straight at each other must slide past, not touch.
    #[test]
    fn wild_saucers_head_on_never_touch() {
        for (dy, other_y) in [(0.0f32, 10.0f32), (0.0, 10.4), (1.0, 10.0)] {
            let mut a = Ufo::new(Vec2::new(0.0, 0.0), 0.0, 10.0 + dy, Color::rgb(1.0, 0.0, 0.0));
            let mut b = Ufo::new(Vec2::new(80.0, 0.0), 0.0, other_y, Color::rgb(0.0, 1.0, 0.0));
            let mut min = f32::MAX;
            for _ in 0..(20 * 60) {
                a.target = Vec2::new(80.0, 0.0);
                a.target_y = a.y;
                b.target = Vec2::new(0.0, 0.0);
                b.target_y = b.y;
                a.retarget = 5.0;
                b.retarget = 5.0;
                let (na, nb) = (Neighbour { pos: a.centre(), vel: a.vel }, Neighbour { pos: b.centre(), vel: b.vel });
                a.update_wild(DT, &[nb]);
                b.update_wild(DT, &[na]);
                min = min.min(gap(&a, &b));
            }
            assert!(min > RADIUS * 2.0, "hulls touched, min centre distance {min}");
        }
    }

    /// The AI is what prevents it: without the neighbour list they collide.
    #[test]
    fn without_the_ai_head_on_saucers_do_collide() {
        let mut a = Ufo::new(Vec2::new(0.0, 0.0), 0.0, 10.0, Color::rgb(1.0, 0.0, 0.0));
        let mut b = Ufo::new(Vec2::new(60.0, 0.0), 0.0, 10.0, Color::rgb(0.0, 1.0, 0.0));
        let mut min = f32::MAX;
        for _ in 0..(15 * 60) {
            a.target = Vec2::new(60.0, 0.0);
            a.target_y = 10.0;
            b.target = Vec2::new(0.0, 0.0);
            b.target_y = 10.0;
            a.retarget = 5.0;
            b.retarget = 5.0;
            a.update_wild(DT, &[]);
            b.update_wild(DT, &[]);
            min = min.min(gap(&a, &b));
        }
        assert!(min < RADIUS * 2.0, "expected a collision without avoidance, min {min}");
    }

    /// A pilot holding W at full boost straight at a saucer is steered around it.
    #[test]
    fn copilot_steers_the_pilot_around_a_saucer() {
        let world = World::generate();
        for boost in [false, true] {
            let mut me = Ufo::new(Vec2::new(0.0, 0.0), 0.0, 10.0, Color::rgb(1.0, 0.0, 1.0));
            let other = Neighbour { pos: Vec3::new(120.0, 10.0, 0.0), vel: Vec3::ZERO };
            let input = Input { forward: true, turbo: boost, ..Default::default() };
            let (mut min, mut peak) = (f32::MAX, 0.0f32);
            for _ in 0..(10 * 60) {
                me.update_flown(&input, DT, &[], &world, &[other]);
                min = min.min((other.pos - me.centre()).len());
                peak = peak.max(me.avoiding);
            }
            assert!(peak > 0.3, "co-pilot should have been working, peak {peak}");
            assert!(min > RADIUS * 2.0, "boost={boost}: hit the saucer, min {min}");
        }
    }
}
