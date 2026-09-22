//! Public VRAM queries used by CLI budgeting and run reports.
//!
//! Best effort: queries return `None` when the platform exposes no telemetry
//! for the selected device; callers then require an explicit `--vram-budget`.

use koharu_ml::{Backend, Device};
#[cfg(all(test, any(target_os = "linux", target_os = "windows")))]
use koharu_runtime::Hardware;

/// Total VRAM of `device` in bytes, when discoverable.
#[must_use]
pub fn total_bytes(device: &Device) -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        nvml_device(device).map(|gpu| {
            gpu.memory_info()
                .ok()
                .map(|memory| memory.total)
                .unwrap_or(0)
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = device;
        None
    }
}

/// Free (not currently in use) VRAM of `device` in bytes, when discoverable.
///
/// Unlike [`total_bytes`] a failed telemetry query yields `None` rather than
/// 0: callers use this value as a ceiling and a bogus zero would refuse every
/// model.
#[must_use]
pub fn free_bytes(device: &Device) -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        nvml_device(device)
            .and_then(|gpu| gpu.memory_info().ok())
            .map(|memory| memory.free)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = device;
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn nvml_device(device: &Device) -> Option<nvml_wrapper::Device<'static>> {
    use std::sync::OnceLock;

    static NVML: OnceLock<Option<nvml_wrapper::Nvml>> = OnceLock::new();
    if device.backend == Backend::Cpu {
        return None;
    }
    let nvml = NVML
        .get_or_init(|| nvml_wrapper::Nvml::init().ok())
        .as_ref()?;
    Some(match nvml.device_by_index(device.index as u32) {
        Ok(gpu) => gpu,
        // DXGI/NVML indices can disagree; fall back to the first GPU.
        Err(_) => nvml.device_by_index(0).ok()?,
    })
}

/// Background sampler tracking the peak GPU memory in use while the run
/// progresses. The value covers the whole GPU (including other processes),
/// which is the honest number when budgeting VRAM.
#[derive(Debug)]
pub struct VramSampler {
    shared: std::sync::Arc<SamplerShared>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Debug)]
struct SamplerShared {
    stop: std::sync::atomic::AtomicBool,
    /// Whole-GPU bytes in use at the first successful sample (0 = unset).
    baseline: std::sync::atomic::AtomicU64,
    /// Peak whole-GPU bytes in use since the sampler started.
    peak_total: std::sync::atomic::AtomicU64,
    /// Peak whole-GPU bytes in use since the last [`VramSampler::reset_window`].
    peak_window: std::sync::atomic::AtomicU64,
}

/// Records one telemetry sample: fixes the baseline on the first success and
/// raises both peaks.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn sample_once(shared: &SamplerShared, gpu: &nvml_wrapper::Device<'static>) {
    if let Ok(memory) = gpu.memory_info() {
        let _ = shared.baseline.compare_exchange(
            0,
            memory.used,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        );
        shared
            .peak_total
            .fetch_max(memory.used, std::sync::atomic::Ordering::Relaxed);
        shared
            .peak_window
            .fetch_max(memory.used, std::sync::atomic::Ordering::Relaxed);
    }
}

