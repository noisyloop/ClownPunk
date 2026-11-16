//! The game: state machine + the per-tick simulation that glues every system together.
//!
//! STATE MACHINE (top level, `Mode`):
//!
//!   Title --confirm--> Playing <--Esc--> Paused
//!                         |  \--health 0--> GameOver --R--> (new run)
//!                         \----finish arch with 3 zombies--> Won --R--> (new run)
//!
//! While `Playing`, `drive` decides which control scheme is active:
//!   drive == None      -> on the skateboard (player.rs)
//!   drive == Some(i)   -> inside car `i`     (car.rs)
//!
//! Each fixed tick (`Game::update`) does, in order:
//!   input/controls -> player or car movement -> shooting & interactions ->
//!   zombie AI -> zombie/player/car contact -> win/lose checks.
use crate::car::{self, Car};
use crate::collision::Obb;
use crate::math::{angle_diff, Color, Vec2, Vec3};
use crate::platform::{rand_range, Input};
use crate::player::{self, Player};
use crate::world::{self, World};
use crate::zombies::{self, ZState, Zombie};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Title,
    Playing,
    Paused,
    GameOver,
    Won,
}

/// A short-lived bullet streak.
pub struct Tracer {
    pub from: Vec3,
    pub to: Vec3,
    pub ttl: f32,
}

/// Floating comic-book text ("BONK!") anchored in the world.
pub struct Popup {
    pub pos: Vec3,
    pub text: &'static str,
    pub ttl: f32,
}

/// Everything the renderer needs to place the first-person camera.
pub struct CamView {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov: f32,
}

/// Cars faster than this hit zombies; slower ones just bump them.
const ROADKILL_SPEED: f32 = 2.5;
const SHOT_RANGE: f32 = 70.0;
const SHOT_COOLDOWN: f32 = 0.35;
const INTERACT_RANGE: f32 = 2.5;
const PLAYER_START: Vec2 = Vec2::new(-39.0, 0.0);
pub const FOV_FOOT: f32 = 72.0;

pub struct Game {
    pub mode: Mode,
    pub world: World,
    pub player: Player,
    pub zombies: Vec<Zombie>,
    pub cars: Vec<Car>,
    pub drive: Option<usize>,
    pub carrying: Option<usize>,
    pub run_time: f32,
    pub final_time: f32,
    pub anim_time: f32,
    pub tracers: Vec<Tracer>,
    pub popups: Vec<Popup>,
    pub toast: Option<(String, f32)>,
    /// Interaction hint shown at the bottom of the screen ("E: Pick up").
    pub prompt: Option<String>,
    /// While driving, how far you've turned your head relative to the car.
    pub cab_yaw: f32,
    pub cab_pitch: f32,
    pub shake: f32,
    pub quit: bool,
}

impl Game {
    /// Fresh random run (new lot layout, new car colours, new zombies).
    pub fn new() -> Game {
        let world = World::generate();
        let mut g = Game {
            mode: Mode::Title,
            world,
            player: Player::new(PLAYER_START),
            zombies: Vec::new(),
            cars: Vec::new(),
            drive: None,
            carrying: None,
            run_time: 0.0,
            final_time: 0.0,
            anim_time: 0.0,
            tracers: Vec::new(),
            popups: Vec::new(),
            toast: None,
            prompt: None,
            cab_yaw: 0.0,
            cab_pitch: 0.0,
            shake: 0.0,
            quit: false,
        };
        g.spawn_cars();
        g.spawn_zombies();
        g
    }

    /// Does the platform layer need the mouse locked right now?
    pub fn wants_mouse_capture(&self) -> bool {
        self.mode == Mode::Playing
    }

    // ------------------------------------------------------------ setup ---

