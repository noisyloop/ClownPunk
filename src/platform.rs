//! The ONLY file that talks to macroquad.
//!
//! Everything the game needs from the outside world goes through here:
//!   * window creation + the frame loop      (`run`, `next_frame`, `frame_time`)
//!   * input, already translated to game actions (`Input`, `poll_input`)
//!   * the 3D camera and box/sphere drawing   (`set_camera`, `draw_box`, ...)
//!   * 2D HUD drawing and text                (`draw_rect`, `draw_text`, ...)
//!   * random numbers                         (`rand_range`)
//!   * looping background music               (`start_music`, `toggle_music`)
//!
//! To port the game to your own winit + wgpu engine, re-implement exactly the
//! public functions in this file. No other module needs to change.
use crate::math::{Color, Mat3, Vec2, Vec3};
use macroquad as mq;
use macroquad::camera::Camera;
use std::cell::RefCell;

// ---------------------------------------------------------------- window ---

/// Opens the window and runs `app` (an async fn that loops on `next_frame`).
pub fn run<F: std::future::Future<Output = ()> + 'static>(app: F) {
    let conf = mq::window::Conf {
        window_title: "ClownPunk".to_owned(),
        window_width: 1280,
        window_height: 720,
        sample_count: 4,
        icon: Some(window_icon()),
        ..Default::default()
    };
    mq::Window::from_config(conf, app);
}

fn window_icon() -> mq::miniquad::conf::Icon {
    fn to_array<const N: usize>(size: usize) -> [u8; N] {
        crate::icon::render(size).try_into().expect("icon size")
    }
    mq::miniquad::conf::Icon {
        small: to_array::<{ 16 * 16 * 4 }>(16),
        medium: to_array::<{ 32 * 32 * 4 }>(32),
        big: to_array::<{ 64 * 64 * 4 }>(64),
    }
}

/// Finish the frame (presents, polls OS events). Call once per loop iteration.
pub async fn next_frame() {
    mq::window::next_frame().await;
}

/// Seconds the last frame took, clamped so a hitch can't explode the simulation.
pub fn frame_time() -> f32 {
    mq::time::get_frame_time().min(0.1)
}

pub fn screen_size() -> Vec2 {
    Vec2::new(mq::window::screen_width(), mq::window::screen_height())
}

// ----------------------------------------------------------------- input ---

/// One frame of player intent, expressed as game actions rather than keys.
/// Fields marked (edge) are true only on the frame the key went down.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,     // (edge) Space
    pub interact: bool, // (edge) E
    pub exit_car: bool, // (edge) F
    pub shoot: bool,    // (edge) left mouse
    pub turbo: bool,    // held: Shift
    pub up: bool,       // held: Space (UFO climbs)
    pub down: bool,     // held: C / Ctrl (UFO descends)
    pub laser: bool,    // (edge) right mouse / G: clown laser burst
    pub confirm: bool,  // (edge) Enter / Space / click, for menus
    pub pause: bool,    // (edge) Esc
    pub restart: bool,  // (edge) R
    pub quit: bool,     // (edge) Q, only acted upon in menus
    pub music: bool,    // (edge) M, toggles the music
    /// Mouse movement this frame in pixels (+x right, +y down); 0 unless captured.
    pub look_dx: f32,
    pub look_dy: f32,
}

impl Input {
    /// The fixed-step loop may run several ticks per rendered frame. One-shot
    /// events (edges and mouse movement) must only be seen by the first tick,
    /// so the loop calls this after each tick.
    pub fn consume_events(&mut self) {
        self.jump = false;
        self.interact = false;
        self.exit_car = false;
        self.shoot = false;
        self.laser = false;
        self.confirm = false;
        self.pause = false;
        self.restart = false;
        self.quit = false;
        self.music = false;
        self.look_dx = 0.0;
        self.look_dy = 0.0;
    }
}

thread_local! {
    static CAPTURED: RefCell<bool> = const { RefCell::new(false) };
    static SKIP_LOOK: RefCell<u8> = const { RefCell::new(0) };
}

/// Lock + hide the cursor (gameplay) or release it (menus / pause).
pub fn set_mouse_captured(captured: bool) {
    CAPTURED.with(|c| {
        if *c.borrow() != captured {
            *c.borrow_mut() = captured;
            mq::input::set_cursor_grab(captured);
            mq::input::show_mouse(!captured);
            // The first delta after a mode switch is garbage; skip it.
            SKIP_LOOK.with(|s| *s.borrow_mut() = 2);
        }
    });
}

