mod fft;
mod model;

use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage};
use koharu_runtime::RuntimeManager;
use tracing::instrument;

use crate::{
    device,
    inpainting::{
        HdStrategyConfig, InpaintForward, binarize_mask, extract_alpha, restore_alpha_channel,
        run_inpaint, try_fill_balloon,
    },
    loading,
};

const HF_REPO: &str = "mayocream/lama-manga";

koharu_runtime::declare_hf_model_package!(
    id: "model:lama:weights",
    repo: "mayocream/lama-manga",
    file: "lama-manga.safetensors",
    bootstrap: false,
    order: 130,
);

pub struct Lama {
    model: model::Lama,
    device: Device,
}

impl Lama {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let weights_path = runtime
            .downloads()
            .huggingface_model(HF_REPO, "lama-manga.safetensors")
            .await?;
        let model = loading::load_buffered_safetensors_path(&weights_path, &device, |vb| {
            model::Lama::load(&vb)
        })?;

        Ok(Self { model, device })
    }

    /// Run inpainting with the manga-tuned default strategy (Crop, 800/128/1280).
    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<DynamicImage> {
        self.inference_with_config(image, mask, &HdStrategyConfig::lama_default())
    }

    /// Run inpainting with a caller-supplied [`HdStrategyConfig`]. Use this to
    /// pick a different strategy (Original / Resize) or tune the trigger /
    /// margin / resize-limit for GPUs with less VRAM.
    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_config(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        cfg: &HdStrategyConfig,
    ) -> Result<DynamicImage> {
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image and mask dimensions dismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }

        let binary_mask = binarize_mask(mask);
        let image_rgb = image.to_rgb8();
        let forward = LamaForward { lama: self };
        let output_rgb = run_inpaint(&forward, &image_rgb, &binary_mask, cfg)?;

        if image.color().has_alpha() {
            let original_alpha = image.to_rgba8();
            let alpha = extract_alpha(&original_alpha);
            let output = restore_alpha_channel(&output_rgb, &alpha, &binary_mask);
            Ok(DynamicImage::ImageRgba8(output))
        } else {
            Ok(DynamicImage::ImageRgb8(output_rgb))
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, image: &Tensor, mask: &Tensor) -> Result<Tensor> {
        self.model.forward(image, mask)
    }

    #[instrument(level = "debug", skip_all)]
    fn inference_model(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let (image_tensor, mask_tensor) = self.preprocess(image, mask)?;
        let output = self.forward(&image_tensor, &mask_tensor)?;
        self.postprocess(&output)
    }

    #[instrument(level = "debug", skip_all)]
    fn preprocess(&self, image: &RgbImage, mask: &GrayImage) -> Result<(Tensor, Tensor)> {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let rgb = image.clone().into_raw();
        let luma = mask.clone().into_raw();

        let image_tensor = (Tensor::from_vec(rgb, (1, h, w, 3), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            * (1. / 255.))?;

        let mask_tensor = Tensor::from_vec(luma, (1, h, w, 1), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            .gt(1.0f32)?;

        Ok((image_tensor, mask_tensor))
    }

    #[instrument(level = "debug", skip_all)]
    fn postprocess(&self, output: &Tensor) -> Result<RgbImage> {
        let output = output.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }
        let output = (output * 255.)?
            .clamp(0., 255.)?
            .permute((1, 2, 0))?
            .to_dtype(DType::U8)?;
        let raw: Vec<u8> = output.flatten_all()?.to_vec1()?;
        RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))
    }
}

/// [`InpaintForward`] impl used by the HD-strategy dispatcher. Applies the
/// balloon-fill fast path on a per-crop basis before falling back to the
/// model forward — flat-background speech bubbles skip the model entirely.
struct LamaForward<'a> {
    lama: &'a Lama,
}

impl InpaintForward for LamaForward<'_> {
    fn forward(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        if mask.pixels().all(|p| p.0[0] == 0) {
            return Ok(image.clone());
        }
        if let Some(filled) = try_fill_balloon(image, mask) {
            return Ok(filled);
        }
        self.lama.inference_model(image, mask)
    }
}

#[cfg(test)]
mod tests {
    use crate::inpainting::restore_alpha_channel;
    use image::{GrayImage, Luma, Rgb, RgbImage};

    const ALPHA_RING_RADIUS: u8 = 7;

    #[test]
    fn rgba_alpha_restore_uses_surrounding_ring() {
        let image = RgbImage::from_pixel(32, 32, Rgb([20, 30, 40]));
        let mut alpha = GrayImage::from_pixel(32, 32, Luma([255]));
        let mut mask = GrayImage::new(32, 32);

        for y in 10..22 {
            for x in 10..22 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
            for x in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
                if x < 32 && y < 32 && mask.get_pixel(x, y).0[0] == 0 {
                    alpha.put_pixel(x, y, Luma([64]));
                }
            }
        }

        let restored = restore_alpha_channel(&image, &alpha, &mask);
        assert_eq!(restored.get_pixel(15, 15).0[3], 64);
        assert_eq!(restored.get_pixel(2, 2).0[3], 255);
    }
}
