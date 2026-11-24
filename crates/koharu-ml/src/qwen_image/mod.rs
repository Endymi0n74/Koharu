//! Qwen-Image 2.1 generation and editing, on its distilled Turbo checkpoint
//! or on the full one.
//!
//! Follows diffusers' `QwenImage21Pipeline` as the Turbo checkpoint configures
//! it: flow-matching Euler steps through the 8 sigmas it saves, which set the
//! step count, no guidance, and a 1024-pixel side.
//! https://github.com/huggingface/diffusers/blob/74fb49b5506a6960d55de0f7e6f4e0631213dc63/src/diffusers/pipelines/qwenimage21/pipeline_qwenimage21.py
//! https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo/blob/fc13ba1986fcb905854dc8de2f52eee395f49b5b/model_index.json
//! Editing is generation conditioned on images, which the model reads in the
//! order given. A mask or a painted annotation is one more condition image
//! the prompt names, as Qwen-Image 2.1's model card describes local edits.
//! https://huggingface.co/Qwen/Qwen-Image-2.1/blob/d26bb61231c349cf6b7896fa83353113880e1ba3/README.md

mod model;

use anyhow::{Context, Result, ensure};
use fast_image_resize::{FilterType, ResizeAlg};
use image::{DynamicImage, RgbImage, RgbaImage};
use koharu_diffusion::{Canceller, ImageParams, SampleMethod};
use koharu_runtime::Repository;

use self::model::{Model, ModelPaths};

/// unsloth's GGUF of Qwen-Image 2.1 Turbo, whose text encoder, vision
/// projector, and VAE are 2.1's.
pub(crate) const TRANSFORMER: Repository = Repository::model(
    "unsloth/Qwen-Image-2.1-Turbo-GGUF",
    "bd53e520a64472adaff780da0b2f2d7eb1605e30",
);
/// Qwen-Image 2.1's own VAE, which stable-diffusion.cpp's guide names; the
/// earlier Qwen-Image and Wan 2.2 VAEs don't stand in for it.
pub(crate) const VAE: Repository = Repository::model(
    "Comfy-Org/Qwen-Image-2.1",
    "aacf31a6d48e707f352ecba714cee0680da3479b",
);
/// Qwen's own GGUF of the Qwen3-VL 8B text encoder, with the vision projector
/// that reads condition images.
pub(crate) const TEXT_ENCODER: Repository = Repository::model(
    "Qwen/Qwen3-VL-8B-Instruct-GGUF",
    "f982a07559d4a2f6c8744d840bf6fccab30eea96",
);
/// leejet's GGUF of Qwen-Image 2.1 itself, not the distilled Turbo: the full
/// checkpoint, converted for stable-diffusion.cpp.
pub(crate) const BASE_TRANSFORMER: Repository = Repository::model(
    "leejet/Qwen-Image-2.1-GGUF",
    "cc11433936a06e9765f7c0c0b1f0436cfd2b9856",
);
/// The uncensored conversion of the full checkpoint, made with
/// stable-diffusion.cpp upstream: the same architecture, so only the diffusion
/// weights swap between the two.
pub(crate) const UNCENSORED_TRANSFORMER: Repository = Repository::model(
    "abenzerps/Qwen-Image-2.1-Uncensored-GGUF",
    "6b34e59458d3eb7ba6a6f86a116aed5253dc02c3",
);
/// The Heretic text encoder the base and the uncensored checkpoints run with:
/// Qwen3-VL 8B with its refusals ablated, and the vision projector that reads
/// their condition images.
pub(crate) const HERETIC_TEXT_ENCODER: Repository = Repository::model(
    "pottokao/Qwen-Image-2.1-Text-Encoder-Heretic-GGUF",
    "23813717f7f9282b372c23c7207468a7a168fa68",
);

/// The VAE's 16× downscale by the transformer's 2×2 patches: sizes are
/// multiples of it.
const MULTIPLE: u32 = 32;
/// diffusers' `VaeImageProcessor.resize`, a PIL Lanczos.
const RESIZE: ResizeAlg = ResizeAlg::Convolution(FilterType::Lanczos3);
/// Turbo's `sample_sigmas`, and the terminal zero diffusers' scheduler
/// appends and sd.cpp takes in the list.
const SIGMAS: [f32; 9] = [
    1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0.0,
];
/// The steps `SIGMAS` make.
const STEPS: usize = SIGMAS.len() - 1;
/// The guidance upstream's guide runs the full checkpoints with
/// (`--cfg-scale 6.0`).
const CFG_SCALE: f32 = 6.0;
/// The steps the full checkpoints sample, as that guide runs them; the Turbo's
/// are its sigmas.
const BASE_STEPS: u32 = 20;

