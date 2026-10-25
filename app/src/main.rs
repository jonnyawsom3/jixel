#![forbid(unsafe_code)]

use clap::{Parser, ValueEnum};
use image::DynamicImage;
use jixel::{
    Arbiter, CodingTransform, ColorEncoding, ColorSpace, DecodingSpeed, EncodeConfig, FlMeta,
    Grain, GroupOrder, JpegTranscodeConfig, LossyModular, Orientation, Speed, distance_from_quality,
    encode_fast_lossless, encode_fast_lossless_u16, encode_image, encode_image_10bit,
    encode_image_12bit, encode_image_16bit, encode_image_with_alpha,
    encode_image_with_alpha_10bit, encode_image_with_alpha_12bit,
    encode_image_with_alpha_16bit, encode_jpeg_lossless_with_config,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(
    name = "jixel",
    version,
    about = "JPEG XL Encoder CLI powered by Jixel",
    long_about = None,
    arg_required_else_help = true
)]
struct Args {
    /// Input image (PNG, PPM, JPEG, WebP, TIFF, BMP, QOI, EXR, HDR, etc.).
    input: PathBuf,

    /// Output JPEG XL file (.jxl).
    output: PathBuf,

    /// Target quality (0..100). Higher is better. Overrides --distance.
    #[arg(short = 'q', long, value_name = "QUALITY")]
    quality: Option<f32>,

    /// Butteraugli distance (0 = visual-lossless range, 1 = default lossy quality; higher is lower quality).
    #[arg(short = 'd', long, value_name = "DISTANCE")]
    distance: Option<f32>,

    /// Use lossless Modular coding (exact by default; see --near-lossless).
    #[arg(short = 'l', long)]
    lossless: bool,

    /// Lossy encoding arm: off, auto, or force.
    #[arg(long, value_enum)]
    lossy_modular: Option<CliLossyModular>,

    /// Encoder speed preset (default: fast).
    #[arg(short = 's', long, value_enum)]
    speed: Option<CliSpeed>,

    /// Lossless decoding speed / density tradeoff (default: slow).
    #[arg(long, value_enum)]
    decoding_speed: Option<CliDecodingSpeed>,

    /// Enable progressive encoding (2-pass lossy or Squeeze lossless).
    #[arg(short = 'p', long)]
    progressive: bool,

    /// Enable repeated-region patch detection; takes precedence over the speed preset.
    #[arg(long, conflicts_with = "no_patches")]
    patches: bool,

    /// Disable repeated-region patch detection; takes precedence over the speed preset.
    #[arg(long, conflicts_with = "patches")]
    no_patches: bool,

    /// Number of worker threads (defaults to the available CPU core count).
    #[arg(short = 'j', long, value_name = "NUM_THREADS")]
    num_threads: Option<usize>,

    /// Input sample bit depth: 8, 10, 12, or 16 (default 8).
    #[arg(short = 'b', long, value_name = "BIT_DEPTH")]
    bit_depth: Option<u8>,

    /// Maximum per-channel error for near-lossless encoding (0 is exact); requires --lossless.
    #[arg(long, value_name = "MAX_ERROR")]
    near_lossless: Option<u16>,

    /// Encode integer-upscaled pixel art using decoder-side nearest-neighbor scaling; requires --lossless.
    #[arg(long)]
    upsampling: bool,

    /// Add synthesized photon grain using the sensor ISO model (e.g. 3200); lossy VarDCT only.
    #[arg(long, value_name = "ISO", conflicts_with_all = ["measured_grain", "grain_lut"])]
    photon_noise_iso: Option<f32>,

    /// Replace measured source grain with synthesized grain at this multiplier (e.g. 1.0); lossy VarDCT only.
    #[arg(long, value_name = "MULTIPLIER", conflicts_with_all = ["photon_noise_iso", "grain_lut"])]
    measured_grain: Option<f32>,

    /// Explicit grain LUT: eight comma-separated strengths in 0..1, one per intensity point.
    #[arg(long, value_name = "V0,V1,V2,V3,V4,V5,V6,V7", conflicts_with_all = ["photon_noise_iso", "measured_grain"])]
    grain_lut: Option<String>,

    /// Number of encoding iterations for timing; only the final result is written.
    #[arg(short = 'i', long, default_value_t = 1, value_name = "ITERATIONS")]
    iterations: u32,

    /// EXIF orientation (1=Normal, 2=FlipH, 3=Rotate180, 4=FlipV, 5=Transpose, 6=Rotate90, 7=Transverse, 8=Rotate270).
    #[arg(short = 'o', long, value_name = "ORIENTATION")]
    orientation: Option<u8>,

