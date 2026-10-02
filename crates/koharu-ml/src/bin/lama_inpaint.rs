use std::{
    collections::HashSet,
    hash::{Hash, Hasher as _},
    path::PathBuf,
};

use anyhow::Result;
use clap::Parser;
use image::GrayImage;
use koharu_ml::lama::{InpaintRequest, LaMa};

#[derive(Debug, Parser)]
#[command(about = "Run LaMa inpainting, optionally repeatedly to probe run-to-run drift")]
struct Cli {
    /// Image to inpaint (PNG/JPG).
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    /// Mask of the holes to fill (white = inpaint, black = keep); generated
    /// as a centered box when omitted.
    #[arg(long, value_name = "FILE")]
    mask: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    /// Run the inference N times in this process and report how many distinct
    /// outputs appear — measures the LaMa stack's run-to-run drift, the last
    /// suspect of the batch pipeline's PNG residue.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let image = image::open(cli.input)?;
    let mask = match &cli.mask {
        Some(path) => image::open(path)?.to_luma8(),
        None => {
            let (width, height) = (image.width(), image.height());
            let mut mask = GrayImage::new(width, height);
            let (x0, y0) = (width / 4, height / 4);
            let (x1, y1) = (width * 3 / 4, height * 3 / 4);
            for y in y0..y1 {
                for x in x0..x1 {
                    mask.put_pixel(x, y, image::Luma([255u8]));
                }
            }
            mask
        }
    };

    koharu_ml::init().await?;
    let model = LaMa::load(koharu_ml::device(cli.cpu)).await?;
    let config = InpaintRequest::default();
    let mut hashes = HashSet::new();
    let mut first: Option<Vec<u8>> = None;
    for run in 0..cli.repeat {
        let output = model.inference(&image, &mask, &config)?;
        let raw = output.into_raw();
        let mut hasher = std::hash::DefaultHasher::new();
        raw.hash(&mut hasher);
        let hash = hasher.finish();
        hashes.insert(hash);
        if cli.repeat > 1 {
            eprintln!("run {run}: {hash:016x}");
        }
        if first.is_none() {
            first = Some(raw);
        }
    }
    if cli.repeat == 1 {
        image::save_buffer(
            "lama_out.png",
            first.as_deref().unwrap_or_default(),
            image.width(),
            image.height(),
            image::ColorType::Rgb8,
        )?;
        println!("lama_out.png");
    } else {
        eprintln!("distincts: {}/{}", hashes.len(), cli.repeat);
    }
    Ok(())
}
