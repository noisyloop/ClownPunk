//! Windows only: embed the clown icon in the .exe (what Explorer shows).
//! The icon is drawn by `src/icon.rs`, the same code the window uses at runtime.
#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("clown.ico");
    std::fs::write(&out, make_ico(&[16, 32, 48, 64, 128])).unwrap();
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        res.compile().unwrap();
    }
}

/// A .ico with one 32-bit BMP image per size.
fn make_ico(sizes: &[usize]) -> Vec<u8> {
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &n in sizes {
        let rgba = icon::render(n);
        let mut img = Vec::new();
        // BITMAPINFOHEADER (height is doubled: XOR bitmap + AND mask).
        img.extend_from_slice(&40u32.to_le_bytes());
        img.extend_from_slice(&(n as i32).to_le_bytes());
        img.extend_from_slice(&((n * 2) as i32).to_le_bytes());
        img.extend_from_slice(&1u16.to_le_bytes());
        img.extend_from_slice(&32u16.to_le_bytes());
        img.extend_from_slice(&[0u8; 24]);
        // Pixels as BGRA, bottom row first.
        for y in (0..n).rev() {
            for x in 0..n {
                let p = (y * n + x) * 4;
                img.extend_from_slice(&[rgba[p + 2], rgba[p + 1], rgba[p], rgba[p + 3]]);
            }
        }
        // AND mask (all zero: alpha channel does the work), rows padded to 32 bits.
        img.extend(std::iter::repeat(0u8).take(n.div_ceil(32) * 4 * n));
        images.push(img);
    }
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&n, img) in sizes.iter().zip(&images) {
        ico.extend_from_slice(&[if n >= 256 { 0 } else { n as u8 }, if n >= 256 { 0 } else { n as u8 }, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        ico.extend(img);
    }
    ico
}
