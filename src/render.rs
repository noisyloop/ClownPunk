//! Turns game state into pictures. Reads `Game` (never mutates it) and draws
//! through the `platform` API only - no macroquad here.
//!
//! Frame layout:
//!   1. 3D pass  : sky, ground, buildings, props, cars, zombies, effects,
//!                 then the first-person "viewmodel" (hands/board/dashboard)
//!   2. 2D pass  : HUD, floating labels, and the menu/overlay screens
use crate::allies::Ally;
use crate::car::{self, Car};
use crate::math::{Color, Mat3, Vec2, Vec3};
use crate::platform::{self as p, Mat};
use crate::player::Player;
use crate::ufo::{self, Ufo};
use crate::state::{CamView, Game, Mode, LASER_RANGE, LASER_TTL};
use crate::world::{self, neon, Building, Prop, PropKind, World};
use crate::zombies::{ZState, Zombie};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const SKY: Color = Color::rgb(0.03, 0.01, 0.10);
const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);

// Clown palette.
const RED: Color = Color::rgb(0.95, 0.12, 0.15);
const YELLOW: Color = Color::rgb(1.0, 0.85, 0.1);
const BLUE: Color = Color::rgb(0.15, 0.35, 0.95);
const WHITE: Color = Color::rgb(0.96, 0.96, 0.98);
const LIME: Color = Color::rgb(0.55, 0.92, 0.18);

pub fn draw(game: &Game) {
    let cam = game.camera();
    p::begin_3d(SKY, cam.pos, cam.yaw, cam.pitch, cam.roll, cam.fov);
    let t = game.anim_time;

    draw_sky(cam.pos);
    draw_ground(cam.pos.x);
    draw_ramps(&game.world, cam.pos.x, t);
    draw_lot_walls(&game.world);
    draw_buildings(&game.world, cam.pos, t);
    draw_props(&game.world, cam.pos, t);
    draw_finish_arch(cam.pos.x, t);
    for (i, c) in game.cars.iter().enumerate() {
        if (c.pos.x - cam.pos.x).abs() < 340.0 {
            draw_car(c, game.drive == Some(i), t);
        }
    }
    for u in &game.saucers {
        if (u.pos.x - cam.pos.x).abs() < 400.0 {
            draw_ufo(u, t, false);
        }
    }
    if !game.flying {
        draw_ufo(&game.ufo, t, true);
    }
    for b in &game.booms {
        draw_boom(b, t);
    }
    for z in &game.zombies {
        if (z.pos.x - cam.pos.x).abs() < 300.0 {
            draw_zombie(z);
        }
    }
    for a in &game.allies {
        if (a.pos.x - cam.pos.x).abs() < 300.0 {
            draw_ally(a, t);
        }
    }
    for tr in &game.tracers {
        let k = (tr.ttl / 0.12).clamp(0.0, 1.0);
        let (beam_c, spark_c) = if tr.ally {
            (Color::rgb(0.5, 1.0, 1.0), Color::rgb(0.3, 0.8, 1.0))
        } else {
            (Color::rgb(1.0, 0.95, 0.5), Color::rgb(1.0, 0.6, 0.2))
        };
        beam(tr.from, tr.to, 0.035 * (0.4 + k), beam_c);
        p::draw_aabb(tr.to, Vec3::new(0.25, 0.25, 0.25), spark_c, Mat::Glow);
    }
    draw_laser_burst(game);
    if game.mode != Mode::Title {
        if game.flying {
            draw_viewmodel_ufo(game, &cam);
        } else {
            match game.drive {
                None => draw_viewmodel_foot(game, &cam),
                Some(ci) => draw_viewmodel_car(game, &game.cars[ci], &cam),
            }
        }
    }

    p::begin_ui();
    draw_labels(game);
    match game.mode {
        Mode::Title => draw_title(game),
        Mode::Playing => draw_hud(game),
        Mode::Paused => {
            draw_hud(game);
            draw_pause();
        }
        Mode::GameOver => {
            draw_hud(game);
            draw_game_over(game);
        }
        Mode::Won => draw_win(game),
    }
}

// =============================================================== helpers ===

/// Thin glowing box between two points (bullet tracer).
fn beam(a: Vec3, b: Vec3, thick: f32, color: Color) {
    let d = b - a;
    let len = d.len();
    if len < 1e-3 {
        return;
    }
    let dir = d * (1.0 / len);
    let right = dir.cross(Y).norm();
    let up = right.cross(dir);
    let m = Mat3 { x: dir, y: up, z: right };
    p::draw_box((a + b) * 0.5, Vec3::new(len, thick, thick), &m, color, Mat::Glow);
}

fn pulse(t: f32, speed: f32, phase: f32) -> f32 {
    0.5 + 0.5 * (t * speed + phase).sin()
}

// ================================================================== sky ===

fn hash01(n: f32) -> f32 {
    let v = (n * 12.9898).sin() * 43758.5453;
    v - v.floor()
}

/// Stars and a moon, drawn at a fixed offset from the camera so they never
/// get closer ("skybox without textures").
fn draw_sky(cam: Vec3) {
    for i in 0..110 {
        let f = i as f32;
        let a = hash01(f + 0.1) * TAU;
        let w = 0.06 + hash01(f * 1.7 + 3.0) * 0.9;
        let r = (1.0 - w * w).sqrt();
        let dir = Vec3::new(a.cos() * r, w, a.sin() * r);
        let size = 1.2 + hash01(f * 3.3) * 2.2;
        let c = match i % 4 {
            0 => Color::rgb(1.0, 0.8, 0.95),
            1 => Color::rgb(0.7, 0.95, 1.0),
            _ => WHITE,
        };
        p::draw_aabb(cam + dir * 520.0, Vec3::new(size, size, size), c, Mat::Sky);
    }
    let moon_dir = Vec3::new(0.55, 0.42, -0.72).norm();
    p::draw_ball(cam + moon_dir * 560.0, 26.0, Color::rgb(1.0, 0.95, 0.85));
    p::draw_ball(cam + moon_dir * 559.0 + Vec3::new(8.0, 4.0, 2.0), 22.0, Color::rgb(1.0, 0.98, 0.92));
}

// ============================================================== ground ===

fn draw_ground(cam_x: f32) {
    // --- Parking lot: 4x4 asphalt tiles (tiles keep vertex-fog accurate).
    let (lx0, lx1) = (world::LOT_MIN_X, world::STRIP_START);
    let (tw, td) = ((lx1 - lx0) / 4.0, 2.0 * world::LOT_HALF_Z / 4.0);
    for ix in 0..4 {
        for iz in 0..4 {
            let shade = if (ix + iz) % 2 == 0 { 0.15 } else { 0.175 };
            let c = Color::rgb(shade, shade, shade + 0.04);
            let cx = lx0 + tw * (ix as f32 + 0.5);
            let cz = -world::LOT_HALF_Z + td * (iz as f32 + 0.5);
            p::draw_aabb(Vec3::new(cx, -0.05, cz), Vec3::new(tw, 0.1, td), c, Mat::Lit);
        }
    }
    // Painted bay lines along the north and south edges + a dashed centre aisle.
    let line = Color::rgb(0.9, 0.85, 0.5);
    for k in 0..10 {
        let x = -41.0 + k as f32 * 5.5;
        for zc in [-25.5, 25.5] {
            p::draw_aabb(Vec3::new(x, 0.01, zc), Vec3::new(0.14, 0.02, 7.0), line, Mat::Lit);
        }
    }
    for k in 0..9 {
        p::draw_aabb(Vec3::new(-40.0 + k as f32 * 7.0, 0.01, 0.0), Vec3::new(3.0, 0.02, 0.2), Color::rgb(0.8, 0.8, 0.85), Mat::Lit);
    }

    // --- The Strip: 8 m tiles laid along the (curving) centre line. Each tile
    // is a rotated box, slightly longer than its pitch so bends leave no gaps.
    let tile = 8.0;
    let reach = 250.0;
    let end_x = world::FINISH_X + 40.0;
    let first = (((cam_x - reach).max(world::STRIP_START - 12.0)) / tile).floor() as i32;
    let last = (((cam_x + reach).min(end_x)) / tile).ceil() as i32;
    for k in first..last {
        let x0 = k as f32 * tile;
        let xm = x0 + tile * 0.5;
        if xm < world::STRIP_START - 12.0 {
            continue;
        }
        let yaw = world::road_yaw(xm);
        let m = Mat3::from_yaw(yaw);
        let c = world::road_point(xm, 0.0);
        let fwd = Vec2::from_yaw(yaw);
        let right = fwd.right_of();
        // Point `a` along the road and `b` to the right of the centre line.
        let at = |a: f32, y: f32, b: f32| {
            let q = c + fwd * a + right * b;
            Vec3::new(q.x, y, q.y)
        };
        let len = tile + 0.8;
        p::draw_box(at(0.0, -0.05, 0.0), Vec3::new(len, 0.1, 26.0), &m, Color::rgb(0.11, 0.11, 0.15), Mat::Lit);
        for s in [-1.0f32, 1.0] {
            p::draw_box(at(0.0, -0.03, s * 16.0), Vec3::new(len, 0.14, 6.0), &m, Color::rgb(0.30, 0.26, 0.38), Mat::Lit);
            let edge = if s > 0.0 { neon(1) } else { neon(0) };
            p::draw_box(at(0.0, 0.05, s * 13.0), Vec3::new(len, 0.05, 0.3), &m, edge, Mat::Glow);
            p::draw_box(at(0.0, 0.01, s * 0.3), Vec3::new(len, 0.02, 0.12), &m, YELLOW, Mat::Lit);
            p::draw_box(at(0.0, 0.01, s * 6.5), Vec3::new(4.0, 0.02, 0.2), &m, WHITE, Mat::Lit);
        }
    }
}