    /// Color encoding preset (default: srgb).
    #[arg(long, value_enum)]
    color_space: Option<CliColorSpace>,

    /// Number of progressive lossy passes (1..4); overrides the default pass count.
    #[arg(long, value_name = "PROGRESSIVE_PASSES")]
    progressive_passes: Option<u32>,

    /// Custom progressive pass shift schedule, comma-separated (e.g. "2,1,0"); overrides --progressive-passes.
    #[arg(long, value_name = "PROGRESSIVE_SHIFTS")]
    progressive_shifts: Option<String>,

    /// Tone-mapping peak display luminance in nits (e.g. 10000 for HDR10).
    #[arg(long, value_name = "INTENSITY_TARGET")]
    intensity_target: Option<f32>,

    /// Tone-mapping minimum display luminance in nits (default: 0).
    #[arg(long, value_name = "MIN_NITS")]
    min_nits: Option<f32>,

    /// Set ToneMapping relative-to-max-display.
    #[arg(long)]
    relative_to_max_display: bool,

    /// Tone-mapping linear transfer threshold (nits, or 0..1 when relative; default: 0).
    #[arg(long, value_name = "LINEAR_BELOW")]
    linear_below: Option<f32>,

    /// Detect thin curvilinear structures as JPEG XL splines (requires the library's splines feature).
    #[arg(long, conflicts_with = "no_splines")]
    splines: bool,

    /// Disable spline coding.
    #[arg(long, conflicts_with = "splines")]
    no_splines: bool,

    /// Detect isolated bright/dark spots using the library's dot coding.
    #[arg(long, conflicts_with = "no_dots")]
    dots: bool,

    /// Disable dot coding.
    #[arg(long, conflicts_with = "dots")]
    no_dots: bool,

    /// Alpha-channel Butteraugli distance (0 keeps alpha lossless; 0..25).
    #[arg(long, value_name = "ALPHA_DISTANCE")]
    alpha_distance: Option<f32>,

    /// Explicitly enable invisible-color preprocessing (enabled by default).
    #[arg(long, conflicts_with = "no_fill_invisible")]
    fill_invisible: bool,

    /// Disable invisible-color preprocessing under fully transparent pixels.
    #[arg(long, conflicts_with = "fill_invisible")]
    no_fill_invisible: bool,

    /// Enable the experimental ink-layer coding path; takes precedence over speed defaults.
    #[arg(long, conflicts_with = "no_ink_layer")]
    ink_layer: bool,

    /// Disable ink-layer coding, including speed-preset defaults.
    #[arg(long, conflicts_with = "ink_layer")]
    no_ink_layer: bool,

    /// Enable experimental vector-layer coding; takes precedence over speed defaults.
    #[arg(long, conflicts_with = "no_vector_layers")]
    vector_layers: bool,

    /// Disable vector-layer coding, including speed-preset defaults.
    #[arg(long, conflicts_with = "vector_layers")]
    no_vector_layers: bool,

    /// Enable progressive DC preview frame.
    #[arg(long)]
    progressive_dc: bool,

    /// Number of finest Squeeze levels for the progressive DC frame; implies --progressive-dc.
    #[arg(long, value_name = "LEVELS")]
    progressive_dc_levels: Option<u32>,

    /// Progressive group order: scanline, center-first, or diagonals (default: scanline).
    #[arg(long, value_enum)]
    group_order: Option<CliGroupOrder>,

    /// Explicitly enable learned-rate transform selection (enabled by default).
    #[arg(long, conflicts_with = "no_learned_rate")]
    learned_rate: bool,

    /// Disable learned-rate transform selection.
    #[arg(long, conflicts_with = "learned_rate")]
    no_learned_rate: bool,

    /// Lossy perceptual metric: ssimulacra2 or cvvdp (default: ssimulacra2).
    #[arg(long, value_enum)]
    arbiter: Option<CliArbiter>,

    /// Lossy coding transform: xyb, ycbcr, or rgb (default: xyb).
    #[arg(long, value_enum)]
    coding_transform: Option<CliCodingTransform>,

    /// Path to binary ICC profile file to embed.
    #[arg(long, value_name = "ICC")]
    icc: Option<PathBuf>,

    /// Path to raw EXIF/TIFF metadata file to embed (without the Exif prefix).
    #[arg(long, value_name = "EXIF")]
    exif: Option<PathBuf>,

    /// Path to XMP metadata XML file to embed.
    #[arg(long, value_name = "XMP")]
    xmp: Option<PathBuf>,

    /// Use the library's ultra-fast lossless encoder.
    #[arg(long, conflicts_with = "jpeg_transcode")]
    fast_lossless: bool,