    /// 4-5 cars at random spots/colours. They keep clear of each other, of the
    /// player's start, and of the lot exit lane so you can always skate out.
    fn spawn_cars(&mut self) {
        let palette = [
            Color::rgb(0.95, 0.15, 0.2),
            Color::rgb(1.0, 0.85, 0.1),
            Color::rgb(0.15, 0.4, 0.95),
            Color::rgb(0.95, 0.35, 0.8),
            Color::rgb(0.2, 0.85, 0.9),
            Color::rgb(0.6, 0.35, 0.95),
            Color::rgb(1.0, 0.55, 0.1),
        ];
        let want = 4 + (rand_range(0.0, 2.0) as usize); // 4 or 5
        let mut tries = 0;
        while self.cars.len() < want && tries < 500 {
            tries += 1;
            let pos = Vec2::new(rand_range(-36.0, 10.0), rand_range(-23.0, 23.0));
            if pos.dist(PLAYER_START) < 12.0 {
                continue;
            }
            if self.cars.iter().any(|c| c.pos.dist(pos) < 9.0) {
                continue;
            }
            // Mostly parked square to the lot, with a little crookedness.
            let quarter = (rand_range(0.0, 4.0) as i32) as f32 * std::f32::consts::FRAC_PI_2;
            let yaw = quarter + rand_range(-0.18, 0.18);
            let color = palette[(rand_range(0.0, palette.len() as f32) as usize) % palette.len()];
            self.cars.push(Car::new(pos, yaw, color));
        }
    }

    /// 6-8 zombies in the lot (they always chase) plus wanderers along the Strip.
    fn spawn_zombies(&mut self) {
        let lot_count = 6 + (rand_range(0.0, 3.0) as usize); // 6..=8
        let mut tries = 0;
        let mut placed = 0;
        while placed < lot_count && tries < 500 {
            tries += 1;
            let pos = Vec2::new(rand_range(-40.0, 14.0), rand_range(-26.0, 26.0));
            if pos.dist(PLAYER_START) < 20.0 || self.cars.iter().any(|c| c.obb().push_out(pos, 1.2).is_some()) {
                continue;
            }
            self.zombies.push(Zombie::new(pos, 500.0));
            placed += 1;
        }
        for _ in 0..9 {
            let pos = Vec2::new(rand_range(world::STRIP_START + 60.0, world::FINISH_X - 40.0), rand_range(-11.0, 11.0));
            let blocked = self.world.statics.iter().any(|o| o.push_out(pos, 1.2).is_some());
            if !blocked {
                self.zombies.push(Zombie::new(pos, 26.0));
            }
        }
    }

    fn start_run(&mut self) {
        self.mode = Mode::Playing;
        self.run_time = 0.0;
        self.say("Skate! Shoot zombies, grab them, load 3 into one car.", 5.0);
    }

    // ----------------------------------------------------------- helpers ---

    pub fn say(&mut self, text: &str, secs: f32) {
        self.toast = Some((text.to_owned(), secs));
    }

    /// Most zombies in any single trunk (what the HUD tracks).
    pub fn best_trunk(&self) -> u8 {
        self.cars.iter().map(|c| c.trunk).max().unwrap_or(0)
    }

    /// Static boxes + every car (optionally skipping the one being driven).
    fn obstacles(&self, skip_car: Option<usize>) -> Vec<Obb> {
        let mut v = self.world.statics.clone();
        for (i, c) in self.cars.iter().enumerate() {
            if Some(i) != skip_car {
                v.push(c.obb());
            }
        }
        v
    }

    // ------------------------------------------------------------ update ---

    /// One fixed simulation tick.
    pub fn update(&mut self, input: &Input, dt: f32) {
        self.anim_time += dt;
        match self.mode {
            Mode::Title => {
                if input.confirm {
                    self.start_run();
                }
                if input.quit {
                    self.quit = true;
                }
            }
            Mode::Paused => {
                if input.pause || input.confirm {
                    self.mode = Mode::Playing;
                }
                if input.restart {
                    *self = Game::new();
                    self.start_run();
                }
                if input.quit {
                    self.quit = true;
                }
            }
            Mode::GameOver | Mode::Won => {
                if input.restart {
                    *self = Game::new();
                    self.start_run();
                }
            }
            Mode::Playing => self.update_playing(input, dt),
        }
    }

