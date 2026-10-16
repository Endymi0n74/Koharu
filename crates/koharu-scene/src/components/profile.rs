//! What a chapter is about, learned from the chapter's own text and persisted
//! with the project so every page is translated with the same names, terms,
//! and voice.
//!
//! This is the "project dossier" the translation stage reads: a single
//! analysis pass fills it, and it can be reviewed or corrected by hand like
//! any other component. Data and local validation only, like the rest of this
//! module.

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Error, Result,
    component::{Component, ValidationContext},
};

use super::Origin;

/// Bounds on what an analysis pass may persist, so one runaway model response
/// cannot bloat the document.
const MAX_TEXT_CHARS: usize = 4096;
const MAX_TERM_CHARS: usize = 512;
const MAX_NOTE_CHARS: usize = 1024;
const MAX_CHARACTERS: usize = 256;
const MAX_GLOSSARY: usize = 1024;

/// The chapter dossier attached to the project: the work, the recurring
/// characters, and the terms whose translation must stay fixed from page to
/// page.
///
/// It carries an [`Origin`] like every component a pipeline writes: the
/// analysis stamps it `Generated`, and an edit by hand flips it to `User`, so
/// a later run can tell a dossier it produced from one a person corrected.
#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DocumentProfile {
    pub origin: Origin,
    pub series: Option<String>,
    pub synopsis: Option<String>,
    pub register: Option<String>,
    pub characters: Vec<ProfileTerm>,
    pub glossary: Vec<ProfileTerm>,
}

impl Default for DocumentProfile {
    fn default() -> Self {
        Self {
            origin: Origin::User,
            series: None,
            synopsis: None,
            register: None,
            characters: Vec::new(),
            glossary: Vec::new(),
        }
    }
}

impl Component for DocumentProfile {
    const KIND: &'static str = "dev.koharu.document.profile";

    fn origin(&self) -> Option<&Origin> {
        Some(&self.origin)
    }

    fn set_origin(&mut self, origin: Origin) -> bool {
        self.origin = origin;
        true
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        for text in [&self.series, &self.synopsis, &self.register]
            .into_iter()
            .flatten()
        {
            if text.chars().count() > MAX_TEXT_CHARS || text.contains('\0') {
                return Err(Error::invalid("document profile text is invalid"));
            }
        }
        if self.characters.len() > MAX_CHARACTERS || self.glossary.len() > MAX_GLOSSARY {
            return Err(Error::invalid("document profile holds too many terms"));
        }
        for term in self.characters.iter().chain(&self.glossary) {
            term.validate()?;
        }
        Ok(())
    }
}

/// One learned term: the source form as it appears in the chapter and the
/// rendering every page must reuse.
#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct ProfileTerm {
    pub source: String,
    pub target: String,
    pub note: Option<String>,
}

impl ProfileTerm {
    fn validate(&self) -> Result<()> {
        let fits = |text: &str, limit: usize| text.chars().count() <= limit && !text.contains('\0');
        let valid = !self.source.trim().is_empty()
            && !self.target.trim().is_empty()
            && fits(&self.source, MAX_TERM_CHARS)
            && fits(&self.target, MAX_TERM_CHARS)
            && self
                .note
                .as_deref()
                .is_none_or(|note| fits(note, MAX_NOTE_CHARS));
        if valid {
            Ok(())
        } else {
            Err(Error::invalid("document profile term is invalid"))
        }
    }
}
