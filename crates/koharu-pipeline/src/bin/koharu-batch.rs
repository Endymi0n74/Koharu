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
    let arguments = Arguments::parse();
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