// ================================================================ ramps ===

/// Jump ramps: a glowing wedge with chevrons, two tall beacons so you can
/// see it coming, and a mint-green LANDING ZONE of striped bars past the end.
fn draw_ramps(w: &World, cam_x: f32, t: f32) {
    for (i, r) in w.ramps.iter().enumerate() {
        if (r.x0 - cam_x).abs() > 330.0 {
            continue;
        }
        let ang = (r.h / r.len).atan();
        let m = Mat3::rot_z(ang);
        let slab_len = (r.len * r.len + r.h * r.h).sqrt();
        let nrm = Vec3::new(-ang.sin(), ang.cos(), 0.0);
        let mid = Vec3::new(r.x0 + r.len * 0.5, r.h * 0.5, 0.0);
        let accent = neon(i + 3);
        // Surface slab + glowing rails.
        p::draw_box(mid - nrm * 0.2, Vec3::new(slab_len, 0.4, r.half_w * 2.0), &m, Color::rgb(0.22, 0.16, 0.34), Mat::Lit);
        for s in [-1.0f32, 1.0] {
            p::draw_box(mid + nrm * 0.1 + Vec3::new(0.0, 0.0, s * (r.half_w - 0.12)), Vec3::new(slab_len, 0.14, 0.26), &m, accent, Mat::Glow);
        }
        // Chevron bars marching up the slope.
        for j in 0..5 {
            let u = (j as f32 + 0.6) / 5.2;
            let glow = pulse(t, 6.0, -(j as f32) * 0.9);
            let pos = Vec3::new(r.x0 + r.len * u, r.h * u, 0.0) + nrm * 0.07;
            p::draw_box(pos, Vec3::new(0.7, 0.06, r.half_w * 1.5), &m, YELLOW.scale(0.35 + 0.65 * glow), Mat::Glow);
        }
        // Solid fill under the slab (stepped, kept just inside the slab's width).
        let n = 8;
        for j in 0..n {
            let top = r.h * j as f32 / n as f32;
            if top > 0.1 {
                let sx = r.x0 + r.len * (j as f32 + 0.5) / n as f32;
                p::draw_aabb(Vec3::new(sx, top * 0.5, 0.0), Vec3::new(r.len / n as f32, top, r.half_w * 2.0 - 0.15), Color::rgb(0.14, 0.1, 0.22), Mat::Lit);
            }
        }
        // Tall beacons at the entrance.
        for s in [-1.0f32, 1.0] {
            let k = 0.55 + 0.45 * pulse(t, 3.0, s);
            p::draw_aabb(Vec3::new(r.x0 - 2.0, 7.0, s * (r.half_w + 2.0)), Vec3::new(0.5, 14.0, 0.5), Color::rgb(1.0, 0.55, 0.1).scale(k), Mat::Glow);
        }
        // Landing zone: striped pad with edge lines.
        let pad0 = r.x0 + r.len + 10.0;
        for j in 0..9 {
            let g = pulse(t, 4.0, j as f32 * -0.7);
            p::draw_aabb(Vec3::new(pad0 + j as f32 * 5.5, 0.03, 0.0), Vec3::new(1.2, 0.03, 18.0), Color::rgb(0.2, 1.0, 0.5).scale(0.3 + 0.5 * g), Mat::Glow);
        }
        for s in [-1.0f32, 1.0] {
            p::draw_aabb(Vec3::new(pad0 + 22.0, 0.03, s * 9.0), Vec3::new(50.0, 0.03, 0.25), Color::rgb(0.2, 1.0, 0.5), Mat::Glow);
        }
    }
}

/// Low concrete perimeter around the parking lot with a glowing cap.
fn draw_lot_walls(w: &World) {
    for o in &w.lot_walls {
        let size = Vec3::new(o.half.x * 2.0, o.height, o.half.y * 2.0);
        let c = Vec3::from_xz(o.center, o.height * 0.5);
        p::draw_aabb(c, size, Color::rgb(0.34, 0.31, 0.42), Mat::Lit);
        p::draw_aabb(Vec3::new(c.x, o.height + 0.08, c.z), Vec3::new(size.x + 0.1, 0.16, size.z + 0.1), neon(0), Mat::Glow);
    }
}

// ============================================================ buildings ===

fn draw_buildings(w: &World, cam: Vec3, t: f32) {
    for (bi, b) in w.buildings.iter().enumerate() {
        let dx = (b.pos.x - cam.x).abs();
        if dx > 340.0 {
            continue;
        }
        draw_building(b, bi, dx < 150.0, t);
    }
}

fn draw_building(b: &Building, bi: usize, detailed: bool, t: f32) {
    let [sx, h, sz] = b.size;
    // Facade frame: local x runs along the facade, local z points out toward the road.
    let n = Vec3::new(b.front.x, 0.0, b.front.y);
    let u = Vec3::new(-b.front.y, 0.0, b.front.x);
    let (width, depth) = (sx, sz);
    let face = Vec3::new(b.pos.x, 0.0, b.pos.y) + n * (depth * 0.5);
    let m = Mat3 { x: u, y: Y, z: n };
    p::draw_box(Vec3::new(b.pos.x, h * 0.5, b.pos.y), Vec3::new(width, h, depth), &m, b.body, Mat::Lit);
    // Place a box at (along facade, height, out from facade).
    let put = |a: f32, y: f32, o: f32, size: Vec3, col: Color, mat: Mat| {
        p::draw_box(face + u * a + Y * y + n * o, size, &m, col, mat);
    };
    let na = neon(b.neon_a);
    let nb = neon(b.neon_b);

    // Neon corner columns and rooftop edge.
    for s in [-1.0f32, 1.0] {
        put(s * (width * 0.5 - 0.35), h * 0.5, 0.2, Vec3::new(0.5, h, 0.4), na, Mat::Glow);
    }
    put(0.0, h - 0.25, 0.25, Vec3::new(width, 0.5, 0.5), nb, Mat::Glow);

    // Big glowing sign: outer neon box, dark inner panel, glowing "letters".
    if width > 13.0 {
        let sy = (h * 0.62).max(7.0).min(h - 3.0);
        let flicker = if (b.style >> 5) & 1 == 1 { 0.75 + 0.25 * pulse(t, 7.0 + bi as f32, bi as f32) } else { 1.0 };
        put(0.0, sy, 0.45, Vec3::new(width * 0.72, 3.6, 0.7), na.scale(flicker), Mat::Glow);
        put(0.0, sy, 0.85, Vec3::new(width * 0.66, 2.6, 0.2), Color::rgb(0.04, 0.02, 0.09), Mat::Lit);
        let letters = 3 + (b.style as usize % 3);
        for i in 0..letters {
            let a = (i as f32 - (letters as f32 - 1.0) * 0.5) * width * 0.11;
            let lh = 1.3 + ((b.style >> (8 + i)) & 1) as f32 * 0.6;
            put(a, sy, 1.0, Vec3::new(width * 0.06, lh, 0.15), nb.scale(flicker), Mat::Glow);
        }
    }

    if !detailed {
        return;
    }
    // Lit window rows (about half of them are on).
    let mut row = 0;
    let mut y = 4.5;
    while y < h - 3.0 {
        if (b.style >> (row % 20)) & 1 == 1 {
            let c = if row % 3 == 0 { Color::rgb(1.0, 0.85, 0.5) } else { Color::rgb(0.55, 0.8, 1.0) };
            put(0.0, y, 0.06, Vec3::new(width * 0.82, 0.8, 0.12), c.scale(0.9), Mat::Glow);
        }
        row += 1;
        y += 3.6;
    }
    // Optional rooftop extras and a vertical blade sign.
    if (b.style >> 20) & 1 == 1 {
        put(-width * 0.5 + 1.0, h * 0.35 + 3.0, 1.6, Vec3::new(0.6, 7.0, 3.0), nb, Mat::Glow);
    }
    if (b.style >> 21) & 1 == 1 {
        p::draw_ball(Vec3::new(b.pos.x, h + width * 0.12, b.pos.y), width * 0.25, na.scale(0.55));
    }
    if (b.style >> 22) & 1 == 1 {
        p::draw_aabb(Vec3::new(b.pos.x, h + 6.0, b.pos.y), Vec3::new(0.8, 12.0, 0.8), nb, Mat::Glow);
    }
}

// ================================================================ props ===

fn draw_props(w: &World, cam: Vec3, t: f32) {
    for (i, pr) in w.props.iter().enumerate() {
        if (pr.pos.x - cam.x).abs() > 300.0 {
            continue;
        }
        match pr.kind {
            PropKind::Streetlight => draw_streetlight(pr),
            PropKind::Palm => draw_palm(pr),
            PropKind::Barrier => draw_barrier(pr, t, i),
            PropKind::TrafficBox => draw_traffic_box(pr, t, i),
            PropKind::Van => draw_van(pr),
        }
    }
}

