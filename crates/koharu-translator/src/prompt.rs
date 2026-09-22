use std::fmt::Write;

use anyhow::Context;
use indoc::indoc;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};

use crate::{Language, TranslationContext, TranslationRequest};

pub(crate) fn prompts(request: &TranslationRequest) -> anyhow::Result<(String, String)> {
    let input = TranslationInput {
        source_language: request.source_language,
        target_language: request.target_language,
        context: &request.context,
        segments: request
            .segments
            .iter()
            .enumerate()
            .map(|(id, text)| TranslationInputSegment { id, text })
            .collect(),
    };
    let user = serde_json::to_string(&input).context("failed to serialize translation input")?;
    Ok((translation_system_prompt(request), user))
}

/// Parsed translator response, with the segments it failed to translate.
#[derive(Debug)]
pub(crate) struct Translations {
    /// One entry per input segment, in input order. A segment the provider did
    /// not return keeps its source text so callers still get an entry for it.
    pub(crate) texts: Vec<String>,
    /// Input indices whose entry is not a translation: missing from the
    /// response, empty, or still written in the source language.
    pub(crate) untranslated: Vec<usize>,
}

impl Translations {
    /// A response that translated every segment (translation APIs that cannot
    /// omit one, such as DeepL or Google Cloud).
    pub(crate) fn complete(texts: Vec<String>) -> Self {
        Self {
            texts,
            untranslated: Vec::new(),
        }
    }
}

pub(crate) fn translations(
    provider: &str,
    text: &str,
    request: &TranslationRequest,
) -> anyhow::Result<Translations> {
    let output = serde_json::from_str::<TranslationOutput>(text).with_context(|| {
        format!(
            "{provider} returned invalid translation JSON for {} segments; response was: {}",
            request.segments.len(),
            snippet(text),
        )
    })?;
    let mut texts = request.segments.clone();
    let mut translated = vec![false; texts.len()];

    for translation in output.translations {
        if translation.id < texts.len() && !translated[translation.id] {
            texts[translation.id] = translation.text;
            translated[translation.id] = true;
        }
    }

    let untranslated = (0..texts.len())
        .filter(|&index| {
            !translated[index]
                || is_untranslated(
                    &request.segments[index],
                    &texts[index],
                    request.target_language,
                )
        })
        .collect();

    Ok(Translations {
        texts,
        untranslated,
    })
}

/// Follow-up request for the indices a first response left untranslated; it
/// keeps the chapter context and the caller's own instructions.
pub(crate) fn retry_request(request: &TranslationRequest, missing: &[usize]) -> TranslationRequest {
    let mut retry = request.clone();
    retry.segments = missing
        .iter()
        .filter_map(|index| request.segments.get(*index).cloned())
        .collect();
    retry.instructions = Some(
        match request
            .instructions
            .as_deref()
            .map(str::trim)
            .filter(|instructions| !instructions.is_empty())
        {
            Some(instructions) => format!("{instructions} {RETRY_INSTRUCTION}"),
            None => RETRY_INSTRUCTION.to_owned(),
        },
    );
    retry
}

/// Instruction added to a follow-up request.
const RETRY_INSTRUCTION: &str = "A previous response left these segments untranslated: \
translate every one of them into the target language, and never return the \
source text as the translation.";

/// Upper bound on the tokens one constrained-JSON response needs.
///
/// The schema forces one `{"id":…,"text":…}` entry per segment, so the budget
/// has to grow with the page: a fixed budget truncated the JSON of dense pages
/// and their tail came back untranslated.
pub(crate) fn output_budget(segments: &[String]) -> usize {
    /// Framing of one `{"id":0,"text":"…"}` entry, separator included.
    const JSON_TOKENS_PER_SEGMENT: usize = 24;
    const MIN_OUTPUT_TOKENS: usize = 768;
    const MAX_OUTPUT_TOKENS: usize = 2560;

    let estimate = segments
        .iter()
        .map(|segment| JSON_TOKENS_PER_SEGMENT + segment.chars().count())
        .sum::<usize>()
        + JSON_TOKENS_PER_SEGMENT;
    estimate.clamp(MIN_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS)
}