pub fn poll_input() -> Input {
    use mq::input::*;
    let captured = CAPTURED.with(|c| *c.borrow());
    let (mut dx, mut dy) = (0.0, 0.0);
    if captured {
        // macroquad reports (last - current) in -1..1 screen space; convert to pixels.
        let d = mouse_delta_position();
        let s = screen_size();
        dx = -d.x * s.x * 0.5;
        dy = -d.y * s.y * 0.5;
        let skip = SKIP_LOOK.with(|s| {
            let mut s = s.borrow_mut();
            let v = *s;
            *s = s.saturating_sub(1);
            v > 0
        });
        if skip {
            dx = 0.0;
            dy = 0.0;
        }
        dx = dx.clamp(-300.0, 300.0);
        dy = dy.clamp(-300.0, 300.0);
    }
    let click = is_mouse_button_pressed(MouseButton::Left);
    Input {
        forward: is_key_down(KeyCode::W) || is_key_down(KeyCode::Up),
        back: is_key_down(KeyCode::S) || is_key_down(KeyCode::Down),
        left: is_key_down(KeyCode::A) || is_key_down(KeyCode::Left),
        right: is_key_down(KeyCode::D) || is_key_down(KeyCode::Right),
        jump: is_key_pressed(KeyCode::Space),
        interact: is_key_pressed(KeyCode::E),
        exit_car: is_key_pressed(KeyCode::F),
        shoot: click && captured,
        turbo: is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift),
        up: is_key_down(KeyCode::Space),
        down: is_key_down(KeyCode::C) || is_key_down(KeyCode::LeftControl),
        laser: captured && (is_mouse_button_pressed(MouseButton::Right) || is_key_pressed(KeyCode::G)),
        confirm: is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) || click,
        pause: is_key_pressed(KeyCode::Escape),
        restart: is_key_pressed(KeyCode::R),
        quit: is_key_pressed(KeyCode::Q),
        music: is_key_pressed(KeyCode::M),
        look_dx: dx,
        look_dy: dy,
    }
}

// ----------------------------------------------------------------- music ---

const MUSIC_VOLUME: f32 = 0.45;

thread_local! {
    /// The looping soundtrack and whether it is currently audible.
    static MUSIC: RefCell<Option<(mq::audio::Sound, bool)>> = const { RefCell::new(None) };
}

/// Start looping a WAV soundtrack. Music is optional: if the sound system
/// can't decode it or there is no audio device, the game just stays quiet.
pub async fn start_music(wav: &[u8]) {
    if env("CLOWNPUNK_NO_MUSIC").is_some() {
        return;
    }
    if let Ok(sound) = mq::audio::load_sound_from_bytes(wav).await {
        mq::audio::play_sound(&sound, mq::audio::PlaySoundParams { looped: true, volume: MUSIC_VOLUME });
        MUSIC.with(|m| *m.borrow_mut() = Some((sound, true)));
    }
}

/// Mute / unmute the soundtrack (M key).
pub fn toggle_music() {
    MUSIC.with(|m| {
        if let Some((sound, on)) = m.borrow_mut().as_mut() {
            *on = !*on;
            mq::audio::set_sound_volume(sound, if *on { MUSIC_VOLUME } else { 0.0 });
        }
    });
}

// ---------------------------------------------------------------- random ---

pub fn seed_rng() {
    mq::rand::srand((mq::miniquad::date::now() * 1000.0) as u64);
}
pub fn rand_range(lo: f32, hi: f32) -> f32 {
    mq::rand::gen_range(lo, hi)
}
pub fn rand_chance(p: f32) -> bool {
    rand_range(0.0, 1.0) < p
}

// ---------------------------------------------------------------- camera ---

/// What the renderer remembers about the active 3D camera (for fog + projection).
struct CamState {
    pos: Vec3,
    matrix: mq::math::Mat4,
}

thread_local! {
    static CAM: RefCell<CamState> = RefCell::new(CamState { pos: Vec3::ZERO, matrix: mq::math::Mat4::IDENTITY });
    // One reusable mesh so drawing a box doesn't allocate every time.
    static MESH: RefCell<mq::models::Mesh> = RefCell::new(mq::models::Mesh { vertices: Vec::new(), indices: Vec::new(), texture: None });
}

fn to_mq(c: Color) -> mq::color::Color {
    mq::color::Color::new(c.r, c.g, c.b, c.a)
}
fn to_mq3(v: Vec3) -> mq::math::Vec3 {
    mq::math::vec3(v.x, v.y, v.z)
}

