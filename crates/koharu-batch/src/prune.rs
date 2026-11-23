//! `prune`: the folders the store holds for nothing, listed, and deleted when
//! `--delete` says so.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::cli::Prune;

/// Lists what the store keeps for nothing, and deletes it when `--delete` says
/// so. Nothing is removed without it, so the listing can be read first.
pub async fn prune(arguments: &Prune) -> Result<()> {
    let root = koharu_runtime::store_root();
    if !root.is_dir() {
        println!("{} holds no store", root.display());
        return Ok(());
    }
    let candidates = plan(root, &referenced())?;
    if candidates.is_empty() {
        println!("the store keeps nothing it could drop");
        return Ok(());
    }
    let mut total = 0;
    for candidate in &candidates {
        let bytes = size(candidate)?;
        total += bytes;
        println!("{}\t{}", mibibytes(bytes), shown(root, candidate));
    }
    println!(
        "\n{} folder(s), {} reclaimable",
        candidates.len(),
        mibibytes(total)
    );
    if !arguments.delete {
        println!("nothing was removed: run it again with --delete");
        return Ok(());
    }
    for candidate in &candidates {
        koharu_runtime::remove_from_store(candidate)
            .await
            .with_context(|| format!("couldn't remove {}", shown(root, candidate)))?;
        println!("removed {}", shown(root, candidate));
    }
    Ok(())
}

/// Every folder a catalog or a release still downloads into: a prune keeps
/// them, and everything they are in, and drops what stands beside them.
fn referenced() -> Vec<PathBuf> {
    let mut folders = koharu_runtime::package_versions();
    folders.extend(koharu_llm::repositories().map(|repository| repository.folder()));
    folders.extend(
        koharu_ml::repositories()
            .iter()
            .chain(koharu_renderer::fonts::repositories().iter())
            .map(|repository| repository.folder()),
    );
    folders
}

/// The folders under `root` that `keep` leaves behind: inside a package a kept
/// version lives in, every version it doesn't hold; deeper, such as a commit
/// under a repository's `snapshots`, the same until a kept folder is reached.
///
/// Folders outside every kept package's namespace stay: an unknown folder is
/// not a stale download, and a prune doesn't guess.
fn plan(root: &Path, keep: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let namespaces: BTreeSet<PathBuf> = keep
        .iter()
        .filter_map(|folder| Some(root.join(folder.strip_prefix(root).ok()?.components().next()?)))
        .collect();
    let mut candidates = Vec::new();
    for namespace in &namespaces {
        collect(namespace, keep, &mut candidates)?;
    }
    Ok(candidates)
}

fn collect(folder: &Path, keep: &[PathBuf], candidates: &mut Vec<PathBuf>) -> Result<()> {
    for child in folders(folder)? {
        if keep.contains(&child) {
            continue;
        }
        if keep.iter().any(|kept| kept.starts_with(&child)) {
            collect(&child, keep, candidates)?;
        } else {
            candidates.push(child);
        }
    }
    Ok(())
}

/// The subfolders of `folder`, without the store's hidden entries: a
/// download's stage and a version's lock.
fn folders(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs_err::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !entry.file_name().to_string_lossy().starts_with('.')
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    Ok(folders)
}

/// What a folder holds, its own files and everything under them.
fn size(folder: &Path) -> Result<u64> {
    let mut total = 0;
    let mut pending = vec![folder.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in fs_err::read_dir(&folder)?.flatten() {
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    Ok(total)
}

fn shown(root: &Path, folder: &Path) -> String {
    folder
        .strip_prefix(root)
        .unwrap_or(folder)
        .display()
        .to_string()
}

fn mibibytes(bytes: u64) -> String {
    const MIB: f64 = (1024 * 1024) as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * MIB))
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with one repository whose commit is kept and one whose commit is
    /// stale, a stale commit beside the kept one, two llama.cpp tags, and a
    /// folder no package owns.
    fn populated(root: &Path) {
        for folder in [
            "huggingface/models/a--b/snapshots/kept",
            "huggingface/models/a--b/snapshots/old",
            "huggingface/models/c--d/snapshots/gone",
            "llama/current",
            "llama/previous",
            "unrelated",
        ] {
            fs_err::create_dir_all(root.join(folder)).unwrap();
        }
        fs_err::write(
            root.join("huggingface/models/a--b/snapshots/old/weights.gguf"),
            b"old",
        )
        .unwrap();
    }

    #[test]
    fn a_prune_keeps_what_a_catalog_still_pins() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);
        let keep = vec![
            root.join("huggingface/models/a--b/snapshots/kept"),
            root.join("llama/current"),
        ];

        let candidates = plan(root, &keep).unwrap();

        assert_eq!(
            candidates,
            [
                root.join("huggingface/models/a--b/snapshots/old"),
                root.join("huggingface/models/c--d"),
                root.join("llama/previous"),
            ]
        );
    }

    #[test]
    fn a_folder_no_package_owns_stays() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);

        let candidates = plan(root, &[root.join("llama/current")]).unwrap();

        assert!(!candidates.contains(&root.join("unrelated")));
    }

    #[test]
    fn the_size_covers_what_a_folder_holds() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);

        assert_eq!(
            size(&root.join("huggingface/models/a--b/snapshots/old")).unwrap(),
            3
        );
    }
}
