//! Page source enumeration for batch translation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Media types accepted as manga pages, mapped to their MIME type.
pub const PAGE_EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
];

/// One page to translate, in reading order.
#[derive(Clone, Debug)]
pub struct PageSource {
    /// Zero-based reading-order position, used for output file names.
    pub index: usize,
    /// Original file name (inside the archive for CBZ inputs).
    pub name: String,
    /// MIME type of the image bytes.
    pub media_type: &'static str,
    /// Where the bytes live: a real path on disk, or an archive entry index.
    pub location: Location,
}

#[derive(Clone, Debug)]
pub enum Location {
    File(PathBuf),
    /// Entry index inside a CBZ archive.
    Entry(usize),
}

fn page_media_type(name: &str) -> Option<&'static str> {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)?;
    PAGE_EXTENSIONS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, media_type)| *media_type)
}

/// Media type of a candidate page file name, or `None` when it is not a page.
#[must_use]
pub fn media_type_of(name: &str) -> Option<&'static str> {
    page_media_type(name)
}

/// One comparable chunk of a file name: a non-digit run or a digit run.
enum Chunk<'a> {
    Text(&'a str),
    Digits(u64),
}

/// Splits the leading chunk off `text` and returns the remainder.
fn split_chunk(text: &str) -> (Chunk<'_>, &str) {
    let split_index = text
        .find(|character: char| character.is_ascii_digit())
        .unwrap_or(text.len());
    let (head, rest) = text.split_at(split_index);
    if !head.is_empty() {
        return (Chunk::Text(head), rest);
    }
    // `text` starts with a digit: consume the whole run.
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    let (digits, remainder) = rest.split_at(end);
    (Chunk::Digits(digits.parse().unwrap_or(u64::MAX)), remainder)
}

/// Natural-sort comparison: digits compare numerically so `p2.png < p10.png`.
pub fn natural_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    let mut left = left;
    let mut right = right;
    loop {
        if left.is_empty() || right.is_empty() {
            return left.len().cmp(&right.len());
        }
        let (left_chunk, left_rest) = split_chunk(left);
        let (right_chunk, right_rest) = split_chunk(right);
        let ordering = match (left_chunk, right_chunk) {
            (Chunk::Digits(value), Chunk::Digits(other)) => value.cmp(&other),
            (Chunk::Digits(_), Chunk::Text(_)) => std::cmp::Ordering::Less,
            (Chunk::Text(_), Chunk::Digits(_)) => std::cmp::Ordering::Greater,
            (Chunk::Text(value), Chunk::Text(other)) => alphanumeric_cmp(value, other),
        };
        if ordering.is_ne() {
            return ordering;
        }
        left = left_rest;
        right = right_rest;
    }
}

fn alphanumeric_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
}

/// Lists the image files of a directory in reading order.
///
/// With `recursive`, pages held in subdirectories join the listing and are
/// named by their path relative to `directory`, so pages from different
/// folders still order against each other. Symbolic links are never followed.
pub fn list_directory_pages(directory: &Path, recursive: bool) -> Result<Vec<PageSource>> {
    let mut entries: Vec<(String, PathBuf, &'static str)> = Vec::new();
    collect_pages(directory, directory, recursive, &mut entries)?;
    entries.sort_by(|left, right| natural_cmp(&left.0, &right.0));
    let pages = entries
        .into_iter()
        .map(|(name, path, media_type)| PageSource {
            index: 0,
            name,
            media_type,
            location: Location::File(path),
        })
        .collect();
    finalize_entries(pages)
}

/// Whether `directory` holds at least one page image, searched the requested
/// way: directly inside it (`recursive == false`) or at any depth.
///
/// Unlike [`list_directory_pages`] this reports an empty listing instead of
/// failing, so callers can ask "are there pages here?" while classifying an
/// input tree.
#[must_use = "the answer decides how the input is classified"]
pub fn has_pages(directory: &Path, recursive: bool) -> bool {
    let mut entries = Vec::new();
    collect_pages(directory, directory, recursive, &mut entries).is_ok() && !entries.is_empty()
}

/// Collects the pages of `directory`, descending into it when `recursive`.
fn collect_pages(
    root: &Path,
    directory: &Path,
    recursive: bool,
    entries: &mut Vec<(String, PathBuf, &'static str)>,
) -> Result<()> {
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", directory.display()))?;
        let path = entry.path();
        // `file_type` does not follow links, so a link to a folder can never
        // send this walk in circles.
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if recursive {
                collect_pages(root, &path, true, entries)?;
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some(media_type) = page_media_type(&file_name) else {
            continue;
        };
        // Joined with `/` so a page keeps the same name on every platform,
        // which the report and the CBZ entry names depend on.
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        entries.push((name, path, media_type));
    }
    Ok(())
}

/// Finalizes the reading-order index after sorting.
pub(super) fn finalize_entries(mut entries: Vec<PageSource>) -> Result<Vec<PageSource>> {
    if entries.is_empty() {
        anyhow::bail!("no manga pages found (expected png, jpg, or webp files)");
    }
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.index = index;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_places_two_before_ten() {
        let mut names = vec!["page10.png", "page2.png", "page1.png"];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(names, ["page1.png", "page2.png", "page10.png"]);
    }

    #[test]
    fn natural_order_handles_multi_run_names_and_padding() {
        let mut names = vec![
            "p1_09.jpg",
            "p1_2.jpg",
            "p01_10.jpg",
            "cover.png",
            "p1_100.jpg",
        ];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(
            names,
            [
                "cover.png",
                "p1_2.jpg",
                "p1_09.jpg",
                "p01_10.jpg",
                "p1_100.jpg"
            ]
        );
    }

    #[test]
    fn only_page_images_are_listed() {
        assert_eq!(page_media_type("a.png"), Some("image/png"));
        assert_eq!(page_media_type("a.webp"), Some("image/webp"));
        assert_eq!(page_media_type("a.txt"), None);
        assert_eq!(page_media_type("a"), None);
    }

    #[test]
    fn recursive_listing_names_pages_after_their_folder() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("p2.png"), b"x").unwrap();
        std::fs::write(directory.path().join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(directory.path().join("sub")).unwrap();
        std::fs::write(directory.path().join("sub").join("p1.png"), b"x").unwrap();

        let flat = list_directory_pages(directory.path(), false).unwrap();
        assert_eq!(
            flat.iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            ["p2.png"],
            "subdirectories are skipped by default"
        );

        let deep = list_directory_pages(directory.path(), true).unwrap();
        assert_eq!(
            deep.iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            ["p2.png", "sub/p1.png"]
        );
        assert_eq!(deep[1].index, 1);
        assert!(matches!(deep[1].location, Location::File(_)));
    }

    #[test]
    fn an_empty_directory_still_reports_that_it_has_no_pages() {
        let directory = tempfile::tempdir().unwrap();
        assert!(list_directory_pages(directory.path(), true).is_err());
    }
}
