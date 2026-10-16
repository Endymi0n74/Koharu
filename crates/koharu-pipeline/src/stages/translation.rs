use anyhow::Result;
use async_trait::async_trait;
use koharu_scene::{
    Authored, DocumentProfile, LanguageTag, Origin, ProfileTerm as SceneProfileTerm, SourceText,
    Translation,
};
use koharu_translator::{
    ProfileRequest, ProfileTerm, TranslationContext, TranslationProfile, TranslationRequest,
    Translator, normalize_segment,
};

use crate::TranslationConfig;

use super::{StageInput, StageProcessor, finish, generation};

const PRODUCER: &str = "dev.koharu.pipeline.translation";

/// Lines of earlier pages handed to the translator as continuity context: enough
/// to carry names, tone, and recurring phrases across a few page boundaries,
/// small enough to keep a page's prompt cheap.
const CONTEXT_LINES: usize = 12;
/// Longest context line kept, so a single sprawling OCR block cannot crowd out
/// the segments actually being translated.
const CONTEXT_LINE_CHARS: usize = 160;
/// Longest chapter text handed to the one profile call, in characters. Reading
/// order is kept, so the analysis still sees the opening of the chapter.
const PROFILE_MAX_CHARS: usize = 12_000;

pub(super) struct Processor {
    config: TranslationConfig,
    translator: Translator,
}

impl Processor {
    pub(super) fn new(config: TranslationConfig, translator: Translator) -> Self {
        Self { config, translator }
    }

    /// The chapter dossier every page's request carries.
    ///
    /// A stored [`DocumentProfile`] wins; when the project has none yet (a
    /// first batch run, say), the chapter's own OCR text is read once,
    /// described by the translation model, and returned so the caller can
    /// persist it. A failing analysis never fails the page: it is logged, and
    /// the page is translated with continuity context alone.
    async fn document_profile(
        &self,
        scene: &koharu_scene::Snapshot,
        source_language: Option<koharu_translator::Language>,
    ) -> Result<(Option<TranslationProfile>, Option<DocumentProfile>)> {
        if let Some(stored) = scene.project_component::<DocumentProfile>()? {
            let profile = to_translation_profile(&stored);
            return Ok(((!profile.is_empty()).then_some(profile), None));
        }
        let segments = chapter_segments(scene)?;
        if segments.is_empty() {
            return Ok((None, None));
        }
        let mut request = ProfileRequest::new(segments, self.config.target_language);
        if let Some(language) = source_language {
            request = request.with_source_language(language);
        }
        match self
            .translator
            .profile(&self.config.model, self.config.generation, request)
            .await
        {
            Ok(Some(profile)) => {
                tracing::info!(
                    characters = profile.characters.len(),
                    glossary = profile.glossary.len(),
                    "learned the chapter's document profile"
                );
                Ok((Some(profile.clone()), Some(to_document_profile(&profile))))
            }
            Ok(None) => Ok((None, None)),
            Err(error) => {
                tracing::warn!(%error, "document profile analysis failed; translating without it");
                Ok((None, None))
            }
        }
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> &'static str {
        Translator::model(&self.config.model)
    }

    fn unload(&self) -> bool {
        self.translator.unload()
    }

    async fn load(&self) -> Result<()> {
        self.translator.load_model(&self.config.model).await
    }

