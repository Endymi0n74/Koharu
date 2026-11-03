use std::fmt;

use anyhow::Result;
use koharu_pipeline::PipelineConfig;
use koharu_renderer::TypesettingConfig;
use koharu_secrets::ExposeSecret as _;
use koharu_translator::{Language, Model, Provider, ProviderConfig, ProvidersConfig};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Error;

#[derive(Clone, Debug, Serialize, Type)]
pub struct Preferences {
    pub pipeline: PipelineConfig,
    pub providers: ProviderPreferences,
    pub typesetting: TypesettingConfig,
    pub languages: Vec<LanguageChoice>,
}

impl Preferences {
    pub(crate) fn load() -> Result<Self> {
        let pipeline = PipelineConfig::load()?;
        let providers = ProvidersConfig::load()?;
        let typesetting = TypesettingConfig::load()?;
        let pipeline = pipeline.read()?;
        let providers = providers.read()?;
        let typesetting = typesetting.read()?;
        Ok(Self {
            pipeline: pipeline.clone(),
            providers: ProviderPreferences::from_config(&providers)?,
            typesetting: typesetting.clone(),
            languages: Language::ALL
                .iter()
                .map(|language| LanguageChoice {
                    tag: language.tag().to_owned(),
                    name: language.to_string(),
                })
                .collect(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct ProviderPreferences {
    pub entries: Vec<ProviderPreference>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct ProviderPreference {
    pub name: String,
    pub config: ProviderConfig,
    pub credential: Option<CredentialInput>,
}

impl ProviderPreferences {
    fn from_config(config: &ProvidersConfig) -> Result<Self> {
        let entries = config
            .entries()
            .into_iter()
            .map(|config| {
                let provider = config.provider();
                let credential = if provider == Provider::Local {
                    None
                } else {
                    let key: &'static str = provider.into();
                    Some(CredentialInput::load(key)?)
                };
                Ok(ProviderPreference {
                    name: provider.name().to_owned(),
                    config,
                    credential,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { entries })
    }

    fn into_config(self) -> Result<ProvidersConfig> {
        let mut configs = Vec::with_capacity(self.entries.len());
        let mut credentials = Vec::with_capacity(self.entries.len().saturating_sub(1));
        for entry in self.entries {
            let provider = entry.config.provider();
            match entry.credential {
                None if provider == Provider::Local => {}
                Some(credential) if provider != Provider::Local => {
                    let key: &'static str = provider.into();
                    credentials.push((key, credential));
                }
                None => anyhow::bail!("missing credential input for {provider}"),
                Some(_) => anyhow::bail!("local translation does not accept credentials"),
            }
            configs.push(entry.config);
        }
        let config = ProvidersConfig::from_entries(configs)?;
        for (key, credential) in credentials {
            credential.save(key)?;
        }
        Ok(config)
    }
}

#[derive(Clone, Default, Deserialize, Serialize, Type)]
pub struct CredentialInput {
    pub configured: bool,
    pub value: Option<String>,
    pub clear: bool,
}

impl fmt::Debug for CredentialInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialInput")
            .field("configured", &self.configured)
            .field("value", &self.value.as_ref().map(|_| "[REDACTED]"))
            .field("clear", &self.clear)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app as it starts on a machine that already saved a preference: it
    /// reads `~/.koharu/config.toml`, so a model left over from an older
    /// default would silently outrank the one measured in
    /// `bench-profile/ja-quality.md`. Ignored: it needs that machine state, the
    /// GGUF in the store, and a GPU.
    #[tokio::test]
    #[ignore = "reads ~/.koharu/config.toml and loads weights: needs machine state and a GPU"]
    async fn saved_preferences_translate_with_the_winner() {
        let preferences = Preferences::load().expect("app preferences");
        let selection = preferences.pipeline.translation.model.clone();
        println!(
            "koharu saved preference resolves: {} {} (quantization {})",
            selection.provider,
            selection.model.as_deref().unwrap_or("<provider default>"),
            selection.quantization.as_deref().unwrap_or("<default>"),
        );
        assert_eq!(selection.provider, Provider::Local);
        assert_eq!(selection.model.as_deref(), Some("hy-mt2-7b"));
        koharu_ml::init().await.expect("ml runtime init");
        let translator = koharu_translator::Translator::from_config(
            koharu_ml::device(false),
            ProvidersConfig::load().expect("providers configuration"),
        )
        .expect("build the translator");
        let request = koharu_translator::TranslationRequest::new(
            ["まずい、財布を忘れた。"],
            Language::French,
        );
        let (provider, texts) = translator
            .translate(
                &selection,
                preferences.pipeline.translation.generation,
                request,
            )
            .await
            .expect("translate with the saved model");
        println!("{provider}: {}", texts[0]);
        assert!(!texts[0].is_empty());
    }

    /// The model the app translates with when nobody picked one: no saved
    /// preference, a fresh install. It must stay on the model that won the
    /// Japanese→French measurement (`bench-profile/ja-quality.md`), because
    /// every other entry point follows this default.
    #[test]
    fn the_default_translation_path_resolves_the_measured_winner() {
        let config = PipelineConfig::default();
        let selection = &config.translation.model;
        assert_eq!(selection.provider, Provider::Local);
        assert_eq!(selection.model.as_deref(), Some("hy-mt2-7b"));
        assert_eq!(selection.quantization.as_deref(), Some("Q4_K_M"));
        assert!(
            !koharu_translator::Translator::supports_vision(
                selection,
                &config.translation.generation
            ),
            "the winner is text-only; the page image must not be claimed"
        );
    }

    /// A real run of the app's default translation path: loads the default
    /// weights, translates one Japanese line and prints what it resolved.
    /// Ignored by default because it needs a GPU and the GGUF in the store.
    #[tokio::test]
    #[ignore = "loads the default model and translates: needs a GPU and the GGUF"]
    async fn the_default_translation_path_translates_with_the_winner() {
        koharu_ml::init().await.expect("ml runtime init");
        let config = PipelineConfig::default();
        let selection = config.translation.model.clone();
        println!(
            "koharu default translation model: {} {} (quantization {})",
            selection.provider,
            selection.model.as_deref().unwrap_or("<provider default>"),
            selection.quantization.as_deref().unwrap_or("<default>"),
        );
        let translator = koharu_translator::Translator::from_config(
            koharu_ml::device(false),
            ProvidersConfig::load().expect("providers configuration"),
        )
        .expect("build the translator");
        let request = koharu_translator::TranslationRequest::new(
            ["まずい、財布を忘れた。", "甘いものは正義！"],
            Language::French,
        );
        let (provider, texts) = translator
            .translate(&selection, config.translation.generation, request)
            .await
            .expect("translate with the default model");
        for text in &texts {
            println!("{provider}: {text}");
        }
        assert_eq!(texts.len(), 2);
        assert!(
            texts.iter().all(|text| !text
                .chars()
                .any(|character| ('\u{3040}'..='\u{30ff}').contains(&character))),
            "a French rendering must not keep kana: {texts:?}"
        );
    }
}

impl CredentialInput {
    fn load(key: &str) -> Result<Self> {
        Ok(Self {
            configured: koharu_secrets::get(key)?
                .is_some_and(|secret| !secret.expose_secret().trim().is_empty()),
            value: None,
            clear: false,
        })
    }

    fn save(self, key: &str) -> Result<()> {
        if self.clear {
            koharu_secrets::delete(key)?;
        } else if let Some(value) = self.value {
            if value.trim().is_empty() {
                koharu_secrets::delete(key)?;
            } else {
                koharu_secrets::set(key, &value.into())?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LanguageChoice {
    pub tag: String,
    pub name: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn save_preferences(
    mut pipeline: PipelineConfig,
    providers: ProviderPreferences,
    typesetting: TypesettingConfig,
) -> std::result::Result<Preferences, Error> {
    remember_pipeline_profiles(&mut pipeline);
    let providers = providers.into_config()?;
    let pipeline_config = PipelineConfig::load()?;
    let providers_config = ProvidersConfig::load()?;
    let typesetting_config = TypesettingConfig::load()?;
    {
        let mut current = pipeline_config.write()?;
        *current = pipeline;
        current.save()?;
    }
    {
        let mut current = providers_config.write()?;
        *current = providers;
        current.save()?;
    }
    {
        let mut current = typesetting_config.write()?;
        *current = typesetting;
        current.save()?;
    }
    Ok(Preferences::load()?)
}

fn remember_pipeline_profiles(config: &mut PipelineConfig) {
    let koharu_pipeline::DetectionModel::KoharuLayoutRFDetrSeg2XL(settings) = &config.detection;
    config.processor.koharu_layout_rfdetr_seg_2xl = Some(settings.clone());
    if let koharu_pipeline::InpaintingModel::Flux2Klein(settings) = &config.inpainting {
        config.processor.flux2_klein = Some(settings.clone());
    }
    if let koharu_pipeline::InpaintingModel::QwenImage(settings) = &config.inpainting {
        config.processor.qwen_image = Some(settings.clone());
    }
    if let koharu_pipeline::InpaintingModel::QwenImageUncensored(settings) = &config.inpainting {
        config.processor.qwen_image_uncensored = Some(settings.clone());
    }
    if let koharu_pipeline::InpaintingModel::RoremMixed(settings) = &config.inpainting {
        config.processor.rorem_mixed = Some(settings.clone());
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_preferences() -> std::result::Result<Preferences, Error> {
    Ok(Preferences::load()?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_translation_models() -> std::result::Result<Vec<Model>, Error> {
    Ok(koharu_translator::Translator::models().await?)
}
