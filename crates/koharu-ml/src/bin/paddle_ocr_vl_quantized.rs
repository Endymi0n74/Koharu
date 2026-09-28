use std::{
    collections::HashSet,
    hash::{Hash, Hasher as _},
    path::PathBuf,
};

use anyhow::Result;
use clap::Parser;
use koharu_ml::paddle_ocr_vl::PaddleOCRVLTask;
use koharu_ml::paddle_ocr_vl_quantized::PaddleOCRVLQuantized;

#[derive(Debug, Parser)]
#[command(about = "Run quantized PaddleOCR-VL-1.6 (gguf) text recognition")]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    /// Run the inference N times in this process and report how many distinct
    /// outputs appear — measures the gguf stack's run-to-run drift on the same
    /// crop, mirroring the torch binary's probe.
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

    koharu_ml::init().await?;
    let model = PaddleOCRVLQuantized::load(koharu_ml::device(cli.cpu)).await?;
    let task = PaddleOCRVLTask::Ocr;
    let mut texts = Vec::new();
    let mut hashes = HashSet::new();
    for run in 0..cli.repeat {
        let text = model.inference(&image, task)?.text;
        let mut hasher = std::hash::DefaultHasher::new();
        text.hash(&mut hasher);
        hashes.insert(hasher.finish());
        if cli.repeat > 1 {
            eprintln!("run {run}: {:016x}", hasher.finish());
        }
        texts.push(text);
    }
    if cli.repeat == 1 {
        println!("{}", texts[0]);
    } else {
        eprintln!("distincts: {}/{}", hashes.len(), cli.repeat);
        if hashes.len() > 1 {
            for (run, text) in texts.iter().enumerate() {
                eprintln!("--- run {run} ---\n{text}");
            }
        }
    }
    Ok(())
}
