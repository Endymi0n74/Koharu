use std::{
    collections::HashSet,
    hash::{Hash, Hasher as _},
    path::PathBuf,
};

use anyhow::{Result, bail};
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

/// Pins down the resolvable variance sources of PyTorch's greedy decoding.
///
/// The jitter root cause is cuBLAS bf16 matmuls: on Ampere+ their split-K
/// reductions accumulate in a nondeterministic order, and this PyTorch build
/// offers no deterministic path for them — both `setDeterministicAlgorithms(true)`
/// and `setAllowBF16ReductionCuBLAS(false)` abort in
/// `gemm_internal_cublas_bfloat16_helper` because banning the reduced-precision
/// split-K requires the cuBLASLt backend this build never routes to. The
/// practical fix is to run the model in fp32 via `KOHARU_TORCH_NO_BF16=1`
/// (see `koharu_ml::backend::set_precision`). The knobs below additionally
/// resolve the private C++ symbols of `at::globalContext()` directly from
/// `torch_cpu.dll`, needing no libtorch C++ headers or `cl.exe` toolchain:
///
/// - `setBenchmarkCuDNN(false)` stops cuDNN from autotuning between calls.
/// - `setAllowTF32CuBLAS/CuDNN(false)` keep fp32 matmuls exact.
#[cfg(windows)]
fn enforce_determinism() -> Result<()> {
    use libloading::os::windows::Library as OsLibrary;

    const CONTEXT: &str = "?globalContext@at@@YAAEAVContext@1@XZ";
    const SET_BENCHMARK: &str = "?setBenchmarkCuDNN@Context@at@@QEAAX_N@Z";
    const SET_TF32_CUBLAS: &str = "?setAllowTF32CuBLAS@Context@at@@QEAAX_N@Z";
    const SET_TF32_CUDNN: &str = "?setAllowTF32CuDNN@Context@at@@QEAAX_N@Z";

    /// Resolves an `at::Context` member from the already-loaded `torch_cpu.dll`.
    ///
    /// # Errors
    /// When `torch_cpu.dll` is not loaded or does not export the symbol.
    fn symbol<T: Copy>(name: &str) -> Result<T> {
        let library = OsLibrary::open_already_loaded("torch_cpu.dll")
            .map_err(|error| anyhow::anyhow!("torch_cpu.dll is not loaded: {error}"))?;
        let address = unsafe { library.get::<T>(name.as_bytes()) }
            .map_err(|error| anyhow::anyhow!("symbol {name} not found: {error}"))?;
        // The handle refers to a module pinned by `open_already_loaded`; leak
        // it so no `FreeLibrary` can ever touch the running torch runtime.
        std::mem::forget(library);
        Ok(*address)
    }

    let global_context: unsafe extern "system" fn() -> *mut std::ffi::c_void = symbol(CONTEXT)?;
    let context = unsafe { global_context() };
    if context.is_null() {
        bail!("at::globalContext() returned null");
    }
    type Setter1 = unsafe extern "system" fn(*mut std::ffi::c_void, bool);
    // `setDeterministicAlgorithms(true)` is intentionally NOT used: in bf16 it
    // aborts on the first matmul (see above), and with fp32 weights its
    // deterministic softmax needs more workspace than an 8 GiB card has left
    // once the fp32 model is resident (verified: CUDA OOM in SoftMax.cu).
    let set_benchmark: Setter1 = symbol(SET_BENCHMARK)?;
    unsafe { set_benchmark(context, false) };
    let set_tf32_cublas: Setter1 = symbol(SET_TF32_CUBLAS)?;
    unsafe { set_tf32_cublas(context, false) };
    let set_tf32_cudnn: Setter1 = symbol(SET_TF32_CUDNN)?;
    unsafe { set_tf32_cudnn(context, false) };
    eprintln!("OCRDBG determinism enforced");
    Ok(())
}

#[cfg(not(windows))]
fn enforce_determinism() -> Result<()> {
    Ok(())
}
