use std::{io::Cursor, sync::Arc};

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageEncoder, RgbImage, codecs::jpeg::JpegEncoder};

use crate::Language;

const MAX_IMAGE_DIMENSION: u32 = 2048;
const JPEG_QUALITY: u8 = 88;

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationRequest {
    pub segments: Vec<String>,
    pub source_language: Option<Language>,
    pub target_language: Language,
    pub instructions: Option<String>,
    pub context: Vec<TranslationContext>,
    pub profile: Option<TranslationProfile>,
    pub image: Option<Arc<DynamicImage>>,
}

impl TranslationRequest {
    #[must_use]
    pub fn new(
        segments: impl IntoIterator<Item = impl Into<String>>,
        target_language: Language,
    ) -> Self {
        Self {
            segments: segments.into_iter().map(Into::into).collect(),
            source_language: None,
            target_language,
            instructions: None,
            context: Vec::new(),
            profile: None,
            image: None,
        }
    }

    #[must_use]
    pub fn with_source_language(mut self, language: Language) -> Self {
        self.source_language = Some(language);
        self
    }

    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    #[must_use]
    pub fn with_image(mut self, image: Arc<DynamicImage>) -> Self {
        self.image = Some(image);
        self
    }

    #[must_use]
    pub fn with_context(mut self, context: impl IntoIterator<Item = TranslationContext>) -> Self {
        self.context = context.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_profile(mut self, profile: TranslationProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    pub(crate) fn prepare_image(&mut self) -> anyhow::Result<()> {
        let Some(image) = self.image.as_ref() else {
            return Ok(());
        };
        let (width, height) = (image.width(), image.height());
        let longest = width.max(height);
        if longest <= MAX_IMAGE_DIMENSION {
            return Ok(());
        }
        let resized_width =
            (u64::from(width) * u64::from(MAX_IMAGE_DIMENSION) / u64::from(longest)).max(1) as u32;
        let resized_height =
            (u64::from(height) * u64::from(MAX_IMAGE_DIMENSION) / u64::from(longest)).max(1) as u32;
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
            .context("failed to resize translation image")?;
        self.image = Some(Arc::new(DynamicImage::ImageRgb8(resized)));
        Ok(())
    }

    pub(crate) fn remove_image(&mut self) {
        self.image = None;
    }
}

pub(crate) struct EncodedImage {
    pub(crate) data: String,
}

impl EncodedImage {
    pub(crate) fn data_url(&self) -> String {
        format!("data:image/jpeg;base64,{}", self.data)
    }
}

pub(crate) fn encode_image(image: &DynamicImage) -> anyhow::Result<EncodedImage> {
    let rgb = image.to_rgb8();
    let mut bytes = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .context("failed to encode translation image")?;
    Ok(EncodedImage {
        data: STANDARD.encode(bytes.into_inner()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TranslationContext {
    pub source: String,
    pub translation: String,
}

impl TranslationContext {
    #[must_use]
    pub fn new(source: impl Into<String>, translation: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            translation: translation.into(),
        }
    }
}

/// What a chapter is *about*, learned from the chapter itself before it is
/// translated: the work, the recurring characters, and the terms whose
/// translation must not drift from page to page.
///
/// The translation stage attaches one to every request of a chapter, so page
/// 1's rendering of a name survives to page 30 instead of being re-invented
/// from the twelve lines of preceding context.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TranslationProfile {
    pub series: Option<String>,
    pub synopsis: Option<String>,
    pub register: Option<String>,
    pub characters: Vec<ProfileTerm>,
    pub glossary: Vec<ProfileTerm>,
}

impl TranslationProfile {
    /// Whether the profile carries nothing worth adding to a prompt.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let blank =
            |value: &Option<String>| value.as_deref().map(str::trim).is_none_or(str::is_empty);
        blank(&self.series)
            && blank(&self.synopsis)
            && blank(&self.register)
            && self.characters.is_empty()
            && self.glossary.is_empty()
    }
}

/// One learned term: the source form as it appears in the chapter and the
/// rendering every page must reuse.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileTerm {
    pub source: String,
    pub target: String,
    pub note: Option<String>,
}

impl ProfileTerm {
    #[must_use]
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            note: None,
        }
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// A chapter's text, handed to the model to build a [`TranslationProfile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRequest {
    pub segments: Vec<String>,
    pub source_language: Option<Language>,
    pub target_language: Language,
}

impl ProfileRequest {
    #[must_use]
    pub fn new(
        segments: impl IntoIterator<Item = impl Into<String>>,
        target_language: Language,
    ) -> Self {
        Self {
            segments: segments.into_iter().map(Into::into).collect(),
            source_language: None,
            target_language,
        }
    }

    #[must_use]
    pub fn with_source_language(mut self, language: Language) -> Self {
        self.source_language = Some(language);
        self
    }
}
