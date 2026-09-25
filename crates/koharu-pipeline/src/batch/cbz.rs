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

/// An open CBZ archive whose entries are read without reopening the file.
///
/// `ZipArchive::new` parses the whole central directory, which a chapter
/// would otherwise pay once per page.
pub struct ArchiveReader {
    archive: ZipArchive<fs::File>,
}

impl ArchiveReader {
    /// Opens the archive at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let archive =
            ZipArchive::new(file).with_context(|| format!("failed to read {}", path.display()))?;
        Ok(Self { archive })
    }

    /// Reads one entry's bytes out of the archive.
    pub fn read_entry(&mut self, entry_index: usize) -> Result<Vec<u8>> {
        let mut entry = self.archive.by_index(entry_index)?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// One entry of an existing output archive carried over into a resumed run.
#[derive(Clone, Debug)]
pub struct Carried {
    /// Reading-order page index the entry belongs to.
    pub page: usize,
    /// Entry index inside the archive being resumed.
    pub source: usize,
    /// Name the entry gets in the rewritten archive.
    pub name: String,
}

/// Streams translated pages into a CBZ written as `<target>.partial` and
/// renamed on success, so an interrupted run never leaves a truncated archive.
///
/// Entries listed at creation are copied out of the archive being replaced
/// before the first translated page that follows them, which keeps the pages a
/// resumed run did not re-translate, in reading order.
pub struct ArchiveOutput {
    writer: Option<ZipWriter<fs::File>>,
    target: std::path::PathBuf,
    partial: std::path::PathBuf,
    finished: bool,
    carried: Vec<Carried>,
    copied: usize,
    existing: Option<ZipArchive<fs::File>>,
}

impl ArchiveOutput {
    /// Creates the output archive; `carried` entries are read from `target`,
    /// so it must be the very archive they were listed from.
    pub fn create(target: &Path, carried: Vec<Carried>) -> Result<Self> {
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
        let existing = if carried.is_empty() {
            None
        } else {
            let source = fs::File::open(target).with_context(|| {
                format!(
                    "failed to open the archive being resumed at {}",
                    target.display()
                )
            })?;
            Some(ZipArchive::new(source).with_context(|| {
                format!(
                    "failed to read the archive being resumed at {}",
                    target.display()
                )
            })?)
        };
        Ok(Self {
            writer: Some(ZipWriter::new(file)),
            target: target.to_owned(),
            partial,
            finished: false,
            carried,
            copied: 0,
            existing,
        })
    }

    /// Adds the page at `page` (reading-order index) under `name`, after the
    /// carried entries that precede it. `extension` is the format the bytes
    /// were actually encoded with.
    pub fn add_page(
        &mut self,
        page: usize,
        name: &str,
        extension: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.copy_carried(page)?;
        self.write_entry(&format!("{}.{}", entry_stem(name), extension), bytes)
    }

    /// Publishes the archive by renaming the partial file onto the target.
    pub fn finish(mut self) -> Result<()> {
        self.copy_carried(usize::MAX)?;
        // Windows cannot rename over a file another handle still owns.
        self.existing = None;
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

    /// Copies the carried entries preceding `page`, preserving reading order.
    fn copy_carried(&mut self, page: usize) -> Result<()> {
        while self.copied < self.carried.len() && self.carried[self.copied].page < page {
            let entry = self.carried[self.copied].clone();
            let bytes = {
                let existing = self
                    .existing
                    .as_mut()
                    .expect("carried entries imply an open archive");
                let mut source = existing
                    .by_index(entry.source)
                    .context("failed to read a carried page out of the archive being resumed")?;
                let mut bytes = Vec::with_capacity(source.size() as usize);
                source.read_to_end(&mut bytes)?;
                bytes
            };
            self.write_entry(&entry.name, &bytes)?;
            self.copied += 1;
        }
        Ok(())
    }

    fn write_entry(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let writer = self
            .writer
            .as_mut()
            .expect("archive writer available until finish");
        writer
            .start_file(
                name,
                FileOptions::<()>::default().compression_method(CompressionMethod::Deflated),
            )
            .context("failed to add a page to the CBZ")?;
        writer.write_all(bytes)?;
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

/// Entry name of an output page: its file name without the extension.
pub fn entry_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_owned())
}

/// Entries of the existing output archive whose page stem is in `stems`, so a
/// resumed run carries them over instead of translating their pages again.
///
/// The carried name keeps the stored bytes but relabels the extension from the
/// content itself: archives written before the output format was honoured
/// could claim `image/jpeg` while holding PNG data.
pub fn carried_entries(target: &Path, stems: &[String]) -> Result<Vec<Carried>> {
    if !target.is_file() {
        return Ok(Vec::new());
    }
    let file =
        fs::File::open(target).with_context(|| format!("failed to open {}", target.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("failed to read {}", target.display()))?;
    let mut carried = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().into_owned();
        let stem = entry_stem(&name);
        let Some(page) = stems.iter().position(|expected| *expected == stem) else {
            continue;
        };
        if carried.iter().any(|other: &Carried| other.page == page) {
            continue;
        }
        let mut head = [0_u8; 16];
        let mut filled = 0;
        while filled < head.len() {
            match entry.read(&mut head[filled..])? {
                0 => break,
                read => filled += read,
            }
        }
        let extension = sniff_extension(&head[..filled]).unwrap_or_else(|| extension_of(&name));
        carried.push(Carried {
            page,
            source: index,
            name: format!("{stem}.{extension}"),
        });
    }
    carried.sort_by_key(|entry| entry.page);
    Ok(carried)
}

/// Output extension matching the bytes an entry actually holds.
fn sniff_extension(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if head.starts_with(b"RIFF") && head.len() >= 12 && &head[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// Extension of a known image entry, normalized the way pages are written.
fn extension_of(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "jpg",
        Some("webp") => "webp",
        _ => "png",
    }
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
        let mut reader = ArchiveReader::open(&archive).unwrap();
        assert_eq!(reader.read_entry(index).unwrap(), b"two");
    }

    fn write_cbz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(
                    *name,
                    FileOptions::<()>::default().compression_method(CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn output_archives_are_published_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.cbz");
        {
            let mut output = ArchiveOutput::create(&target, Vec::new()).unwrap();
            output.add_page(0, "p1", "jpg", b"jpeg-bytes").unwrap();
            output.finish().unwrap();
        }
        assert!(target.exists());
        assert!(!target.with_extension("cbz.partial").exists());

        let mut reader = ArchiveReader::open(&target).unwrap();
        assert_eq!(reader.read_entry(0).unwrap(), b"jpeg-bytes");
    }

    #[test]
    fn dropped_outputs_leave_no_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("aborted.cbz");
        {
            let mut output = ArchiveOutput::create(&target, Vec::new()).unwrap();
            output.add_page(0, "p1", "png", b"x").unwrap();
            // Drop without finishing.
        }
        assert!(!target.exists());
        assert!(
            directory.path().read_dir().unwrap().next().is_none(),
            "the partial file is cleaned up"
        );
    }

    #[test]
    fn carried_entries_keep_untouched_pages_in_reading_order() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.cbz");
        write_cbz(
            &target,
            &[
                ("p1.png", b"one"),
                ("notes.txt", b"ignore"),
                ("p3.png", b"three"),
            ],
        );

        let stems = ["p1", "p2", "p3"].map(str::to_owned).to_vec();
        let carried = carried_entries(&target, &stems).unwrap();
        assert_eq!(
            carried.iter().map(|entry| entry.page).collect::<Vec<_>>(),
            [0, 2]
        );

        // Page 2 is the only one still to translate; pages 1 and 3 are carried
        // before and after it.
        let mut output = ArchiveOutput::create(&target, carried).unwrap();
        output.add_page(1, "p2.png", "png", b"two").unwrap();
        output.finish().unwrap();
        for (index, (bytes, name)) in [
            (0, (b"one".to_vec(), "p1.png")),
            (1, (b"two".to_vec(), "p2.png")),
            (2, (b"three".to_vec(), "p3.png")),
        ] {
            let mut reader = ArchiveReader::open(&target).unwrap();
            assert_eq!(reader.read_entry(index).unwrap(), bytes);
            assert_eq!(list_archive_pages(&target).unwrap()[index].name, name);
        }
    }

    #[test]
    fn carried_entries_relabel_extensions_from_the_stored_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.cbz");
        // A legacy build named the entry after the source page while encoding
        // PNG bytes into it.
        write_cbz(
            &target,
            &[("p1.jpg", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])],
        );

        let carried = carried_entries(&target, &["p1".to_owned()]).unwrap();
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].page, 0);
        assert_eq!(carried[0].name, "p1.png");
    }

    #[test]
    fn carried_entries_ignore_a_missing_or_foreign_archive() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("absent.cbz");
        assert!(
            carried_entries(&missing, &["p1".to_owned()])
                .unwrap()
                .is_empty()
        );

        let foreign = directory.path().join("broken.cbz");
        fs::write(&foreign, b"not a zip").unwrap();
        assert!(carried_entries(&foreign, &["p1".to_owned()]).is_err());
    }
}
