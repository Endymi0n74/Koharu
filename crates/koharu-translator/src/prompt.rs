use std::fmt::Write;

use anyhow::Context;
use indoc::indoc;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};

use crate::{
    Language, ProfileRequest, ProfileTerm, TranslationContext, TranslationProfile,
    TranslationRequest,
};

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
    // The page image already did its job in the response that read it —
    // disambiguating who speaks to whom. Re-sending it for a handful of
    // segments costs the vision encoder and several hundred prompt tokens on
    // every follow-up, so the retry carries text only.
    retry.image = None;
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
    if let Some(profile) = request
        .profile
        .as_ref()
        .filter(|profile| !profile.is_empty())
    {
        prompt.push_str("\n\n");
        prompt.push_str(&profile_block(profile));
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

/// The "Project context" and "Glossary" blocks a learned profile adds to the
/// system prompt.
///
/// The glossary is deliberately constrained: a model handed a list of terms
/// tends to sprinkle them everywhere, which the chapter bench caught it doing
/// (a character's name inserted into unrelated lines, an object swapped for
/// another glossary entry). The rule below pins each entry to the segment
/// that actually contains it.
fn profile_block(profile: &TranslationProfile) -> String {
    let mut block = String::from(
        "Project context (keep these names, terms, and voice consistent across the chapter):",
    );
    let mut line = |label: &str, value: &Option<String>| {
        if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            let _ = write!(block, "\n- {label}: {value}");
        }
    };
    line("Series", &profile.series);
    line("Synopsis", &profile.synopsis);
    line("Register", &profile.register);

    if !profile.characters.is_empty() {
        block.push_str("\n\nCharacters (use these exact renderings):");
        for term in &profile.characters {
            write_term(&mut block, term);
        }
    }
    if !profile.glossary.is_empty() {
        block.push_str("\n\nGlossary (use these exact renderings):");
        for term in &profile.glossary {
            write_term(&mut block, term);
        }
    }

    block.push_str(
        "\n\nProject rules:\n\
         - Apply a character or glossary entry only when its source term appears in the segment being translated; never insert a term that is absent from the segment.\n\
         - Never return the project context, character list, or glossary entries as a translation.",
    );
    block
}

fn write_term(block: &mut String, term: &ProfileTerm) {
    let _ = write!(block, "\n- {} = {}", term.source.trim(), term.target.trim());
    if let Some(note) = term
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        let _ = write!(block, " ({note})");
    }
}