impl VramSampler {
    /// Starts sampling `device` every `interval`, or returns `None` when the
    /// device or platform exposes no telemetry (CPU runs, missing NVML).
    #[must_use]
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub fn start(device: &Device, interval: std::time::Duration) -> Option<Self> {
        let gpu = nvml_device(device)?;
        let shared = std::sync::Arc::new(SamplerShared {
            stop: std::sync::atomic::AtomicBool::new(false),
            baseline: std::sync::atomic::AtomicU64::new(0),
            peak_total: std::sync::atomic::AtomicU64::new(0),
            peak_window: std::sync::atomic::AtomicU64::new(0),
        });
        // One synchronous first sample so the baseline exists even for very
        // short runs that stop before the first timer tick.
        sample_once(&shared, &gpu);
        let worker_shared = std::sync::Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("vram-sampler".to_owned())
            .spawn(move || {
                while !worker_shared
                    .stop
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    std::thread::sleep(interval);
                    if worker_shared
                        .stop
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        break;
                    }
                    sample_once(&worker_shared, &gpu);
                }
            })
            .ok()?;
        Some(Self {
            shared,
            worker: std::sync::Mutex::new(Some(worker)),
        })
    }

    /// Starts sampling with the default interval (200 ms).
    #[must_use]
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub fn start_default(device: &Device) -> Option<Self> {
        Self::start(device, std::time::Duration::from_millis(200))
    }

    /// Starts sampling; always `None` on platforms without NVML support.
    #[must_use]
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub fn start_default(device: &Device) -> Option<Self> {
        let _ = device;
        None
    }

    /// Peak bytes added by this run above the GPU usage observed at sampler
    /// start — the run's own footprint, desktop and other processes excluded.
    #[must_use]
    pub fn peak_bytes(&self) -> Option<u64> {
        let baseline = self.shared.baseline.load(std::sync::atomic::Ordering::Relaxed);
        let peak = self
            .shared
            .peak_total
            .load(std::sync::atomic::Ordering::Relaxed);
        (baseline > 0).then(|| peak.saturating_sub(baseline))
    }

    /// Peak whole-GPU bytes in use since the sampler started, other processes
    /// included. The honest number when budgeting VRAM on a shared desktop.
    #[must_use]
    pub fn whole_gpu_peak_bytes(&self) -> Option<u64> {
        let peak = self
            .shared
            .peak_total
            .load(std::sync::atomic::Ordering::Relaxed);
        (peak > 0).then_some(peak)
    }

    /// Peak whole-GPU bytes in use since the last [`VramSampler::reset_window`].
    #[must_use]
    pub fn window_peak_bytes(&self) -> Option<u64> {
        let peak = self
            .shared
            .peak_window
            .load(std::sync::atomic::Ordering::Relaxed);
        (peak > 0).then_some(peak)
    }

    /// Peak bytes added since the last [`VramSampler::reset_window`] above the
    /// GPU baseline — the window's own footprint (for one report page).
    #[must_use]
    pub fn window_delta_bytes(&self) -> Option<u64> {
        let baseline = self.shared.baseline.load(std::sync::atomic::Ordering::Relaxed);
        let peak = self
            .shared
            .peak_window
            .load(std::sync::atomic::Ordering::Relaxed);
        (baseline > 0).then(|| peak.saturating_sub(baseline))
    }

    /// Starts a new per-page measurement window.
    pub fn reset_window(&self) {
        self.shared
            .peak_window
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Stops the sampling thread and waits for it to finish.
    pub fn stop(&self) {
        self.shared
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            let _ = handle.join();
        }
    }
}

impl Drop for VramSampler {
    fn drop(&mut self) {
        // Signals the thread without joining: the process may be unwinding,
        // and the thread exits on its own within one interval.
        self.shared
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn cpu_devices_have_no_budget() {
        assert_eq!(total_bytes(&Device::cpu()), None);
    }

    #[test]
    fn cpu_devices_have_no_free_bytes() {
        assert_eq!(free_bytes(&Device::cpu()), None);
    }

    #[test]
    fn cpu_devices_have_no_sampler() {
        assert!(VramSampler::start_default(&Device::cpu()).is_none());
    }

    #[test]
    fn sampler_reports_a_peak_on_gpus() {
        let hardware = Hardware::discover();
        let Some(device) = hardware.device() else {
            return;
        };
        if device.backend == Backend::Cpu {
            return;
        }
        let Some(sampler) = VramSampler::start(device, std::time::Duration::from_millis(50))
        else {
            return;
        };
        std::thread::sleep(std::time::Duration::from_millis(250));
        sampler.reset_window();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let window_peak = sampler.window_peak_bytes();
        let whole_gpu_peak = sampler.whole_gpu_peak_bytes();
        let run_peak = sampler.peak_bytes();
        sampler.stop();
        assert!(whole_gpu_peak.is_some_and(|bytes| bytes > 0));
        assert!(window_peak.is_some_and(|bytes| bytes > 0));
        assert!(run_peak.is_some());
        assert!(run_peak.unwrap() <= whole_gpu_peak.unwrap());
        let window_delta = sampler.window_delta_bytes();
        assert!(window_delta.is_some());
        assert!(window_delta.unwrap() <= window_peak.unwrap());
    }
}
