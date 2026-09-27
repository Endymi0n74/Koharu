use std::{collections::HashMap, ffi::OsString, path::PathBuf, sync::Mutex};

use anyhow::{Context as _, Result, anyhow};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// Bumped when the file layout changes; an older file is replaced instead of
/// being read as if it were the current schema.
const VERSION: u32 = 1;

/// OCR text persisted between runs. The key covers the model and the exact
/// pixels of the crop, so a hit returns precisely what the model read from
/// the same input last time — the translation stage then receives the same
/// text on every run even though OCR inference itself is not bit-stable.
pub(crate) struct OcrCache {
    path: PathBuf,
    entries: Mutex<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize)]
struct File {
    version: u32,
    entries: HashMap<String, String>,
}

impl OcrCache {
    /// Opens the cache at `path`. A missing, unreadable, or outdated file
    /// starts empty: persistence is an optimization, never a reason to fail
    /// the run.
    pub(crate) fn open(path: PathBuf) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<File>(&text) {
                Ok(file) if file.version == VERSION => file.entries,
                Ok(file) => {
                    tracing::warn!(
                        path = %path.display(),
                        version = file.version,
                        "unsupported OCR cache version; starting empty"
                    );
                    HashMap::new()
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "unreadable OCR cache; starting empty"
                    );
                    HashMap::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "failed to read the OCR cache; starting empty"
                );
                HashMap::new()
            }
        };
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    /// Key of one OCR input: the model name plus the exact pixels handed to
    /// it. Two crops share a key only when they are byte-identical, so a hit
    /// can never return text that was read from different pixels.
    pub(crate) fn key(model: &str, image: &DynamicImage) -> String {
        let pixels = image.to_rgba8();
        let mut hasher = blake3::Hasher::new();
        hasher.update(model.as_bytes());
        hasher.update(&[0]);
        hasher.update(&pixels.width().to_le_bytes());
        hasher.update(&pixels.height().to_le_bytes());
        hasher.update(pixels.as_raw());
        let mut key = String::with_capacity(64);
        for byte in hasher.finalize().as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(key, "{byte:02x}");
        }
        key
    }

    /// The text recorded for `key`, when a previous run stored it.
    pub(crate) fn lookup(&self, key: &str) -> Option<String> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    /// Records fresh OCR output. The first text for a key wins: the pixels
    /// already decided it, and keeping the first reading is what makes every
    /// later run agree with the first one.
    pub(crate) fn store(&self, entries: impl IntoIterator<Item = (String, String)>) {
        let Ok(mut guard) = self.entries.lock() else {
            return;
        };
        for (key, text) in entries {
            guard.entry(key).or_insert(text);
        }
    }

    /// Replaces the file atomically, so an interrupted run cannot corrupt the
    /// entries it already produced.
    pub(crate) fn save(&self) -> Result<()> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| anyhow!("OCR cache lock is poisoned"))?
            .clone();
        let document = serde_json::to_string(&File {
            version: VERSION,
            entries,
        })
        .context("failed to encode the OCR cache")?;
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut temporary = OsString::from(self.path.as_os_str());
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        std::fs::write(&temporary, &document)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("failed to replace {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn source(seed: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([seed, seed, seed])))
    }

    #[test]
    fn text_survives_reopening_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".ocr-cache.json");
        let key = OcrCache::key("paddleocr-vl-1.6", &source(7));

        let cache = OcrCache::open(path.clone());
        cache.store([(key.clone(), "オーエー".to_owned())]);
        cache.save().unwrap();

        let reopened = OcrCache::open(path);
        assert_eq!(reopened.lookup(&key).as_deref(), Some("オーエー"));
    }

    #[test]
    fn different_pixels_or_a_different_model_read_different_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        let cache = OcrCache::open(path);
        cache.store([(
            OcrCache::key("paddleocr-vl-1.6", &source(1)),
            "ひとつめ".to_owned(),
        )]);

        assert_eq!(
            cache
                .lookup(&OcrCache::key("paddleocr-vl-1.6", &source(1)))
                .as_deref(),
            Some("ひとつめ")
        );
        assert!(
            cache
                .lookup(&OcrCache::key("paddleocr-vl-1.6", &source(2)))
                .is_none()
        );
        assert!(
            cache
                .lookup(&OcrCache::key("manga-ocr", &source(1)))
                .is_none()
        );
    }

    #[test]
    fn the_first_reading_of_a_key_is_kept() {
        let directory = tempfile::tempdir().unwrap();
        let cache = OcrCache::open(directory.path().join("cache.json"));
        let key = OcrCache::key("paddleocr-vl-1.6", &source(3));
        cache.store([(key.clone(), "première".to_owned())]);
        cache.store([(key.clone(), "seconde".to_owned())]);

        assert_eq!(cache.lookup(&key).as_deref(), Some("première"));
    }

    #[test]
    fn an_unreadable_file_starts_empty_without_failing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        std::fs::write(&path, "{ not json").unwrap();

        let cache = OcrCache::open(path);
        assert!(cache.lookup("anything").is_none());
    }
}
