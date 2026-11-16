//! The soundtrack: a fast punk loop, synthesised in code at startup so the game
//! still ships with no audio files.
//!
//! ~184 BPM, 16 bars, in E: distorted power-chord "chugging" on every eighth
//! note, a pumping bass, and a skate-punk drum beat (kick, snare on 2 and 4,
//! constant hi-hat, a snare fill every fourth bar and a crash on the one).
//! Everything is mono, 22.05 kHz, and wraps around so the loop has no gap.

pub const SAMPLE_RATE: u32 = 22_050;
const BPM: f32 = 184.0;
const BARS: usize = 16;
const STEPS_PER_BAR: usize = 8; // eighth notes

/// Chord roots in semitones above E2, one per bar: E E G A | E E G A | C C D D | E G A B.
const ROOTS: [i32; BARS] = [0, 0, 3, 5, 0, 0, 3, 5, 8, 8, 10, 10, 0, 3, 5, 7];

const E2_HZ: f32 = 82.41;
const TAU: f32 = std::f32::consts::TAU;

/// Tiny deterministic noise source (xorshift), so every run sounds identical.
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn semis_to_hz(semis: i32) -> f32 {
    E2_HZ * 2f32.powf(semis as f32 / 12.0)
}

/// Band-limited sawtooth correction (PolyBLEP) so high notes don't alias.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

fn saw(phase: f32, dt: f32) -> f32 {
    2.0 * phase - 1.0 - poly_blep(phase, dt)
}

/// Add `data` into the loop buffer starting at `start`, wrapping past the end
/// so tails (crash, ringing chords) flow into the start of the next loop.
fn mix(buf: &mut [f32], start: usize, data: &[f32]) {
    let n = buf.len();
    for (i, v) in data.iter().enumerate() {
        buf[(start + i) % n] += *v;
    }
}

/// Amplitude envelope: quick attack, exponential decay, short release at the end.
fn env(i: usize, len: usize, decay: f32) -> f32 {
    let attack = (i as f32 / 60.0).min(1.0);
    let release = ((len - i) as f32 / 120.0).min(1.0);
    attack * release * (-(i as f32) / (len as f32 * decay)).exp()
}

/// A distorted power chord (root + fifth + octave, two slightly detuned saws each).
fn guitar_chord(root_hz: f32, len: usize, level: f32) -> Vec<f32> {
    let freqs = [root_hz, root_hz * 1.4983, root_hz * 2.0];
    let mut phases = [[0.0f32; 2]; 3];
    let mut lp = 0.0f32;
    (0..len)
        .map(|i| {
            let mut s = 0.0;
            for (k, f) in freqs.iter().enumerate() {
                for (d, det) in [1.0f32, 1.006].iter().enumerate() {
                    let dt = f * det / SAMPLE_RATE as f32;
                    phases[k][d] = (phases[k][d] + dt) % 1.0;
                    s += saw(phases[k][d], dt);
                }
            }
            // Heavy drive, then a gentle low-pass to take the fizz off.
            let driven = (s * 0.5 * 3.5).tanh();
            lp += 0.5 * (driven - lp);
            lp * env(i, len, 0.9) * level
        })
        .collect()
}

fn bass_note(root_hz: f32, len: usize, level: f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    let mut lp = 0.0f32;
    let dt = root_hz / SAMPLE_RATE as f32;
    (0..len)
        .map(|i| {
            phase = (phase + dt) % 1.0;
            let s = saw(phase, dt) * 0.6 + (phase * TAU).sin() * 0.8;
            lp += 0.18 * (s - lp);
            lp * env(i, len, 0.8) * level
        })
        .collect()
}

fn kick() -> Vec<f32> {
    let len = (0.16 * SAMPLE_RATE as f32) as usize;
    let mut phase = 0.0f32;
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let freq = 48.0 + 120.0 * (-t / 0.025).exp();
            phase += freq / SAMPLE_RATE as f32;
            let click = if i < 40 { 0.4 * (1.0 - i as f32 / 40.0) } else { 0.0 };
            ((phase * TAU).sin() * (-t / 0.09).exp() + click) * 0.95
        })
        .collect()
}

