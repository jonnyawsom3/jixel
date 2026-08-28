```rust
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process;

use jixel::{ColorEncoding, EncodeConfig, Speed};

fn print_usage(program: &str) {
    eprintln!("Usage: {program} <input.png|input.jpg> <output.jxl>");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        print_usage(&args[0]);
        process::exit(2);
    }

    let input_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);

    // Decode PNG/JPEG/etc. using the image crate.
    let image = match image::open(input_path) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("Error: failed to open '{}': {error}", input_path.display());
            process::exit(1);
        }
    };

    // Convert the decoded image to 8-bit RGB.
    let rgb = image.to_rgb8();

    let width = rgb.width() as usize;
    let height = rgb.height() as usize;

    // Encode the image as JPEG XL.
    let jxl = match jixel::encode_image(
        &rgb,
        width,
        height,
        &EncodeConfig::default()
            .with_lossless(false)
            .with_quality(90.0)
            .with_speed(Speed::Slow)
            .with_progressive(false)
            .with_color_encoding(ColorEncoding::srgb()),
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Error: failed to encode '{}': {error}", input_path.display());
            process::exit(1);
        }
    };

    // Write the JPEG XL bitstream to the requested output path.
    if let Err(error) = fs::write(output_path, jxl) {
        eprintln!(
            "Error: failed to write '{}': {error}",
            output_path.display()
        );
        process::exit(1);
    }
}
```
