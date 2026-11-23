mod backend;
mod image;
mod nn;

pub mod baberu_ocr;
pub mod flux3_klein;
pub mod hayai_ocr;
pub mod koharu_layout_highres;
pub mod lama;
pub mod manga_ocr_ar;
pub mod qwen_image;

pub use koharu_diffusion::Canceller;
pub use koharu_runtime::{Device, Repository};

/// Every repository the models download from, so a prune knows which folders
/// the store must keep.
#[must_use]
pub fn repositories() -> Vec<Repository> {
    vec![
        baberu_ocr::BABERU,
        baberu_ocr::DINOV2,
        flux3_klein::TRANSFORMER,
        flux3_klein::DECODER,
        flux3_klein::TEXT_ENCODER,
        hayai_ocr::REPOSITORY,
        koharu_layout_highres::REPOSITORY,
        lama::REPOSITORY,
        manga_ocr_ar::REPOSITORY,
        qwen_image::TRANSFORMER,
        qwen_image::VAE,
        qwen_image::TEXT_ENCODER,
        qwen_image::BASE_TRANSFORMER,
        qwen_image::UNCENSORED_TRANSFORMER,
        qwen_image::HERETIC_TEXT_ENCODER,
    ]
}