/// Which checkpoint a loaded model runs: the distilled Turbo, the full model,
/// or its uncensored conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Checkpoint {
    Turbo,
    Base,
    Uncensored,
}

impl Checkpoint {
    /// What the checkpoint is called in the app and in errors.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Turbo => "Qwen-Image 2.1 Turbo",
            Self::Base => "Qwen-Image 2.1",
            Self::Uncensored => "Qwen-Image 2.1 Uncensored",
        }
    }
}

/// `QwenImage21Pipeline.__call__`'s parameters, each checkpoint's own steps and
/// guidance filling what it leaves unset.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Steers away from it, which the Turbo checkpoint does only with a scale
    /// above 1.
    pub negative_prompt: Option<String>,
    /// Classifier-free guidance's scale; unset keeps the checkpoint's own: none
    /// for Turbo, `CFG_SCALE` for the full checkpoints.
    pub true_cfg_scale: Option<f32>,
    /// Unset, `output_resolution`'s side, or the last condition image's
    /// aspect ratio at its area; rounded down to a multiple of 32.
    pub height: Option<u32>,
    pub width: Option<u32>,
    pub num_images_per_prompt: u32,
    /// sd.cpp's seed; -1 picks one at random.
    pub seed: i64,
    /// The side the output and every condition image are sized to the area of.
    pub output_resolution: u32,
    /// The step count; unset keeps the checkpoint's own: Turbo's eight sigmas,
    /// `BASE_STEPS` for the full checkpoints.
    pub steps: Option<u32>,
    /// A sigma schedule; empty keeps the checkpoint's, which for the full
    /// checkpoints is the one its steps give.
    pub sigmas: &'static [f32],
}

impl Default for Options {
    fn default() -> Self {
        Self {
            negative_prompt: None,
            true_cfg_scale: None,
            height: None,
            width: None,
            num_images_per_prompt: 1,
            seed: -1,
            output_resolution: 1024,
            steps: None,
            sigmas: &[],
        }
    }
}

#[derive(Debug)]
pub struct QwenImage {
    model: Model,
    checkpoint: Checkpoint,
}

impl QwenImage {
    /// What stops a generation under way, from another thread.
    pub fn canceller(&self) -> Canceller {
        self.model.canceller.clone()
    }