    /// Losslessly transcode a JPEG without decoding its DCT coefficients.
    #[arg(long, conflicts_with = "fast_lossless")]
    jpeg_transcode: bool,

    /// Do not store JPEG reconstruction data when using --jpeg-transcode.
    #[arg(long)]
    no_jpeg_reconstruction: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliSpeed {
    #[value(name = "ultra-fast")]
    UltraFast,
    Fastest,
    Fast,
    Medium,
    #[value(name = "medium-slow")]
    MediumSlow,
    Slow,
    #[value(name = "extra-slow")]
    ExtraSlow,
}

impl From<CliSpeed> for Speed {
    fn from(value: CliSpeed) -> Self {
        match value {
            CliSpeed::UltraFast => Speed::UltraFast,
            CliSpeed::Fastest => Speed::Fastest,
            CliSpeed::Fast => Speed::Fast,
            CliSpeed::Medium => Speed::Medium,
            CliSpeed::MediumSlow => Speed::MediumSlow,
            CliSpeed::Slow => Speed::Slow,
            CliSpeed::ExtraSlow => Speed::ExtraSlow,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliDecodingSpeed {
    Fastest,
    Fast,
    Slow,
}

impl From<CliDecodingSpeed> for DecodingSpeed {
    fn from(value: CliDecodingSpeed) -> Self {
        match value {
            CliDecodingSpeed::Fastest => DecodingSpeed::Fastest,
            CliDecodingSpeed::Fast => DecodingSpeed::Fast,
            CliDecodingSpeed::Slow => DecodingSpeed::Slow,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliLossyModular {
    Off,
    Auto,
    Force,
}

impl From<CliLossyModular> for LossyModular {
    fn from(value: CliLossyModular) -> Self {
        match value {
            CliLossyModular::Off => LossyModular::Off,
            CliLossyModular::Auto => LossyModular::Auto,
            CliLossyModular::Force => LossyModular::Force,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliColorSpace {
    Srgb,
    #[value(name = "display-p3")]
    DisplayP3,
    #[value(name = "rec2020-pq")]
    Rec2020Pq,
    #[value(name = "rec2020-hlg")]
    Rec2020Hlg,
    Linear,
}

impl CliColorSpace {
    fn encoding(self) -> ColorEncoding {
        match self {
            Self::Srgb => ColorEncoding::srgb(),
            Self::DisplayP3 => ColorEncoding::display_p3(),
            Self::Rec2020Pq => ColorEncoding::bt2020_pq(),
            Self::Rec2020Hlg => ColorEncoding::bt2020_hlg(),
            Self::Linear => ColorEncoding::srgb_linear(),
        }
    }

    fn fast_lossless_meta(self) -> FlMeta {
        match self {
            Self::Srgb => FlMeta::srgb(),
            Self::DisplayP3 => FlMeta::display_p3(),
            Self::Rec2020Pq => FlMeta::rec2100_pq(),
            Self::Rec2020Hlg => FlMeta::rec2100_hlg(),
            Self::Linear => FlMeta::linear(),
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliGroupOrder {
    Scanline,
    #[value(name = "center-first")]
    CenterFirst,
    Diagonals,
}

impl From<CliGroupOrder> for GroupOrder {
    fn from(value: CliGroupOrder) -> Self {
        match value {
            CliGroupOrder::Scanline => GroupOrder::Scanline,
            CliGroupOrder::CenterFirst => GroupOrder::CenterFirst,
            CliGroupOrder::Diagonals => GroupOrder::Diagonals,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliArbiter {
    #[value(name = "ssimulacra2")]
    Ssimulacra2,
    Cvvdp,
}

impl From<CliArbiter> for Arbiter {
    fn from(value: CliArbiter) -> Self {
        match value {
            CliArbiter::Ssimulacra2 => Arbiter::Ssimulacra2,
            CliArbiter::Cvvdp => Arbiter::Cvvdp,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum CliCodingTransform {
    Xyb,
    #[value(name = "ycbcr")]
    Ycbcr,
    Rgb,
}

impl From<CliCodingTransform> for CodingTransform {
    fn from(value: CliCodingTransform) -> Self {
        match value {
            CliCodingTransform::Xyb => CodingTransform::Xyb,
            CliCodingTransform::Ycbcr => CodingTransform::YCbCr,
            CliCodingTransform::Rgb => CodingTransform::Rgb,
        }
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let grain = grain_from_args(&args)?;
    validate_args(&args, grain)?;

    let orientation = Orientation::from_exif(args.orientation.unwrap_or(1))
        .ok_or_else(|| "--orientation must be an integer from 1 to 8".to_string())?;
    let threads = args.num_threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|available| available.get())
            .unwrap_or(1)
    });
    if threads == 0 {
        return Err("--num-threads must be at least 1".into());
    }

    let icc = read_optional_file(args.icc.as_deref(), "ICC profile")?;
    let exif = read_optional_file(args.exif.as_deref(), "EXIF metadata")?;
    let xmp = read_optional_file(args.xmp.as_deref(), "XMP metadata")?;
    let speed: Speed = args.speed.unwrap_or(CliSpeed::Fast).into();
    let decoding_speed: DecodingSpeed = args
        .decoding_speed
        .unwrap_or(CliDecodingSpeed::Slow)
        .into();
    let color_space = args.color_space.unwrap_or(CliColorSpace::Srgb);
    let lossy_modular = args.lossy_modular.unwrap_or(CliLossyModular::Off);
    let arbiter: Arbiter = args.arbiter.unwrap_or(CliArbiter::Ssimulacra2).into();
    let group_order: GroupOrder = args.group_order.unwrap_or(CliGroupOrder::Scanline).into();
    let coding_transform: CodingTransform = args
        .coding_transform
        .unwrap_or(CliCodingTransform::Xyb)
        .into();

    let mut config = EncodeConfig::default()
        .with_speed(speed)
        .with_num_threads(threads)
        .with_decoding_speed(decoding_speed)
        .with_color_encoding(color_space.encoding())
        .with_orientation(orientation)
        .with_lossless(args.lossless)
        .with_near_lossless(args.near_lossless.unwrap_or(0))
        .with_upsampling(args.upsampling)
        .with_grain(grain)
        .with_lossy_modular(lossy_modular.into())
        .with_min_nits(args.min_nits.unwrap_or(0.0))
        .with_relative_to_max_display(args.relative_to_max_display)
        .with_linear_below(args.linear_below.unwrap_or(0.0))
        .with_coding_transform(coding_transform)
        .with_arbiter(arbiter)
        .with_group_order(group_order)
        .with_learned_rate(!args.no_learned_rate || args.learned_rate)
        .with_fill_invisible(!args.no_fill_invisible || args.fill_invisible)
        .with_splines(args.splines && !args.no_splines)
        .with_dots(args.dots && !args.no_dots)
        .with_progressive_dc(args.progressive_dc || args.progressive_dc_levels.is_some());

    if args.ink_layer {
        config = config.with_ink_layer(true);
    } else if args.no_ink_layer {
        config = config.with_ink_layer(false);
    }
    if args.vector_layers {
        config = config.with_vector_layers(true);
    } else if args.no_vector_layers {
        config = config.with_vector_layers(false);
    }

    if args.patches {
        config = config.with_patches(true);
    } else if args.no_patches {
        config = config.with_patches(false);
    }

    if let Some(quality) = args.quality {
        config = config.with_distance(distance_from_quality(quality));
    } else if let Some(distance) = args.distance {
        config = config.with_distance(if distance == 0.0 { 0.05 } else { distance });
    }
    if let Some(target) = args.intensity_target {
        config = config.with_intensity_target(target);
    }
    if let Some(distance) = args.alpha_distance {
        config = config.with_alpha_distance(distance);
    }
    if let Some(levels) = args.progressive_dc_levels {
        config = config.with_progressive_dc_levels(levels);
    }
    if let Some(profile) = &icc {
        config = config.with_icc_profile(profile.clone());
    }
    if let Some(metadata) = &exif {
        config = config.with_exif(metadata.clone());
    }
    if let Some(metadata) = &xmp {
        config = config.with_xmp(metadata.clone());
    }

    let shifts = parse_progressive_shifts(args.progressive_shifts.as_deref())?;
    config.progressive_passes = args.progressive_passes;
    config.progressive_shifts = shifts;
    if args.progressive || args.progressive_passes.is_some() || args.progressive_shifts.is_some() {
        config = config.with_progressive(true);
    }

    let mut encoded = Vec::new();
    let mut last_encode_seconds = 0.0_f64;

    if args.jpeg_transcode {
        if args.bit_depth.is_some()
            || args.near_lossless.is_some()
            || args.upsampling
            || grain_requested(&args)
            || args.quality.is_some()
            || args.distance.is_some()
            || args.fast_lossless
            || args.progressive
            || args.progressive_passes.is_some()
            || args.progressive_shifts.is_some()
            || args.patches
            || args.no_patches
            || args.intensity_target.is_some()
            || args.min_nits.is_some()
            || args.relative_to_max_display
            || args.linear_below.is_some()
            || args.splines
            || args.no_splines
            || args.dots
            || args.no_dots
            || args.alpha_distance.is_some()
            || args.fill_invisible
            || args.no_fill_invisible
            || args.ink_layer
            || args.no_ink_layer
            || args.vector_layers
            || args.no_vector_layers
            || args.progressive_dc
            || args.progressive_dc_levels.is_some()
            || args.group_order.is_some()
            || args.learned_rate
            || args.no_learned_rate
            || args.arbiter.is_some()
            || args.coding_transform.is_some()
            || args.color_space.is_some()
            || args.decoding_speed.is_some()
            || args.lossy_modular.is_some()
        {
            return Err("one or more image-encoding options cannot be used with --jpeg-transcode; use only --speed, --num-threads, and --no-jpeg-reconstruction for JPEG transcoding".into());
        }
        if args.icc.is_some() || args.exif.is_some() || args.xmp.is_some() || args.orientation.is_some() {
            return Err("--icc, --exif, --xmp, and --orientation are taken from the JPEG itself and cannot be overridden with --jpeg-transcode".into());
        }
        let jpeg = fs::read(&args.input)
            .map_err(|error| format!("failed to read input '{}': {error}", args.input.display()))?;
        for iteration in 1..=args.iterations {
            let iteration_start = Instant::now();
            let jpeg_config = JpegTranscodeConfig::default()
                .with_speed(speed)
                .with_num_threads(threads)
                .with_jpeg_reconstruction(!args.no_jpeg_reconstruction);
            encoded = encode_jpeg_lossless_with_config(&jpeg, &jpeg_config)
                .map_err(|error| format!("JPEG transcode failed: {error}"))?;
            last_encode_seconds = iteration_start.elapsed().as_secs_f64();
            print_iteration(iteration, args.iterations, last_encode_seconds);
        }
    } else {
        if args.no_jpeg_reconstruction {
            return Err("--no-jpeg-reconstruction requires --jpeg-transcode".into());
        }
        if args.fast_lossless
            && (args.near_lossless.is_some()
                || args.upsampling
                || grain_requested(&args)
                || args.speed.is_some()
                || args.decoding_speed.is_some()
                || args.num_threads.is_some()
                || args.quality.is_some()
                || args.distance.is_some()
                || args.progressive
                || args.progressive_passes.is_some()
                || args.progressive_shifts.is_some()
                || args.patches
                || args.no_patches
                || args.intensity_target.is_some()
                || args.min_nits.is_some()
                || args.relative_to_max_display
                || args.linear_below.is_some()
                || args.splines
                || args.no_splines
                || args.dots
                || args.no_dots
                || args.alpha_distance.is_some()
                || args.ink_layer
                || args.no_ink_layer
                || args.vector_layers
                || args.no_vector_layers
                || args.progressive_dc
                || args.progressive_dc_levels.is_some()
                || args.group_order.is_some()
                || args.learned_rate
                || args.no_learned_rate
                || args.arbiter.is_some()
                || args.coding_transform.is_some()
                || args.lossy_modular.is_some()
                || args.fill_invisible
                || args.no_fill_invisible
            )
        {
            return Err("--fast-lossless uses a separate library API that does not accept speed, decoding-speed, thread-count, or lossy-only controls; remove those options. --bit-depth, --color-space, --orientation, and metadata options remain supported".into());
        }

        let image = image::open(&args.input).map_err(|error| {
            format!(
                "failed to open input image '{}': {error}",
                args.input.display()
            )
        })?;
        let has_alpha = image.color().has_alpha();

        if args.fast_lossless {
            let mut meta = color_space.fast_lossless_meta();
            meta.orientation = orientation;
            meta.icc = icc.clone();
            meta.exif = exif.clone();
            meta.xmp = xmp.clone();
            for iteration in 1..=args.iterations {
                let iteration_start = Instant::now();
                encoded = encode_fast_lossless_input(
                    &image,
                    has_alpha,
                    args.bit_depth.unwrap_or(8),
                    &meta,
                )?;
                last_encode_seconds = iteration_start.elapsed().as_secs_f64();
                print_iteration(iteration, args.iterations, last_encode_seconds);
            }
        } else {
            for iteration in 1..=args.iterations {
                let iteration_start = Instant::now();
                encoded = encode_image_input(
                    &image,
                    has_alpha,
                    args.bit_depth.unwrap_or(8),
                    &config,
                )?;
                last_encode_seconds = iteration_start.elapsed().as_secs_f64();
                print_iteration(iteration, args.iterations, last_encode_seconds);
            }
        }
    }

    // Read dimensions before writing so INPUT and OUTPUT may safely be the
    // same path when the decoder has already consumed the source image.
    let (width, height) = image::image_dimensions(&args.input).map_err(|error| {
        format!(
            "failed to read image dimensions from '{}': {error}",
            args.input.display()
        )
    })?;

    fs::write(&args.output, &encoded).map_err(|error| {
        format!(
            "failed to write output '{}': {error}",
            args.output.display()
        )
    })?;

    print_cjxl_summary(
        &args,
        &encoded,
        width as usize,
        height as usize,
        last_encode_seconds,
        if args.fast_lossless { 1 } else { threads },
        speed,
    );
    Ok(())
}

fn grain_requested(args: &Args) -> bool {
    args.photon_noise_iso.is_some() || args.measured_grain.is_some() || args.grain_lut.is_some()
}

fn grain_from_args(args: &Args) -> Result<Grain, String> {
    if let Some(iso) = args.photon_noise_iso {
        if !iso.is_finite() || iso <= 0.0 {
            return Err("--photon-noise-iso must be a finite number greater than 0".into());
        }
        return Ok(Grain::PhotonIso(iso));
    }
    if let Some(multiplier) = args.measured_grain {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err("--measured-grain must be a finite number greater than 0".into());
        }
        return Ok(Grain::Measured(multiplier));
    }
    if let Some(value) = args.grain_lut.as_deref() {
        let values: Result<Vec<f32>, _> = value.split(',').map(str::parse::<f32>).collect();
        let values = values.map_err(|_| "--grain-lut must contain eight comma-separated numbers".to_string())?;
        if values.len() != 8 || values.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("--grain-lut must contain exactly eight finite values in the range 0..1".into());
        }
        let mut lut = [0.0_f32; 8];
        lut.copy_from_slice(&values);
        return Ok(Grain::Lut(lut));
    }
    Ok(Grain::Off)
}

fn validate_args(args: &Args, grain: Grain) -> Result<(), String> {
    if let Some(quality) = args.quality {
        if !quality.is_finite() || !(0.0..=100.0).contains(&quality) {
            return Err("--quality must be a finite number between 0 and 100".into());
        }
        if args.distance.is_some() {
            eprintln!("note: --quality overrides --distance");
        }
    }
    if let Some(distance) = args.distance {
        if !distance.is_finite() || distance < 0.0 {
            return Err("--distance must be a finite number greater than or equal to 0".into());
        }
    }
    if let Some(bit_depth) = args.bit_depth {
        if !matches!(bit_depth, 8 | 10 | 12 | 16) {
            return Err("--bit-depth must be one of 8, 10, 12, or 16".into());
        }
    }
    if args.iterations == 0 {
        return Err("--iterations must be at least 1".into());
    }
    if let Some(passes) = args.progressive_passes {
        if !(1..=4).contains(&passes) {
            return Err("--progressive-passes must be between 1 and 4".into());
        }
    }
    if let Some(distance) = args.alpha_distance {
        if !distance.is_finite() || !(0.0..=25.0).contains(&distance) {
            return Err("--alpha-distance must be a finite number from 0 to 25".into());
        }
    }
    if let Some(levels) = args.progressive_dc_levels {
        if levels == 0 {
            return Err("--progressive-dc-levels must be at least 1".into());
        }
    }
    if let Some(target) = args.intensity_target {
        if !target.is_finite() || target <= 0.0 {
            return Err("--intensity-target must be a finite number greater than 0".into());
        }
        if args.min_nits.unwrap_or(0.0) > target {
            return Err("--min-nits cannot be greater than --intensity-target".into());
        }
    }
    if let Some(value) = args.min_nits {
        if !value.is_finite() || value < 0.0 {
            return Err("--min-nits must be a finite number greater than or equal to 0".into());
        }
    }
    if let Some(value) = args.linear_below {
        if !value.is_finite() || value < 0.0 {
            return Err("--linear-below must be a finite number greater than or equal to 0".into());
        }
        if args.relative_to_max_display && value > 1.0 {
            return Err("--linear-below must be in the range 0..1 when --relative-to-max-display is set".into());
        }
    }
    if let Some(orientation) = args.orientation {
        if !(1..=8).contains(&orientation) {
            return Err("--orientation must be an integer from 1 to 8".into());
        }
    }
    if args.no_jpeg_reconstruction && !args.jpeg_transcode {
        return Err("--no-jpeg-reconstruction requires --jpeg-transcode".into());
    }
    if args.lossless && args.lossy_modular.is_some_and(|mode| mode != CliLossyModular::Off) {
        return Err("--lossy-modular is a lossy-only option and cannot be combined with --lossless".into());
    }
    let speed = args.speed.unwrap_or(CliSpeed::Fast);
    if args.lossy_modular == Some(CliLossyModular::Auto)
        && !matches!(speed, CliSpeed::Slow | CliSpeed::ExtraSlow)
    {
        return Err("--lossy-modular auto requires --speed slow or extra-slow".into());
    }
    if args.splines || args.dots {
        if !matches!(speed, CliSpeed::Slow | CliSpeed::ExtraSlow) {
            return Err("--splines and --dots require --speed slow or extra-slow".into());
        }
        if matches!(args.decoding_speed, Some(CliDecodingSpeed::Fastest | CliDecodingSpeed::Fast)) {
            return Err("--splines and --dots require --decoding-speed slow".into());
        }
    }
    if args.near_lossless.is_some() && !args.lossless {
        return Err("--near-lossless requires --lossless".into());
    }
    if args.upsampling && !args.lossless {
        return Err("--upsampling requires --lossless".into());
    }
    if args.upsampling && (args.progressive || args.progressive_passes.is_some() || args.progressive_shifts.is_some()) {
        return Err("--upsampling cannot be combined with progressive encoding".into());
    }
    if !matches!(grain, Grain::Off) && args.lossless {
        return Err("grain synthesis is available only for lossy VarDCT encoding".into());
    }
    if !matches!(grain, Grain::Off)
        && args.lossy_modular.is_some_and(|mode| mode != CliLossyModular::Off)
    {
        return Err("grain synthesis cannot be combined with --lossy-modular auto or force".into());
    }
    if args.fast_lossless && (args.near_lossless.is_some() || args.upsampling || grain_requested(args)) {
        return Err("--near-lossless, --upsampling, and grain options are not supported by --fast-lossless".into());
    }
    if args.jpeg_transcode && (args.near_lossless.is_some() || args.upsampling || grain_requested(args)) {
        return Err("--near-lossless, --upsampling, and grain options cannot be used with --jpeg-transcode".into());
    }
    if args.jpeg_transcode && args.lossless {
        eprintln!("note: --jpeg-transcode is already lossless; --lossless is redundant");
    }
    if args.fast_lossless && args.lossless {
        eprintln!("note: --fast-lossless already uses a lossless encoder; --lossless is redundant");
    }
    Ok(())
}

fn print_iteration(iteration: u32, total: u32, seconds: f64) {
    if total > 1 {
        eprintln!("Iteration {iteration}/{total}: {:.1} ms", seconds * 1000.0);
    }
}

fn print_cjxl_summary(
    args: &Args,
    encoded: &[u8],
    width: usize,
    height: usize,
    encode_seconds: f64,
    threads: usize,
    speed: Speed,
) {
    let speed_name = speed_name(if args.fast_lossless { Speed::UltraFast } else { speed });
    let mode = if args.jpeg_transcode {
        format!("JPEG transcode, lossless, speed: {speed_name}")
    } else if args.fast_lossless {
        format!("Fast lossless, lossless, speed: {speed_name}")
    } else if args.lossless {
        format!("Modular, lossless, speed: {speed_name}")
    } else {
        let arm = match args.lossy_modular.unwrap_or(CliLossyModular::Off) {
            CliLossyModular::Off => "VarDCT",
            CliLossyModular::Auto => "VarDCT/Modular",
            CliLossyModular::Force => "Modular (where supported)",
        };
        let distance = args
            .quality
            .map(distance_from_quality)
            .or(args.distance.map(|value| if value == 0.0 { 0.05 } else { value }))
            .unwrap_or(1.0);
        format!("{arm}, d{distance:.3}, speed: {speed_name}")
    };
    println!("Encoding [{mode}]");

    let pixels = (width as f64) * (height as f64);
    let bpp = if pixels > 0.0 {
        (encoded.len() as f64 * 8.0) / pixels
    } else {
        0.0
    };
    if encoded.len() >= 1000 {
        println!("Compressed to {:.1} kB ({bpp:.3} bpp).", encoded.len() as f64 / 1000.0);
    } else {
        println!("Compressed to {} bytes ({bpp:.3} bpp).", encoded.len());
    }
    let mp_per_second = if encode_seconds > 0.0 {
        pixels / encode_seconds / 1_000_000.0
    } else {
        0.0
    };
    let thread_label = if threads == 1 { "thread" } else { "threads" };
    println!("{width} x {height}, {mp_per_second:.3} MP/s, {threads} {thread_label}.");
}

fn speed_name(speed: Speed) -> &'static str {
    match speed {
        Speed::UltraFast => "ultra-fast",
        Speed::Fastest => "fastest",
        Speed::Fast => "fast",
        Speed::Medium => "medium",
        Speed::MediumSlow => "medium-slow",
        Speed::Slow => "slow",
        Speed::ExtraSlow => "extra-slow",
        _ => "unknown",
    }
}

fn read_optional_file(path: Option<&Path>, label: &str) -> Result<Option<Vec<u8>>, String> {
    path.map(|path| {
        fs::read(path)
            .map_err(|error| format!("failed to read {label} '{}': {error}", path.display()))
    })
    .transpose()
}

fn parse_progressive_shifts(value: Option<&str>) -> Result<Option<Vec<u32>>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let shifts: Result<Vec<u32>, _> = value.split(',').map(str::parse::<u32>).collect();
    let shifts = shifts
        .map_err(|_| "--progressive-shifts must be comma-separated integers".to_string())?;
    if shifts.is_empty() || shifts.len() > 4 || shifts.iter().any(|shift| *shift > 3) {
        return Err("--progressive-shifts must contain 1 to 4 integers in the range 0..3".into());
    }
    if shifts.last() != Some(&0) {
        return Err("--progressive-shifts must end in 0".into());
    }
    if shifts.windows(2).any(|pair| pair[0] < pair[1]) {
        return Err("--progressive-shifts must be non-increasing".into());
    }
    Ok(Some(shifts))
}

fn encode_image_input(
    image: &DynamicImage,
    has_alpha: bool,
    bit_depth: u8,
    config: &EncodeConfig,
) -> Result<Vec<u8>, String> {
    if !matches!(bit_depth, 8 | 10 | 12 | 16) {
        return Err("bit depth must be 8, 10, 12, or 16".into());
    }

    let width = image.width() as usize;
    let height = image.height() as usize;
    let result = match bit_depth {
        8 => {
            if has_alpha {
                encode_image_with_alpha(image.to_rgba8().as_raw(), width, height, config)
            } else {
                encode_image(image.to_rgb8().as_raw(), width, height, config)
            }
        }
        10 | 12 | 16 => {
            if has_alpha {
                let mut rgba = image.to_rgba16().into_raw();
                scale_samples_to_bit_depth(&mut rgba, bit_depth);
                match bit_depth {
                    10 => encode_image_with_alpha_10bit(&rgba, width, height, config),
                    12 => encode_image_with_alpha_12bit(&rgba, width, height, config),
                    16 => encode_image_with_alpha_16bit(&rgba, width, height, config),
                    _ => unreachable!("bit depth was validated above"),
                }
            } else {
                let mut rgb = image.to_rgb16().into_raw();
                scale_samples_to_bit_depth(&mut rgb, bit_depth);
                match bit_depth {
                    10 => encode_image_10bit(&rgb, width, height, config),
                    12 => encode_image_12bit(&rgb, width, height, config),
                    16 => encode_image_16bit(&rgb, width, height, config),
                    _ => unreachable!("bit depth was validated above"),
                }
            }
        }
        _ => unreachable!("bit depth was validated above"),
    };
    result.map_err(|error| format!("encoding failed: {error}"))
}

fn encode_fast_lossless_input(
    image: &DynamicImage,
    has_alpha: bool,
    bit_depth: u8,
    meta: &FlMeta,
) -> Result<Vec<u8>, String> {
    if !matches!(bit_depth, 8 | 10 | 12 | 16) {
        return Err("bit depth must be 8, 10, 12, or 16".into());
    }

    let width = image.width() as usize;
    let height = image.height() as usize;
    let result = if bit_depth == 8 {
        if has_alpha {
            encode_fast_lossless(
                image.to_rgba8().as_raw(),
                width,
                height,
                ColorSpace::Rgb,
                true,
                meta,
            )
        } else {
            encode_fast_lossless(
                image.to_rgb8().as_raw(),
                width,
                height,
                ColorSpace::Rgb,
                false,
                meta,
            )
        }
    } else if has_alpha {
        let mut rgba = image.to_rgba16().into_raw();
        scale_samples_to_bit_depth(&mut rgba, bit_depth);
        encode_fast_lossless_u16(
            &rgba,
            width,
            height,
            ColorSpace::Rgb,
            true,
            u32::from(bit_depth),
            meta,
        )
    } else {
        let mut rgb = image.to_rgb16().into_raw();
        scale_samples_to_bit_depth(&mut rgb, bit_depth);
        encode_fast_lossless_u16(
            &rgb,
            width,
            height,
            ColorSpace::Rgb,
            false,
            u32::from(bit_depth),
            meta,
        )
    };
    result.map_err(|error| format!("fast lossless encoding failed: {error}"))
}

fn scale_samples_to_bit_depth(samples: &mut [u16], bit_depth: u8) {
    if bit_depth >= 16 {
        return;
    }
    let max_sample = (1_u32 << bit_depth) - 1;
    for sample in samples {
        *sample = (((u32::from(*sample) * max_sample) + 32_767) / 65_535) as u16;
    }
}
