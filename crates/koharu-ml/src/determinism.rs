//! Torch determinism knobs shared by every binary that runs Torch models.
//!
//! The jitter root cause is cuBLAS bf16 matmuls: on Ampere+ their split-K
//! reductions accumulate in a nondeterministic order, and this PyTorch build
//! offers no deterministic path for them — both `setDeterministicAlgorithms(true)`
//! and `setAllowBF16ReductionCuBLAS(false)` abort in
//! `gemm_internal_cublas_bfloat16_helper` because banning the reduced-precision
//! split-K requires the cuBLASLt backend this build never routes to. The
//! practical fix is to run the models in fp32 via `KOHARU_TORCH_NO_BF16=1`
//! (see `crate::backend::set_precision`), and to pin the remaining knobs
//! through the private C++ symbols of `at::globalContext()` resolved directly
//! from `torch_cpu.dll` — no libtorch C++ headers or `cl.exe` toolchain
//! needed. The `CUBLAS_WORKSPACE_CONFIG` half of the recipe must be inherited
//! at process creation (cuBLASLt ignores later updates); binaries handle that
//! by relaunching themselves before torch is initialized.

use anyhow::{Result, bail};

/// Pins down the resolvable variance sources after torch is initialized.
///
/// - `setBenchmarkCuDNN(false)` stops cuDNN from autotuning between calls.
/// - `setAllowTF32CuBLAS/CuDNN(false)` keep fp32 matmuls exact.
///
/// `setDeterministicAlgorithms(true)` is intentionally NOT used: in bf16 it
/// aborts on the first matmul (see the module documentation), and with fp32
/// weights its deterministic softmax needs more workspace than an 8 GiB card
/// has left once the fp32 model is resident (verified: CUDA OOM in
/// SoftMax.cu).
///
/// # Errors
/// When `torch_cpu.dll` is not loaded (call after [`crate::init`]) or does
/// not export the private symbols.
#[cfg(windows)]
pub fn enforce() -> Result<()> {
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
    type Setter = unsafe extern "system" fn(*mut std::ffi::c_void, bool);
    let set_benchmark: Setter = symbol(SET_BENCHMARK)?;
    unsafe { set_benchmark(context, false) };
    let set_tf32_cublas: Setter = symbol(SET_TF32_CUBLAS)?;
    unsafe { set_tf32_cublas(context, false) };
    let set_tf32_cudnn: Setter = symbol(SET_TF32_CUDNN)?;
    unsafe { set_tf32_cudnn(context, false) };
    tracing::debug!("torch determinism knobs applied (cuDNN benchmark off, TF32 off)");
    Ok(())
}

/// No-op on non-Windows hosts: the knobs resolve Windows-specific symbols.
///
/// # Errors
/// Never; the signature matches the Windows implementation for call sites.
#[cfg(not(windows))]
pub fn enforce() -> Result<()> {
    Ok(())
}
