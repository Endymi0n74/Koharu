//! Native Qwen Image 2.1 model assembly.
//!
//! Component mapping follows stable-diffusion.cpp at:
//! https://github.com/leejet/stable-diffusion.cpp/blob/2f886889e6e8b78738d6b87f7191f6018557c551/docs/qwen_image_2.1.md

use std::{path::PathBuf, sync::Mutex};

use anyhow::{Context as _, Result, anyhow, ensure};
use koharu_diffusion::{Context, ContextParams, ImageGenerationParams, RgbaImage, TilingParams};

use crate::Backend;

#[derive(Debug)]
pub(super) struct ModelPaths {
    pub diffusion_model: PathBuf,
    pub text_encoder: PathBuf,
    pub text_encoder_vision: PathBuf,
    pub vae: PathBuf,
}

#[derive(Debug)]
pub(super) struct Model {
    context: Mutex<Context>,
    /// Whether the card lacks headroom for a single-pass VAE decode.
    low_vram: bool,
}

impl Model {
    pub fn new(device: &crate::Device, paths: ModelPaths) -> Result<Self> {
        let context = Context::new(&context_params(device, paths))
            .context("failed to load Qwen Image 2.1 components")?;
        ensure!(
            context.supports_image_generation(),
            "the loaded Qwen Image 2.1 context does not support image generation"
        );
        Ok(Self {
            context: Mutex::new(context),
            low_vram: device.memory_free < 20 * 1024 * 1024 * 1024,
        })
    }

    /// Spatial VAE tiling keeps the decode spike inside the graph-cut budget on
    /// cards without headroom; larger cards decode in a single pass.
    pub(super) fn vae_tiling(&self) -> TilingParams {
        TilingParams {
            enabled: self.low_vram,
            tile_size_x: 512,
            tile_size_y: 512,
            ..TilingParams::default()
        }
    }

    pub fn forward(&self, params: &ImageGenerationParams) -> Result<Vec<RgbaImage>> {
        let mut context = self
            .context
            .lock()
            .map_err(|_| anyhow!("Qwen Image 2.1 context lock was poisoned"))?;
        context
            .generate_image_rgba(params)
            .context("Qwen Image 2.1 inference failed")
    }
}

fn context_params(device: &crate::Device, paths: ModelPaths) -> ContextParams {
    let use_accelerator = device.backend != Backend::Cpu;
    let use_cuda = device.backend == Backend::Cuda;
    let keep_parameters_resident = use_accelerator && device.memory_free >= 20 * 1024 * 1024 * 1024;
    ContextParams {
        diffusion_model_path: Some(paths.diffusion_model),
        llm_path: Some(paths.text_encoder),
        // Editing conditions on reference images through the text encoder; with
        // GGUF weights upstream requires the mmproj projector on --llm_vision.
        llm_vision_path: Some(paths.text_encoder_vision),
        vae_path: Some(paths.vae),
        enable_mmap: true,
        flash_attention: use_cuda,
        diffusion_flash_attention: use_cuda,
        diffusion_conv_direct: use_cuda,
        vae_conv_direct: use_cuda,
        backend: Some(device.name.clone()),
        // The text encoder and denoiser run in separate phases. Cards with less
        // headroom keep source parameters in RAM; high-VRAM cards avoid repeated
        // staging by retaining both quantized models on the accelerator.
        // On constrained cards, cap the managed weight and runner buffers
        // (text/vision encode included) below the live free VRAM, reserving up
        // to 2 GiB so transient spikes cannot trip the driver, but never below
        // 3.5 GiB: the graph cut cannot segment the 8B text encoder any tighter.
        params_backend: (use_accelerator && !keep_parameters_resident).then(|| "*=cpu".to_owned()),
        max_vram: (use_accelerator && !keep_parameters_resident).then(|| {
            let free_gib = device.memory_free as f64 / (1024.0 * 1024.0 * 1024.0);
            format!("{:.2}", (free_gib - 2.0).max(3.5))
        }),
        ..ContextParams::default()
    }
}
