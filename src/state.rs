//! The game: state machine + the per-tick simulation that glues every system together.
//!
//! STATE MACHINE (top level, `Mode`):
//!
//!   Title --confirm--> Playing <--Esc--> Paused
//!                         |  \--health 0--> GameOver --R--> (new run)
//!                         \----finish arch--> Won --R--> (new run)
//!
//! While `Playing`, `drive` decides which control scheme is active:
//!   drive == None      -> on the skateboard (player.rs)
//!   drive == Some(i)   -> inside car `i`     (car.rs)
//!
//! Each fixed tick (`Game::update`) does, in order:
//!   input/controls -> player or car movement -> shooting & interactions ->
//!   clown laser -> your crew (follow / ride / shoot) ->
//!   zombie AI -> zombie/player/car contact -> win/lose checks.
use crate::allies::{self, Ally};
use crate::car::{self, Car};
use crate::collision::Obb;
use crate::math::{angle_diff, approach, Color, Vec2, Vec3};
use crate::platform::{rand_chance, rand_range, Input};
use crate::player::{self, Player};
use crate::ufo::{self, Neighbour, Ufo};
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
    /// Fired by a crew clown (drawn in a different colour).
    pub ally: bool,
}

/// One beam of the clown laser burst, from the clown to a zombie.
pub struct Laser {
    pub from: Vec3,
    pub to: Vec3,
}

/// How far the clown lasers reach, and how long the burst effect lasts.
pub const LASER_RANGE: f32 = 24.0;
pub const LASER_TTL: f32 = 0.6;

/// A short fireball where a saucer was shot down.
pub struct Boom {
    pub pos: Vec3,
    pub ttl: f32,
}
pub const BOOM_TTL: f32 = 0.7;

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
/// How close to a car's centre you must stand to climb in.
const ENTER_RANGE: f32 = 4.2;
const PLAYER_START: Vec2 = Vec2::new(-39.0, 0.0);
/// Where your UFO is parked in the lot.
const UFO_SPOT: Vec2 = Vec2::new(-28.0, -16.0);
/// Wild saucers farther than this from you aren't simulated.
const SAUCER_ACTIVE: f32 = 160.0;
pub const FOV_FOOT: f32 = 72.0;
/// How far away Strip zombies notice you (a car covers this quickly).
const STRIP_AGGRO: f32 = 32.0;
/// Zombies farther than this from you aren't simulated (they just wait).
const ACTIVE_RANGE: f32 = 120.0;
/// Clown power: fills by itself and with kills; spending it fires the lasers.
const LASER_REGEN: f32 = 1.0 / 14.0;
const LASER_PER_KILL: f32 = 0.08;
/// This many zombies within `SURROUND_RADIUS` counts as "surrounded".
const SURROUND_COUNT: usize = 3;
const SURROUND_RADIUS: f32 = 10.0;

pub struct Game {
    pub mode: Mode,
    pub world: World,
    pub player: Player,
    pub zombies: Vec<Zombie>,
    /// Your crew: clowns that follow, ride along and shoot (or still wait to join).
    pub allies: Vec<Ally>,
    pub cars: Vec<Car>,
    /// Your UFO (parked until you board it) and whether you are flying it.
    pub ufo: Ufo,
    pub flying: bool,
    /// Wild saucers roaming the lot and the Strip. They avoid each other and you.
    pub saucers: Vec<Ufo>,
    pub saucers_down: u32,
    pub booms: Vec<Boom>,
    pub drive: Option<usize>,
    /// Zombies killed this run (shots and roadkill).
    pub kills: u32,
    pub run_time: f32,
    pub final_time: f32,
    pub anim_time: f32,
    pub tracers: Vec<Tracer>,
    pub lasers: Vec<Laser>,
    /// Clown power 0..1 (1 = lasers ready) and the burst effect timer/origin.
    pub laser_charge: f32,
    pub laser_burst: f32,
    pub laser_origin: Vec3,
    pub laser_flash: f32,
    /// Three or more zombies are close around you right now.
    pub surrounded: bool,
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
            allies: Vec::new(),
            cars: Vec::new(),
            ufo: Ufo::new(UFO_SPOT, 0.0, ufo::REST_Y, Color::rgb(0.85, 0.2, 0.95)),
            flying: false,
            saucers: Vec::new(),
            saucers_down: 0,
            booms: Vec::new(),
            drive: None,
            kills: 0,
            run_time: 0.0,
            final_time: 0.0,
            anim_time: 0.0,
            tracers: Vec::new(),
            lasers: Vec::new(),
            laser_charge: 0.6,
            laser_burst: 0.0,
            laser_origin: Vec3::ZERO,
            laser_flash: 0.0,
            surrounded: false,
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
        g.spawn_allies();
        g.spawn_saucers();
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
            if pos.dist(PLAYER_START) < 12.0 || pos.dist(UFO_SPOT) < 10.0 {
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
        // Groups of 1-3 shamblers all the way down the Strip, to shoot or run over.
        let mut x = world::STRIP_START + 50.0;
        while x < world::FINISH_X - 40.0 {
            let group = 1 + (rand_range(0.0, 3.0) as usize); // 1..=3
            for _ in 0..group {
                let gx = x + rand_range(-6.0, 6.0);
                let pos = world::road_point(gx, rand_range(-11.0, 11.0));
                let blocked = world::in_clear_zone(gx) || self.world.statics.iter().any(|o| o.push_out(pos, 1.2).is_some());
                if !blocked {
                    self.zombies.push(Zombie::new(pos, STRIP_AGGRO));
                }
            }
            x += rand_range(22.0, 40.0);
        }
    }

    /// Two clowns start with you; more wave from the road all along the Strip
    /// (drive or skate close to recruit them).
    fn spawn_allies(&mut self) {
        for dz in [-2.5f32, 2.5] {
            self.allies.push(Ally::new(PLAYER_START + Vec2::new(-2.5, dz), true));
        }
        for x in [170.0f32, 280.0, 470.0, 570.0, 770.0, 880.0, 1070.0, 1170.0, 1370.0, 1450.0] {
            let side = if rand_chance(0.5) { 1.0 } else { -1.0 };
            let pos = world::road_point(x, side * rand_range(6.0, 10.0));
            self.allies.push(Ally::new(pos, false));
        }
    }