fn draw_streetlight(pr: &Prop) {
    let m = Mat3::from_yaw(pr.yaw);
    let base = Vec3::from_xz(pr.pos, 0.0);
    let at = |l: Vec3| base + m.mul_vec(l);
    let dark = Color::rgb(0.2, 0.2, 0.28);
    p::draw_box(at(Vec3::new(0.0, 3.5, 0.0)), Vec3::new(0.25, 7.0, 0.25), &m, dark, Mat::Lit);
    p::draw_box(at(Vec3::new(1.2, 6.9, 0.0)), Vec3::new(2.4, 0.18, 0.18), &m, dark, Mat::Lit);
    p::draw_blob(at(Vec3::new(2.3, 6.78, 0.0)), Vec3::new(0.5, 0.17, 0.32), &m, pr.color, Mat::Glow);
    // Faint pool of light on the sidewalk below the lamp.
    p::draw_box(at(Vec3::new(2.3, 0.03, 0.0)), Vec3::new(5.0, 0.02, 5.0), &m, Color::rgb(0.22, 0.15, 0.28), Mat::Glow);
}

fn draw_palm(pr: &Prop) {
    let lean = Vec2::from_yaw(pr.yaw) * 0.18;
    let mut top = Vec3::from_xz(pr.pos, 0.0);
    for k in 0..5 {
        let kf = k as f32;
        let c = Vec3::new(pr.pos.x + lean.x * kf * kf * 0.25, 0.65 + kf * 1.25, pr.pos.y + lean.y * kf * kf * 0.25);
        let wdt = 0.55 - kf * 0.05;
        let shade = if k % 2 == 0 { 0.42 } else { 0.34 };
        p::draw_blob(c, Vec3::new(wdt * 0.55, 0.72, wdt * 0.55), &Mat3::IDENTITY, Color::rgb(shade, shade * 0.62, 0.17), Mat::Lit);
        top = c + Y * 0.65;
    }
    for j in 0..7 {
        let a = pr.yaw + j as f32 * TAU / 7.0;
        let m = Mat3::rot_y(-a).mul(&Mat3::rot_z(-0.5));
        let center = top + m.mul_vec(Vec3::new(1.65, 0.0, 0.0));
        let g = 0.55 + 0.15 * (j % 2) as f32;
        p::draw_blob(center, Vec3::new(1.7, 0.09, 0.5), &m, Color::rgb(0.08, g, 0.3), Mat::Lit);
    }
}

fn draw_barrier(pr: &Prop, t: f32, i: usize) {
    let m = Mat3::from_yaw(pr.yaw);
    let base = Vec3::from_xz(pr.pos, 0.0);
    for k in 0..5 {
        let col = if k % 2 == 0 { RED } else { WHITE };
        p::draw_box(base + m.mul_vec(Vec3::new(0.0, 0.45, (k as f32 - 2.0) * 0.68)), Vec3::new(0.8, 0.9, 0.67), &m, col, Mat::Lit);
    }
    let on = pulse(t, 6.0, i as f32) > 0.5;
    let c = if on { Color::rgb(1.0, 0.6, 0.1) } else { Color::rgb(0.4, 0.2, 0.05) };
    p::draw_box(base + m.mul_vec(Vec3::new(0.0, 1.05, 0.0)), Vec3::new(0.3, 0.2, 0.3), &m, c, Mat::Glow);
}

fn draw_traffic_box(pr: &Prop, t: f32, i: usize) {
    let m = Mat3::from_yaw(pr.yaw);
    let base = Vec3::from_xz(pr.pos, 0.0);
    let put = |l: Vec3, s: Vec3, c: Color, mat: Mat| p::draw_box(base + m.mul_vec(l), s, &m, c, mat);
    put(Vec3::new(0.0, 0.8, 0.0), Vec3::new(1.4, 1.6, 1.4), pr.color, Mat::Lit);
    put(Vec3::new(0.72, 0.9, 0.0), Vec3::new(0.05, 1.1, 1.0), Color::rgb(0.25, 0.28, 0.33), Mat::Lit);
    put(Vec3::new(0.75, 1.1, 0.0), Vec3::new(0.04, 0.4, 0.6), neon(1), Mat::Glow);
    let c = if pulse(t, 2.0, i as f32 * 1.3) > 0.5 { Color::rgb(1.0, 0.15, 0.15) } else { Color::rgb(0.2, 1.0, 0.3) };
    put(Vec3::new(0.0, 1.72, 0.0), Vec3::new(0.5, 0.25, 0.5), c, Mat::Glow);
}

fn draw_van(pr: &Prop) {
    let m = Mat3::from_yaw(pr.yaw);
    let base = Vec3::from_xz(pr.pos, 0.0);
    let put = |l: Vec3, s: Vec3, c: Color, mat: Mat| p::draw_box(base + m.mul_vec(l), s, &m, c, mat);
    put(Vec3::new(0.0, 0.5, 0.0), Vec3::new(5.2, 0.5, 2.2), Color::rgb(0.12, 0.12, 0.15), Mat::Lit);
    put(Vec3::new(-0.9, 1.65, 0.0), Vec3::new(3.4, 1.9, 2.3), Color::rgb(0.85, 0.85, 0.92), Mat::Lit);
    put(Vec3::new(-0.9, 1.45, 0.0), Vec3::new(3.42, 0.28, 2.34), pr.color, Mat::Glow);
    put(Vec3::new(1.75, 1.25, 0.0), Vec3::new(1.7, 1.5, 2.3), pr.color.scale(0.8), Mat::Lit);
    put(Vec3::new(2.58, 1.55, 0.0), Vec3::new(0.12, 0.7, 1.9), Color::rgb(0.1, 0.2, 0.32), Mat::Lit);
    for (x, z) in [(1.7, 1.15), (1.7, -1.15), (-1.7, 1.15), (-1.7, -1.15)] {
        put(Vec3::new(x, 0.45, z), Vec3::new(0.9, 0.9, 0.3), Color::rgb(0.07, 0.07, 0.09), Mat::Lit);
    }
    for z in [-0.8, 0.8] {
        put(Vec3::new(-2.62, 1.0, z), Vec3::new(0.1, 0.3, 0.3), Color::rgb(1.0, 0.1, 0.1), Mat::Glow);
    }
}

fn draw_finish_arch(cam_x: f32, t: f32) {
    let fx = world::FINISH_X;
    if (fx - cam_x).abs() > 380.0 {
        return;
    }
    for s in [-1.0f32, 1.0] {
        p::draw_aabb(Vec3::new(fx, 8.0, s * 14.5), Vec3::new(2.2, 16.0, 2.2), Color::rgb(0.9, 0.9, 0.95), Mat::Lit);
        p::draw_aabb(Vec3::new(fx - 1.2, 8.0, s * 14.5), Vec3::new(0.4, 16.0, 0.8), neon(if s > 0.0 { 0 } else { 1 }), Mat::Glow);
        let beam_c = neon(if s > 0.0 { 1 } else { 0 }).scale(0.55 + 0.35 * pulse(t, 3.0, s));
        p::draw_aabb(Vec3::new(fx, 38.0, s * 14.5), Vec3::new(0.7, 44.0, 0.7), beam_c, Mat::Glow);
    }
    for k in 0..14 {
        let z = -14.7 + 1.05 + k as f32 * 2.1;
        let c = if k % 2 == 0 { WHITE } else { neon(0) };
        p::draw_aabb(Vec3::new(fx, 14.5, z), Vec3::new(2.4, 3.0, 2.1), c, Mat::Glow);
    }
    p::draw_aabb(Vec3::new(fx, 17.2, 0.0), Vec3::new(1.6, 2.0, 30.0), neon(1).scale(0.6 + 0.4 * pulse(t, 4.0, 0.0)), Mat::Glow);
    // Checkered finish line on the road.
    for k in 0..13 {
        for r in 0..2 {
            if (k + r) % 2 == 0 {
                p::draw_aabb(Vec3::new(fx - 1.0 + r as f32 * 2.0, 0.05, -12.0 + k as f32 * 2.0), Vec3::new(2.0, 0.04, 2.0), WHITE, Mat::Glow);
            }
        }
    }
}

// ================================================================= cars ===

/// A car's origin and orientation: heading, tipped nose-up by its pitch, lifted by its height.
fn car_frame(c: &Car) -> (Vec3, Mat3) {
    (Vec3::from_xz(c.pos, c.y), Mat3::from_yaw(c.yaw).mul(&Mat3::rot_z(c.pitch)))
}

