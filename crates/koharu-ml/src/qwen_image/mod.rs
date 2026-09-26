//! Qwen Image 2.1 generation, editing, and inpainting.
//!
//! https://github.com/leejet/stable-diffusion.cpp/blob/2f886889e6e8b78738d6b87f7191f6018557c551/docs/qwen_image_2.1.md

mod model;
mod processor;

use anyhow::{Context, Result, ensure};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, RgbImage, RgbaImage};
use koharu_diffusion::{GuidanceParams, ImageGenerationParams, SampleMethod, SampleParams};

use self::{
    model::{Model, ModelPaths},
    processor::QwenImageProcessor,
};

pub use self::processor::{QwenImageInpaintOptions, QwenImageOptions};

crate::model_repository!("leejet/Qwen-Image-2.1-GGUF" @ "cc11433936a06e9765f7c0c0b1f0436cfd2b9856" {
    DIFFUSION_WEIGHTS = "qwen_image_2.1-Q4_K.gguf"
});
crate::model_repository!("pottokao/Qwen-Image-2.1-Text-Encoder-Heretic-GGUF" @ "23813717f7f9282b372c23c7207468a7a168fa68" {
    TEXT_ENCODER_WEIGHTS = "qwen3vl_8b_heretic-Q4_K_M.gguf",
    TEXT_ENCODER_VISION_WEIGHTS = "mmproj-qwen3vl_8b_heretic-f16.gguf"
});
crate::model_repository!("Comfy-Org/Qwen-Image-2.1" @ "9a44dbdb47cefd046be9c0a13476192f34c8db8e" {
    VAE_WEIGHTS = "vae/qwen_image_2.1_vae_bf16.safetensors"
});

/// Upstream recommends `--cfg-scale 6.0` for Qwen Image 2.1.
const CFG_SCALE: f32 = 6.0;

#[derive(Debug)]
pub struct QwenImage {
    model: Model,
}

impl QwenImage {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let (diffusion_model, text_encoder, text_encoder_vision, vae) = tokio::try_join!(
            DIFFUSION_WEIGHTS.resolve(),
            TEXT_ENCODER_WEIGHTS.resolve(),
            TEXT_ENCODER_VISION_WEIGHTS.resolve(),
            VAE_WEIGHTS.resolve(),
        )
        .context("failed to resolve Qwen Image 2.1 model assets")?;
        let model = Model::new(
            &device,
            ModelPaths {
                diffusion_model,
                text_encoder,
                text_encoder_vision,
                vae,
            },
        )?;
        Ok(Self { model })
    }

    /// Generates images from `prompt`. Passing `image` switches to editing mode:
    /// upstream conditions on the reference images instead of generating from
    /// noise alone.
    pub fn inference(
        &self,
        image: &[DynamicImage],
        prompt: &str,
        options: &QwenImageOptions,
    ) -> Result<Vec<RgbaImage>> {
        ensure!(
            !prompt.contains('\0'),
            "prompt contains an interior NUL byte"
        );
        ensure!(
            options.num_inference_steps > 0,
            "num_inference_steps must be greater than zero"
        );
        ensure!(
            options.num_images_per_prompt > 0,
            "num_images_per_prompt must be greater than zero"
        );

        let mut reference_images = Vec::with_capacity(image.len());
        for image in image {
            QwenImageProcessor::check_image_input(image)?;
            reference_images.push(image.clone().to_rgb8());
        }

        let height = options
            .height
            .or_else(|| reference_images.first().map(RgbImage::height))
            .unwrap_or(1024);
        let width = options
            .width
            .or_else(|| reference_images.first().map(RgbImage::width))
            .unwrap_or(1024);
        let height = QwenImageProcessor::align(height);
        let width = QwenImageProcessor::align(width);
        ensure!(width > 0 && height > 0);

        self.model.forward(&ImageGenerationParams {
            prompt: prompt.to_owned(),
            width: i32::try_from(width)?,
            height: i32::try_from(height)?,
            reference_images,
            sample: SampleParams {
                guidance: GuidanceParams {
                    text_cfg: CFG_SCALE,
                    ..GuidanceParams::default()
                },
                sample_method: SampleMethod::Euler,
                sample_steps: options.num_inference_steps,
                ..SampleParams::default()
            },
            seed: options.seed,
            batch_count: options.num_images_per_prompt,
            ..ImageGenerationParams::default()
        })
    }
}

#[derive(Debug)]
pub struct QwenImageInpaint {
    model: Model,
}