    async fn process(&self, input: StageInput) -> Result<koharu_scene::Patch> {
        let mut targets = Vec::new();
        let mut source_language = None;
        if let Some(group) = input.scene.page(input.page)?.text_group()? {
            for layer in group.text_layers()? {
                if !input.contains_entity(layer.id())? {
                    continue;
                }
                let content = layer.content()?;
                let Some(source) = content.source()? else {
                    continue;
                };
                if !source.text.value.trim().is_empty() {
                    source_language = source_language.or_else(|| {
                        source
                            .language
                            .as_ref()
                            .and_then(|tag| tag.as_str().parse().ok())
                    });
                    targets.push((content.id(), source.text.value));
                }
            }
        }
        let (profile, learned) = self.document_profile(&input.scene, source_language).await?;

        let mut request = TranslationRequest::new(
            targets.iter().map(|(_, source)| source.clone()),
            self.config.target_language,
        );
        if let Some(language) = source_language {
            // OCR records the source language it recognized; telling the
            // translator avoids guessing it from the segments alone.
            request = request.with_source_language(language);
        }
        if let Some(profile) = profile {
            request = request.with_profile(profile);
        }
        let context = chapter_context(&input.scene, input.page)?;
        if !context.is_empty() {
            request = request.with_context(context);
        }
        if let Some(instructions) = self.config.instructions.as_deref() {
            request = request.with_instructions(instructions);
        }
        if Translator::supports_vision(&self.config.model, &self.config.generation)
            && let Some(image) = input.images.get(&input.scene, input.page, "source").await?
        {
            request = request.with_image(image);
        }
        let (provider, translated) = self
            .translator
            .translate(&self.config.model, self.config.generation, request)
            .await?;
        let language = LanguageTag::new(self.config.target_language.tag())?;
        let generated = generation(PRODUCER, provider)?;
        let mut edit = input.scene.edit_as(generated.clone());
        if let Some(learned) = learned {
            // Persist the dossier so later pages, later runs, and the app all
            // read the same profile instead of re-deriving it.
            edit.set_project(&learned)?;
        }
        for (entity, _) in &targets {
            edit.observe::<SourceText>(*entity)?;
            edit.observe::<Translation>(*entity)?;
        }
        for ((entity, source), text) in targets.into_iter().zip(translated) {
            if input
                .scene
                .component::<Translation>(entity)?
                .is_some_and(|value| matches!(value.text.origin, Origin::User))
            {
                continue;
            }
            let text = if source.trim() == "\u{2026}" {
                "\u{2026}".to_owned()
            } else {
                normalize_segment(self.config.typography, self.config.target_language, &text)
            };
            edit.set(
                entity,
                &Translation {
                    text: Authored::generated(text, generated.clone()),
                    language: Some(language.clone()),
                },
            )?;
        }
        finish(edit)
    }
}

/// Source/translation pairs from the pages that precede `page` in reading
/// order: earlier pages of the same chapter carry the terminology, character
/// voice, and continuity the translator would otherwise re-invent per page.
fn chapter_context(
    scene: &koharu_scene::Snapshot,
    page: koharu_scene::EntityId,
) -> Result<Vec<TranslationContext>> {
    let mut context = Vec::new();
    for candidate in scene.pages() {
        if candidate.id() == page {
            break;
        }
        let Some(group) = candidate.text_group()? else {
            continue;
        };
        for layer in group.text_layers()? {
            let content = layer.content()?;
            let Some(source) = content.source()? else {
                continue;
            };
            let Some(translation) = content.translation()? else {
                continue;
            };
            let source = clip(source.text.value.trim());
            let translation = clip(translation.text.value.trim());
            if source.is_empty() || translation.is_empty() || source == translation {
                continue;
            }
            context.push(TranslationContext {
                source,
                translation,
            });
        }
    }
    // Keep the lines closest to the page being translated.
    if context.len() > CONTEXT_LINES {
        context.drain(..context.len() - CONTEXT_LINES);
    }
    Ok(context)
}

fn clip(text: &str) -> String {
    let mut clipped: String = text.chars().take(CONTEXT_LINE_CHARS).collect();
    if clipped.len() < text.len() {
        clipped.push('…');
    }
    clipped
}

/// Reads the chapter's source text in reading order, bounded so the analysis
/// prompt stays affordable on a long chapter.
fn chapter_segments(scene: &koharu_scene::Snapshot) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    let mut chars = 0_usize;
    for page in scene.pages() {
        let Some(group) = page.text_group()? else {
            continue;
        };
        for layer in group.text_layers()? {
            let content = layer.content()?;
            let Some(source) = content.source()? else {
                continue;
            };
            let text = source.text.value.trim();
            if text.is_empty() {
                continue;
            }
            chars += text.chars().count();
            segments.push(text.to_owned());
            if chars >= PROFILE_MAX_CHARS {
                return Ok(segments);
            }
        }
    }
    Ok(segments)
}