fn draw_car(c: &Car, driven: bool, t: f32) {
    let (base, m) = car_frame(c);
    let put = |l: Vec3, s: Vec3, col: Color, mat: Mat| p::draw_box(base + m.mul_vec(l), s, &m, col, mat);
    let body = c.color;
    let dark = Color::rgb(0.07, 0.07, 0.09);

    put(Vec3::new(0.45, 0.65, 0.0), Vec3::new(3.5, 0.75, 2.0), body, Mat::Lit);
    // Rear trunk bin: floor + side walls so the inside is visible when the lid opens.
    put(Vec3::new(-1.75, 0.5, 0.0), Vec3::new(0.9, 0.35, 2.0), body.scale(0.7), Mat::Lit);
    for z in [-0.95, 0.95] {
        put(Vec3::new(-1.75, 0.85, z), Vec3::new(0.9, 0.4, 0.12), body, Mat::Lit);
    }
    // Closed trunk lid.
    put(Vec3::new(-1.75, 1.03, 0.0), Vec3::new(0.9, 0.07, 2.0), body, Mat::Lit);

    if !driven {
        // Cabin: glass block + roof. Skipped while driving so we can see out.
        let blob = |l: Vec3, r: Vec3, col: Color| p::draw_blob(base + m.mul_vec(l), r, &m, col, Mat::Lit);
        blob(Vec3::new(-0.2, 1.36, 0.0), Vec3::new(1.3, 0.42, 0.93), Color::rgb(0.12, 0.2, 0.32));
        blob(Vec3::new(-0.2, 1.66, 0.0), Vec3::new(1.15, 0.16, 0.86), body);
    }
    // Door handle on the driver side (left, -Z) so you can see where to stand.
    put(Vec3::new(0.4, 0.95, -1.03), Vec3::new(0.3, 0.06, 0.05), YELLOW, Mat::Glow);

    // Bumpers, lights, neon underglow.
    put(Vec3::new(2.22, 0.45, 0.0), Vec3::new(0.2, 0.3, 2.05), dark, Mat::Lit);
    put(Vec3::new(-2.22, 0.45, 0.0), Vec3::new(0.2, 0.3, 2.05), dark, Mat::Lit);
    for z in [-0.7, 0.7] {
        put(Vec3::new(2.22, 0.78, z), Vec3::new(0.1, 0.22, 0.4), Color::rgb(1.0, 0.95, 0.7), Mat::Glow);
        put(Vec3::new(-2.22, 0.78, z), Vec3::new(0.1, 0.2, 0.4), Color::rgb(1.0, 0.1, 0.1), Mat::Glow);
    }
    put(Vec3::new(0.0, 0.12, 0.0), Vec3::new(3.9, 0.04, 1.7), body.lerp(WHITE, 0.3), Mat::Glow);

    // Wheels (front pair steers).
    for (x, z, front) in [(1.4, 0.95, true), (1.4, -0.95, true), (-1.4, 0.95, false), (-1.4, -0.95, false)] {
        let wm = if front { m.mul(&Mat3::rot_y(-c.steer * 0.45)) } else { m };
        let spin = Mat3::rot_z(-c.wheel_spin / 0.35);
        let wm2 = wm.mul(&spin);
        let wc = base + m.mul_vec(Vec3::new(x, 0.35, z));
        p::draw_blob(wc, Vec3::new(0.38, 0.38, 0.17), &wm2, dark, Mat::Lit);
        p::draw_box(wc + wm.mul_vec(Vec3::new(0.0, 0.0, z.signum() * 0.16)), Vec3::new(0.3, 0.3, 0.06), &wm2, YELLOW, Mat::Lit);
    }

    // Turbo flames out of the back.
    if c.turbo_on {
        for z in [-0.6f32, 0.6] {
            let fl = 1.0 + 0.7 * pulse(t, 45.0, z * 3.0);
            put(Vec3::new(-2.35 - fl * 0.5, 0.5, z), Vec3::new(fl, 0.26, 0.26), Color::rgb(1.0, 0.45, 0.1), Mat::Glow);
            put(Vec3::new(-2.3 - fl * 0.3, 0.5, z), Vec3::new(fl * 0.6, 0.12, 0.12), Color::rgb(1.0, 0.95, 0.6), Mat::Glow);
        }
    }
    // In the air: a bright outline on the ground shows where the car will come down.
    if c.airborne {
        let gm = Mat3::from_yaw(c.yaw);
        let g = Vec3::from_xz(c.pos, 0.05);
        let glow = Color::rgb(0.3, 1.0, 0.5);
        for (dx, dz, sx, sz) in [(0.0, -1.2, 4.6, 0.12), (0.0, 1.2, 4.6, 0.12), (-2.4, 0.0, 0.12, 2.4), (2.4, 0.0, 0.12, 2.4)] {
            p::draw_box(g + gm.mul_vec(Vec3::new(dx, 0.0, dz)), Vec3::new(sx, 0.05, sz), &gm, glow, Mat::Glow);
        }
    }
    // Beacon so you can spot a car across the lot.
    if !driven {
        let k = 0.55 + 0.45 * pulse(t, 4.0, 0.0);
        p::draw_aabb(base + Vec3::new(0.0, 7.0, 0.0), Vec3::new(0.5, 10.0, 0.5), Color::rgb(0.3, 1.0, 0.4).scale(k), Mat::Glow);
    }
}

// ================================================================ UFOs ===

/// A flying saucer: disc hull, glass dome, spinning rim lights, underglow.
/// `parked` adds a landing-pad glow and a beacon so you can spot it in the lot.
fn draw_ufo(u: &Ufo, t: f32, parked: bool) {
    let m = Mat3::rot_x(u.bank).mul(&Mat3::rot_z(u.tilt));
    let c = Vec3::from_xz(u.pos, u.y + if parked { (t * 1.6).sin() * 0.06 } else { (t * 1.3 + u.spin).sin() * 0.12 });
    let put = |l: Vec3, s: Vec3, col: Color, mat: Mat| p::draw_box(c + m.mul_vec(l), s, &m, col, mat);
    let blob = |l: Vec3, r: Vec3, col: Color, mat: Mat| p::draw_blob(c + m.mul_vec(l), r, &m, col, mat);
    let hull = u.color;
    let dark = Color::rgb(0.08, 0.08, 0.12);

    blob(Vec3::new(0.0, 0.0, 0.0), Vec3::new(ufo::RADIUS, 0.55, ufo::RADIUS), hull, Mat::Lit);
    blob(Vec3::new(0.0, -0.25, 0.0), Vec3::new(1.5, 0.35, 1.5), dark, Mat::Lit);
    blob(Vec3::new(0.0, 0.45, 0.0), Vec3::new(1.15, 0.8, 1.15), Color::rgb(0.45, 0.9, 1.0), Mat::Glow);
    blob(Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.8, 0.5, 0.8), Color::rgb(0.9, 1.0, 1.0), Mat::Glow);
    // Rim lights chase around the edge.
    for i in 0..10 {
        let a = i as f32 * TAU / 10.0 + u.spin;
        let k = 0.4 + 0.6 * pulse(t, 6.0, i as f32);
        let col = neon(i).scale(k);
        put(Vec3::new(a.cos() * 2.15, 0.0, a.sin() * 2.15), Vec3::new(0.22, 0.2, 0.22), col, Mat::Glow);
    }
    // Underglow disc.
    put(Vec3::new(0.0, -0.58, 0.0), Vec3::new(1.4, 0.05, 1.4), hull.lerp(WHITE, 0.4), Mat::Glow);
    // Landing legs when parked.
    if parked {
        for a in [0.8f32, 2.4, 4.0, 5.5] {
            put(Vec3::new(a.cos() * 1.6, -0.75, a.sin() * 1.6), Vec3::new(0.14, 0.7, 0.14), dark, Mat::Lit);
            put(Vec3::new(a.cos() * 1.6, -1.1, a.sin() * 1.6), Vec3::new(0.45, 0.07, 0.45), dark, Mat::Lit);
        }
        let k = 0.55 + 0.45 * pulse(t, 4.0, 1.0);
        p::draw_aabb(Vec3::from_xz(u.pos, 7.0), Vec3::new(0.5, 12.0, 0.5), Color::rgb(0.9, 0.3, 1.0).scale(k), Mat::Glow);
        p::draw_aabb(Vec3::from_xz(u.pos, 0.04), Vec3::new(6.0, 0.05, 6.0), Color::rgb(0.9, 0.3, 1.0).scale(0.3 + 0.3 * k), Mat::Glow);
    }
    // Warning flash while the avoidance AI is steering a wild saucer away.
    if !parked && u.avoiding > 0.15 {
        p::draw_aabb(c + Vec3::new(0.0, 1.8, 0.0), Vec3::new(0.35, 0.35, 0.35), Color::rgb(1.0, 0.4, 0.1).scale(u.avoiding), Mat::Glow);
    }
}

fn draw_boom(b: &crate::state::Boom, _t: f32) {
    let k = 1.0 - (b.ttl / crate::state::BOOM_TTL).clamp(0.0, 1.0);
    p::draw_ball(b.pos, 0.8 + 3.5 * k, Color::rgb(1.0, 0.55, 0.15).scale(1.0 - 0.6 * k));
    p::draw_ball(b.pos, 0.4 + 2.0 * k, Color::rgb(1.0, 0.95, 0.6));
}

/// Inside the UFO: a canopy rim, twin cannons that recoil, a targeting
/// frame and a boost bar. Drawn in the camera frame like the other viewmodels.
fn draw_viewmodel_ufo(game: &Game, cam: &CamView) {
    let u = &game.ufo;
    let b = view_basis(cam);
    let place = |r: f32, up: f32, f: f32| cam.pos + b.z * r + b.y * up + b.x * f;
    let dark = Color::rgb(0.08, 0.08, 0.14);
    let kick = (u.shoot_cd / ufo::SHOT_COOLDOWN).clamp(0.0, 1.0);
    // Canopy rim: low dash and two side struts.
    p::draw_box(place(0.0, -0.62, 0.9), Vec3::new(0.5, 0.06, 2.0), &b, dark, Mat::Lit);
    for sd in [-1.0f32, 1.0] {
        p::draw_box(place(sd * 1.0, -0.1, 0.9), Vec3::new(0.12, 1.4, 0.1), &b, dark, Mat::Lit);
        // Cannon barrels at the bottom corners.
        let g = place(sd * 0.42, -0.34, 1.0 - kick * 0.1);
        p::draw_box(g, Vec3::new(0.7, 0.07, 0.07), &b, Color::rgb(0.55, 0.55, 0.65), Mat::Lit);
        p::draw_box(g + b.x * 0.36, Vec3::new(0.06, 0.1, 0.1), &b, hull_glow(u), Mat::Glow);
        if kick > 0.6 {
            p::draw_box(g + b.x * 0.5, Vec3::new(0.14, 0.14, 0.02), &b, Color::rgb(0.6, 1.0, 1.0), Mat::Glow);
        }
    }
    // Boost bar along the dash.
    let bar = 0.05 + 0.8 * u.turbo;
    let col = if u.turbo_on { Color::rgb(1.0, 0.6, 0.1) } else { neon(1) };
    p::draw_box(place(-0.45 + bar * 0.5, -0.57, 0.85), Vec3::new(0.03, 0.03, bar), &b, col, Mat::Glow);
}

