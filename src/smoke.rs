//! Optional headless smoke test, only active when the env var
//! `CLOWN_AUTOSHOT=<path prefix>` is set. It presses "start", stages a few
//! scenes (the title, skating the Strip, driving through zombies), saves PNG
//! screenshots and quits. Handy for CI and for the README pictures:
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
    /// Frame at which the driving scene was staged (0 = not yet).
    drive_at: u32,
    kills_at_drive: u32,
    drive_shot: bool,
}

impl Smoke {
    pub fn from_env() -> Option<Smoke> {
        platform::env("CLOWN_AUTOSHOT").map(|prefix| Smoke { prefix, frame: 0, pending: None, drive_at: 0, kills_at_drive: 0, drive_shot: false })
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
                game.player.pos = Vec2::new(x0, 0.0);
                game.player.yaw = 0.0;
                game.player.pitch = 0.0;
                game.zombies.clear();
                for (dx, dz) in [(7.0, 0.0), (13.0, -3.0), (17.0, 4.0), (24.0, 1.0), (30.0, -4.0)] {
                    game.zombies.push(Zombie::new(Vec2::new(x0 + dx, dz), 60.0));
                }
            }
            48 => input.shoot = true,
            52 => self.shot("skate"),
            // Driving scene: floor it through a line of zombies.
            70 => {
                let x1 = x0 + 200.0;
                game.zombies.clear();
                for (dx, dz) in [(16.0, 0.0), (24.0, -3.0), (32.0, 3.0), (40.0, 0.0), (48.0, -2.0)] {
                    game.zombies.push(Zombie::new(Vec2::new(x1 + dx, dz), 0.0));
                }
                if let Some(car) = game.cars.first_mut() {
                    car.pos = Vec2::new(x1, 0.0);
                    car.yaw = 0.0;
                    car.speed = 13.0;
                }
                game.player.pos = Vec2::new(x1, 0.0);
                game.drive = Some(0);
                self.drive_at = self.frame;
                self.kills_at_drive = game.kills;
            }
            _ => {}
        }
        if self.drive_at > 0 && game.mode == Mode::Playing {
            input.forward = true;
            // Capture just after the first zombie gets flattened (a SPLAT! is up).
            if !self.drive_shot && (game.kills > self.kills_at_drive || self.frame > self.drive_at + 300) {
                self.drive_shot = true;
                self.shot("drive");
            }
        }
        self.frame > 400 || (self.drive_shot && self.frame > self.drive_at + 2 && self.pending.is_none())
    }

    /// Called after drawing, before the buffer swap.
    pub fn capture(&mut self) {
        if let Some(path) = self.pending.take() {
            platform::save_screenshot(&path);
        }
    }
}
