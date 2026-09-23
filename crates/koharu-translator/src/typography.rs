//! Target-language typographic conventions applied to translated text.
//!
//! Normalization runs once, in the translation stage, so raster rendering and
//! PSD export always observe the same corrected text.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Language;

const NARROW_NO_BREAK_SPACE: char = '\u{202F}';
const NO_BREAK_SPACE: char = '\u{00A0}';

/// Punctuation that takes a narrow no-break space before it in French.
const THIN_SPACE_PUNCTUATION: [char; 4] = ['!', '?', ';', '%'];

/// How translated segments are normalized before rendering and export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TypographyProfile {
    /// Apply the conventions of the target language when they are known.
    #[default]
    Auto,
    /// Keep translated text exactly as the translator produced it.
    Off,
}

impl TypographyProfile {
    /// Whether conventions are applied for `language`.
    #[must_use]
    pub fn applies_to(self, language: Language) -> bool {
        match self {
            Self::Off => false,
            Self::Auto => matches!(language, Language::French),
        }
    }
}

/// Normalizes one translated segment.
///
/// Stylized lettering (sound effects, all-caps display text) is preserved so
/// deliberate lettering choices survive the pass.
#[must_use]
pub fn normalize_segment(profile: TypographyProfile, language: Language, text: &str) -> String {
    if !profile.applies_to(language) || is_stylized_lettering(text) {
        return text.to_owned();
    }

    let text = text.replace('\'', "\u{2019}");
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len() + 8);
    for (index, &current) in characters.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|index| characters.get(index))
            .copied();
        let next = characters.get(index + 1).copied();
        match current {
            '«' => {
                output.push('«');
                if next.is_some_and(|next| !next.is_whitespace()) {
                    output.push(NARROW_NO_BREAK_SPACE);
                }
            }
            '»' => {
                if previous.is_some_and(|previous| !previous.is_whitespace()) {
                    output.push(NARROW_NO_BREAK_SPACE);
                }
                output.push('»');
            }
            ':' => {
                if insert_space_before(previous, next) {
                    output.push(NO_BREAK_SPACE);
                }
                output.push(':');
            }
            _ if THIN_SPACE_PUNCTUATION.contains(&current) => {
                let repeated =
                    previous.is_some_and(|previous| THIN_SPACE_PUNCTUATION.contains(&previous));
                if !repeated && previous.is_some_and(|previous| !previous.is_whitespace()) {
                    output.push(NARROW_NO_BREAK_SPACE);
                }
                output.push(current);
            }
            // A regular space hugging a guillemet is upgraded to the narrow one.
            ' ' if previous == Some('«') || next == Some('»') => {
                output.push(NARROW_NO_BREAK_SPACE)
            }
            _ => output.push(current),
        }
    }
    output
}

/// Whether the French colon rule applies to this occurrence.
fn insert_space_before(previous: Option<char>, next: Option<char>) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous.is_whitespace() {
        return false;
    }
    // Times, ratios, and URLs keep a tight colon (12:30, https://…).
    if previous.is_ascii_digit() && next.is_some_and(|next| next.is_ascii_digit()) {
        return false;
    }
    if next == Some('/') {
        return false;
    }
    true
}

/// Sound effects, all-caps display lettering, and punctuation-only
/// interjections (`…?!`) are left untouched.
fn is_stylized_lettering(text: &str) -> bool {
    let mut letters = 0;
    let mut ascii_alphanumerics = 0;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            ascii_alphanumerics += 1;
        }
        if character.is_alphabetic() {
            letters += 1;
            if !character.is_uppercase() {
                return false;
            }
        }
    }
    ascii_alphanumerics == 0 || letters > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: TypographyProfile = TypographyProfile::Auto;

    #[test]
    fn french_punctuation_takes_a_thin_no_break_space() {
        assert_eq!(
            normalize_segment(AUTO, Language::French, "Salut!"),
            "Salut\u{202F}!"
        );
        assert_eq!(
            normalize_segment(AUTO, Language::French, "Quoi?! Vraiment?"),
            "Quoi\u{202F}?! Vraiment\u{202F}?"
        );
        assert_eq!(
            normalize_segment(AUTO, Language::French, "50%!"),
            "50\u{202F}%!"
        );
    }

    #[test]
    fn french_colon_takes_a_no_break_space_except_in_times_and_urls() {
        assert_eq!(
            normalize_segment(AUTO, Language::French, "Vrai: 12:30"),
            "Vrai\u{00A0}: 12:30"
        );
        assert_eq!(
            normalize_segment(AUTO, Language::French, "voir https://exemple.fr"),
            "voir https://exemple.fr"
        );
    }

    #[test]
    fn guillemets_are_normalized_with_narrow_spaces() {
        assert_eq!(
            normalize_segment(AUTO, Language::French, "«Salut»"),
            "«\u{202F}Salut\u{202F}»"
        );
        assert_eq!(
            normalize_segment(AUTO, Language::French, "« Salut »"),
            "«\u{202F}Salut\u{202F}»"
        );
    }

    #[test]
    fn apostrophes_become_typographic() {
        assert_eq!(
            normalize_segment(AUTO, Language::French, "L'ami d'Arthur"),
            "L\u{2019}ami d\u{2019}Arthur"
        );
    }

    #[test]
    fn stylized_lettering_is_preserved() {
        assert_eq!(
            normalize_segment(AUTO, Language::French, "DOOM!!"),
            "DOOM!!"
        );
        assert_eq!(normalize_segment(AUTO, Language::French, "…?!"), "…?!");
        assert_eq!(
            normalize_segment(AUTO, Language::French, "はぁ？"),
            "はぁ？"
        );
    }

    #[test]
    fn other_languages_and_off_profile_are_untouched() {
        assert_eq!(
            normalize_segment(AUTO, Language::English, "Wait! It's 12:30"),
            "Wait! It's 12:30"
        );
        assert_eq!(
            normalize_segment(TypographyProfile::Off, Language::French, "Salut!"),
            "Salut!"
        );
    }

    #[test]
    fn normalization_is_idempotent() {
        let text = "«N'insiste pas!» Vraiment: 50%?!";
        let once = normalize_segment(AUTO, Language::French, text);
        assert_eq!(normalize_segment(AUTO, Language::French, &once), once);
    }
}