    /// Wild saucers: a few over the lot, then a pair every ~100 m down the Strip.
    fn spawn_saucers(&mut self) {
        let palette = [Color::rgb(0.3, 1.0, 0.45), Color::rgb(0.2, 0.9, 1.0), Color::rgb(1.0, 0.85, 0.2), Color::rgb(1.0, 0.4, 0.3)];
        let mut n = 0usize;
        // Wild saucers start well apart so the avoidance AI never begins in a collision.
        let place = |saucers: &mut Vec<Ufo>, x: f32, n: usize| {
            for _ in 0..30 {
                let u = Ufo::wild(x, palette[n % palette.len()]);
                if saucers.iter().all(|o| (o.centre() - u.centre()).len() > 14.0) {
                    saucers.push(u);
                    return;
                }
            }
        };
        for x in [-10.0f32, 0.0, 10.0, 30.0] {
            place(&mut self.saucers, x, n);
            n += 1;
        }
        let mut x = world::STRIP_START + 60.0;
        while x < world::FINISH_X - 30.0 {
            for _ in 0..2 {
                place(&mut self.saucers, x + rand_range(-20.0, 20.0), n);
                n += 1;
            }
            x += rand_range(70.0, 110.0);
        }
    }

    pub fn crew_size(&self) -> usize {
        self.allies.iter().filter(|a| a.recruited).count()
    }

    fn start_run(&mut self) {
        self.mode = Mode::Playing;
        self.run_time = 0.0;
        self.say("Shoot zombies, then hop in any car, hit the ramps and floor it! (Shift = turbo)", 6.0);
    }

    // ----------------------------------------------------------- helpers ---

    pub fn say(&mut self, text: &str, secs: f32) {
        self.toast = Some((text.to_owned(), secs));
    }

    /// Static boxes + every car (optionally skipping the one being driven).
    fn obstacles(&self, skip_car: Option<usize>) -> Vec<Obb> {
        let mut v = self.world.statics.clone();
        for (i, c) in self.cars.iter().enumerate() {
            if Some(i) != skip_car {
                v.push(c.obb());
            }
        }
        if !self.flying {
            v.push(self.ufo.obb());
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
        self.laser_flash = (self.laser_flash - dt).max(0.0);
        self.laser_burst = (self.laser_burst - dt).max(0.0);
        if self.laser_burst <= 0.0 {
            self.lasers.clear();
        }
        self.laser_charge = (self.laser_charge + LASER_REGEN * dt).min(1.0);
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

        self.booms.iter_mut().for_each(|b| b.ttl -= dt);
        self.booms.retain(|b| b.ttl > 0.0);

        if self.flying {
            self.update_flying(input, dt);
        } else {
            match self.drive {
                None => self.update_on_foot(input, dt),
                Some(ci) => self.update_driving(ci, input, dt),
            }
        }
        if !self.flying {
            self.ufo.update_parked(dt, &self.world);
        }
        self.update_saucers(dt);

        // Cars nobody is driving coast to a stop and animate their trunks.
        for i in 0..self.cars.len() {
            if Some(i) != self.drive {
                let boxes = self.obstacles(Some(i)); // everything except this car itself
                self.cars[i].update_parked(dt, &boxes);
            }
        }

        self.update_laser(input);
        self.update_allies(dt);
        self.update_zombies(dt);

        if self.player.health <= 0 {
            self.mode = Mode::GameOver;
        }
    }

    // -------------------------------------------------------- on foot ---

    fn update_on_foot(&mut self, input: &Input, dt: f32) {
        let obstacles = self.obstacles(None);
        self.player.look(input);
        self.player.update_skate(input, dt, &obstacles);
        // Ride up ramps (the board follows the surface; no launching on foot).
        let gh = self.world.ground_height(self.player.pos);
        self.player.ground_y = approach(self.player.ground_y, gh, 10.0 * dt);

        if input.shoot && self.player.shoot_cd <= 0.0 {
            self.fire(&obstacles);
        }
        self.interactions(input);
    }

    /// The pistol: a shot from the eye along the (wobbly) aim direction.
    fn fire(&mut self, obstacles: &[Obb]) {
        self.player.shoot_cd = SHOT_COOLDOWN;
        self.player.recoil = 1.0;
        self.player.muzzle = 0.07;
        let origin = self.player.eye();
        let dir = self.player.shot_dir();
        let right = Vec2::from_yaw(self.player.yaw).right_of();
        let muzzle = origin + Vec3::from_xz(right * 0.22, -0.18) + dir * 0.6;
        self.shoot(origin, dir, muzzle, obstacles, false);
    }

    /// HITSCAN SHOT: an instantaneous ray from `origin` along `dir`. We find
    /// the nearest thing it touches among: world boxes, the ground, zombie
    /// hit-spheres and saucers. A zombie or saucer that is nearest dies.
    fn shoot(&mut self, origin: Vec3, dir: Vec3, muzzle: Vec3, obstacles: &[Obb], from_ufo: bool) {
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
            if z.state != ZState::Walking {
                continue;
            }
            if let Some(t) = z.ray(origin, dir, best) {
                best = t;
                hit = Some(i);
            }
        }
        let mut saucer_hit = None;
        for (i, u) in self.saucers.iter().enumerate() {
            if !u.alive {
                continue;
            }
            if let Some(t) = crate::collision::ray_sphere(origin, dir, u.centre(), ufo::RADIUS) {
                if t < best {
                    best = t;
                    saucer_hit = Some(i);
                    hit = None;
                }
            }
        }

        self.tracers.push(Tracer { from: muzzle, to: origin + dir * best, ttl: 0.12, ally: from_ufo });

        if let Some(i) = saucer_hit {
            self.destroy_saucer(i);
        }
        if let Some(i) = hit {
            let away = Vec2::new(dir.x, dir.z).norm();
            let pos = self.zombies[i].pos;
            self.zombies[i].kill(away * 7.0);
            self.kills += 1;
            self.laser_charge = (self.laser_charge + LASER_PER_KILL).min(1.0);
            let texts = ["BONK!", "BOING!", "POW!", "WHAM!"];
            let text = texts[(rand_range(0.0, 4.0) as usize) % 4];
            self.popups.push(Popup { pos: Vec3::from_xz(pos, 2.2), text, ttl: 0.9 });
        }
    }

    fn destroy_saucer(&mut self, i: usize) {
        let c = self.saucers[i].centre();
        self.saucers[i].alive = false;
        self.saucers_down += 1;
        self.laser_charge = (self.laser_charge + 0.12).min(1.0);
        self.booms.push(Boom { pos: c, ttl: BOOM_TTL });
        self.popups.push(Popup { pos: c + Vec3::new(0.0, 2.5, 0.0), text: "BOOM!", ttl: 0.9 });
        self.shake = self.shake.max(0.3);
    }