fn hull_glow(u: &Ufo) -> Color {
    u.color.lerp(WHITE, 0.5)
}

// ============================================================== zombies ===

fn draw_zombie(z: &Zombie) {
    let tilt = z.tilt();
    let theta = tilt * FRAC_PI_2;
    // Body frame: face along yaw, then fall backwards about the feet.
    let m = Mat3::from_yaw(z.yaw).mul(&Mat3::rot_z(theta));
    let origin = Vec3::from_xz(z.pos, 0.18 * theta.sin());
    let walking = z.state == ZState::Walking;
    let bob = if walking { (z.phase * 2.0).sin().abs() * 0.07 } else { 0.0 };
    let swing = if walking { z.phase.sin() * 0.65 } else { 0.0 };

    let part = |c: Vec3, s: Vec3, col: Color| p::draw_blob(origin + m.mul_vec(c), s * 0.56, &m, col, Mat::Lit);
    // A limb hanging from `pivot`, rotated by `ang` about the body's side axis.
    let limb = |pivot: Vec3, len: f32, s: Vec3, ang: f32, col: Color| {
        let r = Mat3::rot_z(ang);
        let c = pivot + r.mul_vec(Vec3::new(0.0, -len * 0.5, 0.0));
        p::draw_blob(origin + m.mul_vec(c), s * 0.56, &m.mul(&r), col, Mat::Lit);
    };

    let skin = LIME;
    let pants = Color::rgb(0.28, 0.22, 0.45);
    // Legs swing opposite to each other.
    limb(Vec3::new(0.0, 0.72, 0.15), 0.72, Vec3::new(0.26, 0.72, 0.26), swing, pants);
    limb(Vec3::new(0.0, 0.72, -0.15), 0.72, Vec3::new(0.26, 0.72, 0.26), -swing, pants);
    // Torso (torn shirt) and head bob up and down.
    part(Vec3::new(0.0, 1.05 + bob, 0.0), Vec3::new(0.36, 0.7, 0.62), z.shirt);
    part(Vec3::new(0.18, 0.85 + bob, 0.0), Vec3::new(0.04, 0.16, 0.5), skin.scale(0.9)); // belly peeking out
    part(Vec3::new(0.04, 1.62 + bob, 0.0), Vec3::new(0.42, 0.42, 0.42), skin.lerp(WHITE, 0.12));
    part(Vec3::new(0.02, 1.86 + bob, 0.0), Vec3::new(0.3, 0.08, 0.3), Color::rgb(0.1, 0.4, 0.12));
    for s in [-1.0f32, 1.0] {
        part(Vec3::new(0.25, 1.68 + bob, s * 0.1), Vec3::new(0.04, 0.13, 0.13), WHITE);
        part(Vec3::new(0.27, 1.67 + bob, s * 0.08), Vec3::new(0.04, 0.07, 0.07), Color::rgb(0.8, 0.05, 0.05));
    }
    part(Vec3::new(0.25, 1.51 + bob, 0.0), Vec3::new(0.04, 0.07, 0.26), Color::rgb(0.2, 0.05, 0.12));
    // Arms stretched out in front, wobbling.
    for (k, s) in [-1.0f32, 1.0].iter().enumerate() {
        let wob = if walking { (z.phase * 2.0 + k as f32 * PI).sin() * 0.12 } else { 0.0 };
        limb(Vec3::new(0.05, 1.32 + bob, s * 0.38), 0.7, Vec3::new(0.2, 0.7, 0.2), FRAC_PI_2 - 0.1 + wob, skin.scale(0.88));
    }
}

// ================================================================ crew ===

/// A friendly clown: baggy suit, ruffle collar, white face, red nose, rainbow
/// hair and giant shoes. Waiting clowns wave and have a yellow "join me" beam.
fn draw_ally(a: &Ally, t: f32) {
    let m = Mat3::from_yaw(a.yaw);
    let origin = Vec3::from_xz(a.pos, a.y);
    let waiting = !a.recruited;
    let riding = a.seat.is_some();
    let moving = a.vel.len() > 0.5;
    let swing = if moving { a.phase.sin() * 0.65 } else { 0.0 };
    let bob = if moving { (a.phase * 2.0).sin().abs() * 0.06 } else { 0.0 };

    let part = |c: Vec3, s: Vec3, col: Color| p::draw_blob(origin + m.mul_vec(c), s * 0.56, &m, col, Mat::Lit);
    let limb = |pivot: Vec3, len: f32, s: Vec3, ang: f32, col: Color| {
        let r = Mat3::rot_z(ang);
        let c = pivot + r.mul_vec(Vec3::new(0.0, -len * 0.5, 0.0));
        p::draw_blob(origin + m.mul_vec(c), s * 0.56, &m.mul(&r), col, Mat::Lit);
    };
    let pants = a.suit.scale(0.75);
    limb(Vec3::new(0.0, 0.72, 0.17), 0.72, Vec3::new(0.34, 0.72, 0.34), swing, pants);
    limb(Vec3::new(0.0, 0.72, -0.17), 0.72, Vec3::new(0.34, 0.72, 0.34), -swing, pants);
    // Giant shoes.
    for z in [-0.17f32, 0.17] {
        part(Vec3::new(0.2, 0.08, z), Vec3::new(0.62, 0.2, 0.3), RED);
        part(Vec3::new(0.46, 0.1, z), Vec3::new(0.1, 0.12, 0.12), YELLOW);
    }
    // Baggy polka-dot torso with a ruffled collar.
    part(Vec3::new(0.0, 1.05 + bob, 0.0), Vec3::new(0.5, 0.74, 0.74), a.suit);
    for (dy, dz) in [(1.2f32, -0.22f32), (1.0, 0.12), (0.88, -0.1), (1.25, 0.25)] {
        part(Vec3::new(0.25, dy + bob, dz), Vec3::new(0.06, 0.14, 0.14), WHITE);
    }
    part(Vec3::new(0.0, 1.42 + bob, 0.0), Vec3::new(0.42, 0.14, 0.56), WHITE);
    // Head: white face, red nose, eyes, big smile, rainbow hair.
    part(Vec3::new(0.03, 1.68 + bob, 0.0), Vec3::new(0.44, 0.44, 0.44), WHITE);
    part(Vec3::new(0.25, 1.65 + bob, 0.0), Vec3::new(0.16, 0.16, 0.16), RED);
    for s in [-1.0f32, 1.0] {
        part(Vec3::new(0.24, 1.76 + bob, s * 0.11), Vec3::new(0.04, 0.09, 0.09), Color::rgb(0.05, 0.05, 0.1));
        part(Vec3::new(0.0, 1.9 + bob, s * 0.27), Vec3::new(0.3, 0.3, 0.3), a.hair);
    }
    part(Vec3::new(0.22, 1.55 + bob, 0.0), Vec3::new(0.04, 0.06, 0.3), RED);
    part(Vec3::new(-0.02, 2.0 + bob, 0.0), Vec3::new(0.34, 0.26, 0.44), a.hair.lerp(WHITE, 0.25));
    // Arms: pistol arm points when shooting; riders and waiting clowns wave.
    let wave = (t * 7.0 + a.phase).sin() * 0.3;
    if a.aim_t > 0.0 {
        limb(Vec3::new(0.05, 1.32 + bob, 0.4), 0.7, Vec3::new(0.22, 0.7, 0.22), FRAC_PI_2 - 0.05, a.suit.lerp(WHITE, 0.2));
        part(Vec3::new(0.8, 1.3 + bob, 0.4), Vec3::new(0.3, 0.2, 0.12), neon(1));
    } else if waiting || riding {
        limb(Vec3::new(0.05, 1.32 + bob, 0.4), 0.7, Vec3::new(0.22, 0.7, 0.22), PI * 0.82 + wave, a.suit.lerp(WHITE, 0.2));
    } else {
        limb(Vec3::new(0.05, 1.32 + bob, 0.4), 0.7, Vec3::new(0.22, 0.7, 0.22), -swing, a.suit.lerp(WHITE, 0.2));
    }
    let left = if riding { PI * 0.8 - wave } else { swing };
    limb(Vec3::new(0.05, 1.32 + bob, -0.4), 0.7, Vec3::new(0.22, 0.7, 0.22), left, a.suit.lerp(WHITE, 0.2));
    if waiting {
        let k = 0.5 + 0.5 * pulse(t, 3.0, a.phase);
        p::draw_aabb(origin + Vec3::new(0.0, 6.0, 0.0), Vec3::new(0.35, 10.0, 0.35), YELLOW.scale(k), Mat::Glow);
    }
}

