use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use image::{GenericImageView, ImageFormat, RgbaImage};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let logo_bytes = include_bytes!("src/resources/logo.png");

    // Load and make white/near-white pixels transparent
    let img = image::load_from_memory(logo_bytes).expect("Failed to decode logo");
    let (w, h) = img.dimensions();
    let mut rgba = RgbaImage::new(w, h);
    for (x, y, pixel) in img.pixels() {
        let [r, g, b, a] = pixel.0;
        if r > 230 && g > 230 && b > 230 {
            rgba.put_pixel(x, y, image::Rgba([r, g, b, 0]));
        } else {
            rgba.put_pixel(x, y, image::Rgba([r, g, b, a]));
        }
    }

    // Encode back to PNG in memory
    let mut buf = Cursor::new(Vec::new());
    rgba.write_to(&mut buf, ImageFormat::Png).expect("Failed to encode PNG");

    let ansi = logo_art::image_to_ansi(buf.get_ref(), 20);
    fs::write(Path::new(&out_dir).join("logo.ansi"), ansi).unwrap();
    println!("cargo:rerun-if-changed=src/resources/logo.png");
}