    fn update_playing(&mut self, input: &Input, dt: f32) {
        if input.pause {
            self.mode = Mode::Paused;
            return;
        }
        self.run_time += dt;
        self.shake = (self.shake - dt * 3.0).max(0.0);
        if let Some((_, t)) = &mut self.toast {
            *t -= dt;
        }
        if matches!(self.toast, Some((_, t)) if t <= 0.0) {
            self.toast = None;
        }
        self.tracers.iter_mut().for_each(|t| t.ttl -= dt);
        self.tracers.retain(|t| t.ttl > 0.0);
        self.popups.iter_mut().for_each(|p| p.ttl -= dt);
        self.popups.retain(|p| p.ttl > 0.0);

        match self.drive {
            None => self.update_on_foot(input, dt),
            Some(ci) => self.update_driving(ci, input, dt),
        }

        // Cars nobody is driving coast to a stop and animate their trunks.
        for i in 0..self.cars.len() {
            if Some(i) != self.drive {
                let boxes = self.obstacles(Some(i)); // everything except this car itself
                self.cars[i].update_parked(dt, &boxes);
            }
        }

        self.update_zombies(dt);
        self.carried_zombie_follow();

        if self.player.health <= 0 {
            self.mode = Mode::GameOver;
        }
    }

    // -------------------------------------------------------- on foot ---

    fn update_on_foot(&mut self, input: &Input, dt: f32) {
        let obstacles = self.obstacles(None);
        self.player.look(input);
        self.player.update_skate(input, dt, self.carrying.is_some(), &obstacles);

        if input.shoot && self.player.shoot_cd <= 0.0 {
            self.fire(&obstacles);
        }
        self.interactions(input);
    }

    /// HITSCAN SHOT: an instantaneous ray from the eye along the (wobbly)
    /// aim direction. We find the nearest thing it touches among: world boxes,
    /// the ground, and zombie hit-spheres. If a zombie is nearest, it's stunned.
    fn fire(&mut self, obstacles: &[Obb]) {
        self.player.shoot_cd = SHOT_COOLDOWN;
        self.player.recoil = 1.0;
        self.player.muzzle = 0.07;

        let origin = self.player.eye();
        let dir = self.player.shot_dir();
        let mut best = SHOT_RANGE;
        for b in obstacles {
            if let Some(t) = b.ray(origin, dir, best) {
                best = best.min(t);
            }
        }
        if dir.y < -1e-4 {
            best = best.min(-origin.y / dir.y); // the ground plane y = 0
        }
        let mut hit = None;
        for (i, z) in self.zombies.iter().enumerate() {
            if !matches!(z.state, ZState::Walking | ZState::Stunned) {
                continue;
            }
            if let Some(t) = z.ray(origin, dir, best) {
                best = t;
                hit = Some(i);
            }
        }

        let right = Vec2::from_yaw(self.player.yaw).right_of();
        let muzzle = origin + Vec3::from_xz(right * 0.22, -0.18) + dir * 0.6;
        self.tracers.push(Tracer { from: muzzle, to: origin + dir * best, ttl: 0.12 });

        if let Some(i) = hit {
            let away = Vec2::new(dir.x, dir.z).norm();
            let pos = self.zombies[i].pos;
            self.zombies[i].stun(away * 7.0);
            let texts = ["BONK!", "BOING!", "POW!", "WHAM!"];
            let text = texts[(rand_range(0.0, 4.0) as usize) % 4];
            self.popups.push(Popup { pos: Vec3::from_xz(pos, 2.2), text, ttl: 0.9 });
            // If it was the carried one (can't be: Carried isn't hittable), nothing more to do.
        }
    }