fn snare(noise: &mut Noise) -> Vec<f32> {
    let len = (0.2 * SAMPLE_RATE as f32) as usize;
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let body = (t * 190.0 * TAU).sin() * (-t / 0.045).exp() * 0.5;
            let rattle = noise.next() * (-t / 0.07).exp() * 0.7;
            (body + rattle) * 0.8
        })
        .collect()
}

/// High-passed noise burst: `decay` is the time constant in seconds.
fn hat(noise: &mut Noise, secs: f32, decay: f32, level: f32) -> Vec<f32> {
    let len = (secs * SAMPLE_RATE as f32) as usize;
    let mut prev = 0.0f32;
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let n = noise.next();
            let hp = n - prev; // crude high-pass
            prev = n;
            hp * (-t / decay).exp() * level
        })
        .collect()
}

/// Render the whole loop as floating-point samples in -1..1.
pub fn render() -> Vec<f32> {
    let step = (SAMPLE_RATE as f32 * 60.0 / BPM / 2.0).round() as usize; // one eighth note
    let total = step * STEPS_PER_BAR * BARS;
    let mut buf = vec![0.0f32; total];
    let mut noise = Noise(0x1234_5678);

    let kick_s = kick();
    let snare_s = snare(&mut noise);
    let hat_s = hat(&mut noise, 0.07, 0.018, 0.22);
    let crash_s = hat(&mut noise, 0.9, 0.3, 0.28);

    for bar in 0..BARS {
        let root = semis_to_hz(ROOTS[bar]);
        let fill = bar % 4 == 3;
        for s in 0..STEPS_PER_BAR {
            let at = (bar * STEPS_PER_BAR + s) * step;

            // Guitar: ringing chord on the one, palm-muted chugs on the rest.
            if s == 0 {
                mix(&mut buf, at, &guitar_chord(root, step * 3 / 2, 0.55));
            } else {
                let accent = if s % 2 == 0 { 0.5 } else { 0.4 };
                mix(&mut buf, at, &guitar_chord(root, step * 8 / 10, accent));
            }
            // Bass doubles the root on every eighth.
            mix(&mut buf, at, &bass_note(root, step * 9 / 10, 0.5));

            // Drums.
            mix(&mut buf, at, &hat_s);
            let kick_hit = if fill { matches!(s, 0 | 2) } else { matches!(s, 0 | 3 | 4 | 7) };
            let snare_hit = if fill { s >= 4 } else { matches!(s, 2 | 6) };
            if kick_hit {
                mix(&mut buf, at, &kick_s);
            }
            if snare_hit {
                mix(&mut buf, at, &snare_s);
            }
            if s == 0 && bar % 4 == 0 {
                mix(&mut buf, at, &crash_s);
            }
        }
    }

    // Soft-limit, then normalise to a safe peak.
    for v in &mut buf {
        *v = (*v * 0.9).tanh();
    }
    let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    for v in &mut buf {
        *v *= 0.9 / peak;
    }
    buf
}

/// The loop as a 16-bit mono WAV file in memory.
pub fn wav_bytes() -> Vec<u8> {
    let samples = render();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for v in samples {
        out.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_is_about_twenty_one_seconds_and_loud_enough() {
        let s = render();
        let secs = s.len() as f32 / SAMPLE_RATE as f32;
        assert!((20.0..22.0).contains(&secs), "length {secs}s");
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        assert!((0.85..=0.91).contains(&peak), "peak {peak}");
        assert!(rms > 0.1 && rms < 0.6, "rms {rms}");
        assert!(s.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn wav_header_is_valid_and_render_is_deterministic() {
        let w = wav_bytes();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data_len);
        assert_eq!(w, wav_bytes());
    }

    /// `MUSIC_DUMP=/path/song.wav cargo test --release dump -- --ignored` writes the loop for listening/inspection.
    #[test]
    #[ignore]
    fn dump() {
        std::fs::write(std::env::var("MUSIC_DUMP").unwrap(), wav_bytes()).unwrap();
    }
}
