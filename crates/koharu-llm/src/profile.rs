//! A chapter's dossier: what the whole chapter is about, learned from its own
//! text once before its pages are translated, so page 1's rendering of a name
//! holds to page 30 instead of being re-invented from the twelve lines before
//! it.
//!
//! Ported from the fork's `koharu-translator`, where it was
//! `TranslationProfile` (`backend.rs`) asked and parsed by
//! `prompt::profile_prompt`, `prompt::profile_schema` and `prompt::profile`,
//! rendered into the system prompt by `prompt::profile_block`, and kept as the
//! `dev.koharu.document.profile` project component. Upstream has no project
//! component for it: the translation stage holds one dossier per conversation,
//! and learns it again when the model, the target, or the prompt changes.

use std::future::IntoFuture;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use rig_core::{completion::Message, driver::DynModel, operation::Completion};

use crate::model::answers;

/// What a chapter is *about*: the work, the recurring characters, and the terms
/// whose translation must not drift from page to page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub series: Option<String>,
    pub synopsis: Option<String>,
    pub register: Option<String>,
    pub characters: Vec<Term>,
    pub glossary: Vec<Term>,
}

impl Profile {
    /// Whether the profile carries nothing worth adding to a prompt.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let blank = |value: &Option<String>| value.as_deref().is_none_or(|v| v.trim().is_empty());
        blank(&self.series)
            && blank(&self.synopsis)
            && blank(&self.register)
            && self.characters.is_empty()
            && self.glossary.is_empty()
    }

    /// Folds the user's own pairs in: a pair replaces both the learned character
    /// and the learned term that name the same source, and the last pair for a
    /// source wins. The rest of the dossier is kept, so a supplied list sharpens
    /// it rather than replacing it.
    pub fn merge(&mut self, supplied: &[Term]) {
        for pair in supplied
            .iter()
            .filter(|pair| !pair.source.trim().is_empty())
        {
            let source = pair.source.trim();
            self.characters.retain(|term| term.source.trim() != source);
            self.glossary.retain(|term| term.source.trim() != source);
            self.glossary.push(pair.clone());
        }
    }

    /// The "Project context" and "Glossary" blocks the dossier adds to a
    /// translation's system prompt.
    ///
    /// The glossary is deliberately constrained: a model handed a list of terms
    /// tends to sprinkle them everywhere, which the fork's chapter bench caught
    /// it doing (a character's name inserted into unrelated lines, an object
    /// swapped for another glossary entry). The rules below pin each entry to
    /// the text that actually contains it.
    #[must_use]
    pub fn block(&self) -> String {
        let mut block = String::from(
            "Project context (keep these names, terms, and voice consistent across the chapter):",
        );
        let mut line = |label: &str, value: &Option<String>| {
            if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                block.push_str(&format!("\n- {label}: {value}"));
            }
        };
        line("Series", &self.series);
        line("Synopsis", &self.synopsis);
        line("Register", &self.register);

        if !self.characters.is_empty() {
            block.push_str("\n\nCharacters (use these exact renderings):");
            for term in &self.characters {
                term.write(&mut block);
            }
        }
        if !self.glossary.is_empty() {
            block.push_str("\n\nGlossary (use these exact renderings):");
            for term in &self.glossary {
                term.write(&mut block);
            }
        }

        block.push_str(
            "\n\nProject rules:\n\
             - Apply a character or glossary entry only when its source term appears in the text being translated; never insert a term that is absent from it.\n\
             - Never return the project context, character list, or glossary entries as a translation.",
        );
        block
    }
}

/// One term: the source form as it appears in the chapter and the rendering
/// every page must reuse.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Term {
    pub source: String,
    pub target: String,
    /// A short note in the target language, such as who the character is.
    pub note: Option<String>,
}

impl Term {
    fn write(&self, block: &mut String) {
        block.push_str(&format!(
            "\n- {} = {}",
            self.source.trim(),
            self.target.trim()
        ));
        if let Some(note) = self
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            block.push_str(&format!(" ({note})"));
        }
    }
}

/// Reads a chapter's text and describes it, so its translation can be kept
/// consistent: one call per chapter, before its first page. `None` when the
/// chapter has no text or the model found nothing worth recording.
///
/// `texts` are the chapter's texts in reading order; `source` and `target` name
/// the languages in English, `source` unknown when the text doesn't say.
pub async fn learn(
    model: &DynModel<Completion>,
    texts: &[String],
    source: Option<&str>,
    target: &str,
) -> Result<Option<Profile>> {
    if texts.is_empty() {
        return Ok(None);
    }
    let (system, payload) = prompt(texts, source, target);
    let response = answers(model, schema())
        .preamble(&system)
        .build()
        .prompt(Message::user(payload))
        .into_future()
        .await
        .context("the model couldn't describe the chapter")?;
    let profile = parse(&response.output())?;
    Ok((!profile.is_empty()).then_some(profile))
}

