//! Optional headless smoke test, only active when the env var
//! `CLOWN_AUTOSHOT=<path prefix>` is set. It presses "start", stages a few
//! scenes (title, skating the Strip, driving through zombies, a curve, a ramp
//! jump with the crew on board, and the clown lasers), saves PNG screenshots
//! and quits. Handy for CI and for the README pictures:
//!
//!   CLOWN_AUTOSHOT=/tmp/shot xvfb-run -a ./target/release/clownpunk
use crate::math::Vec2;
use crate::platform::{self, Input};
use crate::state::{Game, Mode};
use crate::world;
use crate::zombies::Zombie;

pub struct Smoke {
    prefix: String,
    frame: u32,
    pending: Option<String>,
    /// Frame at which the current scene was staged (0 = not yet).
    drive_at: u32,
    kills_at_drive: u32,
    drive_shot: bool,
    jump_at: u32,
    jump_shot: bool,
    ramp_shot: bool,
    curve_at: u32,
    laser_at: u32,
}

impl Smoke {
    pub fn from_env() -> Option<Smoke> {
        platform::env("CLOWN_AUTOSHOT").map(|prefix| Smoke {
            prefix,
            frame: 0,
            pending: None,
            drive_at: 0,
            kills_at_drive: 0,
            drive_shot: false,
            jump_at: 0,
            jump_shot: false,
            ramp_shot: false,
            curve_at: 0,
            laser_at: 0,
        })
    }

    fn shot(&mut self, name: &str) {
        self.pending = Some(format!("{}_{}.png", self.prefix, name));
    }

    /// Called at the start of every frame. Returns true when it's time to exit.
    pub fn step(&mut self, input: &mut Input, game: &mut Game) -> bool {
        self.frame += 1;
        let x0 = world::STRIP_START + 120.0;
        match self.frame {
            20 => self.shot("title"),
            25 => input.confirm = true,
            // Skating the Strip with zombies ahead; fire one shot.
            40 => {
                game.player.pos = world::road_point(x0, 0.0);
                game.player.yaw = world::road_yaw(x0);
                game.player.pitch = 0.0;
                game.zombies.clear();
                for (dx, dz) in [(7.0, 0.0), (13.0, -3.0), (17.0, 4.0), (24.0, 1.0), (30.0, -4.0)] {
                    game.zombies.push(Zombie::new(world::road_point(x0 + dx, dz), 60.0));
                }
            }
            48 => input.shoot = true,
            52 => self.shot("skate"),
            // Driving scene: floor it through a line of zombies on a bend.
            70 => {
                let x1 = 560.0;
                game.zombies.clear();
                for (dx, dz) in [(16.0, 0.0), (24.0, -3.0), (32.0, 3.0), (40.0, 0.0), (48.0, -2.0)] {
                    game.zombies.push(Zombie::new(world::road_point(x1 + dx, dz), 0.0));
                }
                if let Some(car) = game.cars.first_mut() {
                    car.pos = world::road_point(x1, 0.0);
                    car.yaw = world::road_yaw(x1);
                    car.speed = 13.0;
                }
                game.player.pos = world::road_point(x1, 0.0);
                game.drive = Some(0);
                self.drive_at = self.frame;
                self.kills_at_drive = game.kills;
            }
            _ => {}
        }
        if self.drive_at > 0 && !self.drive_shot && game.mode == Mode::Playing {
            input.forward = true;
            // Capture just after the first zombie gets flattened (a SPLAT! is up).
            if game.kills > self.kills_at_drive || self.frame > self.drive_at + 300 {
                self.drive_shot = true;
                self.shot("drive");
                self.curve_at = self.frame + 3;
            }
        }
        // Scene: turbo through a bend.
        if self.curve_at > 0 && self.frame == self.curve_at {
            let x = 200.0;
            game.zombies.clear();
            game.allies.iter_mut().take(6).for_each(|a| a.recruited = true);
            // Tidy the staged stretch of road so nothing is in the way.
            game.world.statics.retain(|o| !(o.height < 10.0 && o.center.x > 190.0 && o.center.x < 340.0));
            game.world.props.retain(|p| !(p.pos.x > 190.0 && p.pos.x < 340.0 && !matches!(p.kind, world::PropKind::Streetlight | world::PropKind::Palm)));
            let car = &mut game.cars[0];
            car.pos = world::road_point(x, -2.0);
            car.yaw = world::road_yaw(x);
            car.speed = 24.0;
        }
        if self.curve_at > 0 && self.frame > self.curve_at && self.frame <= self.curve_at + 40 {
            input.forward = true;
            input.turbo = true;
            // Hold the line with a crude autopilot.
            let car = &game.cars[0];
            let ahead = world::road_point(car.pos.x + 14.0, 0.0);
            let d = crate::math::angle_diff((ahead.y - car.pos.y).atan2(ahead.x - car.pos.x), car.yaw);
            input.right = d > 0.04;
            input.left = d < -0.04;
            if self.frame == self.curve_at + 40 {
                self.shot("curve");
                self.jump_at = self.frame + 3;
            }
        }
        // Scene: hit a ramp with the whole crew aboard; shoot at the top of the arc.
        if self.jump_at > 0 && self.frame == self.jump_at {
            let r = game.world.ramps[1];
            let car = &mut game.cars[0];
            car.pos = Vec2::new(r.x0 - 24.0, 0.0);
            car.yaw = 0.0;
            car.speed = 22.0;
            car.y = 0.0;
        }
        if self.jump_at > 0 && !self.jump_shot && self.frame > self.jump_at {
            input.forward = true;
            input.turbo = true;
            let car = &game.cars[0];
            if !self.ramp_shot && car.pos.x > game.world.ramps[1].x0 - 13.0 {
                self.ramp_shot = true;
                self.shot("ramp");
            }
            if car.airborne && car.vy < 0.5 {
                self.jump_shot = true;
                self.shot("jump");
                self.laser_at = self.frame + 40;
            }
            if self.frame > self.jump_at + 300 {
                self.jump_shot = true;
                self.laser_at = self.frame + 2;
            }
        }
        // Scene: surrounded on foot, then clown lasers.
        if self.laser_at > 0 {
            if self.frame == self.laser_at {
                game.drive = None;
                game.cars[0].speed = 0.0;
                game.cars[0].y = 0.0;
                game.cars[0].airborne = false;
                let c = Vec2::new(900.0, 0.0);
                game.player.pos = world::road_point(c.x, 0.0);
                game.player.yaw = 0.3;
                game.player.pitch = 0.0;
                game.player.health = 3;
                game.zombies.clear();
                for k in 0..10 {
                    let a = k as f32 * std::f32::consts::TAU / 10.0;
                    let mut z = Zombie::new(game.player.pos + Vec2::from_yaw(a) * (6.0 + (k % 3) as f32 * 2.0), 500.0);
                    z.speed = 0.0;
                    game.zombies.push(z);
                }
                game.laser_charge = 1.0;
            }
            if self.frame == self.laser_at + 6 {
                self.shot("surrounded");
            }
            if self.frame == self.laser_at + 8 {
                input.laser = true;
            }
            if self.frame == self.laser_at + 12 {
                self.shot("laser");
            }
        }
        self.frame > 900 || (self.laser_at > 0 && self.frame > self.laser_at + 14 && self.pending.is_none())
    }

    /// Called after drawing, before the buffer swap.
    pub fn capture(&mut self) {
        if let Some(path) = self.pending.take() {
            platform::save_screenshot(&path);
        }
    }
}
