//! What a project holds: every page's texts and their translations, as
//! Markdown for a reader and JSON for a script.

use std::path::Path;

use anyhow::{Context as _, Result};
use koharu_scene::Session;
use serde_json::json;

use crate::cli::Report;

pub fn report(arguments: &Report) -> Result<()> {
    let session = Session::open(&arguments.project)
        .with_context(|| format!("couldn't open {}", arguments.project.display()))?;
    let project = session.project();
    let mut markdown = String::new();
    let mut pages = Vec::new();
    let (mut translated, mut untranslated) = (0, 0);
    for (index, page) in project.pages().enumerate() {
        markdown.push_str(&format!("## Page {}\n\n", index + 1));
        let mut blocks = Vec::new();
        for (_, block) in project.blocks(page.id)? {
            let source = block.source.content.text();
            if source.trim().is_empty() {
                continue;
            }
            let translation = block
                .translation
                .as_ref()
                .map(|text| text.content.text())
                .filter(|text| !text.trim().is_empty());
            match &translation {
                Some(_) => translated += 1,
                None => untranslated += 1,
            }
            markdown.push_str(&format!(
                "- {} — {}\n",
                source.trim(),
                translation
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("_(untranslated)_")
            ));
            blocks.push(json!({ "source": source, "translation": translation }));
        }
        if blocks.is_empty() {
            markdown.push_str("_No text._\n");
        }
        pages.push(json!({ "page": index + 1, "blocks": blocks }));
    }
    let summary = format!(
        "# {}\n\n{translated} blocks translated, {untranslated} not.\n\n{markdown}",
        title(&arguments.project)
    );
    if let Some(json) = &arguments.json {
        let document = json!({
            "project": arguments.project.display().to_string(),
            "translated": translated,
            "untranslated": untranslated,
            "pages": pages,
        });
        fs_err::write(json, serde_json::to_vec_pretty(&document)?)?;
    }
    match &arguments.markdown {
        Some(markdown) => fs_err::write(markdown, &summary)?,
        None => print!("{summary}"),
    }
    Ok(())
}

/// The project's name, as its file names it.
fn title(project: &Path) -> String {
    project
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Project")
        .to_owned()
}