/// Prompt and payload that ask a model to describe a chapter before it is
/// translated.
fn prompt(texts: &[String], source: Option<&str>, target: &str) -> (String, String) {
    let source = source.unwrap_or("the source language");
    let system = format!(
        "You are a manga translation editor preparing a chapter before it is translated from {source} into {target}.\n\
         Read the chapter text, which is given in reading order, and describe it so a translator can keep the whole chapter consistent.\n\
         \n\
         Return only a JSON object with these fields:\n\
         - \"series\": the work or series the chapter belongs to, or an empty string when the text does not say.\n\
         - \"synopsis\": two or three sentences, in {target}, describing what happens in the chapter.\n\
         - \"register\": one sentence, in {target}, on the overall tone and speech register.\n\
         - \"characters\": one entry per named character that actually appears in the text, each with:\n\
         \x20   - \"source\": the name exactly as written in {source},\n\
         \x20   - \"target\": the name to use in {target},\n\
         \x20   - \"note\": a short {target} note on who the character is, or an empty string.\n\
         - \"glossary\": one entry per recurring non-name term whose translation must stay fixed (places, titles, objects, organisation names, catchphrases), same three fields.\n\
         \x20   Include a term only if it really appears in the chapter text.\n\
         \n\
         Do not translate the chapter here. Do not invent characters or terms that are absent from the text.\n\
         Write every \"target\" and \"note\" value, and the \"synopsis\" and \"register\" values, in {target}."
    );
    let payload = texts
        .iter()
        .enumerate()
        .map(|(id, text)| format!("{id}\t{text}"))
        .collect::<Vec<_>>()
        .join("\n");
    (system, payload)
}

/// Schema of a [`prompt`] response. Every field is required and non-nullable so
/// the constrained decoder has a single shape to emit; blank strings are folded
/// to `None` when parsing.
fn schema() -> schemars::Schema {
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
    let schema = json!({
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
    });
    schemars::Schema::try_from(schema).expect("the dossier schema is valid")
}

/// Parses a [`prompt`] response, dropping blank fields and half-filled terms
/// rather than failing the whole chapter on one bad entry.
fn parse(text: &str) -> Result<Profile> {
    let learned = serde_json::from_str::<Learned>(text).with_context(|| {
        format!(
            "the chapter's description isn't the JSON asked for: {}",
            snippet(text)
        )
    })?;
    let term = |term: LearnedTerm| {
        let (source, target) = (term.source.trim().to_owned(), term.target.trim().to_owned());
        (!source.is_empty() && !target.is_empty()).then(|| Term {
            source,
            target,
            note: non_empty(term.note),
        })
    };
    Ok(Profile {
        series: non_empty(learned.series),
        synopsis: non_empty(learned.synopsis),
        register: non_empty(learned.register),
        characters: learned.characters.into_iter().filter_map(term).collect(),
        glossary: learned.glossary.into_iter().filter_map(term).collect(),
    })
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The start of an answer, for an error that must say what came back without
/// carrying a whole chapter.
fn snippet(text: &str) -> String {
    let mut snippet: String = text.chars().take(200).collect();
    if text.chars().nth(200).is_some() {
        snippet.push('…');
    }
    snippet
}

#[derive(Deserialize)]
struct Learned {
    #[serde(default)]
    series: String,
    #[serde(default)]
    synopsis: String,
    #[serde(default)]
    register: String,
    #[serde(default)]
    characters: Vec<LearnedTerm>,
    #[serde(default)]
    glossary: Vec<LearnedTerm>,
}

#[derive(Deserialize)]
struct LearnedTerm {
    #[serde(default)]
    source: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    note: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supplied_pair_replaces_the_learned_rendering_of_its_source() {
        let mut profile = Profile {
            characters: vec![Term {
                source: " シャーレ ".to_owned(),
                target: "Salle".to_owned(),
                note: None,
            }],
            glossary: vec![Term {
                source: "王都".to_owned(),
                target: "la capitale".to_owned(),
                note: None,
            }],
            ..Profile::default()
        };
        profile.merge(&[
            Term {
                source: "シャーレ".to_owned(),
                target: "Schale".to_owned(),
                note: None,
            },
            Term {
                source: "王都".to_owned(),
                target: "la Cité".to_owned(),
                note: None,
            },
        ]);
        assert!(profile.characters.is_empty());
        assert_eq!(
            profile.glossary,
            [
                Term {
                    source: "シャーレ".to_owned(),
                    target: "Schale".to_owned(),
                    note: None,
                },
                Term {
                    source: "王都".to_owned(),
                    target: "la Cité".to_owned(),
                    note: None,
                },
            ]
        );
    }

    #[test]
    fn an_answer_drops_blank_fields_and_half_filled_terms() {
        let profile = parse(
            r#"{"series":" ","synopsis":"Un chapitre.","register":"","characters":[{"source":"先生","target":"Sensei","note":""},{"source":" ","target":"x","note":""}],"glossary":[]}"#,
        )
        .expect("an answer");
        assert_eq!(profile.series, None);
        assert_eq!(profile.synopsis.as_deref(), Some("Un chapitre."));
        assert_eq!(profile.register, None);
        assert_eq!(
            profile.characters,
            [Term {
                source: "先生".to_owned(),
                target: "Sensei".to_owned(),
                note: None,
            }]
        );
        assert!(parse("not JSON").is_err());
    }

    #[test]
    fn a_dossier_that_says_nothing_adds_nothing() {
        assert!(Profile::default().is_empty());
        assert!(parse("{}").expect("an answer").is_empty());
    }
}
