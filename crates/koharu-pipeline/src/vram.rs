//! Public VRAM query used by CLI budgeting.
//!
//! Best effort: returns `None` when the platform exposes no total for the
//! selected device; callers then require an explicit `--vram-budget`.

use koharu_ml::{Backend, Device};

/// Total VRAM of `device` in bytes, when discoverable.
#[must_use]
pub fn total_bytes(device: &Device) -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        nvml_total(device)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = device;
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn nvml_total(device: &Device) -> Option<u64> {
    if device.backend == Backend::Cpu {
        return None;
    }
    let nvml = nvml_wrapper::Nvml::init().ok()?;
    let gpu = match nvml.device_by_index(device.index as u32) {
        Ok(gpu) => gpu,
        // DXGI/NVML indices can disagree; fall back to the first GPU.
        Err(_) => nvml.device_by_index(0).ok()?,
    };
    gpu.memory_info().ok().map(|memory| memory.total)
}

#[cfg(all(test, any(target_os = "linux", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn cpu_devices_have_no_budget() {
        assert_eq!(total_bytes(&Device::cpu()), None);
    }
}