fn to_translation_profile(profile: &DocumentProfile) -> TranslationProfile {
    TranslationProfile {
        series: profile.series.clone(),
        synopsis: profile.synopsis.clone(),
        register: profile.register.clone(),
        characters: profile.characters.iter().map(to_translation_term).collect(),
        glossary: profile.glossary.iter().map(to_translation_term).collect(),
    }
}

fn to_translation_term(term: &SceneProfileTerm) -> ProfileTerm {
    ProfileTerm {
        source: term.source.clone(),
        target: term.target.clone(),
        note: term.note.clone(),
    }
}

fn to_document_profile(profile: &TranslationProfile) -> DocumentProfile {
    DocumentProfile {
        // The pipeline stamp overwrites this with the edit's generation.
        origin: Origin::User,
        series: profile.series.clone(),
        synopsis: profile.synopsis.clone(),
        register: profile.register.clone(),
        characters: profile.characters.iter().map(to_document_term).collect(),
        glossary: profile.glossary.iter().map(to_document_term).collect(),
    }
}

fn to_document_term(term: &ProfileTerm) -> SceneProfileTerm {
    SceneProfileTerm {
        source: term.source.clone(),
        target: term.target.clone(),
        note: term.note.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::{
        At, Authored, PageDraft, Session, SourceText, TextLayout, TextLayoutKind, Translation,
    };

    /// Adds a page whose text group holds one layer per `(source, translation)`
    /// line; a line without a translation stays untranslated.
    async fn add_page(
        session: &mut Session,
        label: &str,
        lines: &[(String, Option<String>)],
    ) -> koharu_scene::EntityId {
        let mut page = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let id = edit.add_page(PageDraft::new(label, 1.0, 1.0), At::End)?;
                for (source, translation) in lines {
                    let content = edit.add_text_content(id, At::End)?;
                    edit.add_text_layer(
                        id,
                        At::End,
                        content,
                        &TextLayout {
                            origin: Origin::User,
                            kind: TextLayoutKind::Paragraph,
                            angle_degrees: None,
                        },
                    )?;
                    edit.set(
                        content,
                        &SourceText {
                            text: Authored::user(source.clone()),
                            language: Some(LanguageTag::new("ja-JP")?),
                        },
                    )?;
                    if let Some(translation) = translation {
                        edit.set(
                            content,
                            &Translation {
                                text: Authored::user(translation.clone()),
                                language: Some(LanguageTag::new("fr-FR")?),
                            },
                        )?;
                    }
                }
                page = Some(id);
                Ok(())
            })
            .unwrap();
        session.commit(patch).await.unwrap();
        page.expect("the edit assigns the page id")
    }

    fn line(source: &str, translation: Option<&str>) -> (String, Option<String>) {
        (source.to_owned(), translation.map(str::to_owned))
    }

    #[tokio::test]
    async fn chapter_context_carries_the_previous_pages_forward() {
        let mut session = Session::memory().await.unwrap();
        let first = add_page(
            &mut session,
            "one",
            &[line("こんにちは", Some("Bonjour")), line("未翻訳", None)],
        )
        .await;
        let _second = add_page(
            &mut session,
            "two",
            &[
                line("ありがとう", Some("Merci")),
                line("同じ訳", Some("同じ訳")),
            ],
        )
        .await;
        let third = add_page(&mut session, "three", &[line("さようなら", None)]).await;

        let snapshot = session.snapshot();
        assert_eq!(
            chapter_context(&snapshot, third).unwrap(),
            vec![
                TranslationContext {
                    source: "こんにちは".to_owned(),
                    translation: "Bonjour".to_owned(),
                },
                TranslationContext {
                    source: "ありがとう".to_owned(),
                    translation: "Merci".to_owned(),
                },
            ],
            "only translated lines of earlier pages become context"
        );
        assert!(
            chapter_context(&snapshot, first).unwrap().is_empty(),
            "the first page has no earlier page to draw context from"
        );
    }

    #[tokio::test]
    async fn chapter_context_keeps_the_lines_closest_to_the_page() {
        let mut session = Session::memory().await.unwrap();
        let lines = (0..CONTEXT_LINES + 4)
            .map(|index| line(&format!("source-{index}"), Some(&format!("target-{index}"))))
            .collect::<Vec<_>>();
        add_page(&mut session, "one", &lines).await;
        let second = add_page(&mut session, "two", &[line("今", None)]).await;

        let context = chapter_context(&session.snapshot(), second).unwrap();
        assert_eq!(context.len(), CONTEXT_LINES);
        assert_eq!(context.first().unwrap().source, "source-4");
        assert_eq!(context.last().unwrap().source, "source-15");
    }

    #[test]
    fn long_context_lines_are_clipped() {
        assert_eq!(clip("短い"), "短い");
        let clipped = clip(&"a".repeat(CONTEXT_LINE_CHARS + 1));
        assert_eq!(clipped.chars().count(), CONTEXT_LINE_CHARS + 1);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn the_document_profile_round_trips_through_the_request_type() {
        let profile = TranslationProfile {
            series: Some("Musashino Shijin".to_owned()),
            synopsis: Some("Un folkloriste.".to_owned()),
            register: None,
            characters: vec![ProfileTerm {
                source: "柳田".to_owned(),
                target: "Yanagita".to_owned(),
                note: None,
            }],
            glossary: vec![ProfileTerm {
                source: "貴族院".to_owned(),
                target: "la Chambre des pairs".to_owned(),
                note: Some("institution".to_owned()),
            }],
        };

        assert_eq!(
            to_translation_profile(&to_document_profile(&profile)),
            profile
        );
    }

    #[tokio::test]
    async fn chapter_segments_read_every_page_in_order() {
        let mut session = Session::memory().await.unwrap();
        add_page(&mut session, "one", &[line("いち", None), line("に", None)]).await;
        add_page(&mut session, "two", &[line("さん", Some("trois"))]).await;

        assert_eq!(
            chapter_segments(&session.snapshot()).unwrap(),
            ["いち", "に", "さん"],
            "the analysis pass sees the chapter's own text, translations or not"
        );
    }

    fn processor() -> Processor {
        let translator = Translator::from_config(
            koharu_ml::device(true),
            koharu_config::Config::memory(koharu_translator::ProvidersConfig::default()),
        )
        .unwrap();
        Processor::new(TranslationConfig::default(), translator)
    }

    #[tokio::test]
    async fn a_stored_document_profile_is_attached_without_re_running_the_analysis() {
        let mut session = Session::memory().await.unwrap();
        add_page(&mut session, "one", &[line("柳田先生", None)]).await;
        let stored = DocumentProfile {
            series: Some("Musashino Shijin".to_owned()),
            characters: vec![SceneProfileTerm {
                source: "柳田".to_owned(),
                target: "Yanagita".to_owned(),
                note: None,
            }],
            ..DocumentProfile::default()
        };
        let patch = session
            .snapshot()
            .patch(|edit| edit.set_project(&stored))
            .unwrap();
        session.commit(patch).await.unwrap();

        let (profile, learned) = processor()
            .document_profile(
                &session.snapshot(),
                Some(koharu_translator::Language::Japanese),
            )
            .await
            .unwrap();

        assert!(learned.is_none(), "a stored profile is never re-analyzed");
        let profile = profile.expect("the stored profile reaches the request");
        assert_eq!(profile.series.as_deref(), Some("Musashino Shijin"));
        assert_eq!(profile.characters[0].target, "Yanagita");
    }

    #[tokio::test]
    async fn an_empty_stored_document_profile_is_not_attached() {
        let mut session = Session::memory().await.unwrap();
        add_page(&mut session, "one", &[line("未翻訳", None)]).await;
        let patch = session
            .snapshot()
            .patch(|edit| edit.set_project(&DocumentProfile::default()))
            .unwrap();
        session.commit(patch).await.unwrap();

        let (profile, learned) = processor()
            .document_profile(&session.snapshot(), None)
            .await
            .unwrap();

        assert!(profile.is_none() && learned.is_none());
    }
}