/// Whether `translated` still holds the source text instead of a translation.
///
/// A model that ran out of output budget or gave up echoes the input segment;
/// a Latin-script target that comes back with source-script letters was not
/// translated at all.
fn is_untranslated(source: &str, translated: &str, target: Language) -> bool {
    let translated = translated.trim();
    if translated.is_empty() {
        return true;
    }
    if !latin_script(target) {
        return false;
    }
    translated.chars().any(is_source_script)
        || (translated == source.trim() && source.chars().any(is_source_script))
}

/// Languages written with the Latin alphabet, where source-script letters left
/// in the output are a reliable sign that a segment was not translated.
fn latin_script(language: Language) -> bool {
    use Language::*;
    matches!(
        language,
        English
            | French
            | Portuguese
            | BrazilianPortuguese
            | Spanish
            | Italian
            | German
            | Dutch
            | Turkish
            | Polish
            | Czech
            | Hungarian
            | Vietnamese
            | Malay
            | Indonesian
            | Filipino
    )
}

/// Whether a character belongs to a script a Latin-script target never writes
/// with: CJK, Hangul, Cyrillic, Greek, Arabic, Hebrew, Thai, Khmer and the
/// Indic scripts.
fn is_source_script(character: char) -> bool {
    matches!(character as u32,
        0x0370..=0x03FF   // Greek
        | 0x0400..=0x04FF // Cyrillic
        | 0x0590..=0x05FF // Hebrew
        | 0x0600..=0x06FF // Arabic
        | 0x0900..=0x09FF // Devanagari, Bengali
        | 0x0A80..=0x0AFF // Gujarati
        | 0x0B80..=0x0BFF // Tamil
        | 0x0C00..=0x0C7F // Telugu
        | 0x0E00..=0x0E7F // Thai
        | 0x0F00..=0x0FFF // Tibetan
        | 0x1000..=0x109F // Myanmar
        | 0x1100..=0x11FF // Hangul Jamo
        | 0x1780..=0x17FF // Khmer
        | 0x3000..=0x30FF // CJK punctuation, Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF00..=0xFFEF // Halfwidth and fullwidth forms
        | 0x20000..=0x2FFFF // CJK extensions B and later
    )
}

/// Keeps a model response readable in a log line without truncating so hard
/// that the shape of the failure is lost.
///
/// Responses are usually pretty-printed, so line breaks and other control
/// characters are escaped rather than passed through: the snippet is formatted
/// into an error context, and one failure should stay one line.
pub(crate) fn snippet(text: &str) -> String {
    const LIMIT: usize = 2000;
    let trimmed = text.trim();
    let (visible, elided) = match trimmed.char_indices().nth(LIMIT) {
        Some((end, _)) => (&trimmed[..end], true),
        None => (trimmed, false),
    };

    let mut output = String::with_capacity(visible.len());
    for character in visible.chars() {
        match character {
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(output, "\\u{{{:04x}}}", control as u32);
            }
            character => output.push(character),
        }
    }
    if elided {
        let _ = write!(output, "… ({} bytes total)", trimmed.len());
    }
    output
}

pub(crate) fn output_schema(expected: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "translations": {
                "type": "array",
                "minItems": expected,
                "maxItems": expected,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "integer",
                            "minimum": 0,
                            "maximum": expected.saturating_sub(1),
                            "description": "The ID copied from the corresponding input segment."
                        },
                        "text": {
                            "type": "string",
                            "description": "The translation of the input segment with this ID."
                        }
                    },
                    "required": ["id", "text"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["translations"],
        "additionalProperties": false
    })
}