    /// Work out what E would do right now (also drives the on-screen prompt):
    /// get into the nearest car, if you're standing next to one.
    fn interactions(&mut self, input: &Input) {
        self.prompt = None;
        let pp = self.player.pos;
        let mut best: Option<(usize, f32)> = None;
        for (ci, c) in self.cars.iter().enumerate() {
            let d = c.pos.dist(pp);
            if d < ENTER_RANGE && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((ci, d));
            }
        }
        let ufo_d = self.ufo.pos.dist(pp);
        if ufo_d < ENTER_RANGE + 0.8 && best.map_or(true, |(_, bd)| ufo_d < bd) {
            self.prompt = Some("E: Board the UFO and fly".to_owned());
            if input.interact {
                self.enter_ufo();
            }
        } else if let Some((ci, _)) = best {
            self.prompt = Some("E: Get in and drive".to_owned());
            if input.interact {
                self.enter_car(ci);
            }
        }
    }

    // ------------------------------------------------------------ flying ---

    fn enter_ufo(&mut self) {
        self.flying = true;
        self.ufo.yaw = self.player.yaw;
        self.ufo.pitch = self.player.pitch.clamp(-0.8, 0.8);
        self.ufo.vel = Vec3::ZERO;
        self.player.vel = Vec2::ZERO;
        self.player.hop = 0.0;
        self.say("UFO! Mouse aims, W/S/A/D fly, Space/C up & down, click shoots saucers. Shift = boost, F = land & exit.", 7.0);
    }

    /// Everything the avoidance AI should dodge: all live wild saucers nearby.
    fn saucer_neighbours(&self, skip: Option<usize>, near: Vec2) -> Vec<Neighbour> {
        self.saucers
            .iter()
            .enumerate()
            .filter(|(i, u)| u.alive && Some(*i) != skip && u.pos.dist(near) < 90.0)
            .map(|(_, u)| Neighbour { pos: u.centre(), vel: u.vel })
            .collect()
    }

    fn update_flying(&mut self, input: &Input, dt: f32) {
        self.player.hurt_timer = (self.player.hurt_timer - dt).max(0.0);
        self.player.damage_flash = (self.player.damage_flash - dt).max(0.0);
        self.ufo.yaw += input.look_dx * 0.0025;
        self.ufo.pitch = (self.ufo.pitch - input.look_dy * 0.0025).clamp(-1.0, 1.0);

        let others = self.saucer_neighbours(None, self.ufo.pos);
        let boxes = self.obstacles(None);
        let ev = self.ufo.update_flown(input, dt, &boxes, &self.world, &others);
        if ev.impact > 4.0 {
            self.shake = self.shake.max((ev.impact / 20.0).min(1.0));
        }
        if self.ufo.turbo_on {
            self.shake = self.shake.max(0.18);
        }
        self.player.pos = self.ufo.pos;
        self.player.vel = Vec2::ZERO;
        self.player.yaw = self.ufo.yaw;

        if input.shoot && self.ufo.shoot_cd <= 0.0 {
            self.ufo.shoot_cd = ufo::SHOT_COOLDOWN;
            let dir = self.ufo.aim_dir();
            let c = self.ufo.centre();
            // Twin cannons: alternate left / right of the cockpit.
            let side = if (self.anim_time * 6.0) as i32 % 2 == 0 { 1.0 } else { -1.0 };
            let muzzle = c + Vec3::from_xz(self.ufo.right() * (0.9 * side), -0.3) + dir * 2.2;
            let origin = c + Vec3::new(0.0, 0.2, 0.0);
            self.shoot(origin, dir, muzzle, &boxes, true);
        }

        if input.exit_car {
            let gh = self.world.ground_height(self.ufo.pos);
            if self.ufo.y - gh <= ufo::EXIT_Y {
                self.exit_ufo();
                return;
            }
            self.say("Too high to get out. Hold C to land first.", 2.0);
        }
        // WIN: cross the finish arch in anything.
        if self.ufo.pos.x >= world::FINISH_X {
            self.final_time = self.run_time;
            self.mode = Mode::Won;
        }
    }

    fn exit_ufo(&mut self) {
        self.flying = false;
        self.ufo.vel = Vec3::ZERO;
        self.player.pos = self.ufo.pos - self.ufo.right() * (ufo::RADIUS + 1.4);
        self.player.vel = Vec2::ZERO;
        self.player.pitch = 0.0;
        let obstacles = self.obstacles(None);
        crate::collision::resolve_circle(&mut self.player.pos, player::RADIUS, &obstacles);
        self.say("Landed. E next to the UFO to fly again.", 3.0);
    }

    /// Wild saucers wander and dodge each other and your UFO.
    fn update_saucers(&mut self, dt: f32) {
        let leader = if self.flying { self.ufo.pos } else { self.player.pos };
        let leader = match self.drive {
            Some(ci) if !self.flying => self.cars[ci].pos,
            _ => leader,
        };
        let player_ufo = if self.flying { Some(Neighbour { pos: self.ufo.centre(), vel: self.ufo.vel }) } else { None };
        // Snapshot first so everyone reacts to the same instant (order-independent).
        let snap: Vec<(Vec3, Vec3, bool)> = self.saucers.iter().map(|u| (u.centre(), u.vel, u.alive)).collect();
        for i in 0..self.saucers.len() {
            if !self.saucers[i].alive || self.saucers[i].pos.dist(leader) > SAUCER_ACTIVE {
                continue;
            }
            let me = snap[i].0;
            let mut others: Vec<Neighbour> = snap
                .iter()
                .enumerate()
                .filter(|(j, s)| *j != i && s.2 && (s.0 - me).len() < 60.0)
                .map(|(_, s)| Neighbour { pos: s.0, vel: s.1 })
                .collect();
            others.extend(player_ufo);
            self.saucers[i].update_wild(dt, &others);
        }
        self.saucers.retain(|u| u.alive);
    }

    // -------------------------------------------------------- driving ---

    fn enter_car(&mut self, ci: usize) {
        self.drive = Some(ci);
        self.cab_yaw = angle_diff(self.player.yaw, self.cars[ci].yaw).clamp(-1.6, 1.6);
        self.cab_pitch = self.player.pitch.clamp(-0.5, 0.5);
        self.player.vel = Vec2::ZERO;
        self.player.hop = 0.0;
        self.say("Floor it! Shift = turbo, ramps = air, your crew rides along. F to bail out.", 6.0);
    }

    fn update_driving(&mut self, ci: usize, input: &Input, dt: f32) {
        // Free-look head turn relative to the car body.
        self.cab_yaw = (self.cab_yaw + input.look_dx * 0.0025).clamp(-1.7, 1.7);
        self.cab_pitch = (self.cab_pitch - input.look_dy * 0.0025).clamp(-0.6, 0.6);
        self.player.hurt_timer = (self.player.hurt_timer - dt).max(0.0);
        self.player.damage_flash = (self.player.damage_flash - dt).max(0.0);

        let others = self.obstacles(Some(ci));
        let ev = self.cars[ci].update_driven(input, dt, &others, &self.world);
        if ev.impact > 4.0 {
            self.shake = self.shake.max((ev.impact / 20.0).min(1.0));
        }
        if self.cars[ci].turbo_on {
            self.shake = self.shake.max(0.22);
        }
        let above = Vec3::from_xz(self.cars[ci].pos, self.cars[ci].y + 3.0);
        if ev.launched {
            self.popups.push(Popup { pos: above, text: "AIRBORNE!", ttl: 0.9 });
        }
        if let Some(t) = ev.landed_after {
            if t > 0.4 {
                let text = if t > 1.2 { "HUGE AIR!" } else { "NICE LANDING!" };
                self.popups.push(Popup { pos: above, text, ttl: 0.9 });
                self.shake = self.shake.max(0.35);
                // Big jumps charge the clown power.
                self.laser_charge = (self.laser_charge + 0.15 * t.min(2.0)).min(1.0);
            }
        }
        self.player.pos = self.cars[ci].pos;
        self.player.vel = Vec2::ZERO;

        if input.exit_car && !self.cars[ci].airborne {
            self.exit_car(ci);
            return;
        }
        // WIN: cross the finish arch in any car.
        if self.cars[ci].pos.x >= world::FINISH_X {
            self.final_time = self.run_time;
            self.mode = Mode::Won;
        }
    }

    fn exit_car(&mut self, ci: usize) {
        self.cars[ci].speed = 0.0; // handbrake: the car stays where you got out
        let car = &self.cars[ci];
        self.player.pos = car.door_point();
        self.player.yaw = car.yaw + self.cab_yaw;
        self.player.pitch = self.cab_pitch;
        self.player.vel = Vec2::ZERO;
        self.drive = None;
        let obstacles = self.obstacles(None);
        crate::collision::resolve_circle(&mut self.player.pos, player::RADIUS, &obstacles);
        self.say("Out of the car. Shoot them! E next to a car to get back in.", 3.0);
    }

    // ----------------------------------------------------- clown laser ---

    /// Where the lasers shoot from: the clown's chest, or the middle of the car.
    fn laser_source(&self) -> Vec3 {
        if self.flying {
            return self.ufo.centre() + Vec3::new(0.0, 0.3, 0.0);
        }
        match self.drive {
            Some(ci) => Vec3::from_xz(self.cars[ci].pos, self.cars[ci].y + 1.5),
            None => {
                let e = self.player.eye();
                Vec3::new(e.x, e.y - 0.2, e.z)
            }
        }
    }

    /// SUPER POWER: when charged, the clown's eyes fire a ring of lasers that
    /// vaporise every zombie within `LASER_RANGE` at once (even through a
    /// crowd that's closing in). Works on foot or in a car.
    fn update_laser(&mut self, input: &Input) {
        let origin = self.laser_source();
        let centre = origin.xz();
        let near = self.zombies.iter().filter(|z| z.state == ZState::Walking && z.pos.dist(centre) < SURROUND_RADIUS).count();
        self.surrounded = near >= SURROUND_COUNT;

        if !input.laser {
            return;
        }
        if self.laser_charge < 1.0 {
            self.say("Clown power is still charging...", 1.5);
            return;
        }
        let hits: Vec<usize> = (0..self.zombies.len())
            .filter(|&i| self.zombies[i].state == ZState::Walking && self.zombies[i].pos.dist(centre) <= LASER_RANGE)
            .collect();
        if hits.is_empty() {
            self.say("No zombies in range. Save the lasers!", 1.5);
            return;
        }
        self.lasers.clear();
        for (n, &i) in hits.iter().enumerate() {
            let zp = self.zombies[i].pos;
            let away = (zp - centre).norm();
            self.zombies[i].kill(away * 9.0);
            self.kills += 1;
            self.lasers.push(Laser { from: origin, to: Vec3::from_xz(zp, 1.2) });
            if n < 6 {
                self.popups.push(Popup { pos: Vec3::from_xz(zp, 2.3), text: "ZAP!", ttl: 0.9 });
            }
        }
        self.laser_charge = 0.0;
        self.laser_burst = LASER_TTL;
        self.laser_origin = origin;
        self.laser_flash = 0.35;
        self.shake = self.shake.max(0.7);
        let plural = if hits.len() == 1 { "zombie" } else { "zombies" };
        self.say(&format!("CLOWN LASERS! {} {} vaporised", hits.len(), plural), 2.5);
    }

    // ------------------------------------------------------------ crew ---

    /// Recruit waiting clowns you pass, then move the crew: on foot they hover
    /// around you, in a car they climb aboard. Everyone shoots the nearest
    /// zombie in sight every second or so.
    fn update_allies(&mut self, dt: f32) {
        let leader = match self.drive {
            Some(ci) => self.cars[ci].pos,
            None => self.player.pos, // (the player rides along with the UFO)
        };
        // Recruit.
        let mut crew = self.crew_size();
        for i in 0..self.allies.len() {
            if self.allies[i].recruited {
                continue;
            }
            if crew < allies::MAX_CREW && self.allies[i].pos.dist(leader) < allies::RECRUIT_RANGE {
                self.allies[i].recruited = true;
                crew += 1;
                self.say(&format!("A clown joined your crew! ({}/{})", crew, allies::MAX_CREW), 2.5);
            } else {
                self.allies[i].wave(dt);
            }
        }

        // Move.
        let obstacles = self.obstacles(None);
        let n = crew.max(1);
        let mut k = 0;
        for i in 0..self.allies.len() {
            if !self.allies[i].recruited {
                continue;
            }
            match self.drive {
                Some(ci) => {
                    let c = &self.cars[ci];
                    let (along, right, up) = allies::SEATS[k % allies::SEATS.len()];
                    let a = &mut self.allies[i];
                    a.seat = Some(k);
                    a.pos = c.pos + c.forward() * along + c.right() * right;
                    a.y = c.y + up + along * c.pitch.sin();
                    a.vel = Vec2::ZERO;
                    if a.aim_t <= 0.0 {
                        a.yaw = c.yaw;
                    }
                    a.shoot_cd -= dt;
                    a.aim_t = (a.aim_t - dt).max(0.0);
                    a.phase += dt * 5.0;
                }
                None => {
                    let ang = k as f32 * std::f32::consts::TAU / n as f32 + self.anim_time * 0.25;
                    let radius = 3.4 + (k % 2) as f32 * 1.6;
                    let spot = leader + Vec2::from_yaw(ang) * radius;
                    let a = &mut self.allies[i];
                    a.seat = None;
                    a.follow(dt, spot, leader, &obstacles);
                    a.y = a.y.max(self.world.ground_height(a.pos));
                }
            }
            k += 1;
        }

        // Shoot.
        for i in 0..self.allies.len() {
            if !self.allies[i].recruited || self.allies[i].shoot_cd > 0.0 {
                continue;
            }
            let (apos, ay) = (self.allies[i].pos, self.allies[i].y);
            let from = Vec3::from_xz(apos, ay + 1.4);
            let mut best: Option<(usize, f32)> = None;
            for (zi, z) in self.zombies.iter().enumerate() {
                if z.state != ZState::Walking || (z.pos.x - apos.x).abs() > allies::AIM_RANGE {
                    continue;
                }
                let d = z.pos.dist(apos);
                if d < allies::AIM_RANGE && best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((zi, d));
                }
            }
            let Some((zi, d)) = best else {
                self.allies[i].shoot_cd = 0.25; // nothing to shoot: look again soon
                continue;
            };
            let to = Vec3::from_xz(self.zombies[zi].pos, 1.0);
            let dir = (to - from).norm();
            let blocked = self.world.statics.iter().any(|o| o.ray(from, dir, d).map_or(false, |t| t < d - 0.8));
            let a = &mut self.allies[i];
            a.shoot_cd = rand_range(0.8, 1.5);
            if blocked {
                continue;
            }
            a.aim_t = 0.3;
            a.yaw = (to.z - from.z).atan2(to.x - from.x);
            let hit = rand_chance(0.8);
            let end = if hit { to } else { to + Vec3::new(rand_range(-1.5, 1.5), 0.0, rand_range(-1.5, 1.5)) };
            self.tracers.push(Tracer { from, to: end, ttl: 0.12, ally: true });
            if hit {
                let away = Vec2::new(dir.x, dir.z).norm();
                let zp = self.zombies[zi].pos;
                self.zombies[zi].kill(away * 6.0);
                self.kills += 1;
                self.laser_charge = (self.laser_charge + LASER_PER_KILL).min(1.0);
                if rand_chance(0.5) {
                    self.popups.push(Popup { pos: Vec3::from_xz(zp, 2.2), text: "HONK!", ttl: 0.9 });
                }
            }
        }
    }

    // --------------------------------------------------------- zombies ---

    /// Moving cars kill walking zombies and lose speed doing it. Runs BEFORE
    /// zombie collision so a fast car hits zombies instead of bulldozing them.
    fn car_hits_zombies(&mut self) {
        for ci in 0..self.cars.len() {
            if self.cars[ci].speed.abs() < ROADKILL_SPEED || self.cars[ci].y > 1.0 {
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
                    self.zombies[zi].kill(push);
                    self.kills += 1;
                    self.laser_charge = (self.laser_charge + LASER_PER_KILL).min(1.0);
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
            if z.pos.dist(target) <= ACTIVE_RANGE {
                z.update(dt, target, &obstacles);
            }
        }

        // Soft separation so zombies don't stack into one blob.
        let n = self.zombies.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.zombies[i].state != ZState::Walking
                    || self.zombies[j].state != ZState::Walking
                    || (self.zombies[i].pos.x - self.zombies[j].pos.x).abs() > 1.0
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
        if self.drive.is_none() && !self.flying {
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
                    break;
                }
            }
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
            _ if self.flying => {
                let u = &self.ufo;
                let mut pos = Vec3::from_xz(u.pos, u.y + 0.9);
                let s = self.shake * 0.1;
                pos.x += (self.anim_time * 61.0).sin() * s;
                pos.y += (self.anim_time * 73.0).cos() * s;
                let speed_t = (u.speed() / ufo::BOOST_SPEED).min(1.0);
                CamView { pos, yaw: u.yaw, pitch: u.pitch, roll: u.bank * 0.35, fov: 74.0 + 22.0 * speed_t }
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
                    let mut pos = Vec3::from_xz(seat, 1.38 + c.y);
                    let s = self.shake * 0.12;
                    pos.x += (self.anim_time * 61.0).sin() * s;
                    pos.y += (self.anim_time * 73.0).cos() * s;
                    let speed_t = (c.speed.abs() / car::TURBO_SPEED).min(1.0);
                    // The cab tips with the car (nose up on a ramp), less so when looking sideways.
                    let tip = c.pitch * self.cab_yaw.cos();
                    CamView { pos, yaw: c.yaw + self.cab_yaw, pitch: self.cab_pitch + tip, roll: -c.steer * 0.02 * speed_t, fov: 70.0 + 28.0 * speed_t }
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
        g.allies.clear();
        g.saucers.clear();
        g.player = Player::new(Vec2::new(0.0, 0.0));
        g
    }

    fn tick(g: &mut Game, input: &Input) {
        g.update(input, DT);
    }
    fn press_interact(g: &mut Game) {
        tick(g, &Input { interact: true, ..Default::default() });
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
    fn shot_kills_zombie_and_it_stays_down() {
        let mut g = arena();
        let mut z = Zombie::new(Vec2::new(6.0, 0.0), 0.0); // aggro 0: just stands there
        z.speed = 0.0;
        g.zombies.push(z);
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.zombies[0].state, ZState::Dead, "hitscan should kill");
        assert_eq!(g.kills, 1);
        assert!(!g.tracers.is_empty());
        for _ in 0..600 {
            tick(&mut g, &Input::default());
        }
        assert_eq!(g.zombies[0].state, ZState::Dead);
        assert!(g.zombies[0].tilt() > 0.99, "lying flat");
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
    fn strip_is_full_of_zombies() {
        for _ in 0..5 {
            let g = Game::new();
            let strip = g.zombies.iter().filter(|z| z.pos.x > world::STRIP_START).count();
            assert!(strip >= 40, "strip zombies: {strip}");
            // Spread along the whole road, not bunched at the start.
            let far = g.zombies.iter().filter(|z| z.pos.x > world::FINISH_X - 300.0).count();
            assert!(far >= 3, "zombies near the finish: {far}");
        }
    }

    #[test]
    fn getting_out_stops_the_car_and_lets_you_shoot() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(10.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        g.cars[0].speed = 18.0;
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        assert_eq!(g.drive, None);
        assert_eq!(g.cars[0].speed, 0.0);
        let mut z = Zombie::new(g.player.pos + Vec2::new(0.0, 8.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        g.player.yaw = std::f32::consts::FRAC_PI_2; // face +z, toward the zombie
        g.player.pitch = 0.0;
        // The wobble is random, so allow a few shots.
        for _ in 0..30 {
            tick(&mut g, &Input { shoot: true, ..Default::default() });
            if g.zombies[0].state == ZState::Dead {
                break;
            }
            for _ in 0..25 {
                tick(&mut g, &Input::default());
            }
        }
        assert_eq!(g.zombies[0].state, ZState::Dead);
    }

    #[test]
    fn can_get_into_any_car_and_win_at_the_finish() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 3.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        press_interact(&mut g);
        assert_eq!(g.drive, Some(0), "empty car, no zombies needed");
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
    fn too_far_from_a_car_does_nothing() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 12.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        press_interact(&mut g);
        assert_eq!(g.drive, None);
    }

    #[test]
    fn exiting_car_returns_to_board() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(10.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        assert_eq!(g.drive, None);
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
    fn skater_top_speed_is_tame() {
        let mut g = arena();
        let hold_w = Input { forward: true, ..Default::default() };
        for _ in 0..400 {
            tick(&mut g, &hold_w);
            g.player.pos = Vec2::ZERO; // stay in the arena; we only measure speed
        }
        let free = g.player.speed();
        assert!(free > 5.0 && free <= 7.5, "free speed {free}");
    }

    #[test]
    fn car_kills_walking_zombies_and_slows_down() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        g.cars[0].speed = 17.0;
        let mut z = Zombie::new(Vec2::new(3.5, 0.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        for _ in 0..10 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
        }
        assert_eq!(g.zombies[0].state, ZState::Dead);
        assert!(g.cars[0].speed < 17.0);
        assert_eq!(g.kills, 1);
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

    // ---------------------------------------------------------------- new ---

    /// Autopilot: steer toward a point 14 m ahead on the road's centre line.
    fn autopilot(car: &Car, turbo: bool) -> Input {
        let ahead = world::road_point(car.pos.x + 14.0, 0.0);
        let want = (ahead.y - car.pos.y).atan2(ahead.x - car.pos.x);
        let d = angle_diff(want, car.yaw);
        Input { forward: true, turbo, right: d > 0.04, left: d < -0.04, ..Default::default() }
    }

    #[test]
    fn the_road_curves_but_never_too_sharply() {
        let mut max_z = 0.0f32;
        let mut max_slope = 0.0f32;
        let mut x = world::STRIP_START;
        while x < world::FINISH_X {
            max_z = max_z.max(world::road_z(x).abs());
            max_slope = max_slope.max(world::road_yaw(x).abs());
            x += 0.5;
        }
        assert!(max_z > 15.0, "the road should visibly swing, max offset {max_z}");
        assert!(max_slope < 0.6, "bends must stay drivable, max heading {max_slope}");
        // The lot exit, the finish and every ramp run are dead straight.
        assert_eq!(world::road_z(world::STRIP_START + 10.0), 0.0);
        assert_eq!(world::road_z(world::FINISH_X), 0.0);
        for r in &g_ramps() {
            assert_eq!(world::road_z(r.x0), 0.0);
            assert_eq!(world::road_z(r.x0 + 70.0), 0.0);
        }
    }

    fn g_ramps() -> Vec<world::Ramp> {
        Game::new().world.ramps.clone()
    }

    #[test]
    fn turbo_beats_normal_top_speed_and_drains_the_tank() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        for _ in 0..240 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
            g.cars[0].pos = Vec2::new(0.0, 0.0);
        }
        let normal = g.cars[0].speed;
        assert!(normal <= car::MAX_SPEED + 0.01, "normal {normal}");
        let mut top = 0.0f32;
        for _ in 0..120 {
            tick(&mut g, &Input { forward: true, turbo: true, ..Default::default() });
            g.cars[0].pos = Vec2::new(0.0, 0.0);
            top = top.max(g.cars[0].speed);
        }
        assert!(top > car::MAX_SPEED + 6.0, "turbo top speed {top}");
        assert!(g.cars[0].turbo < 0.75, "tank should drain, {}", g.cars[0].turbo);
        // Let off: speed bleeds back to normal and the tank refills.
        for _ in 0..600 {
            tick(&mut g, &Input { forward: true, ..Default::default() });
            g.cars[0].pos = Vec2::new(0.0, 0.0);
        }
        assert!(g.cars[0].speed <= car::MAX_SPEED + 0.01);
        assert!(g.cars[0].turbo > 0.9);
    }

    #[test]
    fn ramps_launch_the_car_and_it_always_lands_on_the_road() {
        // Slow, normal and full-turbo approaches, with the car arriving a bit crooked.
        for (turbo, yaw_off, z_off) in [(false, 0.0, 0.0), (true, 0.0, 0.0), (true, 0.12, 2.0), (false, -0.1, -2.5), (true, -0.12, 3.0)] {
            for ri in 0..5 {
                let mut g = arena();
                let r = g.world.ramps[ri];
                g.cars.push(Car::new(Vec2::new(r.x0 - 30.0, z_off), yaw_off, Color::rgb(1.0, 0.0, 0.0)));
                g.drive = Some(0);
                g.cars[0].speed = if turbo { 22.0 } else { 14.0 };
                let (mut flew, mut max_y) = (false, 0.0f32);
                for _ in 0..600 {
                    tick(&mut g, &Input { forward: true, turbo, ..Default::default() });
                    flew |= g.cars[0].airborne;
                    max_y = max_y.max(g.cars[0].y);
                    if flew && !g.cars[0].airborne {
                        break;
                    }
                }
                let c = &g.cars[0];
                assert!(flew, "ramp {ri} turbo={turbo}: never took off (x={}, y={})", c.pos.x, c.y);
                assert!(max_y > 1.5, "should get real height, got {max_y}");
                assert!(!c.airborne && c.y == 0.0, "should have landed");
                if yaw_off == 0.0 {
                    assert!(c.pos.x > r.x0 + r.len, "landed past the ramp, x={}", c.pos.x);
                }
                assert!(c.pos.x < r.x0 + r.len + 64.0, "landed inside the clear landing zone, x={}", c.pos.x);
                assert!(c.pos.y.abs() < world::ROAD_HALF, "landed on the road, z={}", c.pos.y);
                assert!(angle_diff(c.yaw, 0.0).abs() < 0.2, "landing assist straightens the car, yaw={}", c.yaw);
                assert!(c.speed > 8.0, "keeps going after landing, speed={}", c.speed);
            }
        }
    }

    #[test]
    fn cars_fly_over_low_obstacles_but_not_walls() {
        let mut g = arena();
        g.world.statics.push(Obb::aabb(30.0, 0.0, 1.0, 5.0, 1.0)); // a low barrier line
        g.cars.push(Car::new(Vec2::new(20.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        g.cars[0].speed = 15.0;
        g.cars[0].y = 3.0;
        g.cars[0].airborne = true;
        g.cars[0].vy = 6.0;
        for _ in 0..54 {
            tick(&mut g, &Input::default());
        }
        assert!(g.cars[0].pos.x > 33.0, "flew over the low barrier, x={}", g.cars[0].pos.x);
    }

    #[test]
    fn autopilot_can_drive_the_whole_winding_strip_with_ramps() {
        for turbo in [false, true] {
            let mut g = arena();
            g.world.statics.clear();
            g.cars.push(Car::new(Vec2::new(world::STRIP_START + 5.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
            g.drive = Some(0);
            let mut max_off = 0.0f32;
            let mut flights = 0;
            let mut was_air = false;
            for _ in 0..(60 * 150) {
                let inp = autopilot(&g.cars[0], turbo && g.cars[0].turbo > 0.3);
                tick(&mut g, &inp);
                let c = &g.cars[0];
                max_off = max_off.max((c.pos.y - world::road_z(c.pos.x)).abs());
                if c.airborne && !was_air {
                    flights += 1;
                }
                was_air = c.airborne;
                if g.mode == Mode::Won {
                    break;
                }
            }
            assert_eq!(g.mode, Mode::Won, "turbo={turbo}: never reached the finish (x={})", g.cars[0].pos.x);
            assert!(max_off < world::ROAD_HALF, "stayed on the road, max offset {max_off}");
            assert_eq!(flights, 5, "one flight per ramp");
        }
    }

    #[test]
    fn clown_lasers_vaporise_the_surrounding_zombies() {
        let mut g = arena();
        g.laser_charge = 1.0;
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            let mut z = Zombie::new(g.player.pos + Vec2::from_yaw(a) * 5.0, 500.0);
            z.speed = 0.0;
            g.zombies.push(z);
        }
        let mut far = Zombie::new(g.player.pos + Vec2::new(60.0, 0.0), 0.0);
        far.speed = 0.0;
        g.zombies.push(far);
        tick(&mut g, &Input::default());
        assert!(g.surrounded, "8 zombies at 5 m is surrounded");
        tick(&mut g, &Input { laser: true, ..Default::default() });
        let dead = g.zombies.iter().filter(|z| z.state == ZState::Dead).count();
        assert_eq!(dead, 8, "everything in range dies");
        assert_eq!(g.zombies[8].state, ZState::Walking, "out of range survives");
        assert_eq!(g.kills, 8);
        assert!(g.laser_charge < 0.1, "power is spent");
        assert_eq!(g.lasers.len(), 8);
        // Uncharged lasers do nothing.
        let mut z = Zombie::new(g.player.pos + Vec2::new(3.0, 0.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        tick(&mut g, &Input { laser: true, ..Default::default() });
        assert_eq!(g.zombies[9].state, ZState::Walking);
    }

    #[test]
    fn lasers_also_work_from_the_car() {
        let mut g = arena();
        g.cars.push(Car::new(Vec2::new(0.0, 0.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        g.drive = Some(0);
        g.laser_charge = 1.0;
        let mut z = Zombie::new(Vec2::new(0.0, 9.0), 0.0);
        z.speed = 0.0;
        g.zombies.push(z);
        tick(&mut g, &Input { laser: true, ..Default::default() });
        assert_eq!(g.zombies[0].state, ZState::Dead);
    }

    #[test]
    fn crew_follows_you_shoots_zombies_and_rides_in_the_car() {
        let mut g = arena();
        g.allies.push(Ally::new(Vec2::new(-3.0, 2.0), true));
        g.allies.push(Ally::new(Vec2::new(-3.0, -2.0), true));
        // They hover near you when you move.
        g.player.pos = Vec2::new(30.0, 0.0);
        for _ in 0..240 {
            tick(&mut g, &Input::default());
        }
        for a in &g.allies {
            assert!(a.pos.dist(g.player.pos) < 9.0, "ally follows, dist {}", a.pos.dist(g.player.pos));
        }
        // They shoot zombies for you.
        for k in 0..6 {
            let mut z = Zombie::new(g.player.pos + Vec2::new(10.0 + k as f32, (k as f32) - 3.0), 0.0);
            z.speed = 0.0;
            g.zombies.push(z);
        }
        for _ in 0..(60 * 12) {
            tick(&mut g, &Input::default());
        }
        let dead = g.zombies.iter().filter(|z| z.state == ZState::Dead).count();
        assert!(dead >= 4, "crew killed {dead} of 6");
        assert!(g.kills as usize >= dead);
        // They climb aboard when you drive, and hop off when you get out.
        g.cars.push(Car::new(g.player.pos + Vec2::new(0.0, 3.0), 0.0, Color::rgb(1.0, 0.0, 0.0)));
        press_interact(&mut g);
        assert_eq!(g.drive, Some(0));
        tick(&mut g, &Input::default());
        for a in &g.allies {
            assert!(a.seat.is_some());
            assert!(a.pos.dist(g.cars[0].pos) < 3.0, "rider is on the car");
        }
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        tick(&mut g, &Input::default());
        assert!(g.allies.iter().all(|a| a.seat.is_none()));
    }

    #[test]
    fn waiting_clowns_join_when_you_pass_and_the_crew_is_capped() {
        let mut g = arena();
        g.allies.push(Ally::new(Vec2::new(40.0, 0.0), false));
        g.allies.push(Ally::new(Vec2::new(300.0, 0.0), false));
        g.player.pos = Vec2::new(36.0, 0.0);
        tick(&mut g, &Input::default());
        assert!(g.allies[0].recruited, "near clown joins");
        assert!(!g.allies[1].recruited, "far clown keeps waving");
        assert_eq!(g.crew_size(), 1);
        // Fill the crew; extra recruits are turned away.
        g.allies.clear();
        for _ in 0..allies::MAX_CREW {
            g.allies.push(Ally::new(g.player.pos + Vec2::new(2.0, 0.0), true));
        }
        g.allies.push(Ally::new(g.player.pos + Vec2::new(4.0, 0.0), false));
        tick(&mut g, &Input::default());
        assert_eq!(g.crew_size(), allies::MAX_CREW);
    }

    #[test]
    fn nothing_sits_in_a_ramp_landing_zone() {
        for _ in 0..5 {
            let g = Game::new();
            for z in &g.zombies {
                assert!(!(world::in_clear_zone(z.pos.x - 4.0) && world::in_clear_zone(z.pos.x + 4.0)) || z.pos.x < world::STRIP_START, "zombie in landing zone at {}", z.pos.x);
            }
            for r in &g.world.ramps {
                let blockers = g.world.statics.iter().filter(|o| o.center.x > r.x0 - 14.0 && o.center.x < r.x0 + r.len + 64.0 && o.half.x < 5.0 && o.center.y.abs() < 12.0 && o.height > 0.6).count();
                // The only boxes allowed near a ramp are its own two flanks (height 0.5, filtered out).
                assert_eq!(blockers, 0, "obstacle near ramp at {}", r.x0);
            }
        }
    }

    #[test]
    fn skater_turbo_is_faster_but_still_tame_without_it() {
        let mut g = arena();
        let hold = Input { forward: true, turbo: true, ..Default::default() };
        let mut top = 0.0f32;
        for _ in 0..120 {
            tick(&mut g, &hold);
            g.player.pos = Vec2::ZERO;
            top = top.max(g.player.speed());
        }
        assert!(top > 8.5, "turbo skating {top}");
    }

    // ------------------------------------------------------------ UFO ---

    fn fly(g: &mut Game) {
        g.player.pos = g.ufo.pos + Vec2::new(3.0, 0.0);
        press_interact(g);
        assert!(g.flying, "E next to the parked UFO boards it");
    }

    #[test]
    fn parked_ufo_is_in_the_lot_and_clear_of_cars() {
        for _ in 0..30 {
            let g = Game::new();
            assert!(!g.flying);
            assert!(g.cars.iter().all(|c| c.pos.dist(g.ufo.pos) > 8.0));
            assert!(g.saucers.len() > 15, "saucers: {}", g.saucers.len());
        }
    }

    #[test]
    fn board_fly_land_and_exit_the_ufo() {
        let mut g = arena();
        g.ufo.pos = Vec2::new(10.0, 10.0);
        g.player.pos = Vec2::new(30.0, 10.0);
        press_interact(&mut g);
        assert!(!g.flying, "too far away");
        fly(&mut g);
        for _ in 0..120 {
            tick(&mut g, &Input { up: true, forward: true, ..Default::default() });
        }
        assert!(g.ufo.y > 8.0 && g.ufo.pos.x > 10.0, "flew: y {} x {}", g.ufo.y, g.ufo.pos.x);
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        assert!(g.flying, "can't step out in mid air");
        for _ in 0..400 {
            tick(&mut g, &Input { down: true, ..Default::default() });
        }
        assert!(g.ufo.y < 3.0, "landed, y {}", g.ufo.y);
        tick(&mut g, &Input { exit_car: true, ..Default::default() });
        assert!(!g.flying);
        assert!(g.player.pos.dist(g.ufo.pos) > 2.0);
        for _ in 0..120 {
            tick(&mut g, &Input::default());
        }
        assert!(g.ufo.y < 2.0 && g.ufo.speed() == 0.0, "parked again");
    }

    #[test]
    fn ufo_cannons_shoot_down_a_saucer_and_miss_when_aimed_away() {
        let mut g = arena();
        g.ufo.pos = Vec2::new(0.0, 0.0);
        g.ufo.y = 10.0;
        fly(&mut g);
        g.ufo.y = 10.0;
        let mut s = Ufo::wild(40.0, Color::rgb(0.3, 1.0, 0.4));
        s.pos = Vec2::new(30.0, 0.0);
        s.y = 10.0;
        g.saucers = vec![s];
        g.ufo.pitch = 0.5;
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.saucers_down, 0, "aimed too high");
        g.ufo.pitch = 0.0;
        g.ufo.yaw = 0.0;
        for _ in 0..30 {
            tick(&mut g, &Input::default());
        }
        g.saucers[0].pos = Vec2::new(30.0, 0.0);
        g.saucers[0].y = g.ufo.y;
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.saucers_down, 1);
        assert!(g.saucers.is_empty(), "wreck removed");
        assert!(!g.booms.is_empty());
    }

    #[test]
    fn the_pistol_also_hits_saucers() {
        let mut g = arena();
        let mut s = Ufo::wild(40.0, Color::rgb(0.3, 1.0, 0.4));
        s.pos = Vec2::new(12.0, 0.0);
        s.y = g.player.eye().y;
        g.saucers = vec![s];
        tick(&mut g, &Input { shoot: true, ..Default::default() });
        assert_eq!(g.saucers_down, 1);
    }

    #[test]
    fn flying_into_a_crowd_of_saucers_never_touches_one() {
        let mut g = arena();
        g.ufo.pos = Vec2::new(0.0, 0.0);
        fly(&mut g);
        g.ufo.y = 10.0;
        for k in 0..8 {
            let mut s = Ufo::wild(100.0, Color::rgb(0.3, 1.0, 0.4));
            s.pos = Vec2::new(40.0 + k as f32 * 18.0, (k as f32 - 3.5) * 2.5);
            s.y = 10.0 + (k % 3) as f32;
            g.saucers.push(s);
        }
        let mut min = f32::MAX;
        for _ in 0..(25 * 60) {
            tick(&mut g, &Input { forward: true, ..Default::default() });
            for s in &g.saucers {
                min = min.min((s.centre() - g.ufo.centre()).len());
            }
            let n = g.saucers.len();
            for i in 0..n {
                for j in (i + 1)..n {
                    min = min.min((g.saucers[i].centre() - g.saucers[j].centre()).len());
                }
            }
        }
        assert!(min > ufo::RADIUS * 2.0 - 0.45, "saucers overlapped, min {min}");
    }
}
