use std::{
    collections::HashSet,
    hash::{Hash, Hasher as _},
    path::PathBuf,
};

use anyhow::Result;
use clap::{Parser, ValueEnum};
use koharu_ml::paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask};

#[derive(Debug, Parser)]
#[command(about = "Run PaddleOCR-VL-1.6 element recognition")]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_enum, default_value_t = Task::Ocr)]
    task: Task,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    /// Run the inference N times in this process and report how many distinct
    /// outputs appear — separates per-process state variance from per-call
    /// kernel variance when hunting nondeterminism.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Task {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl From<Task> for PaddleOCRVLTask {
    fn from(value: Task) -> Self {
        match value {
            Task::Ocr => Self::Ocr,
            Task::Table => Self::Table,
            Task::Formula => Self::Formula,
            Task::Chart => Self::Chart,
            Task::Spotting => Self::Spotting,
            Task::Seal => Self::Seal,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();
    // The workspace variable must be inherited at process creation for
    // cuBLASLt to honor it, so this may transparently restart the binary
    // before any model work happens; the FFI knobs run after `init()`
    // because torch_cpu.dll is only loaded by the runtime itself.
    relaunch_with_cublas_workspace()?;
    let cli = Cli::parse();
    let image = image::open(cli.input)?;
    koharu_ml::init().await?;
    enforce_determinism()?;
    let device = koharu_ml::device(cli.cpu);
    let model = PaddleOCRVL::load(device).await?;
    let mut texts = Vec::new();
    let mut hashes = HashSet::new();
    for run in 0..cli.repeat {
        let result = model.inference(&image, cli.task.into())?;
        let mut hasher = std::hash::DefaultHasher::new();
        result.text.hash(&mut hasher);
        hashes.insert(hasher.finish());
        if cli.repeat > 1 {
            eprintln!("run {run}: {:016x}", hasher.finish());
        }
        texts.push(result.text);
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
/// Guarantees `CUBLAS_WORKSPACE_CONFIG=:4096:8` exists for fp32 mode by
/// relaunching the binary with it when it was not inherited.
///
/// With fp32 weights that variable pins cuBLASLt's per-shape algorithm
/// choice: without it every fp32 run's vision-encoder hashes drift, with it
/// 11 of 12 measured runs were bitwise-identical across all 118 probed
/// stages. Updating the environment from inside the process — via both
/// `std::env::set_var` (Win32 block) and UCRT's `_putenv` (`_environ` copy)
/// — is NOT observed by cuBLASLt's handle creation on this stack, so the
/// variable must be inherited at process creation; spawning ourselves with
/// it reproduces exactly that state, forwarding output and the exit code.
/// The rare residual run-to-run GEMM tie-break (~1 run in 12, starting at a
/// `L{i} attncore` hash) never changed the decoded text.
///
/// # Errors
/// When spawning or waiting on the child process fails.
fn relaunch_with_cublas_workspace() -> Result<()> {
    if std::env::var_os("KOHARU_TORCH_NO_BF16").is_none()
        || std::env::var_os("CUBLAS_WORKSPACE_CONFIG").is_some()
        || std::env::var_os("KOHARU_CUBLAS_RELAUNCHED").is_some()
    {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    command.env("CUBLAS_WORKSPACE_CONFIG", ":4096:8");
    command.env("KOHARU_CUBLAS_RELAUNCHED", "1");
    let status = command.status()?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Pins down the resolvable variance sources of PyTorch's greedy decoding.
///
/// The knobs live in [`koharu_ml::determinism`] so the batch binary applies
/// the same recipe; this binary additionally probes stage hashes through
/// `KOHARU_OCR_DEBUG`.
fn enforce_determinism() -> Result<()> {
    koharu_ml::determinism::enforce()
}
