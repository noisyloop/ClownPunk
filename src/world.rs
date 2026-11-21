//! The static level: a parking lot that opens onto a long, winding neon boulevard.
//!
//! Layout (top-down, +X is "down the Strip", +Z is the player's right at yaw 0):
//!
//!   x: -45 ........ 20 | 20 ................................ FINISH_X
//!        PARKING LOT   |            THE STRIP
//!
//! The Strip always makes progress along +X, but its centre line swings left
//! and right: `road_z(x)` is the lateral offset of the road at distance `x`.
//! Curvy sections alternate with dead-straight sections that hold a jump RAMP
//! and a long clear landing zone, so a flight always ends on open road.
//! Everything that sits "beside the road" is placed with `road_point`.
//!
//! `World` holds the things that never move: collision boxes, buildings,
//! ramps and props (lamps, palms, barriers...). Dynamic things live elsewhere.
//! `World` holds the things that never move: collision boxes, buildings and
//! props (lamps, palms, barriers...). Dynamic things live in other modules.
use crate::collision::Obb;
use crate::math::{Color, Vec2};
use crate::platform::{rand_chance, rand_range};

pub const LOT_MIN_X: f32 = -45.0;
pub const LOT_HALF_Z: f32 = 30.0;
pub const STRIP_START: f32 = 20.0;
pub const STRIP_LEN: f32 = 1500.0;
pub const FINISH_X: f32 = STRIP_START + STRIP_LEN;
pub const ROAD_HALF: f32 = 13.0;
/// Inner face of the invisible wall that keeps you on the sidewalk.
pub const SIDE_WALL_Z: f32 = 19.0;

/// Curvy stretches of the Strip: (start x, end x, amplitude, shape).
/// shape 0 = one big swerve, shape 1 = an S-bend. Outside these the road is
/// straight along z = 0 (the lot exit, the ramp runs and the finish).
const CURVES: [(f32, f32, f32, u8); 5] = [
    (120.0, 320.0, 28.0, 0),
    (420.0, 620.0, 18.0, 1),
    (720.0, 920.0, -28.0, 0),
    (1020.0, 1220.0, -18.0, 1),
    (1320.0, 1420.0, 10.0, 0),
];

/// Lateral offset of the road's centre line at distance `x` along the Strip.
pub fn road_z(x: f32) -> f32 {
    for &(a, b, amp, kind) in &CURVES {
        if x > a && x < b {
            let t = (x - a) / (b - a);
            let bump = (std::f32::consts::PI * t).sin().powi(2);
            return match kind {
                0 => amp * bump,
                _ => amp * (std::f32::consts::TAU * t).sin() * bump,
            };
        }
    }
    0.0
}

/// Heading of the road at `x` (0 = straight down +X).
pub fn road_yaw(x: f32) -> f32 {
    ((road_z(x + 0.25) - road_z(x - 0.25)) / 0.5).atan()
}

/// The point `off` metres to the right of the road's centre line at `x`.
pub fn road_point(x: f32, off: f32) -> Vec2 {
    let c = Vec2::new(x, road_z(x));
    c + Vec2::from_yaw(road_yaw(x)).right_of() * off
}

/// A jump ramp on the dead-straight part of the road (centred on z = 0).
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pub x0: f32,
    pub len: f32,
    pub half_w: f32,
    pub h: f32,
}

impl Ramp {
    /// Height of the ramp surface above the ground at a point (0 off the ramp).
    pub fn height_at(&self, p: Vec2) -> f32 {
        let lx = p.x - self.x0;
        if lx < 0.0 || lx > self.len || p.y.abs() > self.half_w {
            0.0
        } else {
            self.h * lx / self.len
        }
    }
}

const RAMP_STARTS: [f32; 5] = [50.0, 340.0, 640.0, 940.0, 1240.0];

/// Is `x` inside a ramp's no-obstacles zone?
pub fn in_clear_zone(x: f32) -> bool {
    RAMP_STARTS.iter().any(|&x0| x > x0 - 14.0 && x < x0 + 16.0 + 64.0)
}

pub const NEON: [Color; 6] = [
    Color::rgb(1.0, 0.15, 0.7),  // hot pink
    Color::rgb(0.1, 0.9, 1.0),   // cyan
    Color::rgb(0.65, 0.3, 1.0),  // purple
    Color::rgb(1.0, 0.9, 0.2),   // yellow
    Color::rgb(0.3, 1.0, 0.5),   // mint
    Color::rgb(1.0, 0.5, 0.15),  // orange
];

pub fn neon(i: usize) -> Color {
    NEON[i % NEON.len()]
}

/// A casino/hotel block. All look-variation is rolled once at generation time
/// so rendering stays deterministic frame to frame.
pub struct Building {
    pub pos: Vec2,    // ground-plane centre
    pub front: Vec2,  // unit vector the decorated facade faces
    /// Facade width (along the street), height, depth (away from the street).
    pub size: [f32; 3],
    pub body: Color,
    pub neon_a: usize,
    pub neon_b: usize,
    pub style: u32, // random bits: dome / pylon / which window rows are lit...
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Streetlight,
    Palm,
    Barrier,
    TrafficBox,
    Van,
}