    /// Work out what E would do right now (also drives the on-screen prompt).
    fn interactions(&mut self, input: &Input) {
        self.prompt = None;
        let pp = self.player.pos;

        // 1) Carrying a zombie? Look for a trunk to load it into.
        if let Some(zi) = self.carrying {
            for ci in 0..self.cars.len() {
                if self.cars[ci].rear_point().dist(pp) < INTERACT_RANGE {
                    if self.cars[ci].trunk >= car::CAPACITY {
                        self.prompt = Some("Trunk is full!".to_owned());
                    } else {
                        self.prompt = Some("E: Load zombie into trunk".to_owned());
                        if input.interact {
                            self.cars[ci].trunk += 1;
                            self.cars[ci].open_trunk_briefly();
                            self.zombies[zi].state = ZState::InTrunk;
                            self.carrying = None;
                            let n = self.cars[ci].trunk;
                            let pos = self.cars[ci].rear_point();
                            self.popups.push(Popup { pos: Vec3::from_xz(pos, 2.0), text: "THUNK!", ttl: 0.9 });
                            if n >= car::CAPACITY {
                                self.say("Car READY! Press E at its driver door (left side).", 5.0);
                            } else {
                                self.say(&format!("Zombie loaded! Trunk {}/{}", n, car::CAPACITY), 2.5);
                            }
                        }
                    }
                    return;
                }
            }
            // Carrying near a ready car's door: remind the player instead of entering.
            for c in &self.cars {
                if c.drivable() && c.door_point().dist(pp) < INTERACT_RANGE {
                    self.prompt = Some("Load your zombie first (E at a trunk)".to_owned());
                    return;
                }
            }
            return;
        }

        // 2) Ready car's driver door -> get in.
        for ci in 0..self.cars.len() {
            if self.cars[ci].drivable() && self.cars[ci].door_point().dist(pp) < INTERACT_RANGE {
                self.prompt = Some("E: Get in and drive".to_owned());
                if input.interact {
                    self.enter_car(ci);
                }
                return;
            }
        }

        // 3) Nearest stunned zombie -> pick up.
        let mut best: Option<(usize, f32)> = None;
        for (i, z) in self.zombies.iter().enumerate() {
            if z.can_be_picked_up() {
                let d = z.pos.dist(pp);
                if d < INTERACT_RANGE && best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((i, d));
                }
            }
        }
        if let Some((i, _)) = best {
            self.prompt = Some("E: Pick up zombie".to_owned());
            if input.interact {
                self.zombies[i].state = ZState::Carried;
                self.carrying = Some(i);
                self.say("Got one! Find a car and press E at its rear.", 3.0);
            }
        }
    }

    // -------------------------------------------------------- driving ---

    fn enter_car(&mut self, ci: usize) {
        self.drive = Some(ci);
        self.cab_yaw = angle_diff(self.player.yaw, self.cars[ci].yaw).clamp(-1.6, 1.6);
        self.cab_pitch = self.player.pitch.clamp(-0.5, 0.5);
        self.player.vel = Vec2::ZERO;
        self.player.hop = 0.0;
        self.say("Drive to the finish arch at the end of the Strip! F to bail out.", 5.0);
    }

    fn update_driving(&mut self, ci: usize, input: &Input, dt: f32) {
        // Free-look head turn relative to the car body.
        self.cab_yaw = (self.cab_yaw + input.look_dx * 0.0025).clamp(-1.7, 1.7);
        self.cab_pitch = (self.cab_pitch - input.look_dy * 0.0025).clamp(-0.6, 0.6);
        self.player.hurt_timer = (self.player.hurt_timer - dt).max(0.0);
        self.player.damage_flash = (self.player.damage_flash - dt).max(0.0);

        let others = self.obstacles(Some(ci));
        let impact = self.cars[ci].update_driven(input, dt, &others);
        self.cars[ci].tick_trunk_anim(dt);
        if impact > 4.0 {
            self.shake = self.shake.max((impact / 20.0).min(1.0));
        }
        self.player.pos = self.cars[ci].pos;
        self.player.vel = Vec2::ZERO;

        if input.exit_car {
            self.exit_car(ci);
            return;
        }
        // WIN: cross the finish arch while driving a car holding 3+ zombies.
        if self.cars[ci].pos.x >= world::FINISH_X && self.cars[ci].trunk >= car::CAPACITY {
            self.final_time = self.run_time;
            self.mode = Mode::Won;
        }
    }

    fn exit_car(&mut self, ci: usize) {
        let car = &self.cars[ci];
        self.player.pos = car.door_point();
        self.player.yaw = car.yaw + self.cab_yaw;
        self.player.pitch = self.cab_pitch;
        self.player.vel = Vec2::ZERO;
        self.drive = None;
        let obstacles = self.obstacles(None);
        crate::collision::resolve_circle(&mut self.player.pos, player::RADIUS, &obstacles);
        self.say("Back on the board. Press E at the driver door to get in again.", 3.0);
    }

    // --------------------------------------------------------- zombies ---

    /// Moving cars flatten walking zombies and lose speed doing it. Runs BEFORE
    /// zombie collision so a fast car hits zombies instead of bulldozing them.
    fn car_hits_zombies(&mut self) {
        for ci in 0..self.cars.len() {
            if self.cars[ci].speed.abs() < ROADKILL_SPEED {
                continue;
            }
            let circles = self.cars[ci].circles();
            let fwd = self.cars[ci].forward();
            for zi in 0..self.zombies.len() {
                let z = &self.zombies[zi];
                if z.state != ZState::Walking {
                    continue;
                }
                let r = Car::circle_radius() + zombies::RADIUS;
                if circles.iter().any(|c| c.dist(z.pos) < r) {
                    let side = (z.pos - self.cars[ci].pos).norm();
                    let push = fwd * self.cars[ci].speed * 0.5 + side * 4.0;
                    let pos = z.pos;
                    self.zombies[zi].stun(push);
                    self.cars[ci].speed *= 0.72;
                    self.popups.push(Popup { pos: Vec3::from_xz(pos, 2.2), text: "SPLAT!", ttl: 0.9 });
                    if Some(ci) == self.drive {
                        self.shake = self.shake.max(0.5);
                    }
                }
            }
        }
    }

    fn update_zombies(&mut self, dt: f32) {
        self.car_hits_zombies();

        // Solid things zombies bump into. Fast cars are excluded: they hit
        // zombies (above) rather than shoving them.
        let mut obstacles = self.world.statics.clone();
        for c in &self.cars {
            if c.speed.abs() < ROADKILL_SPEED {
                obstacles.push(c.obb());
            }
        }
        let target = match self.drive {
            Some(ci) => self.cars[ci].pos,
            None => self.player.pos,
        };
        for z in &mut self.zombies {
            z.update(dt, target, &obstacles);
        }

        // Soft separation so zombies don't stack into one blob.
        let n = self.zombies.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if !matches!(self.zombies[i].state, ZState::Walking | ZState::Stunned)
                    || !matches!(self.zombies[j].state, ZState::Walking | ZState::Stunned)
                {
                    continue;
                }
                let d = self.zombies[j].pos - self.zombies[i].pos;
                let dist = d.len();
                if dist < 0.9 && dist > 1e-4 {
                    let push = d * ((0.9 - dist) * 0.5 / dist);
                    self.zombies[i].pos -= push;
                    self.zombies[j].pos += push;
                }
            }
        }

        // Zombie touches the skater: lose a heart, get knocked back.
        if self.drive.is_none() {
            let pp = self.player.pos;
            for zi in 0..self.zombies.len() {
                let z = &self.zombies[zi];
                if z.state != ZState::Walking || z.recoil > 0.0 || z.pos.dist(pp) > player::RADIUS + zombies::RADIUS + 0.15 {
                    continue;
                }
                let away = (pp - z.pos).norm();
                if self.player.hurt(away) {
                    self.shake = 0.5;
                    self.zombies[zi].recoil = 1.2;
                    self.zombies[zi].vel = -away * 3.0;
                    if let Some(ci) = self.carrying.take() {
                        // Ouch: you drop your zombie.
                        self.zombies[ci].state = ZState::Stunned;
                        self.zombies[ci].stun = 3.0;
                        self.zombies[ci].pos = pp + away * -1.2;
                        self.say("Dropped your zombie!", 2.0);
                    }
                    break;
                }
            }
        }
    }

    /// A carried zombie just rides on the player's shoulder (position only matters for logic).
    fn carried_zombie_follow(&mut self) {
        if let Some(zi) = self.carrying {
            self.zombies[zi].pos = self.player.pos;
        }
    }

    // ----------------------------------------------------------- camera ---

    pub fn camera(&self) -> CamView {
        match self.mode {
            Mode::Title => {
                // Slow orbit around the lot behind the title text.
                let t = self.anim_time * 0.15;
                let c = Vec2::new(-12.0, 0.0);
                let pos = Vec3::new(c.x + t.cos() * 42.0, 14.0, c.y + t.sin() * 42.0);
                let d = Vec3::new(c.x - pos.x, -pos.y, c.y - pos.z);
                let flat = (d.x * d.x + d.z * d.z).sqrt();
                CamView { pos, yaw: d.z.atan2(d.x), pitch: d.y.atan2(flat), roll: 0.0, fov: 60.0 }
            }
            _ => match self.drive {
                None => {
                    let p = &self.player;
                    let mut pos = p.eye();
                    pos.y += (p.bob * 2.0).sin() * 0.025 * (p.speed() / 10.0).min(1.0);
                    CamView { pos, yaw: p.yaw, pitch: p.pitch, roll: p.lean, fov: FOV_FOOT + p.speed() * 0.5 }
                }
                Some(ci) => {
                    let c = &self.cars[ci];
                    let seat = c.pos - c.forward() * 0.35 - c.right() * 0.45;
                    let mut pos = Vec3::from_xz(seat, 1.38);
                    let s = self.shake * 0.12;
                    pos.x += (self.anim_time * 61.0).sin() * s;
                    pos.y += (self.anim_time * 73.0).cos() * s;
                    let speed_t = (c.speed.abs() / car::MAX_SPEED).min(1.0);
                    CamView { pos, yaw: c.yaw + self.cab_yaw, pitch: self.cab_pitch, roll: -c.steer * 0.02 * speed_t, fov: 70.0 + 20.0 * speed_t }
                }
            },
        }
    }
}

