//! Determinism tracing for Torch model forwards (`KOHARU_DETTRACE=1`).
//!
//! Hashes (blake3) the intermediate tensors of the layout detector forward,
//! stage by stage, to locate the first run-to-run divergence point without
//! relying on the postprocess. The goal of the bisection is to identify which
//! CUDA kernel (DINO conv, attention, projector conv, segmentation head)
//! introduces the variance that DETHASH observes downstream.
//!
//! Hashing forces a device→host copy of the traced tensor, so the probe is
//! expensive and only active when the environment variable is set. The
//! `DETTRACE` records are compared across runs: the first differing stage
//! name is the kernel to instrument further.

use std::sync::atomic::{AtomicBool, Ordering};

use koharu_torch::{Kind, Tensor};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether `KOHARU_DETTRACE` is set. Sampled once per process.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Initializes the probe from the process environment. Call once at startup.
pub fn init() {
    ENABLED.store(
        std::env::var_os("KOHARU_DETTRACE").is_some(),
        Ordering::Relaxed,
    );
}

/// Hashes a tensor (any dtype, any device) and logs `DETTRACE <stage> <hash>`
/// plus its shape. Values are compared bit-exactly across runs; a differing
/// hash at stage N with matching hashes at N-1 localizes the nondeterministic
/// kernel to the operation between the two stages.
pub fn trace(stage: &str, tensor: &Tensor) {
    if !enabled() {
        return;
    }
    let size = tensor.size();
    let values = match tensor.kind() {
        Kind::Float => {
            let tensor = tensor.to_device(koharu_torch::Device::Cpu).contiguous();
            let length = tensor.numel();
            let mut values = vec![0.0f32; length];
            if tensor.f_copy_data(&mut values, length).is_err() {
                return;
            }
            hash_f32(&values)
        }
        Kind::Half | Kind::BFloat16 => {
            let tensor = tensor
                .to_device(koharu_torch::Device::Cpu)
                .to_kind(Kind::Float)
                .contiguous();
            let length = tensor.numel();
            let mut values = vec![0.0f32; length];
            if tensor.f_copy_data(&mut values, length).is_err() {
                return;
            }
            hash_f32(&values)
        }
        _ => {
            // Fallback: hash the raw bytes through the untyped byte copy.
            let tensor = tensor.to_device(koharu_torch::Device::Cpu).contiguous();
            let length = tensor.numel() * std::mem::size_of::<u8>().max(1) * 4;
            let mut values = vec![0u8; length];
            if tensor.f_copy_data_u8(&mut values, length).is_err() {
                return;
            }
            let mut hasher = blake3::Hasher::new();
            hasher.update(&values);
            hasher.finalize().to_hex().to_string()
        }
    };
    eprintln!("DETTRACE {stage} {values} shape={size:?}");
}

fn hash_f32(values: &[f32]) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        // Hash the bit pattern, not the float, so -0.0 vs 0.0 and NaN payloads
        // are distinguished: run-to-run bit drift is exactly what we hunt.
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    hasher.update(&bytes);
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_by_default_without_env() {
        // `init` is process-global; in the test harness the variable is unset.
        std::env::remove_var("KOHARU_DETTRACE");
        super::init();
        assert!(!super::enabled());
    }
}
