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
    // Read by `koharu_ml::backend::set_precision` through `var_os`, i.e. by
    // Rust itself — unlike torch's own `getenv`, so an in-process `set_var`
    // here is honored by every model loader later in the run.
    if arguments.torch_fp32 {
        // Runs once, single-threaded, before any reader could observe it.
        unsafe {
            std::env::set_var("KOHARU_TORCH_NO_BF16", "1");
        }
        // cuBLASLt picks its per-shape GEMM algorithm from heuristics that can
        // tie-break differently per process, which moves detection boxes by a
        // pixel and turns OCR-cache hits into misses. Only a variable inherited
        // at process creation is honored (an in-process `set_var`/`_putenv` is
        // invisible to handle creation), so the batch must be started with it —
        // warn loudly instead of silently degrading to a non-reproducible run.
        if std::env::var_os("CUBLAS_WORKSPACE_CONFIG").is_none() {
            tracing::warn!(
                "--torch-fp32 is only bitwise-reproducible with CUBLAS_WORKSPACE_CONFIG=:4096:8 set before starting koharu-batch; detection boxes may drift by a pixel between runs otherwise"
            );
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