    /// What the loaded checkpoint is called.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.checkpoint.name()
    }

    /// Loads the Turbo checkpoint once for generation and editing, its weights
    /// placed by stable-diffusion.cpp's own fit to the device's free memory.
    pub async fn load(device: &crate::Device) -> Result<Self> {
        Self::load_checkpoint(device, Checkpoint::Turbo).await
    }

    /// Loads the full Qwen-Image 2.1 checkpoint, with the Heretic text encoder.
    pub async fn load_base(device: &crate::Device) -> Result<Self> {
        Self::load_checkpoint(device, Checkpoint::Base).await
    }

    /// Loads the uncensored conversion of the full checkpoint: the diffusion
    /// weights drop the base model's content refusals, while the Heretic text
    /// encoder and the shared VAE stay.
    pub async fn load_uncensored(device: &crate::Device) -> Result<Self> {
        Self::load_checkpoint(device, Checkpoint::Uncensored).await
    }

    async fn load_checkpoint(device: &crate::Device, checkpoint: Checkpoint) -> Result<Self> {
        let paths = async {
            let [vae] = VAE
                .resolve(["vae/qwen_image_2.1_vae_bf16.safetensors"])
                .await?;
            // 4-bit, as KoharuBatch's other diffusion models load.
            let [transformer] = match checkpoint {
                Checkpoint::Turbo => {
                    TRANSFORMER
                        .resolve(["qwen-image-2.1-turbo-Q4_K_M.gguf"])
                        .await?
                }
                Checkpoint::Base => {
                    BASE_TRANSFORMER
                        .resolve(["qwen_image_2.1-Q4_K.gguf"])
                        .await?
                }
                Checkpoint::Uncensored => {
                    UNCENSORED_TRANSFORMER
                        .resolve(["qwen-image-2.1-UC-Q4_K_M.gguf"])
                        .await?
                }
            };
            let [text_encoder, vision] = match checkpoint {
                Checkpoint::Turbo => {
                    TEXT_ENCODER
                        .resolve([
                            "Qwen3VL-8B-Instruct-Q4_K_M.gguf",
                            "mmproj-Qwen3VL-8B-Instruct-F16.gguf",
                        ])
                        .await?
                }
                Checkpoint::Base | Checkpoint::Uncensored => {
                    HERETIC_TEXT_ENCODER
                        .resolve([
                            "qwen3vl_8b_heretic-Q4_K_M.gguf",
                            "mmproj-qwen3vl_8b_heretic-f16.gguf",
                        ])
                        .await?
                }
            };
            anyhow::Ok(ModelPaths {
                transformer,
                text_encoder,
                vision,
                vae,
            })
        }
        .await
        .with_context(|| format!("failed to resolve {}'s files", checkpoint.name()))?;
        let model = Model::new(device, &paths)?;
        Ok(Self { model, checkpoint })
    }

    /// Images of `prompt`, conditioned on `images` when there are any. They
    /// carry alpha, which the prompt asks for as the model card describes.
    pub fn generate(
        &self,
        prompt: &str,
        images: &[DynamicImage],
        options: &Options,
    ) -> Result<Vec<RgbaImage>> {
        ensure!(
            !prompt.contains('\0'),
            "prompt contains an interior NUL byte"
        );
        ensure!(
            options.num_images_per_prompt > 0 && options.output_resolution > 0,
            "num_images_per_prompt and output_resolution must be greater than zero"
        );
        ensure!(
            options.true_cfg_scale.is_none_or(f32::is_finite),
            "true_cfg_scale must be finite"
        );
        let area = options.output_resolution * options.output_resolution;
        let (width, height) = match images.last() {
            Some(last) => {
                let (width, height) = dimensions(area, last.width(), last.height());
                (
                    options.width.unwrap_or(width),
                    options.height.unwrap_or(height),
                )
            }
            None => (
                options.width.unwrap_or(options.output_resolution),
                options.height.unwrap_or(options.output_resolution),
            ),
        };
        let (width, height) = (width / MULTIPLE * MULTIPLE, height / MULTIPLE * MULTIPLE);
        ensure!(
            width > 0 && height > 0,
            "height and width must be at least {MULTIPLE}"
        );

        // One resize feeds both the text encoder and the VAE. The images go as
        // RGB, which sd.cpp gives an opaque alpha, as diffusers' `RGBA`
        // conversion does.
        let references = images
            .iter()
            .map(|image| {
                let (width, height) = dimensions(area, image.width(), image.height());
                crate::image::resize(&image.to_rgb8(), width, height, RESIZE)
            })
            .collect::<Result<Vec<RgbImage>>>()?;
        let cfg = options.true_cfg_scale.unwrap_or(match self.checkpoint {
            Checkpoint::Turbo => 1.0,
            Checkpoint::Base | Checkpoint::Uncensored => CFG_SCALE,
        });
        let steps = options.steps.unwrap_or(match self.checkpoint {
            Checkpoint::Turbo => STEPS as u32,
            Checkpoint::Base | Checkpoint::Uncensored => BASE_STEPS,
        });
        let sigmas: &[f32] = match options.sigmas.is_empty() {
            false => options.sigmas,
            true => match self.checkpoint {
                Checkpoint::Turbo => &SIGMAS,
                Checkpoint::Base | Checkpoint::Uncensored => &[],
            },
        };
        self.model.forward(&ImageParams {
            prompt,
            negative_prompt: options.negative_prompt.as_deref().unwrap_or_default(),
            width: Some(width),
            height: Some(height),
            references: &references,
            cfg: Some(cfg),
            sample_method: Some(SampleMethod::Euler),
            steps: Some(steps),
            sigmas,
            seed: Some(options.seed),
            count: Some(options.num_images_per_prompt),
            ..ImageParams::default()
        })
    }
}

/// `calculate_dimensions`: `target_area` at the image's aspect ratio, each
/// side rounded to a multiple of 32 as Python's `round` rounds, half to even.
fn dimensions(target_area: u32, image_width: u32, image_height: u32) -> (u32, u32) {
    let ratio = f64::from(image_width) / f64::from(image_height);
    let width = (f64::from(target_area) * ratio).sqrt();
    let height = width / ratio;
    let round = |side: f64| ((side / f64::from(MULTIPLE)).round_ties_even() as u32) * MULTIPLE;
    (round(width), round(height))
}
