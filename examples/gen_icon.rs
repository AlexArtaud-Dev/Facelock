//! Régénère assets/facelock.ico (16 à 256 px) et assets/logo.png depuis src/icon.rs.
//!   cargo run --example gen_icon
#[path = "../src/icon.rs"]
mod icon;

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::ExtendedColorType;

fn main() {
    let frames: Vec<IcoFrame> = [16u32, 20, 24, 32, 40, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            let px = icon::rgba(s as usize, icon::Variant::Active);
            IcoFrame::as_png(&px, s, s, ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let f = std::fs::File::create("assets/facelock.ico").unwrap();
    IcoEncoder::new(f).encode_images(&frames).unwrap();
    let big = icon::rgba(512, icon::Variant::Active);
    image::save_buffer("assets/logo.png", &big, 512, 512, ExtendedColorType::Rgba8).unwrap();
    println!("assets/facelock.ico + assets/logo.png générés");
}