/// Prompt and payload that ask a model to describe a chapter before it is
/// translated: the work, its recurring characters, and the terms whose
/// rendering must stay fixed.
pub(crate) fn profile_prompt(request: &ProfileRequest) -> (String, String) {
    let source = request
        .source_language
        .map(|language| language.to_string())
        .unwrap_or_else(|| "the source language".to_owned());
    let system = format!(
        indoc! {"
            You are a manga translation editor preparing a chapter before it is translated from {source} into {target}.
            Read the chapter text, which is given in reading order, and describe it so a translator can keep the whole chapter consistent.

            Return only a JSON object with these fields:
            - \"series\": the work or series the chapter belongs to, or an empty string when the text does not say.
            - \"synopsis\": two or three sentences, in {target}, describing what happens in the chapter.
            - \"register\": one sentence, in {target}, on the overall tone and speech register.
            - \"characters\": one entry per named character that actually appears in the text, each with:
                - \"source\": the name exactly as written in {source},
                - \"target\": the name to use in {target},
                - \"note\": a short {target} note on who the character is, or an empty string.
            - \"glossary\": one entry per recurring non-name term whose translation must stay fixed (places, titles, objects, organisation names, catchphrases), same three fields.
                Include a term only if it really appears in the chapter text.

            Do not translate the chapter here. Do not invent characters or terms that are absent from the text.
            Write every \"target\" and \"note\" value, and the \"synopsis\" and \"register\" values, in {target}.
        "},
        source = source,
        target = request.target_language,
    )
    .trim_end()
    .to_owned();
    let user = request
        .segments
        .iter()
        .enumerate()
        .map(|(id, text)| format!("{id}\t{text}"))
        .collect::<Vec<_>>()
        .join("\n");
    (system, user)
}

/// Schema of a [`profile_prompt`] response. Every field is required and
/// non-nullable so the constrained decoder has a single shape to emit; blank
/// strings are folded to `None` when parsing.
pub(crate) fn profile_schema() -> Value {
    let term = json!({
        "type": "object",
        "properties": {
            "source": { "type": "string", "description": "The term exactly as written in the source language." },
            "target": { "type": "string", "description": "The fixed rendering to use in the target language." },
            "note": { "type": "string", "description": "A short target-language note, or an empty string." }
        },
        "required": ["source", "target", "note"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "series": { "type": "string" },
            "synopsis": { "type": "string" },
            "register": { "type": "string" },
            "characters": { "type": "array", "items": term },
            "glossary": { "type": "array", "items": term }
        },
        "required": ["series", "synopsis", "register", "characters", "glossary"],
        "additionalProperties": false
    })
}

/// Parses a [`profile_prompt`] response, dropping blank fields and half-filled
/// terms rather than failing the whole chapter on one bad entry.
pub(crate) fn profile(provider: &str, text: &str) -> anyhow::Result<TranslationProfile> {
    let output = serde_json::from_str::<ProfileOutput>(text).with_context(|| {
        format!(
            "{provider} returned an invalid document profile; response was: {}",
            snippet(text),
        )
    })?;
    let term = |term: ProfileOutputTerm| -> Option<ProfileTerm> {
        let source = term.source.trim().to_owned();
        let target = term.target.trim().to_owned();
        (!source.is_empty() && !target.is_empty()).then(|| ProfileTerm {
            source,
            target,
            note: non_empty(term.note),
        })
    };
    let profile = TranslationProfile {
        series: non_empty(output.series),
        synopsis: non_empty(output.synopsis),
        register: non_empty(output.register),
        characters: output.characters.into_iter().filter_map(term).collect(),
        glossary: output.glossary.into_iter().filter_map(term).collect(),
    };
    Ok(profile)
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[derive(Deserialize)]
struct ProfileOutput {
    #[serde(default)]
    series: String,
    #[serde(default)]
    synopsis: String,
    #[serde(default)]
    register: String,
    #[serde(default)]
    characters: Vec<ProfileOutputTerm>,
    #[serde(default)]
    glossary: Vec<ProfileOutputTerm>,
}

#[derive(Deserialize)]
struct ProfileOutputTerm {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
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
        let parsed = translations("test", response, &request(&source, Language::Japanese)).unwrap();

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

        let parsed = translations("test", response, &request(&source, Language::Japanese)).unwrap();
        assert_eq!(parsed.texts, ["one", "world"]);
        assert_eq!(parsed.untranslated, [0]);
    }

    #[test]
    fn source_text_in_a_latin_target_is_reported() {
        let source = ["こんにちは".to_owned()];
        let response = r#"{"translations":[{"id":0,"text":"こんにちは"}]}"#;

        let parsed = translations("test", response, &request(&source, Language::French)).unwrap();
        assert_eq!(parsed.untranslated, [0]);
    }

    #[test]
    fn latin_output_that_matches_the_source_is_not_flagged() {
        let source = ["OK".to_owned()];
        let response = r#"{"translations":[{"id":0,"text":"OK"}]}"#;

        let parsed = translations("test", response, &request(&source, Language::French)).unwrap();
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
            .with_profile(sample_profile())
            .with_instructions("Use informal speech.");
        let retry = retry_request(&request, &[1]);

        assert_eq!(retry.segments, ["two"]);
        assert_eq!(retry.source_language, Some(Language::Japanese));
        assert_eq!(retry.context, request.context);
        assert_eq!(retry.profile, request.profile);
        let instructions = retry.instructions.as_deref().unwrap_or_default();
        assert!(
            instructions.contains("Use informal speech."),
            "{instructions}"
        );
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
    fn the_follow_up_carries_the_missing_segments_without_the_page_image() {
        let request = TranslationRequest::new(["a", "b", "c"], Language::French)
            .with_context([TranslationContext::new("old", "ancien")])
            .with_image(std::sync::Arc::new(image::DynamicImage::new_rgb8(1, 1)));
        let retry = retry_request(&request, &[1, 2]);

        assert_eq!(retry.segments, ["b", "c"], "only the missing segments");
        assert!(
            retry.image.is_none(),
            "the page image has already been read once; re-sending it costs the \
             vision encoder and hundreds of prompt tokens per follow-up"
        );
        assert_eq!(
            retry.context, request.context,
            "the continuity context still applies"
        );
    }

    #[test]
    fn image_context_does_not_expand_the_translation_scope() {
        let request = TranslationRequest::new(["text"], Language::English)
            .with_image(std::sync::Arc::new(image::DynamicImage::new_rgb8(1, 1)));
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("attached original page image"));
        assert!(prompt.contains("Translate only the supplied segments"));
    }

    fn sample_profile() -> TranslationProfile {
        TranslationProfile {
            series: Some("Musashino Shijin".to_owned()),
            synopsis: Some("Un folkloriste refuse d'écrire sur les Jeux.".to_owned()),
            register: None,
            characters: vec![
                ProfileTerm::new("柳田國男", "Yanagita Kunio").with_note("le folkloriste"),
            ],
            glossary: vec![ProfileTerm::new("貴族院", "la Chambre des pairs")],
        }
    }

    #[test]
    fn project_profile_is_appended_before_context() {
        let request = TranslationRequest::new(["こんにちは"], Language::French)
            .with_source_language(Language::Japanese)
            .with_profile(sample_profile())
            .with_context([TranslationContext::new("old", "traduction")]);
        let prompt = translation_system_prompt(&request);

        assert!(prompt.contains("Project context"), "{prompt}");
        assert!(prompt.contains("Series: Musashino Shijin"), "{prompt}");
        assert!(
            !prompt.contains("- Register:"),
            "a blank field is omitted: {prompt}"
        );
        assert!(
            prompt.contains("柳田國男 = Yanagita Kunio (le folkloriste)"),
            "{prompt}"
        );
        assert!(prompt.contains("貴族院 = la Chambre des pairs"), "{prompt}");
        assert!(
            prompt.contains("never insert a term that is absent from the segment"),
            "{prompt}"
        );
        let style = prompt.find("French style requirements").expect("style");
        let profile = prompt.find("Project context").expect("profile");
        let context = prompt.find("Context requirements").expect("context");
        assert!(style < profile && profile < context, "{prompt}");
    }

    #[test]
    fn an_empty_profile_adds_no_project_block() {
        let request = TranslationRequest::new(["hi"], Language::French)
            .with_profile(TranslationProfile::default());
        assert!(!translation_system_prompt(&request).contains("Project context"));
    }

    #[test]
    fn profile_analysis_prompt_carries_language_and_chapter() {
        let request = ProfileRequest::new(["こんにちは", "ありがとう"], Language::French)
            .with_source_language(Language::Japanese);
        let (system, user) = profile_prompt(&request);

        assert!(system.contains("from Japanese into French"), "{system}");
        assert!(system.contains("Do not invent"), "{system}");
        assert_eq!(user, "0\tこんにちは\n1\tありがとう");
    }

    #[test]
    fn profile_response_folds_blanks_and_drops_half_terms() {
        let response = r#"{
            "series": "  ",
            "synopsis": " Une histoire. ",
            "register": "",
            "characters": [
                {"source": "柳田", "target": "Yanagita", "note": ""},
                {"source": "incomplete", "target": "", "note": "n"}
            ],
            "glossary": []
        }"#;
        let profile = profile("test", response).unwrap();

        assert_eq!(profile.series, None);
        assert_eq!(profile.synopsis.as_deref(), Some("Une histoire."));
        assert_eq!(profile.register, None);
        assert_eq!(profile.characters.len(), 1);
        assert_eq!(profile.characters[0].source, "柳田");
        assert_eq!(profile.characters[0].target, "Yanagita");
        assert_eq!(profile.characters[0].note, None);
        assert!(!profile.is_empty());
    }

    #[test]
    fn an_invalid_profile_response_is_an_error() {
        assert!(profile("test", "not json").is_err());
        assert_eq!(
            profile("test", "{}").unwrap(),
            TranslationProfile::default()
        );
    }
}