fn translation_system_prompt(request: &TranslationRequest) -> String {
    let source = request
        .source_language
        .map(|language| language.to_string())
        .unwrap_or_else(|| "the detected source language".to_owned());
    let mut prompt = format!(
        indoc! {"
            You are a professional manga translator.

            Translation requirements:
            - Translate every input segment from {source} into natural {target}.
            - Preserve meaning, character voice, emotional tone, relationship nuance, emphasis, and sound effects.
            - Localize idioms and sound effects naturally while keeping wording concise enough for speech bubbles.
            - Use surrounding segments only for disambiguation and continuity; never merge or split segments.
            - Write every translated `text` value only in {target}; do not include source text, notes, explanations, or alternatives.
            - Never preserve or repeat original-language text; translate names, terms, and sound effects using natural {target} conventions.

            Output requirements:
            - Each input segment has a numeric `id`.
            - Return only a JSON object whose `translations` array contains one object with `id` and translated `text` for every input segment.
            - Copy every input ID exactly once; order does not matter.
            - Never merge, split, omit, duplicate, or add segments.
        "},
        source = source,
        target = request.target_language,
    )
    .trim_end()
    .to_owned();

    if let Some(guidance) = source_guidance(request.source_language, request.target_language) {
        prompt.push_str("\n\n");
        prompt.push_str(&guidance);
    }
    if let Some(style) = target_style(request.target_language) {
        prompt.push_str("\n\n");
        prompt.push_str(style);
    }

    if !request.context.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(indoc! {"
            Context requirements:
            Use the supplied context only to preserve terminology, character voice, and dialogue continuity.
            Do not translate or return the context entries.
        "}.trim_end());
    }

    if request.image.is_some() {
        prompt.push_str("\n\n");
        prompt.push_str(indoc! {"
            Image requirements:
            Use the attached original page image as visual context for speaker identity, tone, layout, and ambiguous OCR.
            Translate only the supplied segments; do not add text seen in the image that is absent from the input segments.
        "}.trim_end());
    }

    if let Some(instructions) = request
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        prompt.push_str("\n\nAdditional instructions:\n");
        prompt.push_str(instructions);
    }
    prompt
}

/// Source-language guidance for a translation *into* `target`.
///
/// Japanese and Russian both carry meaning in morphology that a small model
/// tends to drop when rendering directly into a Latin-script language: the
/// honorific/particle layer in Japanese, verbal aspect and diminutives in
/// Russian. Calling that out explicitly is cheaper than a pivot through
/// English and keeps the single-pass pipeline intact.
///
/// Returns `None` for other sources so the prompt stays unchanged for the
/// languages that do not need it.
fn source_guidance(source: Option<Language>, target: Language) -> Option<String> {
    let source = source?;
    let guidance = match source {
        Language::Japanese => format!(
            indoc! {"
                Japanese source guidance:
                - Honorifics and name suffixes (-san, -kun, -sama, -sensei) signal rank and intimacy: render that relationship in {target} rather than dropping it or copying the suffix verbatim.
                - Sentence-final particles and speech-endings (ね, よ, ぞ, ぜ, か) carry tone: convey the tone in {target}, do not transliterate them.
                - Onomatopoeia and sound effects follow {target} manga conventions; localize them, and do not leave the kana in place.
            "},
            target = target,
        ),
        Language::Russian => format!(
            indoc! {"
                Russian source guidance:
                - Verbal aspect (completed vs ongoing) is often implied rather than stated: choose the {target} tense that preserves the aspect.
                - Diminutives and patronymics signal affection, familiarity, or respect: carry that nuance into {target} instead of using the bare name.
                - Flexible word order marks emphasis: keep the emphasis natural in {target} rather than mirroring the Russian order.
            "},
            target = target,
        ),
        _ => return None,
    };
    Some(guidance.trim_end().to_owned())
}

/// Target-language style guidance for `target`.
///
/// The base prompt only says "into natural {target}", which is too thin for a
/// language whose register, idiom, and punctuation differ sharply from the
/// English the prompt itself is written in. French is the case this repo cares
/// about (it also gets a typography pass in [`crate::typography`]), so it gets
/// an explicit style block; other targets keep the base prompt unchanged.
fn target_style(target: Language) -> Option<&'static str> {
    match target {
        Language::French => Some(indoc! {"
            French style requirements:
            - Write natural, idiomatic French as it would appear in a manga: favour spoken phrasing and contractions over literal wording.
            - Avoid anglicisms and calques; do not translate word for word.
            - Keep the register consistent with the character's tone (tu/vous, formal/informal) and the emotional beat of the scene.
            - Punctuation and spacing follow French conventions; the pipeline normalizes them afterwards, so do not add your own thin spaces.
        "}.trim_end()),
        _ => None,
    }
}

#[derive(Serialize)]
struct TranslationInput<'a> {
    source_language: Option<Language>,
    target_language: Language,
    context: &'a [TranslationContext],
    segments: Vec<TranslationInputSegment<'a>>,
}

#[derive(Serialize)]
struct TranslationInputSegment<'a> {
    id: usize,
    text: &'a str,
}

#[derive(Debug, Deserialize)]
struct TranslationOutput {
    translations: Vec<TranslationOutputSegment>,
}

#[derive(Debug, Deserialize)]
struct TranslationOutputSegment {
    #[serde(deserialize_with = "deserialize_segment_id")]
    id: usize,
    text: String,
}

fn deserialize_segment_id<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SegmentId {
        Number(usize),
        String(String),
    }

    match SegmentId::deserialize(deserializer)? {
        SegmentId::Number(id) => Ok(id),
        SegmentId::String(id) => id.trim().parse().map_err(de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Request around source segments, for the response parsers.
    fn request(segments: &[String], target: Language) -> TranslationRequest {
        TranslationRequest::new(segments.iter().cloned(), target)
    }

    #[test]
    fn snippet_keeps_a_response_on_one_line() {
        let flattened = snippet("{\n  \"translations\": [\n\t\"hello\"\n  ]\n}");

        assert!(!flattened.contains('\n'), "{flattened}");
        assert!(!flattened.contains('\t'), "{flattened}");
        assert_eq!(flattened, r#"{\n  "translations": [\n\t"hello"\n  ]\n}"#);
    }

    #[test]
    fn snippet_escapes_other_control_characters() {
        assert_eq!(snippet("before\u{7}after"), r"before\u{0007}after");
    }

    #[test]
    fn snippet_reports_the_length_it_elided() {
        let flattened = snippet(&"x".repeat(2_500));

        assert!(flattened.starts_with(&"x".repeat(2_000)));
        assert!(
            flattened.ends_with("\u{2026} (2500 bytes total)"),
            "{flattened}"
        );
    }

    #[test]
    fn parses_plain_json() {
        let source = ["one".to_owned(), "two".to_owned()];
        let response = r#"{"translations":[{"id":0,"text":"hello"},{"id":1,"text":"world"}]}"#;
        let parsed =
            translations("test", response, &request(&source, Language::Japanese)).unwrap();

        assert_eq!(parsed.texts, ["hello", "world"]);
        assert!(parsed.untranslated.is_empty());
    }

    #[test]
    fn rejects_wrapped_and_malformed_json() {
        let source = ["one".to_owned(), "two".to_owned()];
        for response in [
            "```json\n{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"}]}\n```",
            r#"{translations: [{id: 0, text: 'hello'}, {id: 1, text: 'world'},],}"#,
            r#"Here is the result: {"translations": [{"id": 0, "text": "hello"}, {"id": 1, "text": "world"},]}"#,
            "{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"",
        ] {
            assert!(
                translations("test", response, &request(&source, Language::Japanese)).is_err(),
                "{response}"
            );
        }
    }

    #[test]
    fn parse_error_preserves_the_root_failure_and_response() {
        let source = ["one".to_owned()];
        let response = "{\n  \"translations\": [{\"id\": 0}]\n}";
        let error = format!(
            "{:#}",
            translations("test", response, &request(&source, Language::Japanese))
                .expect_err("missing text should fail")
        );

        assert!(error.contains("missing field `text`"), "{error}");
        assert!(
            error.contains(r#"response was: {\n  "translations": [{"id": 0}]\n}"#),
            "{error}"
        );
    }

    #[test]
    fn restores_input_order_from_ids() {
        let source = ["one".to_owned(), "two".to_owned()];
        let response = r#"{"translations":[{"id":1,"text":"world"},{"id":0,"text":"hello"}]}"#;
        assert_eq!(
            translations("test", response, &request(&source, Language::Japanese))
                .unwrap()
                .texts,
            ["hello", "world"]
        );
    }

    #[test]
    fn tolerates_duplicate_missing_and_out_of_range_ids() {
        let source = ["one".to_owned(), "two".to_owned()];
        let short = r#"{"translations":[{"id":1,"text":"world"}]}"#;
        assert_eq!(
            translations("test", short, &request(&source, Language::Japanese))
                .unwrap()
                .texts,
            ["one", "world"]
        );

        let response = concat!(
            r#"{"translations":["#,
            r#"{"id":0,"text":"hello"},"#,
            r#"{"id":0,"text":"duplicate"},"#,
            r#"{"id":9,"text":"extra"}"#,
            "]}"
        );
        assert_eq!(
            translations("test", response, &request(&source, Language::Japanese))
                .unwrap()
                .texts,
            ["hello", "two"]
        );
    }

    #[test]
    fn missing_segments_are_reported_not_silently_echoed() {
        let source = ["one".to_owned(), "two".to_owned()];
        let response = r#"{"translations":[{"id":1,"text":"world"}]}"#;

        let parsed =
            translations("test", response, &request(&source, Language::Japanese)).unwrap();
        assert_eq!(parsed.texts, ["one", "world"]);
        assert_eq!(parsed.untranslated, [0]);
    }

    #[test]
    fn source_text_in_a_latin_target_is_reported() {
        let source = ["こんにちは".to_owned()];
        let response = r#"{"translations":[{"id":0,"text":"こんにちは"}]}"#;

        let parsed =
            translations("test", response, &request(&source, Language::French)).unwrap();
        assert_eq!(parsed.untranslated, [0]);
    }

    #[test]
    fn latin_output_that_matches_the_source_is_not_flagged() {
        let source = ["OK".to_owned()];
        let response = r#"{"translations":[{"id":0,"text":"OK"}]}"#;

        let parsed =
            translations("test", response, &request(&source, Language::French)).unwrap();
        assert!(parsed.untranslated.is_empty(), "{:?}", parsed.untranslated);
    }

    #[test]
    fn the_output_budget_grows_with_the_page_and_stays_bounded() {
        assert_eq!(output_budget(&["hello".to_owned()]), 768);

        let dense = vec!["a sentence that is reasonably long".to_owned(); 40];
        assert!(output_budget(&dense) > 1000);
        assert!(output_budget(&dense) <= 2560);

        let huge = vec!["x".repeat(200); 40];
        assert_eq!(output_budget(&huge), 2560, "the budget must stay bounded");
    }

    #[test]
    fn a_retry_keeps_the_context_and_the_caller_instructions() {
        let request = TranslationRequest::new(["one", "two"], Language::French)
            .with_source_language(Language::Japanese)
            .with_context([TranslationContext::new("source", "traduction")])
            .with_instructions("Use informal speech.");
        let retry = retry_request(&request, &[1]);

        assert_eq!(retry.segments, ["two"]);
        assert_eq!(retry.source_language, Some(Language::Japanese));
        assert_eq!(retry.context, request.context);
        let instructions = retry.instructions.as_deref().unwrap_or_default();
        assert!(instructions.contains("Use informal speech."), "{instructions}");
        assert!(
            instructions.contains("never return the source text"),
            "{instructions}"
        );
        assert!(translation_system_prompt(&retry).contains("left these segments untranslated"));
    }

    #[test]
    fn prompt_payload_contains_ordered_context() {
        let request = TranslationRequest::new(["new"], Language::English)
            .with_context([TranslationContext::new("old", "previous")]);
        let (_, user) = prompts(&request).unwrap();
        let input: serde_json::Value = serde_json::from_str(&user).unwrap();
        assert_eq!(input["context"][0]["source"], "old");
        assert_eq!(input["context"][0]["translation"], "previous");
        assert_eq!(input["segments"][0]["id"], 0);
        assert_eq!(input["segments"][0]["text"], "new");
    }

    #[test]
    fn system_prompt_encodes_invariants_and_custom_instructions() {
        let request = TranslationRequest::new(["hello"], Language::Korean)
            .with_source_language(Language::Japanese)
            .with_instructions("Use informal speech.");
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("from Japanese into natural Korean"));
        assert!(prompt.contains("Copy every input ID exactly once"));
        assert!(prompt.contains("Use informal speech."));
    }

    #[test]
    fn japanese_source_gets_source_guidance_name_for_the_target() {
        let request = TranslationRequest::new(["こんにちは"], Language::French)
            .with_source_language(Language::Japanese);
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("Japanese source guidance"), "{prompt}");
        assert!(
            prompt.contains("render that relationship in French"),
            "{prompt}"
        );
        assert!(prompt.contains("localize them"), "{prompt}");
    }

    #[test]
    fn russian_source_gets_aspect_and_diminutive_guidance() {
        let request = TranslationRequest::new(["привет"], Language::French)
            .with_source_language(Language::Russian);
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("Russian source guidance"), "{prompt}");
        assert!(prompt.contains("preserves the aspect"), "{prompt}");
        assert!(prompt.contains("carry that nuance into French"), "{prompt}");
    }

    #[test]
    fn source_guidance_is_scoped_to_japanese_and_russian() {
        let english = translation_system_prompt(
            &TranslationRequest::new(["hello"], Language::French)
                .with_source_language(Language::English),
        );
        assert!(!english.contains("source guidance"), "{english}");

        let unknown =
            translation_system_prompt(&TranslationRequest::new(["hello"], Language::French));
        assert!(!unknown.contains("source guidance"), "{unknown}");
    }

    #[test]
    fn french_target_gets_a_style_block_and_other_targets_do_not() {
        let french = translation_system_prompt(&TranslationRequest::new(["hi"], Language::French));
        assert!(french.contains("French style requirements"), "{french}");
        assert!(french.contains("Avoid anglicisms"), "{french}");

        let english =
            translation_system_prompt(&TranslationRequest::new(["hi"], Language::English));
        assert!(!english.contains("French style requirements"), "{english}");
    }

    #[test]
    fn guidance_is_appended_before_context_and_image_requirements() {
        let request = TranslationRequest::new(["text"], Language::French)
            .with_source_language(Language::Japanese)
            .with_context([TranslationContext::new("old", "traduction")])
            .with_image(std::sync::Arc::new(image::DynamicImage::new_rgb8(1, 1)));
        let prompt = translation_system_prompt(&request);

        let guidance = prompt
            .find("Japanese source guidance")
            .expect("guidance present");
        let style = prompt
            .find("French style requirements")
            .expect("style present");
        let context = prompt
            .find("Context requirements")
            .expect("context present");
        let image = prompt.find("Image requirements").expect("image present");
        assert!(
            guidance < style && style < context && context < image,
            "{prompt}"
        );
    }

    #[test]
    fn schema_requires_the_expected_number_of_id_text_pairs() {
        let schema = output_schema(3);
        let translations = &schema["properties"]["translations"];
        assert_eq!(translations["minItems"], 3);
        assert_eq!(translations["maxItems"], 3);
        assert_eq!(translations["items"]["properties"]["id"]["minimum"], 0);
        assert_eq!(translations["items"]["properties"]["id"]["maximum"], 2);
        assert_eq!(translations["items"]["additionalProperties"], false);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn empty_custom_instructions_are_ignored() {
        let request = TranslationRequest::new(["hello"], Language::English).with_instructions("  ");
        assert!(!translation_system_prompt(&request).contains("Additional instructions"));
    }

    #[test]
    fn context_is_reference_only() {
        let request = TranslationRequest::new(["Where is she?"], Language::Japanese)
            .with_context([TranslationContext::new("I saw Alice.", "アリスを見た。")]);
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("dialogue continuity"));
        assert!(prompt.contains("Do not translate or return the context"));
    }

    #[test]
    fn image_context_does_not_expand_the_translation_scope() {
        let request = TranslationRequest::new(["text"], Language::English)
            .with_image(std::sync::Arc::new(image::DynamicImage::new_rgb8(1, 1)));
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("attached original page image"));
        assert!(prompt.contains("Translate only the supplied segments"));
    }
}
