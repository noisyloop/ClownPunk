# Clown Skate Zombie Haul

A small offline first-person 3D game sketch in Rust. You are a clown on a
skateboard. Green zombies shamble after you through a parking lot. Shoot them
with a wobbly toy pistol (or run them over), jump in any car, then drive the neon
Las-Vegas-style Strip to the finish arch. A run takes about 5 minutes.

Everything is drawn with coloured boxes and spheres: no assets, no textures.
The only dependency is [macroquad](https://crates.io/crates/macroquad) (window,
input, GL drawing, random numbers).

## Download (no install, no Rust needed)

Pick your system, unpack, and run `clown_skate_zombie_haul`:

| System | Download |
|--------|----------|
| Windows (64-bit) | [clown-skate-zombie-haul-windows-x64.zip](https://github.com/noisyloop/ClownPunk/releases/latest/download/clown-skate-zombie-haul-windows-x64.zip) |
| macOS (Apple Silicon) | [clown-skate-zombie-haul-macos-arm64.zip](https://github.com/noisyloop/ClownPunk/releases/latest/download/clown-skate-zombie-haul-macos-arm64.zip) |
| Linux (64-bit) | [clown-skate-zombie-haul-linux-x64.zip](https://github.com/noisyloop/ClownPunk/releases/latest/download/clown-skate-zombie-haul-linux-x64.zip) |

All releases: <https://github.com/noisyloop/ClownPunk/releases>

Notes: the builds are unsigned. Windows SmartScreen may say "unknown publisher"
(More info > Run anyway). On macOS run
`xattr -d com.apple.quarantine clown_skate_zombie_haul` once, then
`./clown_skate_zombie_haul`. On Linux, `chmod +x` it if needed.

[![Build](https://github.com/noisyloop/ClownPunk/actions/workflows/build.yml/badge.svg)](https://github.com/noisyloop/ClownPunk/actions/workflows/build.yml)

## Run it from source

```
cargo run --release
```

Linux needs the usual X11/OpenGL runtime libraries (`libx11`, `libxi`, `libgl`);
macOS and Windows work out of the box. Audio is not used (macroquad's `audio`
feature is off), so there is no ALSA dependency.

## Controls

| Key / mouse            | On the skateboard                          | In the car              |
|------------------------|--------------------------------------------|-------------------------|
| **W** / **S**          | push (accelerate) / brake                  | gas / brake, then reverse |
| **A** / **D**          | carve left / right                         | steer                   |
| **Mouse**              | look (yaw + pitch); yaw also turns the board | look around the cabin |
| **Space**              | ollie (small hop)                          | -                       |
| **Left click**         | shoot the pistol                           | -                       |
| **E**                  | get into the nearest car (stand next to it) | -                      |
| **F**                  | -                                          | get out of the car      |
| **Esc**                | pause / resume                             | pause / resume          |
| **R**                  | restart (pause, Game Over and Win screens) | same                    |
| **Q**                  | quit (title and pause screens)             | same                    |

## How to play

1. **Parking lot.** 6-8 zombies chase you. They are slower than your top speed
   but faster than you when you stand still, so keep moving. A zombie that
   touches you costs 1 of 3 hearts and knocks you back. 0 hearts = Game Over.
2. **Shoot** a zombie and it goes down for good. The HUD counts them.
3. Stand next to **any car** (they all have a green beacon) and press **E** to
   get in. No zombies needed.
4. **The Strip.** Drive down the boulevard and run zombies over: a moving car
   kills them and loses a little speed doing it. Barriers, vans and traffic
   cabinets are solid. **F** bails out at any time (**E** next to a car gets
   you back in).
5. Cross the glowing **finish arch** in a car to win.

## Code map (read in this order)

| File | What it does |
|------|--------------|
| `src/main.rs` | Entry point and the **fixed-timestep loop** (60 Hz ticks fed by an accumulator). |
| `src/state.rs` | The **game state machine** (`Mode`) and the per-tick simulation that connects everything: shooting, interactions, zombie contact, win/lose. Also holds the tests. |
| `src/player.rs` | **Skateboard movement**: heading vs velocity, grip and drift, ollie, mouse look, gun wobble. |
| `src/car.rs` | Arcade **car driving**. |
| `src/zombies.rs` | Zombie **AI states** (walking / dead) and bullet hit volumes. |
| `src/world.rs` | The static level: lot, Strip, casinos, lamps, palms, obstacles. |
| `src/collision.rs` | **Collision and raycasting**: circle-vs-oriented-box push-out, ray-vs-box (slab), ray-vs-sphere. |
| `src/render.rs` | Draws the scene, first-person hands/board/dashboard, HUD and menus, using only `platform`. |
| `src/platform.rs` | **The only file that calls macroquad.** Window, input, camera, drawing, random. |
| `src/math.rs` | Tiny `Vec2` / `Vec3` / `Mat3` / `Color` so gameplay does not depend on any engine's types. |
| `src/smoke.rs` | Optional headless screenshot test (only active with an env var). |

### Conventions

* +Y is up. Ground positions are `Vec2 { x: world X, y: world Z }`.
* A heading `yaw` points along `(cos yaw, sin yaw)`; "right" is `(-sin yaw, cos yaw)`.
  Increasing yaw turns right. Boxes use local axes x = forward, y = up, z = right.
* There is no physics library. Moving things are circles, static things are
  oriented boxes, and "physics" is: move, push out of overlaps, remove the
  velocity pointing into the surface.

### Swapping in your own engine (winit + wgpu)

Re-implement the public functions of `platform.rs` and nothing else changes:

* loop: `run`, `next_frame`, `frame_time`, `screen_size`
* input: `poll_input` (returns an `Input` of game actions, not raw keys),
  `set_mouse_captured`
* 3D: `begin_3d` (first-person camera), `begin_ui`, `world_to_screen`,
  `draw_box` (oriented box with fake face shading + distance fog, see `Mat`),
  `draw_aabb`, `draw_ball`
* 2D: `draw_rect`, `draw_circle`, `draw_ring`, `draw_text`, `text_width`
* misc: `seed_rng`, `rand_range`, `rand_chance`, `save_screenshot`, `env`

There is no lighting engine: `draw_box` bakes a fixed directional shade into
each face colour and fades colours toward the night colour with distance, which
is cheap to reproduce in a wgpu vertex shader.

### Tuning

Most feel parameters are plain constants at the top of `player.rs`, `car.rs`,
`zombies.rs` and `state.rs` (top speed, zombie speed, shot cooldown,
interaction range, ...), and `world.rs` (Strip length, obstacle density).

## Tests

```
cargo test --release
```

The tests run the real simulation without a window: shoot -> dead,
walls blocking bullets, get in any car / drive / finish,
damage and Game Over, skater top speed, roadkill, pause, and the collision maths.

Headless screenshot smoke test (Linux, needs `xvfb`):

```
CLOWN_AUTOSHOT=/tmp/shot xvfb-run -a ./target/release/clown_skate_zombie_haul
```

## Known simplifications (it is a sketch)

* The renderer draws the latest simulation state without interpolating between
  fixed ticks, so on very high refresh rates motion can look slightly steppy.
* Zombie pathing is "walk at the target and slide along walls"; they can get
  stuck behind big obstacles.
* No sound, no save data, no gamepad.