/// First-person camera. Builds a look-at camera from yaw/pitch:
///   forward = (cos(pitch)cos(yaw), sin(pitch), cos(pitch)sin(yaw))
/// `roll` tilts the horizon (used for carving lean). Also clears the screen.
pub fn begin_3d(sky: Color, pos: Vec3, yaw: f32, pitch: f32, roll: f32, fov_deg: f32) {
    mq::window::clear_background(to_mq(sky));
    let fwd = Vec3::new(pitch.cos() * yaw.cos(), pitch.sin(), pitch.cos() * yaw.sin());
    let right = fwd.cross(Vec3::new(0.0, 1.0, 0.0)).norm();
    let up = Vec3::new(0.0, 1.0, 0.0) * roll.cos() + right * roll.sin();
    let cam = mq::camera::Camera3D {
        position: to_mq3(pos),
        target: to_mq3(pos + fwd),
        up: to_mq3(up),
        fovy: fov_deg.to_radians(),
        aspect: None,
        projection: mq::camera::Projection::Perspective,
        render_target: None,
        viewport: None,
        z_near: 0.08,
        z_far: 700.0,
    };
    mq::camera::set_camera(&cam);
    CAM.with(|c| {
        let mut c = c.borrow_mut();
        c.pos = pos;
        c.matrix = cam.matrix();
    });
}

/// Switch to 2D screen-space drawing (HUD). Origin top-left, y down, pixels.
pub fn begin_ui() {
    mq::camera::set_default_camera();
}

/// Project a world point to screen pixels (None if behind the camera).
/// This is just `matrix * point` followed by the perspective divide.
pub fn world_to_screen(p: Vec3) -> Option<Vec2> {
    let m = CAM.with(|c| c.borrow().matrix);
    let clip = m * mq::math::vec4(p.x, p.y, p.z, 1.0);
    if clip.w <= 0.01 {
        return None;
    }
    let ndc = clip / clip.w;
    if ndc.z > 1.0 || ndc.x.abs() > 1.5 || ndc.y.abs() > 1.5 {
        return None;
    }
    let s = screen_size();
    Some(Vec2::new((ndc.x * 0.5 + 0.5) * s.x, (1.0 - (ndc.y * 0.5 + 0.5)) * s.y))
}

// --------------------------------------------------------------- drawing ---

/// How a surface reacts to light. There is no lighting engine: "lighting" is a
/// fixed directional shade baked into each face colour, plus distance fog.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mat {
    /// Shaded by face direction, fades into the night fog.
    Lit,
    /// Emissive neon: ignores shading, fogs out much less.
    Glow,
    /// Skybox dots: no shading, no fog.
    Sky,
}

const FOG_COLOR: Color = Color::rgb(0.07, 0.03, 0.14);
const FOG_START: f32 = 50.0;
const FOG_END: f32 = 330.0;
const LIGHT_DIR: Vec3 = Vec3::new(-0.35, 0.8, 0.45);

/// Draw an oriented box: `center` in world space, full `size` (x,y,z) in model
/// space, `rot` maps model axes to world axes. Each of the 6 faces gets its own
/// shade, which is what gives the low-poly look.
pub fn draw_box(center: Vec3, size: Vec3, rot: &Mat3, color: Color, mat: Mat) {
    let cam = CAM.with(|c| c.borrow().pos);
    let h = size * 0.5;
    let light = LIGHT_DIR.norm();
    // (face normal axis, tangent u, tangent v) in model space; windings don't
    // matter because macroquad doesn't cull back faces.
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (rot.x, rot.y, rot.z),
        (-rot.x, rot.y, rot.z),
        (rot.y, rot.x, rot.z),
        (-rot.y, rot.x, rot.z),
        (rot.z, rot.x, rot.y),
        (-rot.z, rot.x, rot.y),
    ];
    let extents = [(h.x, h.y, h.z), (h.x, h.y, h.z), (h.y, h.x, h.z), (h.y, h.x, h.z), (h.z, h.x, h.y), (h.z, h.x, h.y)];
    MESH.with(|m| {
        let mut m = m.borrow_mut();
        m.vertices.clear();
        m.indices.clear();
        for (i, ((n, u, v), (hn, hu, hv))) in faces.iter().zip(extents.iter()).enumerate() {
            let shade = match mat {
                Mat::Lit => 0.58 + 0.5 * n.dot(light).max(0.0),
                _ => 1.0,
            };
            let base = *n * *hn + center;
            let corners = [
                base + *u * *hu + *v * *hv,
                base + *u * *hu - *v * *hv,
                base - *u * *hu - *v * *hv,
                base - *u * *hu + *v * *hv,
            ];
            let start = (i * 4) as u16;
            for c in corners {
                let d = (c - cam).len();
                let t = ((d - FOG_START) / (FOG_END - FOG_START)).clamp(0.0, 1.0);
                let fogged = match mat {
                    Mat::Lit => color.scale(shade).lerp(FOG_COLOR, t),
                    Mat::Glow => color.lerp(FOG_COLOR, t * 0.7),
                    Mat::Sky => color,
                };
                m.vertices.push(mq::models::Vertex::new(c.x, c.y, c.z, 0.0, 0.0, to_mq(fogged)));
            }
            m.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
        mq::models::draw_mesh(&m);
    });
}

