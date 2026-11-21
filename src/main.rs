//! ClownPunk - entry point and the fixed-timestep loop.
//!
//! The simulation always advances in exact 1/60 s steps (`TICK`), no matter how
//! fast frames are rendered: each frame we add the elapsed real time to an
//! accumulator and run as many fixed ticks as fit. This keeps physics and AI
//! deterministic and independent of frame rate.
mod allies;
mod car;
mod collision;
mod icon;
mod math;
mod music;
mod platform;
mod player;
mod render;
mod smoke;
mod state;
mod world;
mod zombies;

use state::Game;

const TICK: f32 = 1.0 / 60.0;

async fn app() {
    platform::seed_rng();
    platform::start_music(&music::wav_bytes()).await;
    let mut game = Game::new();
    let mut accumulator = 0.0f32;
    let mut input = platform::Input::default();

    // Optional headless smoke test (see smoke.rs); None in normal play.
    let mut smoke = smoke::Smoke::from_env();

    loop {
        // Mouse lock follows the game mode (locked while playing).
        platform::set_mouse_captured(game.wants_mouse_capture());

        // Merge this frame's input with any leftover one-shot events.
        let fresh = platform::poll_input();
        if fresh.music {
            platform::toggle_music();
        }
        input.forward = fresh.forward;
        input.back = fresh.back;
        input.left = fresh.left;
        input.right = fresh.right;
        input.jump |= fresh.jump;
        input.interact |= fresh.interact;
        input.exit_car |= fresh.exit_car;
        input.shoot |= fresh.shoot;
        input.turbo = fresh.turbo;
        input.laser |= fresh.laser;
        input.confirm |= fresh.confirm;
        input.pause |= fresh.pause;
        input.restart |= fresh.restart;
        input.quit |= fresh.quit;
        input.look_dx += fresh.look_dx;
        input.look_dy += fresh.look_dy;

        // The smoke test scripts input on top of the real thing.
        if let Some(sm) = &mut smoke {
            if sm.step(&mut input, &mut game) {
                break;
            }
        }

        accumulator += platform::frame_time();
        while accumulator >= TICK {
            game.update(&input, TICK);
            input.consume_events(); // one-shot events are seen by exactly one tick
            accumulator -= TICK;
        }

        if game.quit {
            break;
        }
        render::draw(&game);
        if let Some(sm) = &mut smoke {
            sm.capture(); // screenshots must be taken before the buffer swap
        }
        platform::next_frame().await;
    }
}

fn main() {
    platform::run(app());
}
