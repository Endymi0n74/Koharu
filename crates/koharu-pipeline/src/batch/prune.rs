//! Store pruning: identifies store artifacts the code no longer references —
//! Hugging Face model and dataset repositories, and runtime packages of
//! releases other than the pinned one — and deletes them on request.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The runtime package families the code installs, with the release tag they
/// resolve today. Any sibling release directory of the same family left by an
/// older build is an orphan (models inside the HF tree are separate). CUDA
/// wheels live under one flat directory per wheel and are versioned by the
/// pinned `Cargo.toml`, so the whole `cuda` directory survives pruning.
const RUNTIME_RELEASES: &[(&str, &str)] = &[
    ("llama", "b10903"),
    ("torch", "v2.13.0.7"),
    ("diffusion", "master-920-2f88688"),
];

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
        "abenzerps/Qwen-Image-2.1-Uncensored-GGUF",
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

/// The Hugging Face dataset repository the renderer pins its fonts from,
/// folded to the store's directory form.
fn referenced_datasets() -> BTreeSet<String> {
    BTreeSet::from(["mayocream--fonts".to_owned()])
}

/// Lists the directories under `<store>/hugging-face/<kind>` that no pinned
/// file references: candidates for `prune_store` to delete. `kind` is
/// `models` or `datasets`.
///
/// # Errors
/// When the directory cannot be read. A missing directory is not an error:
/// it simply holds no orphans.
fn orphan_hf_repositories(
    store: &Path,
    kind: &str,
    referenced: &BTreeSet<String>,
) -> Result<Vec<(PathBuf, u64)>> {
    let hf_dir = store.join("hugging-face").join(kind);
    let Ok(entries) = std::fs::read_dir(&hf_dir) else {
        return Ok(Vec::new());
    };
    let mut orphans = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", hf_dir.display()))?;
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

/// Lists the model directories under `<store>/hugging-face/models` that no
/// pinned file references: candidates for `prune_store` to delete.
///
/// # Errors
/// When the models directory cannot be read.
pub fn orphan_repositories(store: &Path) -> Result<Vec<(PathBuf, u64)>> {
    orphan_hf_repositories(store, "models", &referenced_repositories())
}

/// Lists the dataset directories under `<store>/hugging-face/datasets` that
/// no pinned file references.
///
/// # Errors
/// When the datasets directory cannot be read.
pub fn orphan_datasets(store: &Path) -> Result<Vec<(PathBuf, u64)>> {
    orphan_hf_repositories(store, "datasets", &referenced_datasets())
}

/// Lists the runtime release directories of [`RUNTIME_RELEASES`]'s families
/// whose release tag is not the pinned one — a `packages/llama/bNNNN` from an
/// older build, a `packages/torch/vOLD`. Whole families absent from the list
/// (the flat `cuda` wheel directory) are left alone.
///
/// # Errors
/// When a runtime directory cannot be read. A missing directory is not an
/// error: it simply holds no orphans.
pub fn orphan_runtimes(store: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut orphans = Vec::new();
    for (family, current) in RUNTIME_RELEASES {
        let family_dir = store.join(family);
        let Ok(entries) = std::fs::read_dir(&family_dir) else {
            continue;
        };
        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read {}", family_dir.display()))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == *current {
                continue;
            }
            let bytes = directory_bytes(&path);
            orphans.push((path, bytes));
        }
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
    }

    #[test]
    fn orphan_datasets_leave_the_pinned_font_repository() {
        let store = tempfile::tempdir().unwrap();
        let datasets = store.path().join("hugging-face").join("datasets");
        std::fs::create_dir_all(datasets.join("mayocream--fonts")).unwrap();
        std::fs::create_dir_all(datasets.join("someone--old-fonts")).unwrap();

        let orphans = orphan_datasets(store.path()).unwrap();
        let names = orphans
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["someone--old-fonts"]);
    }

    #[test]
    fn orphan_runtimes_keep_only_the_pinned_releases() {
        let store = tempfile::tempdir().unwrap();
        for (family, release) in [
            ("llama", "b10902"),
            ("llama", "b10903"),
            ("torch", "v2.13.0.7"),
            ("torch", "v2.12.0.0"),
            ("diffusion", "master-920-2f88688"),
            ("cuda", "nvidia-cublas--13.6.0.2"),
        ] {
            let dir = store.path().join(family).join(release);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("blob"), [0u8; 8]).unwrap();
        }

        let orphans = orphan_runtimes(store.path()).unwrap();
        let names = orphans
            .iter()
            .map(|(path, _)| {
                let relative = path.strip_prefix(store.path()).unwrap();
                relative.to_string_lossy().replace('\\', "/")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from(["llama/b10902".to_owned(), "torch/v2.12.0.0".to_owned(),]),
            "current releases stay, stale ones go, the cuda wheel tree is untouched"
        );
        assert_eq!(orphans.iter().map(|(_, bytes)| *bytes).sum::<u64>(), 16);
    }

    #[test]
    fn a_missing_runtime_family_is_no_error() {
        let store = tempfile::tempdir().unwrap();
        assert_eq!(orphan_runtimes(store.path()).unwrap(), Vec::new());
        assert_eq!(orphan_datasets(store.path()).unwrap(), Vec::new());
    }
}
