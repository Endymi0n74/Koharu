//! `koharu-batch` translates a whole chapter — a folder of images or a CBZ —
//! in one command. The run itself lives in `koharu_pipeline::batch` so its
//! logic and its tests sit in the library; this binary owns only argument
//! parsing and the process exit, including the unsafe teardown below.

use anyhow::Result;
use clap::Parser;

use koharu_pipeline::batch::cli::Arguments;
use koharu_pipeline::batch::run;

/// Ends the process with `code`, skipping native teardown entirely.
///
/// After a full GPU run, process shutdown segfaults inside the CUDA driver
/// (`nvcuda64.dll` fault, observed in both debug and release) and would
/// replace the documented exit code with `0xC0000005` — turning a successful
/// chapter into a failure for any script checking the exit status. Every
/// output file is already written and stderr is unbuffered at the call
/// sites, so nothing depends on CRT destructors or DLL detach handlers;
/// terminating directly is what the kernel does after any such crash anyway.
fn exit_with(code: i32) -> ! {
    #[cfg(windows)]
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    // Only reachable if the kernel refused to terminate this process.
    std::process::exit(code)
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn TerminateProcess(process: *mut core::ffi::c_void, exit_code: u32) -> i32;
}

#[tokio::main]
async fn main() -> Result<()> {
    // The library reports through `tracing` (OCR cache statistics, retries,
    // model loads); nothing else installs a subscriber for this process, so
    // events would be dropped. They join the progress lines on stderr at INFO
    // level, tunable through RUST_LOG.
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    let arguments = Arguments::parse();
    // cuBLASLt only reads CUBLAS_WORKSPACE_CONFIG from the environment
    // inherited at process creation, so with --torch-fp32 the binary
    // transparently restarts itself with it when it was not inherited
    // (no-op when the variable is already set, for planning-only runs, or
    // after the first restart). KOHARU_TORCH_NO_BF16 needs no such dance:
    // `koharu_ml::backend::set_precision` reads it through `var_os`, i.e. by
    // Rust itself, so an in-process `set_var` is honored by every model
    // loader later in the run.
    relaunch_with_cublas_workspace(&arguments)?;
    if arguments.torch_fp32 {
        // Runs once, single-threaded, before any reader could observe it.
        unsafe {
            std::env::set_var("KOHARU_TORCH_NO_BF16", "1");
        }
        tracing::info!("--torch-fp32: KOHARU_TORCH_NO_BF16=1, Torch models will load in fp32");
    }
    match run::run(arguments).await {
        // Both paths terminate directly: the native teardown of a GPU run
        // would replace this code with 0xC0000005.
        Ok(code) => exit_with(code),
        Err(error) => {
            eprintln!("Error: {error:?}");
            exit_with(1);
        }
    }
}

/// Guarantees `CUBLAS_WORKSPACE_CONFIG=:4096:8` exists for `--torch-fp32` by
/// relaunching the binary with it when it was not inherited.
///
/// With fp32 weights that variable pins cuBLASLt's per-shape algorithm
/// choice; without it the heuristics tie-break differently per process,
/// moving detection boxes by a pixel and turning OCR-cache hits into misses
/// (measured on a replayed chapter: 25/25 hits with the variable inherited,
/// ~35% of misses without). cuBLASLt only honors a variable inherited at
/// process creation — updating it from inside (`std::env::set_var`, UCRT
/// `_putenv`) is invisible to handle creation — so spawning ourselves with
/// it reproduces exactly that state. This mirrors the mechanism already
/// proven in the `paddle_ocr_vl` dev binary. Runs before any model work,
/// forwards stdout/stderr and the exit code, and is a no-op when the
/// variable is already set, when `--torch-fp32` is off (fp32 is also
/// implied by non-planning runs only), or after the first restart
/// (`KOHARU_CUBLAS_RELAUNCHED` guard against an endless loop).
///
/// # Errors
/// When spawning or waiting on the child process fails.
fn relaunch_with_cublas_workspace(arguments: &Arguments) -> Result<()> {
    if !arguments.torch_fp32
        || std::env::var_os("CUBLAS_WORKSPACE_CONFIG").is_some()
        || std::env::var_os("KOHARU_CUBLAS_RELAUNCHED").is_some()
    {
        return Ok(());
    }
    tracing::info!(
        "--torch-fp32: restarting with CUBLAS_WORKSPACE_CONFIG=:4096:8 inherited (cuBLASLt only honors it at process creation)"
    );
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    command.env("CUBLAS_WORKSPACE_CONFIG", ":4096:8");
    command.env("KOHARU_CUBLAS_RELAUNCHED", "1");
    let status = command.status()?;
    exit_with(status.code().unwrap_or(1))
}
