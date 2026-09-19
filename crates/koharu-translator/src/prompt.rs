use anyhow::Context;
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
    let output = crate::json::from_str::<TranslationOutput>(text)
        .with_context(|| format!("{provider} returned invalid translation JSON"))?;
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
/// The schema forces one `{\"id\":…,\"text\":…}` entry per segment, so the
/// budget has to grow with the page: a fixed budget truncated the JSON of
/// dense pages and their tail came back untranslated.
pub(crate) fn output_budget(segments: &[String]) -> usize {
    /// Framing of one `{\"id\":0,\"text\":\"…\"}` entry, separator included.
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
        concat!(
            "You are a professional manga translator. ",
            "Translate every input segment from {source} into natural {target}. ",
            "Preserve character voice, emotional tone, relationship nuance, emphasis, and sound ",
            "effects while keeping wording concise enough for speech bubbles. ",
            "Write every returned `text` in {target}: never copy a source segment as its ",
            "translation, and never leave a segment untranslated. ",
            "Each input segment has a numeric `id`. Return only a JSON object whose ",
            "`translations` array contains one object with `id` and translated `text` for every ",
            "input segment. Copy every input ID exactly once; order does not matter. Never merge, ",
            "split, omit, or add segments."
        ),
        source = source,
        target = request.target_language,
    );

    if !request.context.is_empty() {
        prompt.push_str(
            " The `context` array holds earlier lines of this same chapter with the translation \
used for them: keep names, places, and recurring phrases consistent with those, so the \
chapter reads as one continuous translation. Do not translate or return the context \
entries.",
        );
    }

    if request.image.is_some() {
        prompt.push_str(
            " Use the attached original page image as visual context for speaker identity, tone, layout, and ambiguous OCR. Translate only the supplied segments; do not add text seen in the image that is absent from the input segments.",
        );
    }

    if let Some(instructions) = request
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        prompt.push_str(" Additional instructions: ");
        prompt.push_str(instructions);
    }
    prompt
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

    fn request(segments: &[&str], target: Language) -> TranslationRequest {
        TranslationRequest::new(segments.iter().copied(), target)
    }

    #[test]
    fn parses_plain_json_and_markdown_fences() {
        let request = request(&["one", "two"], Language::French);
        let expected = vec!["hello".to_owned(), "world".to_owned()];
        for response in [
            r#"{"translations":[{"id":0,"text":"hello"},{"id":1,"text":"world"}]}"#,
            "```json\n{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"}]}\n```",
            "```JSON\n{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"}]}\n```",
            "```\n{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"}]}\n```",
        ] {
            let parsed = translations("test", response, &request).unwrap();
            assert_eq!(parsed.texts, expected);
            assert!(parsed.untranslated.is_empty());
        }
    }

    #[test]
    fn repairs_malformed_llm_json() {
        let request = request(&["one", "two"], Language::French);
        let expected = vec!["hello".to_owned(), "world".to_owned()];
        for response in [
            r#"{translations: [{id: 0, text: 'hello'}, {id: 1, text: 'world'},],}"#,
            r#"Here is the result: {"translations": [{"id": 0, "text": "hello"}, {"id": 1, "text": "world"},]}"#,
            "{\"translations\":[{\"id\":0,\"text\":\"hello\"},{\"id\":1,\"text\":\"world\"",
            r#"{"translations":[{"id":"0","text":"hello"},{"id":"1","text":"world"}]}"#,
        ] {
            let parsed = translations("test", response, &request).unwrap();
            assert_eq!(parsed.texts, expected);
            assert!(parsed.untranslated.is_empty());
        }
    }

    #[test]
    fn restores_input_order_from_ids() {
        let request = request(&["one", "two"], Language::French);
        let response = r#"{"translations":[{"id":1,"text":"world"},{"id":0,"text":"hello"}]}"#;
        assert_eq!(
            translations("test", response, &request).unwrap().texts,
            ["hello", "world"]
        );
    }

    #[test]
    fn missing_segments_are_reported_not_silently_echoed() {
        let request = request(&["one", "two"], Language::French);
        let short = r#"{"translations":[{"id":1,"text":"world"}]}"#;
        let parsed = translations("test", short, &request).unwrap();
        assert_eq!(parsed.texts, ["one", "world"]);
        assert_eq!(parsed.untranslated, [0]);
    }

    #[test]
    fn source_text_left_in_a_latin_target_is_reported() {
        let request = request(&["こんにちは", "Salut !"], Language::French);
        let response = concat!(
            r#"{"translations":["#,
            r#"{"id":0,"text":"こんにちは"},"#,
            r#"{"id":1,"text":"Bonjour !"}"#,
            "]}"
        );
        assert_eq!(
            translations("test", response, &request)
                .unwrap()
                .untranslated,
            [0]
        );
    }

    #[test]
    fn latin_output_that_matches_the_source_is_not_flagged() {
        let request = request(&["OK", "SASUKE"], Language::French);
        let response = concat!(
            r#"{"translations":["#,
            r#"{"id":0,"text":"OK"},"#,
            r#"{"id":1,"text":"SASUKE"}"#,
            "]}"
        );
        assert!(
            translations("test", response, &request)
                .unwrap()
                .untranslated
                .is_empty()
        );
    }

    #[test]
    fn the_output_budget_grows_with_the_page_and_stays_bounded() {
        let short = vec!["a".to_owned()];
        assert_eq!(output_budget(&short), 768);

        let dense = vec!["こんにちは、元気ですか？".to_owned(); 40];
        assert!(output_budget(&dense) > 1000);
        assert!(output_budget(&dense) <= 2560);

        let huge = vec!["long line".to_owned(); 400];
        assert_eq!(output_budget(&huge), 2560, "the budget must stay bounded");
    }

    #[test]
    fn a_retry_keeps_the_context_and_the_caller_instructions() {
        let request = request(&["one", "two"], Language::French)
            .with_source_language(Language::Japanese)
            .with_context([TranslationContext::new("old", "previous")])
            .with_instructions("Use informal speech.");
        let retry = retry_request(&request, &[1]);
        assert_eq!(retry.segments, ["two"]);
        assert_eq!(retry.source_language, Some(Language::Japanese));
        assert_eq!(retry.context, request.context);
        let instructions = retry
            .instructions
            .as_deref()
            .expect("a retry always adds instructions");
        assert!(instructions.contains("Use informal speech."));
        assert!(instructions.contains("never return the source text"));
        assert!(translation_system_prompt(&retry).contains("left these segments untranslated"));
    }

    #[test]
    fn tolerates_duplicate_missing_and_out_of_range_ids() {
        let request = request(&["one", "two"], Language::French);
        let short = r#"{"translations":[{"id":1,"text":"world"}]}"#;
        assert_eq!(
            translations("test", short, &request).unwrap().texts,
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
            translations("test", response, &request).unwrap().texts,
            ["hello", "two"]
        );
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
        assert!(prompt.contains("earlier lines of this same chapter"));
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
