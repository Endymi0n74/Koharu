//! Qwen Image 2.1 image preprocessing.
//!
//! https://github.com/leejet/stable-diffusion.cpp/blob/2f886889e6e8b78738d6b87f7191f6018557c551/docs/qwen_image_2.1.md

use anyhow::{Result, ensure};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, GrayImage, Luma, RgbImage, RgbaImage};

/// The Qwen Image 2.1 VAE and its editing path require dimensions divisible by
/// 32 (upstream: "Use image dimensions divisible by 32").
pub(super) const DIMENSION_ALIGNMENT: u32 = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QwenImageOptions {
    pub height: Option<u32>,
    pub width: Option<u32>,
    pub num_inference_steps: i32,
    pub seed: i64,
    pub num_images_per_prompt: i32,
}

impl Default for QwenImageOptions {
    fn default() -> Self {
        Self {
            height: None,
            width: None,
            num_inference_steps: 20,
            seed: -1,
            num_images_per_prompt: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QwenImageInpaintOptions {
    pub padding_mask_crop: Option<u32>,
    pub strength: f64,
    pub num_inference_steps: usize,
    pub seed: i64,
}

impl Default for QwenImageInpaintOptions {
    fn default() -> Self {
        Self {
            padding_mask_crop: None,
            strength: 0.8,
            // 14 requested steps sample 12 (stable-diffusion.cpp drops
            // ceil(steps/10) at this strength). Measured on the RTX 3070:
            // indistinguishable from 18 sampled steps (same-seed PSNR
            // 12↔18 ≈ 18↔27) for 17% less inpainting time.
            num_inference_steps: 14,
            seed: -1,
        }
    }
}

pub(super) struct QwenImageProcessor;

impl QwenImageProcessor {
    /// Rounds a dimension down to the model alignment, keeping it usable.
    pub(super) fn align(value: u32) -> u32 {
        (value / DIMENSION_ALIGNMENT) * DIMENSION_ALIGNMENT
    }

    pub(super) fn check_image_input(image: &DynamicImage) -> Result<()> {
        ensure!(
            image.width() >= 64 && image.height() >= 64,
            "Qwen Image 2.1 images must be at least 64x64, got {}x{}",
            image.width(),
            image.height()
        );
        let long = image.width().max(image.height());
        let short = image.width().min(image.height());
        ensure!(
            f64::from(long) / f64::from(short) <= 8.0,
            "Qwen Image 2.1 image aspect ratio must not exceed 8:1, got {}x{}",
            image.width(),
            image.height()
        );
        Ok(())
    }

    pub(super) fn _resize_to_target_area(image: &DynamicImage, target_area: u32) -> DynamicImage {
        let scale = (f64::from(target_area)
            / (f64::from(image.width()) * f64::from(image.height())))
        .sqrt();
        let width = (f64::from(image.width()) * scale).floor().max(1.0) as u32;
        let height = (f64::from(image.height()) * scale).floor().max(1.0) as u32;
        let source = image.to_rgb8();
        let mut resized = RgbImage::new(width, height);
        Resizer::new()
            .resize(
                &source,
                &mut resized,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(false),
            )
            .expect("source and destination images have the same pixel type");
        DynamicImage::ImageRgb8(resized)
    }

    // https://github.com/huggingface/diffusers/blob/a37f6f8394ac2a7ee8360c3abea811efe54512b1/src/diffusers/image_processor.py#L288-L375
    pub(super) fn get_crop_region(
        mask_image: &GrayImage,
        width: u32,
        height: u32,
        pad: u32,
    ) -> Option<(u32, u32, u32, u32)> {
        let mut left = mask_image.width();
        let mut top = mask_image.height();
        let mut right = 0;
        let mut bottom = 0;
        for (x, y, pixel) in mask_image.enumerate_pixels() {
            if pixel.0[0] == 0 {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
        if right <= left || bottom <= top {
            return None;
        }

        let mut x1 = i64::from(left.saturating_sub(pad));
        let mut y1 = i64::from(top.saturating_sub(pad));
        let mut x2 = i64::from(right.saturating_add(pad).min(mask_image.width()));
        let mut y2 = i64::from(bottom.saturating_add(pad).min(mask_image.height()));
        let crop_ratio = (x2 - x1) as f64 / (y2 - y1) as f64;
        let processing_ratio = f64::from(width) / f64::from(height);

        if crop_ratio > processing_ratio {
            let desired_height = (x2 - x1) as f64 / processing_ratio;
            let difference = (desired_height - (y2 - y1) as f64) as i64;
            y1 -= difference / 2;
            y2 += difference - difference / 2;
            if y2 >= i64::from(mask_image.height()) {
                let overflow = y2 - i64::from(mask_image.height());
                y2 -= overflow;
                y1 -= overflow;
            }
            if y1 < 0 {
                y2 -= y1;
                y1 = 0;
            }
            if y2 >= i64::from(mask_image.height()) {
                y2 = i64::from(mask_image.height());
            }
        } else {
            let desired_width = (y2 - y1) as f64 * processing_ratio;
            let difference = (desired_width - (x2 - x1) as f64) as i64;
            x1 -= difference / 2;
            x2 += difference - difference / 2;
            if x2 >= i64::from(mask_image.width()) {
                let overflow = x2 - i64::from(mask_image.width());
                x2 -= overflow;
                x1 -= overflow;
            }
            if x1 < 0 {
                x2 -= x1;
                x1 = 0;
            }
            if x2 >= i64::from(mask_image.width()) {
                x2 = i64::from(mask_image.width());
            }
        }

        Some((
            u32::try_from(x1).ok()?,
            u32::try_from(y1).ok()?,
            u32::try_from(x2).ok()?,
            u32::try_from(y2).ok()?,
        ))
    }

    pub(super) fn _resize_and_fill(image: &DynamicImage, width: u32, height: u32) -> DynamicImage {
        let ratio = f64::from(width) / f64::from(height);
        let source_ratio = f64::from(image.width()) / f64::from(image.height());
        let resized_width = if ratio > source_ratio {
            width
        } else {
            image.width() * height / image.height()
        };
        let resized_height = if ratio <= source_ratio {
            height
        } else {
            image.height() * width / image.width()
        };
        let source = image.to_rgb8();
        let mut resized = RgbImage::new(resized_width, resized_height);
        Resizer::new()
            .resize(
                &source,
                &mut resized,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
                    .use_alpha(false),
            )
            .expect("source and destination images have the same pixel type");
        let mut output = RgbImage::new(width, height);
        image::imageops::overlay(
            &mut output,
            &resized,
            i64::from((width - resized_width) / 2),
            i64::from((height - resized_height) / 2),
        );
        DynamicImage::ImageRgb8(output)
    }

    pub(super) fn _resize_and_crop(
        image: &DynamicImage,
        width: u32,
        height: u32,
    ) -> Result<DynamicImage> {
        ensure!(
            image.width() >= width && image.height() >= height,
            "cannot center-crop {}x{} image to {}x{}",
            image.width(),
            image.height(),
            width,
            height
        );
        let left = (image.width() - width) / 2;
        let top = (image.height() - height) / 2;
        Ok(image.crop_imm(left, top, width, height))
    }

    /// Composites a generated RGBA frame over the source so transparent
    /// texels fall back to the original pixels before mask blending.
    pub(super) fn flatten_over_source(
        generated: &RgbaImage,
        source: &RgbImage,
    ) -> Result<RgbImage> {
        ensure!(
            generated.dimensions() == source.dimensions(),
            "generated/source dimensions differ: generated={}x{}, source={}x{}",
            generated.width(),
            generated.height(),
            source.width(),
            source.height()
        );
        let mut output = source.clone();
        for (x, y, pixel) in generated.enumerate_pixels() {
            let amount = f32::from(pixel.0[3]) / 255.0;
            let original = output.get_pixel(x, y).0;
            let target = output.get_pixel_mut(x, y);
            for (channel, original) in original.iter().enumerate() {
                target.0[channel] = (f32::from(pixel.0[channel]) * amount
                    + f32::from(*original) * (1.0 - amount))
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        Ok(output)
    }

    pub(super) fn binarize(image: &mut GrayImage) {
        for pixel in image.pixels_mut() {
            *pixel = Luma([if pixel.0[0] < 128 { 0 } else { 255 }]);
        }
    }

    pub(super) fn apply_overlay(
        mask: &GrayImage,
        init_image: &RgbImage,
        image: RgbImage,
        crop_coords: Option<(u32, u32, u32, u32)>,
    ) -> Result<RgbImage> {
        let image = if let Some((x1, y1, x2, y2)) = crop_coords {
            let crop = Self::_resize_and_crop(&DynamicImage::ImageRgb8(image), x2 - x1, y2 - y1)?
                .to_rgb8();
            let mut output = RgbImage::new(init_image.width(), init_image.height());
            image::imageops::overlay(&mut output, &crop, i64::from(x1), i64::from(y1));
            output
        } else {
            image
        };

        ensure!(image.dimensions() == init_image.dimensions());
        ensure!(mask.dimensions() == init_image.dimensions());
        let mut output = image;
        for (x, y, pixel) in output.enumerate_pixels_mut() {
            let amount = f32::from(mask.get_pixel(x, y).0[0]) / 255.0;
            let original = init_image.get_pixel(x, y).0;
            for (channel, original) in original.iter().enumerate() {
                pixel.0[channel] = (f32::from(pixel.0[channel]) * amount
                    + f32::from(*original) * (1.0 - amount))
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use image::{GrayImage, Luma, Rgb, RgbImage, RgbaImage};

    use super::{QwenImageInpaintOptions, QwenImageProcessor};

    #[test]
    fn align_keeps_dimensions_on_the_model_grid() {
        assert_eq!(QwenImageProcessor::align(0), 0);
        assert_eq!(QwenImageProcessor::align(31), 0);
        assert_eq!(QwenImageProcessor::align(32), 32);
        assert_eq!(QwenImageProcessor::align(1000), 992);
    }

    #[test]
    fn defaults_keep_upstream_strength_with_twelve_sampled_steps() {
        let options = QwenImageInpaintOptions::default();
        // 14 requested steps run 12 sampling iterations at strength 0.8.
        assert_eq!(options.num_inference_steps, 14);
        assert_eq!(options.strength, 0.8);
        assert_eq!(options.seed, -1);
        assert_eq!(options.padding_mask_crop, None);
    }

    #[test]
    fn overlay_without_a_crop_preserves_unmasked_pixels() {
        let initial = RgbImage::from_pixel(2, 1, Rgb([10, 20, 30]));
        let generated = RgbImage::from_pixel(2, 1, Rgb([200, 210, 220]));
        let mut mask = GrayImage::new(2, 1);
        mask.put_pixel(1, 0, Luma([u8::MAX]));

        let output = QwenImageProcessor::apply_overlay(&mask, &initial, generated, None).unwrap();

        assert_eq!(output.get_pixel(0, 0), initial.get_pixel(0, 0));
        assert_eq!(output.get_pixel(1, 0), &Rgb([200, 210, 220]));
    }

    #[test]
    fn flatten_falls_back_to_the_source_for_transparent_texels() {
        let source = RgbImage::from_raw(2, 1, vec![10, 20, 30, 40, 50, 60]).unwrap();
        let generated =
            RgbaImage::from_raw(2, 1, vec![200, 210, 220, 0, 100, 110, 120, 255]).unwrap();

        let output = QwenImageProcessor::flatten_over_source(&generated, &source).unwrap();

        // Fully transparent: the source pixel survives untouched.
        assert_eq!(output.get_pixel(0, 0), &Rgb([10, 20, 30]));
        // Fully opaque: the generated pixel wins.
        assert_eq!(output.get_pixel(1, 0), &Rgb([100, 110, 120]));
    }

    #[test]
    fn check_image_input_rejects_tiny_and_extreme_images() {
        let tiny = image::DynamicImage::ImageRgb8(RgbImage::new(32, 32));
        assert!(QwenImageProcessor::check_image_input(&tiny).is_err());

        let strip = image::DynamicImage::ImageRgb8(RgbImage::new(2304, 256));
        assert!(QwenImageProcessor::check_image_input(&strip).is_err());
    }
}
