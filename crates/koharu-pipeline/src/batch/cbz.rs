//! CBZ (zip) input extraction and output packaging.

use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::FileOptions};

use super::pages::{Location, PageSource, media_type_of, natural_cmp};

/// Lists the page entries of a CBZ archive in reading order.
pub fn list_archive_pages(archive_path: &Path) -> Result<Vec<PageSource>> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read {}", archive_path.display()))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` rejects paths escaping the archive root.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().into_owned();
        let Some(media_type) = media_type_of(&name) else {
            continue;
        };
        entries.push(PageSource {
            index: 0,
            name,
            media_type,
            location: Location::Entry(index),
        });
    }
    entries.sort_by(|left, right| natural_cmp(&left.name, &right.name));
    super::pages::finalize_entries(entries)
}

/// Reads one entry's bytes out of a CBZ archive.
pub fn read_entry(archive_path: &Path, entry_index: usize) -> Result<(Vec<u8>, String)> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read {}", archive_path.display()))?;
    let mut entry = archive.by_index(entry_index)?;
    let name = entry
        .enclosed_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("entry-{entry_index}"));
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok((bytes, name))
}

/// Streams translated pages into a CBZ written as `<target>.partial` and
/// renamed on success, so an interrupted run never leaves a truncated archive.
pub struct ArchiveOutput {
    writer: Option<ZipWriter<fs::File>>,
    target: std::path::PathBuf,
    partial: std::path::PathBuf,
    finished: bool,
}

impl ArchiveOutput {
    pub fn create(target: &Path) -> Result<Self> {
        let partial = target.with_extension(format!(
            "{}partial",
            target
                .extension()
                .map_or_else(String::new, |extension| format!(
                    "{}.",
                    extension.to_string_lossy()
                ))
        ));
        let file = fs::File::create(&partial)
            .with_context(|| format!("failed to create {}", partial.display()))?;
        Ok(Self {
            writer: Some(ZipWriter::new(file)),
            target: target.to_owned(),
            partial,
            finished: false,
        })
    }

    /// Adds one rendered page under `name`.
    pub fn add_page(&mut self, name: &str, media_type: &str, bytes: &[u8]) -> Result<()> {
        let extension = media_type
            .strip_prefix("image/")
            .unwrap_or("png")
            .replace("jpeg", "jpg");
        let entry = format!("{}.{}", sanitize_entry_name(name), extension);
        let writer = self
            .writer
            .as_mut()
            .expect("archive writer available until finish");
        writer
            .start_file(
                entry,
                FileOptions::<()>::default().compression_method(CompressionMethod::Deflated),
            )
            .context("failed to add a page to the CBZ")?;
        writer.write_all(bytes)?;
        Ok(())
    }

    /// Publishes the archive by renaming the partial file onto the target.
    pub fn finish(mut self) -> Result<()> {
        let writer = self
            .writer
            .take()
            .expect("archive writer available until finish");
        writer
            .finish()
            .with_context(|| format!("failed to write {}", self.partial.display()))?;
        self.finished = true;
        fs::rename(&self.partial, &self.target)
            .with_context(|| format!("failed to publish {}", self.target.display()))?;
        Ok(())
    }
}

impl Drop for ArchiveOutput {
    fn drop(&mut self) {
        if !self.finished {
            let _ = fs::remove_file(&self.partial);
        }
    }
}

fn sanitize_entry_name(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cbz(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in [
            ("p10.png", b"ten".as_slice()),
            ("p2.png", b"two".as_slice()),
            ("notes.txt", b"skip".as_slice()),
        ] {
            writer
                .start_file(
                    name,
                    FileOptions::<()>::default().compression_method(CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn archives_list_pages_in_natural_order_without_other_files() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("chapter.cbz");
        sample_cbz(&archive);

        let pages = list_archive_pages(&archive).unwrap();
        assert_eq!(
            pages
                .iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            ["p2.png", "p10.png"]
        );
        assert_eq!(pages[0].media_type, "image/png");
        assert!(matches!(pages[0].location, Location::Entry(1)));
    }

    #[test]
    fn entries_read_their_original_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("chapter.cbz");
        sample_cbz(&archive);

        let pages = list_archive_pages(&archive).unwrap();
        let Location::Entry(index) = pages[0].location else {
            panic!("archive pages point at raw entries");
        };
        let (bytes, name) = read_entry(&archive, index).unwrap();
        assert_eq!(bytes, b"two");
        assert_eq!(name, "p2.png");
    }

    #[test]
    fn output_archives_are_published_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.cbz");
        {
            let mut output = ArchiveOutput::create(&target).unwrap();
            output.add_page("p1", "image/jpeg", b"jpeg-bytes").unwrap();
            output.finish().unwrap();
        }
        assert!(target.exists());
        assert!(!target.with_extension("cbz.partial").exists());

        let (bytes, name) = read_entry(&target, 0).unwrap();
        assert_eq!(bytes, b"jpeg-bytes");
        assert_eq!(name, "p1.jpg");
    }

    #[test]
    fn dropped_outputs_leave_no_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("aborted.cbz");
        {
            let mut output = ArchiveOutput::create(&target).unwrap();
            output.add_page("p1", "image/png", b"x").unwrap();
            // Drop without finishing.
        }
        assert!(!target.exists());
        assert!(
            directory.path().read_dir().unwrap().next().is_none(),
            "the partial file is cleaned up"
        );
    }
}