impl QwenImageInpaint {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let (diffusion_model, text_encoder, text_encoder_vision, vae) = tokio::try_join!(
            DIFFUSION_WEIGHTS.resolve(),
            TEXT_ENCODER_WEIGHTS.resolve(),
            TEXT_ENCODER_VISION_WEIGHTS.resolve(),
            VAE_WEIGHTS.resolve(),
        )
        .context("failed to resolve Qwen Image 2.1 model assets")?;
        let model = Model::new(
            &device,
            ModelPaths {
                diffusion_model,
                text_encoder,
                text_encoder_vision,
                vae,
            },
        )?;
        Ok(Self { model })
    }

    /// Edits `image` inside `mask_image` following `prompt`.
    ///
    /// The edited crop is conditioned on itself as a reference image, matching
    /// upstream's `-r` editing mode, while the native denoise mask keeps the
    /// unmasked latent close to the source.
    pub fn inference(
        &self,
        prompt: &str,
        image: &DynamicImage,
        image_reference: Option<&DynamicImage>,
        mask_image: &DynamicImage,
        options: &QwenImageInpaintOptions,
    ) -> Result<DynamicImage> {
        ensure!(
            image.width() == mask_image.width() && image.height() == mask_image.height(),
            "image/mask dimensions differ: image={}x{}, mask={}x{}",
            image.width(),
            image.height(),
            mask_image.width(),
            mask_image.height()
        );
        ensure!(
            !prompt.contains('\0'),
            "prompt contains an interior NUL byte"
        );
        ensure!(
            options.strength > 0.0 && options.strength <= 1.0,
            "Qwen Image 2.1 inpaint strength must be greater than zero and at most one"
        );
        ensure!(
            options.num_inference_steps > 0,
            "num_inference_steps must be greater than zero"
        );

        let mut image = image.clone();
        if u64::from(image.width()) * u64::from(image.height()) > 1024 * 1024 {
            image = QwenImageProcessor::_resize_to_target_area(&image, 1024 * 1024);
        }
        let width = QwenImageProcessor::align(image.width());
        let height = QwenImageProcessor::align(image.height());
        ensure!(width > 0 && height > 0);
        let source = image.to_rgb8();
        let mut image = RgbImage::new(width, height);
        Resizer::new()
            .resize(
                &source,
                &mut image,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(false),
            )
            .expect("source and destination images have the same pixel type");
        let source_mask = mask_image.to_luma8();
        let mut mask_image = image::GrayImage::new(width, height);
        Resizer::new()
            .resize(
                &source_mask,
                &mut mask_image,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(false),
            )
            .expect("source and destination masks have the same pixel type");

        let crop_coords = options.padding_mask_crop.and_then(|padding| {
            QwenImageProcessor::get_crop_region(&mask_image, width, height, padding)
        });
        let (init_image, mut native_mask) = if let Some((x1, y1, x2, y2)) = crop_coords {
            let image_crop = DynamicImage::ImageRgb8(
                image::imageops::crop_imm(&image, x1, y1, x2 - x1, y2 - y1).to_image(),
            );
            let mask_crop = DynamicImage::ImageLuma8(
                image::imageops::crop_imm(&mask_image, x1, y1, x2 - x1, y2 - y1).to_image(),
            );
            (
                QwenImageProcessor::_resize_and_fill(&image_crop, width, height).to_rgb8(),
                QwenImageProcessor::_resize_and_fill(&mask_crop, width, height).to_luma8(),
            )
        } else {
            (image.clone(), mask_image.clone())
        };
        QwenImageProcessor::binarize(&mut native_mask);

        let mut reference_images = vec![init_image.clone()];
        if let Some(image_reference) = image_reference {
            QwenImageProcessor::check_image_input(image_reference)?;
            reference_images.push(image_reference.clone().to_rgb8());
        }

        let strength = if options.strength >= 1.0 {
            1.0
        } else {
            let effective_steps = options.num_inference_steps
                - (options.num_inference_steps as f64 * (1.0 - options.strength)).floor() as usize;
            let boundary = effective_steps as f32 / options.num_inference_steps as f32;
            f32::from_bits(boundary.to_bits() - 1)
        };

        let generated = self
            .model
            .forward(&ImageGenerationParams {
                prompt: prompt.to_owned(),
                width: i32::try_from(width)?,
                height: i32::try_from(height)?,
                init_image: Some(init_image),
                reference_images,
                mask_image: Some(native_mask),
                sample: SampleParams {
                    guidance: GuidanceParams {
                        text_cfg: CFG_SCALE,
                        ..GuidanceParams::default()
                    },
                    sample_method: SampleMethod::Euler,
                    sample_steps: i32::try_from(options.num_inference_steps)?,
                    ..SampleParams::default()
                },
                seed: options.seed,
                batch_count: 1,
                strength,
                ..ImageGenerationParams::default()
            })?
            .into_iter()
            .next()
            .context("Qwen Image 2.1 returned no inpainted image")?;

        // Qwen Image 2.1 emits RGBA; transparent texels fall back to the source
        // before the mask overlay blends the rest.
        let generated = QwenImageProcessor::flatten_over_source(&generated, &image)?;
        let generated =
            QwenImageProcessor::apply_overlay(&mask_image, &image, generated, crop_coords)?;
        Ok(DynamicImage::ImageRgb8(generated))
    }
}
