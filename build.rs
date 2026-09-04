use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/Intel_Graphics_logo.png");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut decoder = png::Decoder::new(png::io::Cursor::new(include_bytes!(
        "assets/Intel_Graphics_logo.png"
    )));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut encoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut encoded).unwrap();
    let source = &encoded[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => panic!("indexed PNG was not expanded"),
    };
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for pixel in source.chunks_exact(channels) {
        match channels {
            1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            4 => rgba.extend_from_slice(pixel),
            _ => unreachable!(),
        }
    }
    File::create(out_dir.join("intel_logo_rgba8.bin"))
        .unwrap()
        .write_all(&rgba)
        .unwrap();
    File::create(out_dir.join("intel_logo_meta.rs"))
        .unwrap()
        .write_all(
            format!(
                "const INTEL_LOGO_WIDTH: u32 = {};\nconst INTEL_LOGO_HEIGHT: u32 = {};\n",
                info.width, info.height
            )
            .as_bytes(),
        )
        .unwrap();
}