/// A smooth lit ellipsoid with the same shading and fog as `draw_box`: `radii`
/// are the half-sizes along the model axes, `rot` orients them. A cheap way to
/// get round heads, limbs, wheels and leaves instead of blocks.
pub fn draw_blob(center: Vec3, radii: Vec3, rot: &Mat3, color: Color, mat: Mat) {
    const RINGS: usize = 8;
    const SEGS: usize = 12;
    let cam = CAM.with(|c| c.borrow().pos);
    let light = LIGHT_DIR.norm();
    MESH.with(|m| {
        let mut m = m.borrow_mut();
        m.vertices.clear();
        m.indices.clear();
        for r in 0..=RINGS {
            let theta = std::f32::consts::PI * r as f32 / RINGS as f32;
            for s in 0..=SEGS {
                let phi = std::f32::consts::TAU * s as f32 / SEGS as f32;
                let u = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
                let pos = center + rot.mul_vec(Vec3::new(u.x * radii.x, u.y * radii.y, u.z * radii.z));
                let n = rot.mul_vec(Vec3::new(u.x / radii.x, u.y / radii.y, u.z / radii.z)).norm();
                let d = (pos - cam).len();
                let t = ((d - FOG_START) / (FOG_END - FOG_START)).clamp(0.0, 1.0);
                let col = match mat {
                    Mat::Lit => color.scale(0.58 + 0.5 * n.dot(light).max(0.0)).lerp(FOG_COLOR, t),
                    Mat::Glow => color.lerp(FOG_COLOR, t * 0.7),
                    Mat::Sky => color,
                };
                m.vertices.push(mq::models::Vertex::new(pos.x, pos.y, pos.z, 0.0, 0.0, to_mq(col)));
            }
        }
        for r in 0..RINGS {
            for s in 0..SEGS {
                let a = (r * (SEGS + 1) + s) as u16;
                let b = a + (SEGS + 1) as u16;
                m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        mq::models::draw_mesh(&m);
    });
}

/// Axis-aligned convenience wrapper.
pub fn draw_aabb(center: Vec3, size: Vec3, color: Color, mat: Mat) {
    draw_box(center, size, &Mat3::IDENTITY, color, mat);
}

/// Flat-coloured sphere (moon, pistol-ball effects).
pub fn draw_ball(center: Vec3, radius: f32, color: Color) {
    mq::models::draw_sphere(to_mq3(center), radius, None, to_mq(color));
}

// ------------------------------------------------------------------ 2D UI ---

pub fn draw_rect(x: f32, y: f32, w: f32, h: f32, color: Color) {
    mq::shapes::draw_rectangle(x, y, w, h, to_mq(color));
}
pub fn draw_circle(x: f32, y: f32, r: f32, color: Color) {
    mq::shapes::draw_circle(x, y, r, to_mq(color));
}
pub fn draw_ring(x: f32, y: f32, r: f32, thick: f32, color: Color) {
    mq::shapes::draw_circle_lines(x, y, r, thick, to_mq(color));
}
/// Text with its baseline at `y`.
pub fn draw_text(text: &str, x: f32, y: f32, size: f32, color: Color) {
    mq::text::draw_text(text, x, y, size, to_mq(color));
}
pub fn text_width(text: &str, size: f32) -> f32 {
    mq::text::measure_text(text, None, size as u16, 1.0).width
}

// ------------------------------------------------------------- debugging ---

/// Save the current frame to a PNG (used by the automated smoke test).
pub fn save_screenshot(path: &str) {
    mq::texture::get_screen_data().export_png(path);
}

/// Read an environment variable (test hooks only).
pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
