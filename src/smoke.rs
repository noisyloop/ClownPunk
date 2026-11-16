//! Optional headless smoke test, only active when the env var
//! `CLOWN_AUTOSHOT=<path prefix>` is set. It presses "start", lets the game
//! run for a moment, saves two PNG screenshots and quits. Handy for CI:
//!
//!   CLOWN_AUTOSHOT=/tmp/shot xvfb-run -a ./target/release/clown_skate_zombie_haul
use crate::platform::{self, Input};

pub struct Smoke {
    prefix: String,
    frame: u32,
    pending: Option<String>,
}

impl Smoke {
    pub fn from_env() -> Option<Smoke> {
        platform::env("CLOWN_AUTOSHOT").map(|prefix| Smoke { prefix, frame: 0, pending: None })
    }

    /// Called at the start of every frame. Returns true when it's time to exit.
    pub fn step(&mut self, input: &mut Input) -> bool {
        self.frame += 1;
        match self.frame {
            20 => self.pending = Some(format!("{}_title.png", self.prefix)),
            25 => input.confirm = true,
            90 => self.pending = Some(format!("{}_play.png", self.prefix)),
            _ => {}
        }
        self.frame > 100
    }

    /// Called after drawing, before the buffer swap.
    pub fn capture(&mut self) {
        if let Some(path) = self.pending.take() {
            platform::save_screenshot(&path);
        }
    }
}