/// The clown-laser burst: a spinning ring of rainbow spokes and a thick beam
/// from the clown to every zombie that got zapped.
fn draw_laser_burst(g: &Game) {
    if g.laser_burst <= 0.0 {
        return;
    }
    let k = (g.laser_burst / LASER_TTL).clamp(0.0, 1.0); // 1 -> 0
    // Beams start a couple of metres out and a bit below the eyes, so they
    // never fill the screen from inside the camera.
    let o = g.laser_origin - Vec3::new(0.0, 0.35, 0.0);
    for i in 0..16 {
        let a = i as f32 * TAU / 16.0 + (1.0 - k) * 4.0;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let reach = LASER_RANGE * (1.0 - 0.35 * k);
        let end = Vec3::new(o.x, o.y.max(0.4), o.z) + dir * reach;
        beam(o + dir * 2.2, end, 0.04 + 0.08 * k, neon(i));
    }
    for l in &g.lasers {
        let from = l.from - Vec3::new(0.0, 0.35, 0.0);
        let start = from + (l.to - from).norm() * 2.2;
        beam(start, l.to, 0.05 + 0.12 * k, neon(1).lerp(WHITE, 0.4));
        beam(start, l.to, 0.02 + 0.04 * k, WHITE);
        p::draw_ball(l.to, 0.25 + 0.7 * k, neon(0));
    }
}

// =========================================================== viewmodels ===

/// Camera-aligned frame: x forward, y up, z right, honouring camera roll.
fn view_basis(cam: &CamView) -> Mat3 {
    let fwd = Player::look_dir(cam.yaw, cam.pitch);
    let right0 = fwd.cross(Y).norm();
    let up0 = right0.cross(fwd);
    let up = up0 * cam.roll.cos() + right0 * cam.roll.sin();
    let right = fwd.cross(up);
    Mat3 { x: fwd, y: up, z: right }
}

/// First person on the board: board + clown shoes at the bottom, left hand
/// balancing, right hand holding the wobbly toy pistol.
fn draw_viewmodel_foot(game: &Game, cam: &CamView) {
    let pl = &game.player;
    let b = view_basis(cam);
    let place = |r: f32, u: f32, f: f32| cam.pos + b.z * r + b.y * u + b.x * f;
    let skate_bob = (pl.bob * 2.0).sin() * 0.006 * (pl.speed() / 10.0).min(1.0);

    // Board: nose-up a little so we see its top. Blue deck, black grip tape,
    // red nose, yellow tail, yellow wheels.
    let bm = b.mul(&Mat3::rot_z(0.10));
    let bc = place(0.0, -0.66 + skate_bob, 1.25);
    let on_board = |l: Vec3| bc + bm.mul_vec(l);
    p::draw_box(bc, Vec3::new(1.0, 0.045, 0.32), &bm, BLUE, Mat::Lit);
    p::draw_box(on_board(Vec3::new(0.0, 0.004, 0.0)), Vec3::new(0.74, 0.05, 0.26), &bm, Color::rgb(0.12, 0.12, 0.16), Mat::Lit);
    p::draw_box(on_board(Vec3::new(0.44, 0.003, 0.0)), Vec3::new(0.12, 0.049, 0.32), &bm, RED, Mat::Lit);
    p::draw_box(on_board(Vec3::new(-0.44, 0.003, 0.0)), Vec3::new(0.12, 0.049, 0.32), &bm, YELLOW, Mat::Lit);
    for (x, z) in [(0.3, 0.18), (0.3, -0.18), (-0.3, 0.18), (-0.3, -0.18)] {
        p::draw_blob(on_board(Vec3::new(x, -0.06, z)), Vec3::new(0.06, 0.055, 0.04), &bm, YELLOW, Mat::Lit);
    }
    // Giant clown shoes planted across the board.
    for (x, z) in [(0.17, 0.0), (-0.17, 0.0)] {
        p::draw_blob(on_board(Vec3::new(x, 0.08, z)), Vec3::new(0.09, 0.065, 0.18), &bm, RED, Mat::Lit);
        p::draw_blob(on_board(Vec3::new(x, 0.08, z + 0.17)), Vec3::new(0.07, 0.06, 0.05), &bm, YELLOW, Mat::Lit);
    }

    // Left hand: blue sleeve, red cuff, white glove.
    let lh = |l: Vec3| place(-0.30, -0.30 + skate_bob, 0.55) + b.mul_vec(l);
    p::draw_box(lh(Vec3::new(-0.22, -0.05, -0.04)), Vec3::new(0.3, 0.11, 0.11), &b, BLUE, Mat::Lit);
    p::draw_box(lh(Vec3::new(-0.07, -0.03, -0.02)), Vec3::new(0.04, 0.14, 0.14), &b, RED, Mat::Lit);
    p::draw_blob(lh(Vec3::ZERO), Vec3::new(0.075, 0.075, 0.075), &b, WHITE, Mat::Lit);

    // Right hand + pistol. The gun rotates by the same wobble that perturbs the bullets.
    let (wy, wp) = pl.wobble();
    let kick = pl.recoil;
    let gm = b.mul(&Mat3::rot_y(-wy)).mul(&Mat3::rot_z(wp + kick * 0.5));
    let gc = place(0.24, -0.24 + skate_bob + kick * 0.03, 0.62 - kick * 0.1);
    let g = |l: Vec3, s: Vec3, c: Color, mat: Mat| p::draw_box(gc + gm.mul_vec(l), s, &gm, c, mat);
    p::draw_blob(gc + gm.mul_vec(Vec3::new(-0.03, -0.06, 0.0)), Vec3::new(0.075, 0.075, 0.075), &gm, WHITE, Mat::Lit);
    g(Vec3::new(-0.2, -0.07, 0.0), Vec3::new(0.28, 0.10, 0.10), BLUE, Mat::Lit);
    g(Vec3::new(-0.06, -0.06, 0.0), Vec3::new(0.04, 0.14, 0.14), RED, Mat::Lit);
    g(Vec3::new(0.09, 0.03, 0.0), Vec3::new(0.2, 0.08, 0.07), RED, Mat::Lit);
    g(Vec3::new(0.26, 0.04, 0.0), Vec3::new(0.17, 0.05, 0.05), YELLOW, Mat::Lit);
    g(Vec3::new(0.0, -0.04, 0.0), Vec3::new(0.06, 0.11, 0.06), BLUE, Mat::Lit);
    g(Vec3::new(0.36, 0.04, 0.0), Vec3::new(0.035, 0.06, 0.06), Color::rgb(1.0, 0.5, 0.1), Mat::Glow);
    if pl.muzzle > 0.0 {
        g(Vec3::new(0.46, 0.04, 0.0), Vec3::new(0.2, 0.2, 0.03), Color::rgb(1.0, 0.95, 0.4), Mat::Glow);
        g(Vec3::new(0.46, 0.04, 0.0), Vec3::new(0.2, 0.03, 0.2), Color::rgb(1.0, 0.7, 0.2), Mat::Glow);
    }
}

/// Inside the car: dash, a clown-coloured steering wheel that turns with
/// A/D, gloved hands on it, A-pillars, and a glowing speed bar.
/// Everything is positioned in the CAR's frame (not the camera's) so it stays
/// put when you turn your head.
fn draw_viewmodel_car(_game: &Game, c: &Car, _cam: &CamView) {
    let (base, m) = car_frame(c);
    let put = |l: Vec3, s: Vec3, col: Color, mat: Mat| p::draw_box(base + m.mul_vec(l), s, &m, col, mat);

    put(Vec3::new(0.95, 1.08, 0.0), Vec3::new(0.5, 0.12, 1.95), Color::rgb(0.1, 0.1, 0.16), Mat::Lit);
    let speed_t = (c.speed.abs() / car::TURBO_SPEED).clamp(0.0, 1.0);
    let bar = 0.1 + 0.7 * speed_t;
    let bar_col = if c.turbo_on { Color::rgb(1.0, 0.6, 0.1) } else { neon(1).lerp(neon(0), speed_t) };
    put(Vec3::new(0.72, 1.15, -0.85 + bar * 0.5), Vec3::new(0.03, 0.03, bar), bar_col, Mat::Glow);
    for s in [-1.0f32, 1.0] {
        put(Vec3::new(1.0, 1.6, s * 0.97), Vec3::new(0.1, 1.0, 0.1), Color::rgb(0.1, 0.1, 0.16), Mat::Lit);
        put(Vec3::new(0.75, 1.25, s * 1.1), Vec3::new(0.2, 0.12, 0.22), Color::rgb(0.12, 0.12, 0.18), Mat::Lit); // side mirrors
    }

    // Steering wheel: ring of 14 segments, tilted back toward the driver.
    let centre = Vec3::new(0.5, 1.0, -0.45);
    let wm = m.mul(&Mat3::rot_z(0.55));
    let wheel_angle = c.steer * 1.3;
    let radius = 0.17;
    let seg_color = [RED, YELLOW, BLUE];
    for k in 0..14 {
        let a = k as f32 * TAU / 14.0 + wheel_angle;
        let off = Vec3::new(0.0, radius * a.sin(), radius * a.cos());
        let sm = wm.mul(&Mat3::rot_x(-(a + FRAC_PI_2)));
        p::draw_box(base + m.mul_vec(centre) + wm.mul_vec(off), Vec3::new(0.04, 0.045, 0.11), &sm, seg_color[k % 3], Mat::Lit);
    }
    p::draw_box(base + m.mul_vec(centre), Vec3::new(0.07, 0.08, 0.08), &wm, RED, Mat::Lit); // clown-nose horn
    // Gloved hands at 9 and 3 o'clock (they rotate with the wheel).
    for side in [0.0f32, PI] {
        let a = side + wheel_angle;
        let off = Vec3::new(-0.03, radius * a.sin(), radius * a.cos());
        let pos = base + m.mul_vec(centre) + wm.mul_vec(off);
        p::draw_box(pos, Vec3::new(0.09, 0.09, 0.09), &wm, WHITE, Mat::Lit);
        p::draw_box(pos + wm.mul_vec(Vec3::new(-0.15, -0.03, 0.0)), Vec3::new(0.2, 0.065, 0.065), &wm, BLUE, Mat::Lit);
    }
}