pub struct Prop {
    pub kind: PropKind,
    pub pos: Vec2,
    pub yaw: f32,
    pub color: Color,
}

pub struct World {
    /// Everything solid that never moves (walls, barriers, vans...).
    pub statics: Vec<Obb>,
    /// The lot's perimeter walls, kept separately so the renderer can draw them.
    pub lot_walls: Vec<Obb>,
    pub buildings: Vec<Building>,
    pub props: Vec<Prop>,
    pub ramps: Vec<Ramp>,
}

impl World {
    /// Ground height under a point: 0 on the flat, up to a ramp's height on one.
    pub fn ground_height(&self, p: Vec2) -> f32 {
        self.ramps.iter().fold(0.0, |h, r| h.max(r.height_at(p)))
    }

    pub fn generate() -> World {
        let mut w = World { statics: Vec::new(), lot_walls: Vec::new(), buildings: Vec::new(), props: Vec::new(), ramps: Vec::new() };
        w.build_walls();
        w.build_ramps();
        w.build_buildings();
        w.build_street_furniture();
        w.build_obstacles();
        w
    }

    /// Invisible-ish collision walls enclosing the lot and the Strip.
    fn build_walls(&mut self) {
        let hz = LOT_HALF_Z;
        let end_x = FINISH_X + 40.0;
        let mid = (LOT_MIN_X + end_x) * 0.5;
        let s = &mut self.statics;
        // Lot: west, north, south.
        s.push(Obb::aabb(LOT_MIN_X - 1.0, 0.0, 1.0, hz + 2.0, 3.0));
        s.push(Obb::aabb((LOT_MIN_X + STRIP_START) * 0.5, -hz - 1.0, (STRIP_START - LOT_MIN_X) * 0.5 + 2.0, 1.0, 3.0));
        s.push(Obb::aabb((LOT_MIN_X + STRIP_START) * 0.5, hz + 1.0, (STRIP_START - LOT_MIN_X) * 0.5 + 2.0, 1.0, 3.0));
        // Lot east wall, leaving the road-wide exit gap (|z| < ROAD_HALF).
        let seg = (hz - ROAD_HALF) * 0.5;
        s.push(Obb::aabb(STRIP_START + 0.5, -(ROAD_HALF + seg), 0.5, seg, 3.0));
        s.push(Obb::aabb(STRIP_START + 0.5, ROAD_HALF + seg, 0.5, seg, 3.0));
        self.lot_walls = self.statics.clone();
        let s = &mut self.statics;
        // The Strip's side walls (at the back of the sidewalks) follow the road
        // in 16 m pieces; the end wall closes the finish.
        let mut x = STRIP_START + 1.0;
        while x < end_x {
            let xm = x + 8.0;
            let yaw = road_yaw(xm);
            let n = Vec2::from_yaw(yaw).right_of();
            for side in [-1.0f32, 1.0] {
                let c = road_point(xm, 0.0) + n * (side * (SIDE_WALL_Z + 0.5));
                s.push(Obb::new(c, Vec2::new(8.8, 0.5), yaw, 40.0));
            }
            x += 16.0;
        }
        s.push(Obb::aabb(end_x + 1.0, 0.0, 1.0, SIDE_WALL_Z + 1.0, 40.0));
        let _ = mid;
    }

    fn build_buildings(&mut self) {
        // Casinos down both sides of the Strip, packed shoulder to shoulder and
        // turned to follow the bends in the road.
        for side in [-1.0f32, 1.0] {
            let mut x = STRIP_START + 4.0;
            while x < FINISH_X + 40.0 {
                let width = rand_range(15.0, 27.0);
                let depth = rand_range(18.0, 30.0);
                let height = rand_range(16.0, 52.0);
                let xc = x + width * 0.5;
                let yaw = road_yaw(xc);
                let n = Vec2::from_yaw(yaw).right_of();
                // A little extra clearance so a building on the inside of a bend
                // never pokes onto the sidewalk.
                let pos = road_point(xc, 0.0) + n * (side * (SIDE_WALL_Z + 2.5 + depth * 0.5));
                self.add_building(pos, n * (-side), width, depth, height);
                x += width * yaw.cos().max(0.6) + rand_range(0.0, 1.5);
            }
        }
        // Dull backdrop blocks around the parking lot so it isn't an empty void.
        for side in [-1.0f32, 1.0] {
            let mut x = LOT_MIN_X;
            while x < STRIP_START {
                let width = rand_range(14.0, 22.0);
                let depth = rand_range(16.0, 26.0);
                let height = rand_range(10.0, 26.0);
                self.add_building(Vec2::new(x + width * 0.5, side * (LOT_HALF_Z + 2.0 + depth * 0.5)), Vec2::new(0.0, -side), width, depth, height);
                x += width + 1.0;
            }
        }
        for i in 0..3 {
            let depth = 20.0;
            self.add_building(Vec2::new(LOT_MIN_X - 3.0 - depth * 0.5, -22.0 + i as f32 * 22.0), Vec2::new(1.0, 0.0), 22.0, depth, rand_range(12.0, 30.0));
        }
    }

