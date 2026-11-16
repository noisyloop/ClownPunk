//! The window / taskbar icon: a neon clown face, drawn in code so the game
//! still ships with no asset files. Rendered once at startup at the three
//! sizes the OS asks for, with 4x4 supersampling for smooth edges.

type Rgb = [f32; 3];

const BG: Rgb = [0.12, 0.04, 0.25];
const HAIR_L: Rgb = [1.0, 0.15, 0.7]; // hot pink
const HAIR_R: Rgb = [0.1, 0.9, 1.0]; // cyan
const FACE: Rgb = [0.98, 0.96, 0.94];
const NOSE: Rgb = [0.95, 0.1, 0.15];
const DARK: Rgb = [0.1, 0.05, 0.15];
const GREEN: Rgb = [0.35, 0.9, 0.35];

fn circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Colour at a point of the unit square, or None if transparent (outside the rounded tile).
fn shade(x: f32, y: f32) -> Option<Rgb> {
    let (qx, qy) = ((x - 0.5).abs() - 0.32, (y - 0.5).abs() - 0.32);
    if qx.max(0.0).hypot(qy.max(0.0)) > 0.18 {
        return None;
    }
    let mut c = BG;
    // Big poofy hair on both sides, plus a green mohawk tuft on top.
    if circle(x, y, 0.2, 0.46, 0.18) {
        c = HAIR_L;
    }
    if circle(x, y, 0.8, 0.46, 0.18) {
        c = HAIR_R;
    }
    if circle(x, y, 0.5, 0.2, 0.13) {
        c = GREEN;
    }
    // Face.
    if circle(x, y, 0.5, 0.56, 0.31) {
        c = FACE;
    }
    // Eyes with diamond-ish dark rings.
    for ex in [0.37, 0.63] {
        if circle(x, y, ex, 0.47, 0.065) {
            c = DARK;
        }
        if circle(x, y, ex, 0.47, 0.03) {
            c = FACE;
        }
    }
    // Smile: lower arc of a ring.
    let d = ((x - 0.5).powi(2) + (y - 0.6).powi(2)).sqrt();
    if (0.2..=0.245).contains(&d) && y > 0.68 {
        c = NOSE;
    }
    // Nose with a little shine.
    if circle(x, y, 0.5, 0.58, 0.085) {
        c = NOSE;
    }
    if circle(x, y, 0.47, 0.55, 0.022) {
        c = [1.0, 0.7, 0.7];
    }
    Some(c)
}

/// RGBA bytes, `size` x `size`.
pub fn render(size: usize) -> Vec<u8> {
    const SS: usize = 4;
    let mut out = Vec::with_capacity(size * size * 4);
    for py in 0..size {
        for px in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if let Some(c) = shade(x, y) {
                        r += c[0];
                        g += c[1];
                        b += c[2];
                        a += 1.0;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let (cr, cg, cb) = if a > 0.0 { (r / a, g / a, b / a) } else { (0.0, 0.0, 0.0) };
            for v in [cr, cg, cb, a / n] {
                out.push((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_transparent_corners() {
        for n in [16usize, 32, 64] {
            let px = render(n);
            assert_eq!(px.len(), n * n * 4);
            assert!(px[3] < 64, "corner is (nearly) transparent");
            let mid = (n / 2 * n + n / 2) * 4;
            assert_eq!(px[mid + 3], 255, "centre is opaque");
        }
    }

    /// `ICON_DUMP=/path/prefix cargo test --release dump -- --ignored` writes raw RGBA for a preview.
    #[test]
    #[ignore]
    fn dump() {
        let prefix = std::env::var("ICON_DUMP").unwrap();
        for n in [16usize, 32, 64, 256] {
            std::fs::write(format!("{prefix}_{n}.rgba"), render(n)).unwrap();
        }
    }
}