// ================================================================== HUD ===

fn ui_scale() -> f32 {
    (p::screen_size().y / 720.0).max(0.6)
}

fn shadow_text(text: &str, x: f32, y: f32, size: f32, col: Color) {
    p::draw_text(text, x + 2.0, y + 2.0, size, Color::rgba(0.0, 0.0, 0.0, 0.8));
    p::draw_text(text, x, y, size, col);
}
fn shadow_text_c(text: &str, cx: f32, y: f32, size: f32, col: Color) {
    shadow_text(text, cx - p::text_width(text, size) * 0.5, y, size, col);
}
fn panel(x: f32, y: f32, w: f32, h: f32, border: Color) {
    p::draw_rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, border);
    p::draw_rect(x, y, w, h, Color::rgba(0.05, 0.02, 0.12, 0.82));
}

/// A labelled progress bar (turbo tank, clown power).
fn gauge(x: f32, y: f32, w: f32, h: f32, frac: f32, col: Color, label: &str, s: f32) {
    p::draw_rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, Color::rgba(0.0, 0.0, 0.0, 0.7));
    p::draw_rect(x, y, w, h, Color::rgba(0.1, 0.05, 0.2, 0.85));
    p::draw_rect(x, y, w * frac.clamp(0.0, 1.0), h, col);
    shadow_text(label, x + 8.0 * s, y + h - 5.0 * s, 18.0 * s, WHITE);
}

fn objective(g: &Game) -> String {
    if g.flying {
        return format!("FINISH ARCH {:.0} m away. Shoot the saucers! F = land & exit", (world::FINISH_X - g.ufo.pos.x).max(0.0));
    }
    if let Some(ci) = g.drive {
        format!("FINISH ARCH {:.0} m away. Shift = turbo, ramps = air, F = get out", (world::FINISH_X - g.cars[ci].pos.x).max(0.0))
    } else {
        "Shoot zombies (click). Press E next to any car to get in and drive".to_owned()
    }
}

fn draw_hud(g: &Game) {
    let s = ui_scale();
    let sz = p::screen_size();
    let cx = sz.x * 0.5;

    // Damage flash.
    if g.player.damage_flash > 0.0 {
        p::draw_rect(0.0, 0.0, sz.x, sz.y, Color::rgba(1.0, 0.0, 0.1, (g.player.damage_flash * 0.5).min(0.4)));
    }

    // Crosshair (hidden while driving: nothing to aim).
    if g.drive.is_none() || g.flying {
        let cy = sz.y * 0.5;
        let col = WHITE;
        for (dx, dy, w, h) in [(-14.0, -1.5, 10.0, 3.0), (4.0, -1.5, 10.0, 3.0), (-1.5, -14.0, 3.0, 10.0), (-1.5, 4.0, 3.0, 10.0)] {
            p::draw_rect(cx + dx * s - 1.0, cy + dy * s - 1.0, w * s + 2.0, h * s + 2.0, Color::rgba(0.0, 0.0, 0.0, 0.7));
            p::draw_rect(cx + dx * s, cy + dy * s, w * s, h * s, col);
        }
        p::draw_circle(cx, cy, 2.0 * s, RED);
    }

    // Health: clown noses.
    for i in 0..crate::player::MAX_HEALTH {
        let x = (34.0 + i as f32 * 46.0) * s;
        let y = 40.0 * s;
        if i < g.player.health {
            p::draw_circle(x, y, 17.0 * s, Color::rgb(0.2, 0.0, 0.05));
            p::draw_circle(x, y, 15.0 * s, RED);
            p::draw_circle(x - 5.0 * s, y - 5.0 * s, 4.5 * s, Color::rgb(1.0, 0.7, 0.7));
        } else {
            p::draw_ring(x, y, 15.0 * s, 3.0, Color::rgba(0.9, 0.3, 0.3, 0.5));
        }
    }
    // Kill counter.
    p::draw_rect(12.0 * s, 70.0 * s, 330.0 * s, 32.0 * s, Color::rgba(0.04, 0.0, 0.1, 0.7));
    shadow_text(&format!("ZOMBIES DOWN  {}", g.kills), 20.0 * s, 94.0 * s, 26.0 * s, LIME);
    if g.saucers_down > 0 || g.flying {
        p::draw_rect(12.0 * s, 106.0 * s, 330.0 * s, 32.0 * s, Color::rgba(0.04, 0.0, 0.1, 0.7));
        shadow_text(&format!("SAUCERS DOWN  {}", g.saucers_down), 20.0 * s, 130.0 * s, 26.0 * s, neon(1));
    }
    // Co-pilot AI status while the UFO is steering around a saucer.
    if g.flying && g.ufo.avoiding > 0.1 {
        let k = pulse(g.anim_time, 12.0, 0.0);
        shadow_text_c("CO-PILOT AVOIDING COLLISION", cx, sz.y * 0.36, 30.0 * s, Color::rgb(1.0, 0.6, 0.15).lerp(YELLOW, k));
    }

    // Timer.
    let secs = g.run_time as u32;
    let timer = format!("{:02}:{:02}", secs / 60, secs % 60);
    shadow_text(&timer, sz.x - p::text_width(&timer, 40.0 * s) - 24.0 * s, 46.0 * s, 40.0 * s, YELLOW);

    // Objective banner.
    let obj = objective(g);
    let tw = p::text_width(&obj, 26.0 * s);
    panel(cx - tw * 0.5 - 16.0 * s, 14.0 * s, tw + 32.0 * s, 40.0 * s, neon(2));
    p::draw_text(&obj, cx - tw * 0.5, 42.0 * s, 26.0 * s, WHITE);

    // Turbo + clown-power gauges and the crew count (bottom left).
    let (turbo, turbo_on) = match g.drive {
        _ if g.flying => (g.ufo.turbo, g.ufo.turbo_on),
        Some(ci) => (g.cars[ci].turbo, g.cars[ci].turbo_on),
        None => (g.player.turbo, g.player.turbo_on),
    };
    let ready = g.laser_charge >= 1.0;
    let (bx, bw, bh) = (20.0 * s, 280.0 * s, 24.0 * s);
    let by = sz.y - 150.0 * s;
    let turbo_col = if turbo_on { Color::rgb(1.0, 0.85, 0.2) } else { Color::rgb(1.0, 0.45, 0.1) };
    gauge(bx, by, bw, bh, turbo, turbo_col, "TURBO  [Shift]", s);
    let laser_col = if ready { neon(0).lerp(neon(1), pulse(g.anim_time, 8.0, 0.0)) } else { Color::rgb(0.55, 0.3, 0.9) };
    let laser_label = if ready { "CLOWN LASERS READY  [Right-click / G]" } else { "CLOWN POWER" };
    gauge(bx, by + 36.0 * s, bw, bh, g.laser_charge, laser_col, laser_label, s);
    shadow_text(&format!("CREW  {}/{}", g.crew_size(), crate::allies::MAX_CREW), bx, by + 86.0 * s, 26.0 * s, YELLOW);

    // SURROUNDED call-out.
    if g.surrounded && ready {
        let txt = "SURROUNDED!  RIGHT-CLICK / G = CLOWN LASERS";
        let k = pulse(g.anim_time, 10.0, 0.0);
        shadow_text_c(txt, cx, sz.y * 0.3, 38.0 * s, neon(0).lerp(YELLOW, k));
    }
    // Air time while flying.
    if g.flying {
        let gh = g.world.ground_height(g.ufo.pos);
        shadow_text_c(&format!("ALT  {:.0} m", g.ufo.y - gh), cx, sz.y * 0.22, 36.0 * s, neon(4));
    }
    if let Some(ci) = g.drive {
        if g.cars[ci].airborne {
            shadow_text_c(&format!("AIR  {:.1}s", g.cars[ci].air_time), cx, sz.y * 0.22, 40.0 * s, neon(4));
        }
    }
    // Speed streaks at the screen edges while the turbo burns.
    if turbo_on {
        let step = (g.anim_time * 30.0).floor();
        for i in 0..26 {
            let seed = i as f32 * 1.37 + step * 0.73;
            let edge = hash01(seed) * sz.x * 0.2;
            let x = if i % 2 == 0 { edge } else { sz.x - edge };
            let y = hash01(seed * 2.3) * sz.y;
            let len = (50.0 + hash01(seed * 3.1) * 130.0) * s;
            p::draw_rect(x, y, 2.5 * s, len, Color::rgba(0.85, 0.95, 1.0, 0.35));
        }
    }
    // Flash when the lasers fire.
    if g.laser_flash > 0.0 {
        p::draw_rect(0.0, 0.0, sz.x, sz.y, Color::rgba(1.0, 0.4, 0.95, (g.laser_flash * 0.9).min(0.35)));
    }

    // Toast message and interaction prompt.
    if let Some((msg, _)) = &g.toast {
        shadow_text_c(msg, cx, 86.0 * s, 26.0 * s, YELLOW);
    }
    if g.drive.is_none() && !g.flying {
        if let Some(pr) = &g.prompt {
            let tw = p::text_width(pr, 30.0 * s);
            panel(cx - tw * 0.5 - 14.0 * s, sz.y * 0.68 - 30.0 * s, tw + 28.0 * s, 44.0 * s, YELLOW);
            p::draw_text(pr, cx - tw * 0.5, sz.y * 0.68, 30.0 * s, WHITE);
        }
    }

    // Speed readout.
    let speed = match g.drive {
        _ if g.flying => g.ufo.speed() * 3.6,
        Some(ci) => g.cars[ci].speed.abs() * 3.6,
        None => g.player.speed() * 3.6,
    };
    let label = format!("{:.0} km/h", speed);
    shadow_text(&label, sz.x - p::text_width(&label, 34.0 * s) - 24.0 * s, sz.y - 24.0 * s, 34.0 * s, neon(1));
    if g.flying {
        shadow_text("F: land & exit", sz.x - 230.0 * s, sz.y - 62.0 * s, 24.0 * s, WHITE);
    }
    if g.drive.is_some() {
        shadow_text("F: get out", sz.x - 170.0 * s, sz.y - 62.0 * s, 24.0 * s, WHITE);
    }
}

