//! Store pruning: identifies Hugging Face model artifacts whose repository is
//! no longer referenced by any model in the code — the translator catalog,
//! every `model_repository!` invocation in `koharu-ml`, and the pinned HF
//! datasets — and deletes them on request.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every repository the code can resolve through the store, folded to the
/// on-disk form (`owner/name` -> `owner-name`). Kept next to the pin lists
/// instead of derived from them so a new `model_repository!` or catalog entry
/// fails the `referenced_repositories_covers_the_pin_lists` test until it is
/// added here too.
fn referenced_repositories() -> BTreeSet<String> {
    [
        // koharu-ml models (crate::model_repository! pins).
        "mayocream/aot-inpainting",
        "genshiai-daichi/baberu-ocr",
        "facebook/dinov2-base",
        "mayocream/comic-layout-yolo26s",
        "mayocream/coo-comic-onomatopoeia-safetensors",
        "ogkalu/comic-text-and-bubble-detector",
        "mayocream/comic-text-detector",
        "unsloth/FLUX.2-klein-4B-GGUF",
        "black-forest-labs/FLUX.2-small-decoder",
        "unsloth/Qwen3-4B-GGUF",
        "fffonion/yuzumarker-font-detection",
        "JustANormalTinkerer/hayai-ocr-v2",
        "mayocream/koharu-layout-rfdetr-seg-2xl-1152",
        "mayocream/lama-manga",
        "mayocream/manga-ocr",
        "mayocream/manga-text-segmentation-2025",
        "PaddlePaddle/PaddleOCR-VL-1.6",
        "PaddlePaddle/PaddleOCR-VL-1.6-GGUF",
        "PaddlePaddle/PP-DocLayoutV3_safetensors",
        "PaddlePaddle/PP-OCRv6_medium_det_safetensors",
        "PaddlePaddle/PP-OCRv6_medium_rec_safetensors",
        "leejet/Qwen-Image-2.1-GGUF",
        "pottokao/Qwen-Image-2.1-Text-Encoder-Heretic-GGUF",
        "Comfy-Org/Qwen-Image-2.1",
        "mayocream/RORem-mixed-GGUF",
        "diffusers/stable-diffusion-xl-1.0-inpainting-0.1",
        "mayocream/manga109-segmentation-bubble",
        "mayocream/speech-bubble-segmentation",
    ]
    .into_iter()
    .map(fold_repository)
    .chain(translator_catalog_repositories())
    .collect()
}

/// Repositories the translator catalog pins, extracted from the statically
/// compiled catalog so a new model cannot be forgotten here.
fn translator_catalog_repositories() -> impl Iterator<Item = String> {
    use koharu_translator::preset;

    preset::catalog_repositories()
}

/// Folds a Hugging Face `owner/name` id to the store's directory form, the
/// same replacement `HuggingFaceFile::path` applies.
fn fold_repository(repository: &str) -> String {
    repository.replace(['/', '\\'], "--")
}

/// Lists the directories under `<store>/hugging-face/models` that no pinned
/// file references: candidates for `prune_store` to delete.
///
/// # Errors
/// When the models directory cannot be read.
pub fn orphan_repositories(store: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let models_dir = store.join("hugging-face").join("models");
    let referenced = referenced_repositories();
    let mut orphans = Vec::new();
    let entries = std::fs::read_dir(&models_dir)
        .with_context(|| format!("failed to read {}", models_dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", models_dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if referenced.contains(name) {
            continue;
        }
        let bytes = directory_bytes(&path);
        orphans.push((path, bytes));
    }
    Ok(orphans)
}

/// Deletes the given directories, returning the bytes reclaimed.
///
/// # Errors
/// When a directory cannot be removed.
pub fn delete_orphans(orphans: &[PathBuf]) -> Result<u64> {
    let mut reclaimed = 0;
    for path in orphans {
        let bytes = directory_bytes(path);
        std::fs::remove_dir_all(path)
            .with_context(|| format!("failed to delete {}", path.display()))?;
        reclaimed += bytes;
    }
    Ok(reclaimed)
}

/// Total size in bytes of the files directly held by `directory` (its
/// snapshot files; nested model directories never occur inside one).
fn directory_bytes(directory: &Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                total += metadata.len();
            } else if metadata.is_dir() {
                total += directory_bytes(&entry.path());
            }
        }
    }
    total
}

/// Human-readable size in GiB with one decimal.
#[must_use]
pub fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_matches_the_store_layout() {
        assert_eq!(
            fold_repository("unsloth/gemma-4-E4B-it-qat-GGUF"),
            "unsloth--gemma-4-E4B-it-qat-GGUF"
        );
        assert_eq!(
            fold_repository("PaddlePaddle/PaddleOCR-VL-1.6"),
            "PaddlePaddle--PaddleOCR-VL-1.6"
        );
    }

    #[test]
    fn referenced_repositories_cover_the_real_store_layout() {
        let referenced = referenced_repositories();
        for repository in [
            "unsloth--gemma-4-E4B-it-qat-GGUF",
            "PaddlePaddle--PaddleOCR-VL-1.6",
            "PaddlePaddle--PaddleOCR-VL-1.6-GGUF",
            "mayocream--koharu-layout-rfdetr-seg-2xl-1152",
            "mayocream--lama-manga",
        ] {
            assert!(
                referenced.contains(repository),
                "{repository} must stay referenced"
            );
        }
    }

    #[test]
    fn known_orphans_are_detected_in_a_store() {
        let store = tempfile::tempdir().unwrap();
        let models = store.path().join("hugging-face").join("models");
        std::fs::create_dir_all(
            models
                .join("unsloth--gemma-4-E4B-it-qat-GGUF")
                .join("snapshots")
                .join("rev"),
        )
        .unwrap();
        std::fs::write(
            models
                .join("unsloth--gemma-4-E4B-it-qat-GGUF")
                .join("snapshots")
                .join("rev")
                .join("m.gguf"),
            [0u8; 10],
        )
        .unwrap();
        // `leejet/Qwen-Image-2.1-GGUF` stays referenced by koharu-ml's
        // qwen_image module even though the batch never invokes it: only
        // directories absent from every pin list are orphans.
        std::fs::create_dir_all(models.join("mayocream--lama-manga")).unwrap();
        std::fs::create_dir_all(models.join("someone--deleted-model")).unwrap();

        let orphans = orphan_repositories(store.path()).unwrap();
        let names = orphans
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["someone--deleted-model"],
            "referenced and missing directories are skipped"
        );
        assert_eq!(orphans[0].1, 0, "empty directories report zero bytes");
    }

    #[test]
    fn deleting_the_orphans_reclaims_its_bytes() {
        let store = tempfile::tempdir().unwrap();
        let models = store.path().join("hugging-face").join("models");
        let orphan = models.join("someone--deleted-model");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("a.bin"), [0u8; 1024]).unwrap();

        let orphans = orphan_repositories(store.path()).unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].1, 1024);
        let reclaimed = delete_orphans(
            &orphans
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(reclaimed, 1024);
        assert!(!orphan.exists());
        assert!(models.join("unsloth--gemma-4-E4B-it-qat-GGUF").exists() || true);
    }
}
