//! The static level: a parking lot that opens onto a long neon boulevard.
//!
//! Layout (top-down, +X is "down the Strip", +Z is the player's right at yaw 0):
//!
//!   x: -45 ........ 20 | 20 ................................ FINISH_X
//!        PARKING LOT   |            THE STRIP (road z in -13..13,
//!                      |            sidewalks to +-19, casinos beyond)
//!
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
    pub size: [f32; 3], // x extent, height, z extent
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
}

impl World {
    pub fn generate() -> World {
        let mut w = World { statics: Vec::new(), lot_walls: Vec::new(), buildings: Vec::new(), props: Vec::new() };
        w.build_walls();
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
        // The Strip's side walls (at the back of the sidewalks) and end wall.
        let sx = (STRIP_START + 1.0 + end_x) * 0.5;
        let shx = (end_x - STRIP_START - 1.0) * 0.5;
        s.push(Obb::aabb(sx, -SIDE_WALL_Z - 0.5, shx, 0.5, 40.0));
        s.push(Obb::aabb(sx, SIDE_WALL_Z + 0.5, shx, 0.5, 40.0));
        s.push(Obb::aabb(end_x + 1.0, 0.0, 1.0, SIDE_WALL_Z + 1.0, 40.0));
        let _ = mid;
    }

    fn build_buildings(&mut self) {
        // Casinos down both sides of the Strip, packed shoulder to shoulder.
        for side in [-1.0f32, 1.0] {
            let mut x = STRIP_START + 4.0;
            while x < FINISH_X + 40.0 {
                let width = rand_range(15.0, 27.0);
                let depth = rand_range(18.0, 30.0);
                let height = rand_range(16.0, 52.0);
                self.add_building(Vec2::new(x + width * 0.5, side * (SIDE_WALL_Z + 1.0 + depth * 0.5)), Vec2::new(0.0, -side), width, depth, height);
                x += width + rand_range(0.0, 1.5);
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
            self.add_building(Vec2::new(LOT_MIN_X - 3.0 - depth * 0.5, -22.0 + i as f32 * 22.0), Vec2::new(1.0, 0.0), depth, 22.0, rand_range(12.0, 30.0));
        }
    }

    fn add_building(&mut self, pos: Vec2, front: Vec2, sx: f32, sz: f32, h: f32) {
        let hue = rand_range(0.0, 1.0);
        // Dark, slightly tinted bodies so the neon trim pops.
        let body = Color::rgb(0.10 + 0.10 * hue, 0.06 + 0.06 * (1.0 - hue), 0.20 + 0.10 * hue);
        let bits = (rand_range(0.0, 4_000_000.0)) as u32;
        self.buildings.push(Building {
            pos,
            front,
            size: [sx, h, sz],
            body,
            neon_a: (rand_range(0.0, 6.0) as usize) % 6,
            neon_b: (rand_range(0.0, 6.0) as usize) % 6,
            style: bits,
        });
    }

    fn build_street_furniture(&mut self) {
        let mut x = STRIP_START + 12.0;
        while x < FINISH_X + 20.0 {
            for side in [-1.0f32, 1.0] {
                self.props.push(Prop {
                    kind: PropKind::Streetlight,
                    pos: Vec2::new(x, side * (ROAD_HALF + 1.3)),
                    yaw: if side > 0.0 { std::f32::consts::PI * 1.5 } else { std::f32::consts::PI * 0.5 },
                    color: Color::rgb(1.0, 0.8, 0.95),
                });
                self.props.push(Prop {
                    kind: PropKind::Palm,
                    pos: Vec2::new(x + 13.0, side * (ROAD_HALF + 3.6)),
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
                let ox = x + rand_range(-4.0, 4.0);
                let pos = Vec2::new(ox, z);
                let roll = rand_range(0.0, 1.0);
                if roll < 0.45 {
                    let yaw = rand_range(-0.35, 0.35);
                    self.props.push(Prop { kind: PropKind::Barrier, pos, yaw, color: Color::rgb(0.95, 0.2, 0.2) });
                    // Barrier model is long along local Z (across the road).
                    self.statics.push(Obb::new(pos, Vec2::new(1.7, 0.4), yaw + std::f32::consts::FRAC_PI_2, 0.9));
                } else if roll < 0.75 {
                    self.props.push(Prop { kind: PropKind::TrafficBox, pos, yaw: 0.0, color: Color::rgb(0.45, 0.5, 0.55) });
                    self.statics.push(Obb::new(pos, Vec2::new(0.7, 0.7), 0.0, 1.6));
                } else {
                    let yaw = rand_range(-0.2, 0.2);
                    let col = neon((rand_range(0.0, 6.0)) as usize).lerp(Color::rgb(1.0, 1.0, 1.0), 0.3);
                    self.props.push(Prop { kind: PropKind::Van, pos, yaw, color: col });
                    self.statics.push(Obb::new(pos, Vec2::new(2.6, 1.15), yaw, 2.4));
                }
            }
            x += 26.0;
        }
    }
}