// ======================================================================
// Tests: drive the real simulation with scripted input (no window needed).
// Run with `cargo test --release`.
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// A game already in `Playing`, with an empty arena so tests are deterministic.
    fn arena() -> Game {
        let mut g = Game::new();
        g.mode = Mode::Playing;
        g.world.statics.clear();
        g.cars.clear();
        g.zombies.clear();
        g.player = Player::new(Vec2::new(0.0, 0.0));
        g
    }

    fn tick(g: &mut Game, input: &Input) {
        g.update(input, DT);
    }
    fn press_interact(g: &mut Game) {
        tick(g, &Input { interact: true, ..Default::default() });
    }
    fn stunned_zombie_at(pos: Vec2) -> Zombie {
        let mut z = Zombie::new(pos, 0.0);
        z.stun(Vec2::ZERO);
        z.stun = 5.0;
        z.state = ZState::Stunned;
        z
    }

    #[test]
    fn spawns_expected_counts() {
        for _ in 0..20 {
            let g = Game::new();
            assert!((4..=5).contains(&g.cars.len()), "cars: {}", g.cars.len());
            let lot = g.zombies.iter().filter(|z| z.aggro > 100.0).count();
            assert!((6..=8).contains(&lot), "lot zombies: {lot}");
        }
    }

    #[test]
    fn shot_stuns_zombie_and_it_wakes_up_after_six_seconds() {
        let mut g = arena();
        let mut z = Zombie::new(Vec2::new(6.0, 0.0), 0.0); // aggro 0: just stands there
        z.speed = 0.0;
        g.zombies.push(z);
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.zombies[0].state, ZState::Stunned, "hitscan should stun");
        assert!(!g.tracers.is_empty());
        // Still down after 5 s, back up after 6.2 s.
        for _ in 0..300 {
            tick(&mut g, &Input::default());
        }
        assert_eq!(g.zombies[0].state, ZState::Stunned);
        for _ in 0..72 {
            tick(&mut g, &Input::default());
        }
        assert_eq!(g.zombies[0].state, ZState::Walking);
    }

    #[test]
    fn walls_block_bullets() {
        let mut g = arena();
        g.world.statics.push(Obb::aabb(3.0, 0.0, 0.5, 3.0, 5.0));
        let mut z = Zombie::new(Vec2::new(6.0, 0.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.zombies[0].state, ZState::Walking);
    }

    #[test]
    fn full_haul_flow_ends_in_a_win() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 12.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        for _ in 0..3 {
            g.zombies.push(stunned_zombie_at(Vec2::new(0.5, 0.0)));
        }
        for n in 1..=3u8 {
            // Pick up the next stunned zombie.
            g.player.pos = Vec2::new(0.0, 0.0);
            press_interact(&mut g);
            assert!(g.carrying.is_some(), "pickup #{n}");
            // Walk to the trunk and load it.
            g.player.pos = g.cars[0].rear_point();
            press_interact(&mut g);
            assert!(g.carrying.is_none(), "loaded #{n}");
            assert_eq!(g.cars[0].trunk, n);
            assert_eq!(g.cars[0].drivable(), n == 3);
        }
        assert_eq!(g.zombies.iter().filter(|z| z.state == ZState::InTrunk).count(), 3);

        // Get in at the driver door, then roll over the finish line.
        g.player.pos = g.cars[0].door_point();
        press_interact(&mut g);
        assert_eq!(g.drive, Some(0));
        g.cars[0].pos = Vec2::new(world::FINISH_X - 6.0, 0.0);
        g.cars[0].yaw = 0.0;
        g.cars[0].speed = 15.0;
        for _ in 0..120 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
            if g.mode == Mode::Won {
                break;
            }
        }
        assert_eq!(g.mode, Mode::Won);
    }

    #[test]
    fn cannot_drive_until_trunk_has_three() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 12.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.cars[0].trunk = 2;
        g.player.pos = g.cars[0].door_point();
        press_interact(&mut g);
        assert_eq!(g.drive, None);
    }

    #[test]
    fn finish_line_without_enough_zombies_does_not_win() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(world::FINISH_X - 3.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.cars[0].trunk = 2;
        g.drive = Some(0);
        g.cars[0].speed = 15.0;
        for _ in 0..60 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
        }
        assert_eq!(g.mode, Mode::Playing);
    }

    #[test]
    fn exiting_car_keeps_zombies_in_trunk_and_returns_to_board() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(10.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.cars[0].trunk = 3;
        g.drive = Some(0);
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        assert_eq!(g.drive, None);
        assert_eq!(g.cars[0].trunk, 3);
        assert!(g.player.pos.dist(g.cars[0].door_point()) < 1.0);
    }

    #[test]
    fn zombie_touch_costs_a_heart_and_three_hits_end_the_run() {
        let mut g = arena();
        for expected in [2, 1, 0] {
            let mut z = Zombie::new(g.player.pos + Vec2::new(0.8, 0.0), 500.0);
            z.speed = 0.0;
            g.zombies = vec![z];
            tick(&mut g, &Input::default());
            assert_eq!(g.player.health, expected);
            assert!(g.player.vel.x < -1.0, "knocked back");
            // Wait out the invulnerability window.
            for _ in 0..100 {
                if g.mode != Mode::Playing {
                    break;
                }
                g.zombies.clear();
                tick(&mut g, &Input::default());
            }
            g.player.vel = Vec2::ZERO;
        }
        assert_eq!(g.mode, Mode::GameOver);
        // R restarts into a fresh run.
        tick(&mut g, &Input { restart: true, ..Default::default() });
        assert_eq!(g.mode, Mode::Playing);
        assert_eq!(g.player.health, 3);
    }

    #[test]
    fn carrying_slows_the_skater() {
        let mut g = arena();
        let hold_w = Input { forward: true, ..Default::default() };
        for _ in 0..400 {
            tick(&mut g, &hold_w);
            g.player.pos = Vec2::ZERO; // stay in the arena; we only measure speed
        }
        let free = g.player.speed();
        g.player.vel = Vec2::ZERO;
        g.zombies.push(stunned_zombie_at(Vec2::new(0.5, 0.0)));
        press_interact(&mut g);
        assert!(g.carrying.is_some());
        for _ in 0..400 {
            tick(&mut g, &hold_w);
            g.player.pos = Vec2::ZERO;
        }
        assert!(free > 9.0 && free <= 11.5, "free speed {free}");
        assert!(g.player.speed() < 6.0, "carry speed {}", g.player.speed());
    }

    #[test]
    fn car_flattens_walking_zombies_and_slows_down() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.cars[0].trunk = 3;
        g.drive = Some(0);
        g.cars[0].speed = 20.0;
        let mut z = Zombie::new(Vec2::new(3.5, 0.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        for _ in 0..10 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
        }
        assert_eq!(g.zombies[0].state, ZState::Stunned);
        assert!(g.cars[0].speed < 20.0);
    }

    #[test]
    fn pause_freezes_the_clock() {
        let mut g = arena();
        tick(&mut g, &Input::default());
        tick(&mut g, &Input { pause: true, ..Default::default() });
        assert_eq!(g.mode, Mode::Paused);
        let t = g.run_time;
        for _ in 0..30 {
            tick(&mut g, &Input::default());
        }
        assert_eq!(g.run_time, t);
        tick(&mut g, &Input { pause: true, ..Default::default() });
        assert_eq!(g.mode, Mode::Playing);
    }
}