/// Floating text anchored in the world: trunk counters over cars, popups, finish sign.
fn draw_labels(g: &Game) {
    if g.mode == Mode::Title {
        return;
    }
    let s = ui_scale();
    if g.drive.is_none() && !g.flying {
        let u = &g.ufo;
        let dist = (Vec3::from_xz(u.pos, 0.0) - g.camera().pos).len();
        if let Some(sp) = p::world_to_screen(Vec3::from_xz(u.pos, 3.2)) {
            if dist < 90.0 {
                let size = (34.0 * s * (22.0 / dist.max(6.0)).clamp(0.55, 1.3)).max(16.0);
                let w = p::text_width("FLY!", size);
                p::draw_rect(sp.x - w * 0.5 - 8.0, sp.y - size * 0.85, w + 16.0, size * 1.1, Color::rgba(0.0, 0.0, 0.0, 0.55));
                p::draw_text("FLY!", sp.x - w * 0.5, sp.y, size, Color::rgb(0.95, 0.4, 1.0));
            }
        }
    }
    if g.drive.is_none() && !g.flying {
        for c in &g.cars {
            let Some(sp) = p::world_to_screen(Vec3::from_xz(c.pos, 2.8)) else { continue };
            let dist = (Vec3::from_xz(c.pos, 0.0) - g.camera().pos).len();
            if dist > 70.0 {
                continue;
            }
            let (txt, col) = ("DRIVE!".to_owned(), Color::rgb(0.3, 1.0, 0.4));
            let size = (34.0 * s * (22.0 / dist.max(6.0)).clamp(0.55, 1.3)).max(16.0);
            let w = p::text_width(&txt, size);
            p::draw_rect(sp.x - w * 0.5 - 8.0, sp.y - size * 0.85, w + 16.0, size * 1.1, Color::rgba(0.0, 0.0, 0.0, 0.55));
            p::draw_text(&txt, sp.x - w * 0.5, sp.y, size, col);
        }
    }
    for a in g.allies.iter().filter(|a| !a.recruited) {
        let Some(sp) = p::world_to_screen(Vec3::from_xz(a.pos, a.y + 2.9)) else { continue };
        let dist = (Vec3::from_xz(a.pos, 0.0) - g.camera().pos).len();
        if dist > 90.0 {
            continue;
        }
        let size = (30.0 * s * (22.0 / dist.max(6.0)).clamp(0.55, 1.3)).max(14.0);
        let txt = "JOIN US!";
        let w = p::text_width(txt, size);
        p::draw_rect(sp.x - w * 0.5 - 8.0, sp.y - size * 0.85, w + 16.0, size * 1.1, Color::rgba(0.0, 0.0, 0.0, 0.55));
        p::draw_text(txt, sp.x - w * 0.5, sp.y, size, YELLOW);
    }
    for pu in &g.popups {
        let k = 1.0 - pu.ttl / 0.9;
        if let Some(sp) = p::world_to_screen(pu.pos + Vec3::new(0.0, k * 0.9, 0.0)) {
            let size = 44.0 * s * (1.0 + (1.0 - k) * 0.3);
            let col = [RED, YELLOW, neon(1)][(pu.text.len()) % 3];
            shadow_text_c(pu.text, sp.x, sp.y, size, col);
        }
    }
    if let Some(sp) = p::world_to_screen(Vec3::new(world::FINISH_X, 21.0, 0.0)) {
        shadow_text_c("FINISH", sp.x, sp.y, 70.0 * s, neon(3));
    }
}

// ================================================================ menus ===

fn clown_title(text: &str, cx: f32, y: f32, size: f32, t: f32) {
    let colors = [RED, YELLOW, BLUE];
    // Fixed advance (the font is monospaced) so big letters never collide.
    let widths: Vec<f32> = text.chars().map(|_| size * 0.52).collect();
    let total: f32 = widths.iter().sum();
    let mut x = cx - total * 0.5;
    for (i, ch) in text.chars().enumerate() {
        let bounce = (t * 3.0 + i as f32 * 0.6).sin() * size * 0.06;
        let st = ch.to_string();
        p::draw_text(&st, x + 4.0, y + bounce + 5.0, size, Color::rgba(0.0, 0.0, 0.0, 0.8));
        p::draw_text(&st, x, y + bounce, size, colors[i % 3]);
        x += widths[i];
    }
}

fn dim_screen() {
    let sz = p::screen_size();
    p::draw_rect(0.0, 0.0, sz.x, sz.y, Color::rgba(0.02, 0.0, 0.06, 0.62));
}

fn draw_title(g: &Game) {
    let s = ui_scale();
    let sz = p::screen_size();
    let cx = sz.x * 0.5;
    dim_screen();
    clown_title("CLOWNPUNK", cx, sz.y * 0.28, 120.0 * s, g.anim_time);
    shadow_text_c("skate. shoot. squash.", cx, sz.y * 0.28 + 52.0 * s, 32.0 * s, Color::rgb(1.0, 0.15, 0.7));
    let lines = [
        "Shoot the zombies, jump in any car, run them over and drive the Strip to the finish!",
        "",
        "W / S  push / brake      A / D or mouse  turn      SPACE  ollie",
        "LEFT CLICK  shoot      E  get in a car (stand next to it)",
        "F  get out of car      M  music on/off      ESC  pause      R  restart",
    ];
    let mut y = sz.y * 0.56;
    for l in lines {
        shadow_text_c(l, cx, y, 24.0 * s, WHITE);
        y += 32.0 * s;
    }
    if (g.anim_time * 2.0).sin() > -0.3 {
        shadow_text_c("Click or press ENTER to start", cx, sz.y * 0.88, 38.0 * s, YELLOW);
    }
    shadow_text_c("(Q quits)", cx, sz.y * 0.93, 20.0 * s, Color::rgb(0.7, 0.7, 0.8));
}

fn draw_pause() {
    let s = ui_scale();
    let sz = p::screen_size();
    dim_screen();
    shadow_text_c("PAUSED", sz.x * 0.5, sz.y * 0.4, 96.0 * s, YELLOW);
    shadow_text_c("ESC / click: resume     R: restart run     Q: quit", sz.x * 0.5, sz.y * 0.54, 30.0 * s, WHITE);
}

fn draw_game_over(g: &Game) {
    let s = ui_scale();
    let sz = p::screen_size();
    dim_screen();
    shadow_text_c("GAME OVER", sz.x * 0.5, sz.y * 0.4, 100.0 * s, RED);
    shadow_text_c("The zombies got the clown.", sz.x * 0.5, sz.y * 0.5, 34.0 * s, WHITE);
    shadow_text_c(&format!("Survived {:.0}s and put down {} zombies", g.run_time, g.kills), sz.x * 0.5, sz.y * 0.57, 26.0 * s, Color::rgb(0.8, 0.8, 0.9));
    shadow_text_c("Press R to restart", sz.x * 0.5, sz.y * 0.7, 40.0 * s, YELLOW);
}

fn draw_win(g: &Game) {
    let s = ui_scale();
    let sz = p::screen_size();
    dim_screen();
    let secs = g.final_time;
    clown_title("YOU WIN!", sz.x * 0.5, sz.y * 0.36, 110.0 * s, g.anim_time);
    shadow_text_c(&format!("Run time  {:02}:{:05.2}", (secs / 60.0) as u32, secs % 60.0), sz.x * 0.5, sz.y * 0.5, 50.0 * s, neon(1));
    shadow_text_c("The zombies are hauled. The clown rules the Strip.", sz.x * 0.5, sz.y * 0.58, 28.0 * s, WHITE);
    shadow_text_c("Press R to restart", sz.x * 0.5, sz.y * 0.72, 40.0 * s, YELLOW);
}