    fn add_building(&mut self, pos: Vec2, front: Vec2, width: f32, depth: f32, h: f32) {
        let hue = rand_range(0.0, 1.0);
        // Dark, slightly tinted bodies so the neon trim pops.
        let body = Color::rgb(0.10 + 0.10 * hue, 0.06 + 0.06 * (1.0 - hue), 0.20 + 0.10 * hue);
        let bits = (rand_range(0.0, 4_000_000.0)) as u32;
        self.buildings.push(Building {
            pos,
            front,
            size: [width, h, depth],
            body,
            neon_a: (rand_range(0.0, 6.0) as usize) % 6,
            neon_b: (rand_range(0.0, 6.0) as usize) % 6,
            style: bits,
        });
    }

    /// Jump ramps on the straight stretches, each with solid side flanks so
    /// nothing drives into the wedge sideways.
    fn build_ramps(&mut self) {
        for (i, &x0) in RAMP_STARTS.iter().enumerate() {
            let r = Ramp { x0, len: 16.0, half_w: 6.0, h: 3.8 + 0.4 * (i % 3) as f32 };
            for side in [-1.0f32, 1.0] {
                self.statics.push(Obb::aabb(x0 + r.len * 0.5, side * (r.half_w + 0.35), r.len * 0.5, 0.35, 0.5));
            }
            self.ramps.push(r);
        }
    }

    fn build_street_furniture(&mut self) {
        let mut x = STRIP_START + 12.0;
        while x < FINISH_X + 20.0 {
            for side in [-1.0f32, 1.0] {
                self.props.push(Prop {
                    kind: PropKind::Streetlight,
                    pos: road_point(x, side * (ROAD_HALF + 1.3)),
                    yaw: road_yaw(x) + if side > 0.0 { std::f32::consts::PI * 1.5 } else { std::f32::consts::PI * 0.5 },
                    color: Color::rgb(1.0, 0.8, 0.95),
                });
                self.props.push(Prop {
                    kind: PropKind::Palm,
                    pos: road_point(x + 13.0, side * (ROAD_HALF + 3.6)),
                    yaw: rand_range(0.0, 6.28),
                    color: Color::rgb(0.1, 0.65, 0.3),
                });
            }
            x += 26.0;
        }
    }

    /// Barriers, parked boxy vans and traffic cabinets to swerve around.
    /// One or two per 26 m slot, never blocking the whole 26 m road.
    fn build_obstacles(&mut self) {
        let mut x = STRIP_START + 55.0;
        while x < FINISH_X - 40.0 {
            if in_clear_zone(x) {
                x += 26.0; // ramp run-up and landing zone: nothing to crash into
                continue;
            }
            let count = if rand_chance(0.35) { 2 } else { 1 };
            let mut zs: Vec<f32> = Vec::new();
            for _ in 0..count {
                let mut z = rand_range(-10.5, 10.5);
                for _ in 0..6 {
                    if zs.iter().all(|o| (o - z).abs() > 9.0) {
                        break;
                    }
                    z = rand_range(-10.5, 10.5);
                }
                zs.push(z);
                let mut ox = x + rand_range(-4.0, 4.0);
                if in_clear_zone(ox) {
                    ox = x; // keep slot-edge jitter from leaking into a clear zone
                    if in_clear_zone(ox) {
                        continue;
                    }
                }
                let pos = road_point(ox, z);
                let ry = road_yaw(ox);
                let roll = rand_range(0.0, 1.0);
                if roll < 0.45 {
                    let yaw = rand_range(-0.35, 0.35) + ry;
                    self.props.push(Prop { kind: PropKind::Barrier, pos, yaw, color: Color::rgb(0.95, 0.2, 0.2) });
                    // Barrier model is long along local Z (across the road).
                    self.statics.push(Obb::new(pos, Vec2::new(1.7, 0.4), yaw + std::f32::consts::FRAC_PI_2, 0.9));
                } else if roll < 0.75 {
                    self.props.push(Prop { kind: PropKind::TrafficBox, pos, yaw: ry, color: Color::rgb(0.45, 0.5, 0.55) });
                    self.statics.push(Obb::new(pos, Vec2::new(0.7, 0.7), ry, 1.6));
                } else {
                    let yaw = rand_range(-0.2, 0.2) + ry;
                    let col = neon((rand_range(0.0, 6.0)) as usize).lerp(Color::rgb(1.0, 1.0, 1.0), 0.3);
                    self.props.push(Prop { kind: PropKind::Van, pos, yaw, color: col });
                    self.statics.push(Obb::new(pos, Vec2::new(2.6, 1.15), yaw, 2.4));
                }
            }
            x += 26.0;
        }
    }
}
