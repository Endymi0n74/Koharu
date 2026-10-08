//! Handing Torch's cached CUDA blocks back to the driver.
//!
//! Dropping a tensor only returns its blocks to the caching allocator's pool —
//! the driver keeps the reservation and `nvidia-smi` still counts it as used.
//! ggml (llama.cpp) allocates outside that pool with plain `cudaMalloc`, so a
//! pool left full by the vision phases makes its loads fail while the GPU
//! reports 0 MiB free: on a symptom, not a cause ("invalid vector subscript",
//! the empty-token quirk, see memory.md's VRAM piège). [`empty`] releases the
//! unused blocks so the memory is visible to whatever loads next.

/// Empties the CUDA caching allocator, making cached-but-unused blocks
/// visible to the driver again.
///
/// Best-effort and never fatal: a no-op on non-Windows hosts, in processes
/// that never loaded the CUDA libraries (CPU runs), or when the exported
/// symbol is missing from the shipped runtime. Call it while the allocator
/// is quiescent — between pipeline phases or after dropping a model.
pub fn empty() {
    #[cfg(windows)]
    windows::empty();
}

#[cfg(windows)]
mod windows {
    use libloading::os::windows::Library as OsLibrary;

    /// `void at::accelerator::emptyCache()` — dispatches to
    /// `c10::cuda::CUDACachingAllocator::emptyCache()`. Exported by
    /// `torch_cpu.dll` in both shipped runtimes (2.12.1 and 2.13.0.7).
    const EMPTY_CACHE: &str = "?emptyCache@accelerator@at@@YAXXZ";

    /// The CUDA libraries only join processes that ran on the GPU; probing
    /// them keeps CPU runs away from accelerator hook lookups.
    const CUDA_LIBRARIES: [&str; 2] = ["c10_cuda.dll", "torch_cuda.dll"];

    pub(super) fn empty() {
        if !CUDA_LIBRARIES.iter().any(|name| module_loaded(name)) {
            return;
        }
        let Some(function) = symbol::<unsafe extern "system" fn()>(EMPTY_CACHE) else {
            tracing::debug!("torch emptyCache symbol unavailable; cache not released");
            return;
        };
        unsafe { function() };
        tracing::debug!("torch CUDA caching allocator emptied");
    }

    /// Whether `name` is already part of this process.
    ///
    /// The handle is leaked on purpose: dropping a probe must never decrement
    /// the running torch runtime's module refcount (same treatment as
    /// `determinism::enforce`).
    fn module_loaded(name: &str) -> bool {
        OsLibrary::open_already_loaded(name)
            .map(|library| {
                std::mem::forget(library);
                true
            })
            .unwrap_or(false)
    }

    /// Resolves an exported symbol from an already-loaded torch DLL.
    fn symbol<T: Copy>(name: &str) -> Option<T> {
        let library = OsLibrary::open_already_loaded("torch_cpu.dll").ok()?;
        let address = unsafe { library.get::<T>(name.as_bytes()) }.ok()?;
        std::mem::forget(library);
        Some(*address)
    }
}
